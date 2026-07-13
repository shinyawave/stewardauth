// SPDX-FileCopyrightText: 2026 shinyawave
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Background auto-confirm engine.
//!
//! Periodically polls Steam for pending confirmations and auto-accepts the
//! categories the user enabled per account (market and/or trade). Runs as a
//! single long-lived task spawned from `lib.rs` setup.
//!
//! # Rate-limit safety
//! Steam rate-limits the `mobileconf/getlist` endpoint, so this engine is
//! deliberately conservative:
//!   - It processes **at most one account per tick** (`TICK` = 5s), so the global
//!     request rate never exceeds ~1 `getlist` per 5s regardless of account count.
//!   - Each account is polled no more often than the configured interval
//!     (default 60s, floor `MIN_INTERVAL` = 15s).
//!   - On any error (rate limit, network, expired session) the account backs off
//!     exponentially up to `BACKOFF_CAP`, resetting on the next success.
//! This is strictly more conservative than manual on-demand loading.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tokio::sync::Mutex;

use crate::commands::{self, AppState};
use crate::steam::confirmations;
use crate::vault::store;

/// How often the scheduler wakes up.
const TICK: Duration = Duration::from_secs(5);
/// Never poll a single account more often than this, even if the user sets a
/// smaller interval.
const MIN_INTERVAL: u32 = 15;
/// Upper bound on per-account exponential backoff after errors.
const BACKOFF_CAP: u32 = 600;

pub const EVENT_CONFIRMED: &str = "auto-confirm-confirmed";
pub const EVENT_ERROR: &str = "auto-confirm-error";

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ConfirmedEvent {
    steam_id: String,
    count: usize,
    market: usize,
    trade: usize,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct ErrorEvent {
    steam_id: String,
    message: String,
}

/// Per-account scheduling state, kept in the task (never persisted).
struct Sched {
    /// When this account is next eligible to be polled.
    due: Instant,
    /// Current backoff in seconds (equals the base interval when healthy).
    backoff: u32,
    /// Whether the last attempt errored — used to emit an error event only on
    /// the transition into an error state (avoids per-tick spam).
    erroring: bool,
}

/// Spawn the engine. Returns immediately; the loop runs for the app's lifetime.
pub fn spawn(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        run(app).await;
    });
}

async fn run(app: AppHandle) {
    let mut schedule: HashMap<String, Sched> = HashMap::new();

    loop {
        tokio::time::sleep(TICK).await;

        // Snapshot config (app dir + master) without holding the guard across await.
        let (app_dir, master) = {
            let state = app.state::<Mutex<AppState>>();
            let guard = state.lock().await;
            match guard.app_data_dir.clone() {
                Some(dir) => (dir, guard.master.clone()),
                None => continue, // not initialised yet
            }
        };

        // Read the manifest each tick — cheap, and the single source of truth for
        // enabled flags + interval. If it fails (e.g. encrypted vault still
        // locked), skip this tick quietly.
        let vault = match store::load_vault(&app_dir, master.as_deref()) {
            Ok(v) => v,
            Err(_) => continue,
        };

        let base = vault.poll_interval_secs.max(MIN_INTERVAL);
        let now = Instant::now();

        // Enabled accounts = at least one category on.
        let enabled: Vec<&crate::vault::model::AccountSummary> = vault
            .accounts
            .iter()
            .filter(|a| a.auto_confirm_market || a.auto_confirm_trade)
            .collect();

        // Drop schedule entries for accounts no longer enabled.
        let enabled_ids: std::collections::HashSet<&str> =
            enabled.iter().map(|a| a.steam_id.as_str()).collect();
        schedule.retain(|id, _| enabled_ids.contains(id.as_str()));

        // Register newly-enabled accounts (due immediately; the one-per-tick cap
        // naturally staggers their first fire by TICK).
        for acc in &enabled {
            schedule.entry(acc.steam_id.clone()).or_insert(Sched {
                due: now,
                backoff: base,
                erroring: false,
            });
        }

        // Pick the single most-overdue account whose time has come.
        let target = schedule
            .iter()
            .filter(|(_, s)| s.due <= now)
            .min_by_key(|(_, s)| s.due)
            .map(|(id, _)| id.clone());

        let Some(steam_id) = target else { continue };

        // Find its flags (still present — we just built `enabled`).
        let Some(acc) = enabled.iter().find(|a| a.steam_id == steam_id) else {
            continue;
        };
        let want_market = acc.auto_confirm_market;
        let want_trade = acc.auto_confirm_trade;

        // Process. On any failure, back off; on success, reset to base.
        let outcome = process_account(
            &app,
            &app_dir,
            master.as_deref(),
            &steam_id,
            want_market,
            want_trade,
        )
        .await;

        let sched = schedule.get_mut(&steam_id).expect("just inserted/retained");
        match outcome {
            Ok((market, trade)) => {
                sched.backoff = base;
                sched.erroring = false;
                sched.due = now + Duration::from_secs(base as u64);
                let count = market + trade;
                if count > 0 {
                    let _ = app.emit(
                        EVENT_CONFIRMED,
                        ConfirmedEvent {
                            steam_id: steam_id.clone(),
                            count,
                            market,
                            trade,
                        },
                    );
                }
            }
            Err(message) => {
                // Exponential backoff, capped.
                sched.backoff = (sched.backoff.saturating_mul(2)).clamp(base, BACKOFF_CAP);
                sched.due = now + Duration::from_secs(sched.backoff as u64);
                // Emit only on transition into the error state.
                if !sched.erroring {
                    sched.erroring = true;
                    let _ = app.emit(
                        EVENT_ERROR,
                        ErrorEvent {
                            steam_id: steam_id.clone(),
                            message,
                        },
                    );
                }
            }
        }
    }
}

/// Poll one account and accept the enabled categories.
///
/// Returns `Ok((market_count, trade_count))` accepted on success, or
/// `Err(message)` on any failure (rate limit, network, expired session).
async fn process_account(
    app: &AppHandle,
    app_dir: &std::path::Path,
    master: Option<&str>,
    steam_id: &str,
    want_market: bool,
    want_trade: bool,
) -> Result<(usize, usize), String> {
    let state = app.state::<Mutex<AppState>>();

    let session = commands::get_or_restore_session(&state, steam_id)
        .await
        .map_err(|e| format!("{e:?}"))?;

    let account = commands::load_account(steam_id, app_dir, master)
        .await
        .map_err(|e| format!("{e:?}"))?;

    let items = confirmations::fetch(&account, &session)
        .await
        .map_err(|e| e.to_string())?;

    // Collect ids matching an enabled category.
    let mut ids: Vec<String> = Vec::new();
    let mut market = 0usize;
    let mut trade = 0usize;
    for item in &items {
        match item.category.as_str() {
            "market" if want_market => {
                ids.push(item.id.clone());
                market += 1;
            }
            "trade" if want_trade => {
                ids.push(item.id.clone());
                trade += 1;
            }
            _ => {}
        }
    }

    if ids.is_empty() {
        return Ok((0, 0));
    }

    confirmations::respond(&account, &session, &ids, true)
        .await
        .map_err(|e| e.to_string())?;

    Ok((market, trade))
}
