// SPDX-FileCopyrightText: 2026 shinyawave
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Tauri IPC commands — the public surface the React frontend calls.
//!
//! All commands are `async` and return `Result<T, AppError>` so errors cross
//! IPC as `{ "kind": "...", "message": "..." }`.  Response types use
//! `#[serde(rename_all = "camelCase")]` so the frontend receives idiomatic
//! camelCase field names.
//!
//! # Security invariant
//! No command returns `shared_secret`, `identity_secret`, session tokens, or any
//! other credential — with one deliberate exception: `export_mafile`, which
//! reconstructs a maFile JSON only on an explicit user "Copy maFile" action.
//! Otherwise only summaries, codes, and status strings cross IPC.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::State;
use tokio::sync::Mutex;

use crate::error::AppError;
use crate::steam::{
    confirmations::{self, ConfirmationItem},
    login_approve::{self, PendingLogin},
    session::{self, SteamSession},
    time::{self, TimeSync},
    totp,
};
use crate::vault::{
    model::AccountSummary,
    store,
};

// ── AppState ──────────────────────────────────────────────────────────────────

/// Shared mutable state managed by Tauri.
///
/// Wrapped in `tokio::sync::Mutex` so guards are `Send` and can be held across
/// `.await` points safely.  In practice we always drop the guard before any
/// async operation to keep critical-section scope minimal.
pub struct AppState {
    /// Path to the application data directory (set at startup from Tauri).
    pub app_data_dir: Option<PathBuf>,
    /// Master password, held in memory after `unlock_vault` is called.
    pub master: Option<String>,
    /// Shared HTTP client (keeps a connection pool alive).
    pub client: reqwest::Client,
    /// Steam time-sync offset; default 0 until an external sync is done.
    pub time_sync: TimeSync,
    /// Active sessions keyed by `steam_id`.
    /// `Arc` lets us clone a reference out of the Mutex before awaiting.
    pub sessions: HashMap<String, Arc<SteamSession>>,
    /// Mirror of the persisted "minimize to tray" setting, read by the
    /// window-close handler without touching disk.
    pub minimize_to_tray: bool,
    /// Live maFiles watcher; kept alive for the app's lifetime (None until started).
    pub watcher: Option<notify::RecommendedWatcher>,
    /// In-progress authenticator-linking session (None when not linking).
    pub link_session: Option<crate::steam::link::LinkSession>,
}

impl Default for AppState {
    fn default() -> Self {
        Self {
            app_data_dir: None,
            master: None,
            client: reqwest::Client::new(),
            time_sync: TimeSync { offset_secs: 0 },
            sessions: HashMap::new(),
            minimize_to_tray: false,
            watcher: None,
            link_session: None,
        }
    }
}

// ── Response types ─────────────────────────────────────────────────────────────

/// The current TOTP code for an account together with how many seconds remain
/// in the current 30-second window.
///
/// JSON shape (camelCase): `{ "code": "ABCDE", "secondsRemaining": 14 }`
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CodeResponse {
    /// 5-character Steam Guard code.
    pub code: String,
    /// Seconds remaining until the code rotates (1–30).
    pub seconds_remaining: u32,
}

/// IPC-only wire DTO for an account summary.
///
/// JSON shape: `{ "steamId": "...", "accountName": "...", "status": "..." }`.
///
/// This is separate from the storage [`AccountSummary`] so the on-disk
/// (snake_case) format and the wire (camelCase) format are independently
/// controlled.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AccountSummaryIpc {
    pub steam_id: String,
    pub account_name: String,
    pub status: String,
    pub auto_confirm_market: bool,
    pub auto_confirm_trade: bool,
    pub mafile_name: String,
    pub proxy: String,
}

impl From<&AccountSummary> for AccountSummaryIpc {
    fn from(summary: &AccountSummary) -> Self {
        Self {
            steam_id: summary.steam_id.clone(),
            account_name: summary.account_name.clone(),
            status: summary.status.clone(),
            auto_confirm_market: summary.auto_confirm_market,
            auto_confirm_trade: summary.auto_confirm_trade,
            mafile_name: summary.mafile_name.clone(),
            proxy: summary.proxy.clone(),
        }
    }
}

/// IPC wire DTO for a group.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GroupIpc {
    pub name: String,
    pub members: Vec<String>,
}

impl From<&crate::vault::model::Group> for GroupIpc {
    fn from(g: &crate::vault::model::Group) -> Self {
        Self {
            name: g.name.clone(),
            members: g.members.clone(),
        }
    }
}

/// IPC wire DTO for a proxy.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyIpc {
    pub id: String,
    pub scheme: String,
    pub host: String,
    pub port: u16,
    pub user: String,
    pub pass: String,
    pub favorite: bool,
}

impl From<&crate::vault::model::Proxy> for ProxyIpc {
    fn from(p: &crate::vault::model::Proxy) -> Self {
        Self {
            id: p.id(),
            scheme: p.scheme.clone(),
            host: p.host.clone(),
            port: p.port,
            user: p.user.clone(),
            pass: p.pass.clone(),
            favorite: p.favorite,
        }
    }
}

/// IPC wire DTO for a text-assign report.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AssignReportIpc {
    pub assigned: usize,
    pub unmatched: Vec<String>,
}

/// Result of a proxy connectivity check.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProxyCheckIpc {
    pub ok: bool,
    pub latency_ms: Option<u32>,
    pub error: Option<String>,
}

/// IPC wire DTO for UI settings.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettingsIpc {
    pub language: String,
    pub minimize_to_tray: bool,
    pub accent_hue: u32,
    pub mafile_naming: String,
    pub common_mafile_format: bool,
}

impl From<&crate::settings::AppSettings> for AppSettingsIpc {
    fn from(s: &crate::settings::AppSettings) -> Self {
        Self {
            language: s.language.clone(),
            minimize_to_tray: s.minimize_to_tray,
            accent_hue: s.accent_hue,
            mafile_naming: s.mafile_naming.clone(),
            common_mafile_format: s.common_mafile_format,
        }
    }
}

// ── Pure helper — testable without Tauri ──────────────────────────────────────

/// Compute how many seconds remain in the current 30-second TOTP window.
///
/// Returns a value in `[1, 30]`.  At `now % 30 == 0` the window has just
/// rotated, so there are exactly 30 seconds remaining.
pub fn seconds_remaining(now: u64) -> u32 {
    let rem = (now % 30) as u32;
    if rem == 0 { 30 } else { 30 - rem }
}

// ── Shared helper: load an Account from Keychain + vault ─────────────────────

/// Build a minimal [`crate::vault::model::Account`] by combining the secret-free
/// [`AccountSummary`] from the vault with the secrets stored in the Keychain.
///
/// Returns `Err(AppError)` if the account is not found in the vault or the
/// Keychain entry is missing/corrupt.
pub(crate) async fn load_account(
    steam_id: &str,
    app_dir: &std::path::Path,
    master: Option<&str>,
) -> Result<crate::vault::model::Account, AppError> {
    let vault = store::load_vault(app_dir, master)?;
    let summary = vault
        .accounts
        .iter()
        .find(|a| a.steam_id == steam_id)
        .ok_or_else(|| {
            AppError::SteamError(format!("account {steam_id} not found in vault"))
        })?
        .clone();

    if !summary.mafile_name.is_empty() {
        return crate::vault::mafiles::read(app_dir, &summary.mafile_name)
            .map_err(|e| AppError::SteamError(format!("cannot read maFile: {e:?}")));
    }

    Err(AppError::SteamError(format!(
        "no maFile on disk for account {steam_id}"
    )))
}

/// Snapshot `(app_data_dir, master)` from state, dropping the guard before any
/// await. Used by the group/export commands.
async fn app_dir_and_master(
    state: &State<'_, Mutex<AppState>>,
) -> Result<(PathBuf, Option<String>), AppError> {
    let guard = state.lock().await;
    let dir = guard.app_data_dir.clone().ok_or_else(|| {
        AppError::SteamError("app data directory not initialised".into())
    })?;
    Ok((dir, guard.master.clone()))
}

// ── Commands ──────────────────────────────────────────────────────────────────

/// List all accounts stored in the vault manifest.
///
/// JSON returned: `Vec<AccountSummaryIpc>` where each item has
/// `{ "steamId": "...", "accountName": "...", "status": "..." }`.
#[tauri::command]
pub async fn list_accounts(
    state: State<'_, Mutex<AppState>>,
) -> Result<Vec<AccountSummaryIpc>, AppError> {
    let (app_dir, master) = {
        let guard = state.lock().await;
        let dir = guard.app_data_dir.clone().ok_or_else(|| {
            AppError::SteamError("app data directory not initialised".into())
        })?;
        (dir, guard.master.clone())
    };
    // Guard dropped — no async held across it.
    let vault = store::load_vault(&app_dir, master.as_deref())?;
    Ok(vault.accounts.iter().map(AccountSummaryIpc::from).collect())
}

/// Get the current Steam Guard TOTP code for the account identified by `steam_id`.
///
/// Loads `shared_secret` from the macOS Keychain, decodes it, and generates
/// the code using the time-synced clock.
///
/// JSON returned: `CodeResponse` → `{ "code": "ABCDE", "secondsRemaining": 14 }`.
#[tauri::command]
pub async fn get_code(
    steam_id: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<CodeResponse, AppError> {
    // Snapshot what we need from state and release the lock immediately.
    let (time_sync, app_dir, master) = {
        let guard = state.lock().await;
        let dir = guard.app_data_dir.clone().ok_or_else(|| {
            AppError::SteamError("app data directory not initialised".into())
        })?;
        (guard.time_sync, dir, guard.master.clone())
    };

    // Load the account's secrets — file-first (maFiles), Keychain fallback for any
    // not-yet-migrated account. Using load_account keeps get_code in sync with the
    // maFiles storage model instead of reading the (now-removed) Keychain entry.
    let account = load_account(&steam_id, &app_dir, master.as_deref()).await?;
    let secret_bytes = totp::decode_shared_secret(&account.shared_secret)?;

    let local_unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let now = time::steam_now(&time_sync, local_unix);

    let code = totp::generate_steam_code(&secret_bytes, now);
    let secs = seconds_remaining(now);

    Ok(CodeResponse { code, seconds_remaining: secs })
}

/// Import maFiles from a set of paths (individual `.maFile`s, `.zip` archives, or
/// folders — with or without an SDA `manifest.json`). Returns an `ImportReport`.
#[tauri::command]
pub async fn import_paths(
    paths: Vec<String>,
    password: Option<String>,
    master: Option<String>,
    state: State<'_, Mutex<AppState>>,
) -> Result<crate::vault::import::ImportReport, AppError> {
    let (app_dir, state_master) = {
        let guard = state.lock().await;
        let dir = guard.app_data_dir.clone().ok_or_else(|| {
            AppError::SteamError("app data directory not initialised".into())
        })?;
        (dir, guard.master.clone())
    };
    let effective_master = master.or(state_master);
    let app_settings = crate::settings::load(&app_dir);
    let ext = if app_settings.common_mafile_format { "maFile" } else { "json" };
    crate::vault::import::import_paths_inner(
        &app_dir,
        &paths,
        password.as_deref(),
        effective_master.as_deref(),
        &app_settings.mafile_naming,
        ext,
    )
}

/// Fetch pending mobile confirmations for `steam_id`.
///
/// Requires an active (non-expired) session created via `login`.
///
/// JSON returned: `Vec<ConfirmationItem>` where each item has
/// `{ "id": "...", "kind": "...", "title": "...", "summary": "...", "icon": "..." }`.
#[tauri::command]
pub async fn fetch_confirmations(
    steam_id: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<Vec<ConfirmationItem>, AppError> {
    let (app_dir, master) = {
        let guard = state.lock().await;
        let dir = guard.app_data_dir.clone().ok_or_else(|| {
            AppError::SteamError("app data directory not initialised".into())
        })?;
        (dir, guard.master.clone())
    };

    // Reuse an in-memory session, or transparently restore one from the stored
    // refresh token. Only errors with SessionExpired when there is genuinely no
    // usable saved session (first login, or the token is revoked/stale).
    let session_arc = get_or_restore_session(&state, &steam_id).await?;

    let account = load_account(&steam_id, &app_dir, master.as_deref()).await?;
    confirmations::fetch(&account, &*session_arc)
        .await
        .map_err(AppError::from)
}

/// Return a valid session for `steam_id`, transparently restoring it from the
/// Keychain-stored refresh token when there is no valid in-memory session.
///
/// This is why a normal restart does NOT force re-login: only when there is no
/// saved session or the stored refresh token is no longer valid do we surface
/// `SessionExpired` (which drives the frontend's re-login prompt).
pub(crate) async fn get_or_restore_session(
    state: &State<'_, Mutex<AppState>>,
    steam_id: &str,
) -> Result<Arc<SteamSession>, AppError> {
    // Resolve the account's current proxy up-front so we can (a) reuse a cached
    // session only when it still routes through the same proxy, and (b) restore
    // through the right proxy otherwise. Without the proxy check, assigning a
    // proxy after a session was already cached would be silently ignored — the
    // stale (direct) session keeps getting reused and Steam keeps rate-limiting.
    let (app_dir, master) = app_dir_and_master(state).await?;
    let proxy = store::resolve_proxy(&app_dir, steam_id, master.as_deref()).unwrap_or(None);
    let want_proxy = proxy.as_ref().map(|p| p.id());

    // 1. A valid in-memory session with the SAME proxy wins.
    {
        let guard = state.lock().await;
        if let Some(s) = guard
            .sessions
            .get(steam_id)
            .filter(|s| s.is_valid() && s.proxy().map(|p| p.id()) == want_proxy)
            .cloned()
        {
            return Ok(s);
        }
    }

    // 2. Restore from the persisted refresh token (no guard held across await),
    // routing through the resolved proxy.
    let restored = session::restore_session(steam_id, proxy.as_ref(), &app_dir)
        .await
        .map_err(|_| AppError::SessionExpired)?;
    let arc = Arc::new(restored);

    // 3. Cache the restored session for subsequent calls.
    {
        let mut guard = state.lock().await;
        guard.sessions.insert(steam_id.to_string(), arc.clone());
    }
    Ok(arc)
}

/// Accept or deny a batch of confirmations by their IDs.
#[tauri::command]
pub async fn respond_confirmation(
    steam_id: String,
    ids: Vec<String>,
    accept: bool,
    state: State<'_, Mutex<AppState>>,
) -> Result<(), AppError> {
    let (app_dir, master) = {
        let guard = state.lock().await;
        let dir = guard.app_data_dir.clone().ok_or_else(|| {
            AppError::SteamError("app data directory not initialised".into())
        })?;
        (dir, guard.master.clone())
    };

    let session_arc = get_or_restore_session(&state, &steam_id).await?;

    let account = load_account(&steam_id, &app_dir, master.as_deref()).await?;
    confirmations::respond(&account, &*session_arc, &ids, accept)
        .await
        .map_err(AppError::from)
}

/// List pending login-approval sessions for `steam_id`.
///
/// Requires an active (non-expired) session. Each returned [`PendingLogin`]
/// includes a `client_id` string the frontend passes back to
/// `respond_login_approval`.
///
/// JSON returned: `Vec<PendingLogin>` — no secrets.
#[tauri::command]
pub async fn list_login_approvals(
    steam_id: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<Vec<PendingLogin>, AppError> {
    let (app_dir, master) = {
        let guard = state.lock().await;
        let dir = guard.app_data_dir.clone().ok_or_else(|| {
            AppError::SteamError("app data directory not initialised".into())
        })?;
        (dir, guard.master.clone())
    };

    let session_arc = get_or_restore_session(&state, &steam_id).await?;
    let account = load_account(&steam_id, &app_dir, master.as_deref()).await?;
    login_approve::list_pending(&account, &*session_arc)
        .await
        .map_err(AppError::from)
}

/// Approve or deny a single pending login session for `steam_id`.
///
/// `client_id` is the string from [`PendingLogin::client_id`].
/// `approve = true` → approve; `false` → deny.
#[tauri::command]
pub async fn respond_login_approval(
    steam_id: String,
    client_id: String,
    approve: bool,
    state: State<'_, Mutex<AppState>>,
) -> Result<(), AppError> {
    let (app_dir, master) = {
        let guard = state.lock().await;
        let dir = guard.app_data_dir.clone().ok_or_else(|| {
            AppError::SteamError("app data directory not initialised".into())
        })?;
        (dir, guard.master.clone())
    };

    let session_arc = get_or_restore_session(&state, &steam_id).await?;
    let account = load_account(&steam_id, &app_dir, master.as_deref()).await?;
    login_approve::respond(&account, &*session_arc, &client_id, approve)
        .await
        .map_err(AppError::from)
}

/// Perform a Steam credential login for the account identified by `steam_id`.
///
/// The TOTP guard code is generated automatically from the stored `shared_secret`.
///
/// Returns the status string `"ok"` on success.
#[tauri::command]
pub async fn login(
    steam_id: String,
    username: String,
    password: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<String, AppError> {
    let (app_dir, time_sync, master) = {
        let guard = state.lock().await;
        let dir = guard.app_data_dir.clone().ok_or_else(|| {
            AppError::SteamError("app data directory not initialised".into())
        })?;
        (dir, guard.time_sync, guard.master.clone())
    };

    let account = load_account(&steam_id, &app_dir, master.as_deref()).await?;

    // Generate the current TOTP code for the guard challenge.
    let secret_bytes = totp::decode_shared_secret(&account.shared_secret)?;
    let local_unix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let now = time::steam_now(&time_sync, local_unix);
    let guard_code = totp::generate_steam_code(&secret_bytes, now);

    // Resolve the account's proxy so the session routes through it.
    let proxy = store::resolve_proxy(&app_dir, &steam_id, master.as_deref())
        .unwrap_or(None);

    // Perform the async login (spawns a blocking thread internally).
    // Guard is NOT held across this await.
    let new_session = session::login(&account, &username, &password, &guard_code, proxy.as_ref(), &app_dir).await?;

    // Store the session — re-acquire lock after the await.
    {
        let mut guard = state.lock().await;
        guard.sessions.insert(steam_id, Arc::new(new_session));
    }

    Ok("ok".into())
}

/// Unlock the vault by validating the master password and storing it in memory.
///
/// Validates the password by attempting to load the vault manifest.
#[tauri::command]
pub async fn unlock_vault(
    master: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<(), AppError> {
    let app_dir = {
        let guard = state.lock().await;
        guard.app_data_dir.clone().ok_or_else(|| {
            AppError::SteamError("app data directory not initialised".into())
        })?
    };

    // Validate the password by loading the vault (errors on wrong password).
    let _vault = store::load_vault(&app_dir, Some(&master))?;

    // Password is correct — store it.
    let mut guard = state.lock().await;
    guard.master = Some(master);
    Ok(())
}

/// Enable or disable vault encryption.
///
/// When enabling, `master` must be provided; it will be stored in memory.
/// When disabling, the current in-memory master is used to read the vault, then
/// the vault is rewritten unencrypted.
#[tauri::command]
pub async fn set_encryption(
    enabled: bool,
    master: Option<String>,
    state: State<'_, Mutex<AppState>>,
) -> Result<(), AppError> {
    let (app_dir, current_master) = {
        let guard = state.lock().await;
        let dir = guard.app_data_dir.clone().ok_or_else(|| {
            AppError::SteamError("app data directory not initialised".into())
        })?;
        (dir, guard.master.clone())
    };

    if enabled {
        let pw = master
            .as_deref()
            .ok_or_else(|| {
                AppError::SteamError("master password required to enable encryption".into())
            })?;

        // Load with current master (may be None if vault was unencrypted).
        let mut vault = store::load_vault(&app_dir, current_master.as_deref())?;
        vault.encrypted = true;
        store::write_manifest(&app_dir, &vault, Some(pw))?;

        // Update in-memory master.
        let mut guard = state.lock().await;
        guard.master = Some(pw.to_owned());
    } else {
        // Load with current master to decrypt.
        let mut vault = store::load_vault(&app_dir, current_master.as_deref())?;
        vault.encrypted = false;
        store::write_manifest(&app_dir, &vault, None)?;

        // Clear in-memory master.
        let mut guard = state.lock().await;
        guard.master = None;
    }

    Ok(())
}

/// Remove an account from the vault and Keychain.
#[tauri::command]
pub async fn remove_account(
    steam_id: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<(), AppError> {
    let (app_dir, master) = {
        let guard = state.lock().await;
        let dir = guard.app_data_dir.clone().ok_or_else(|| {
            AppError::SteamError("app data directory not initialised".into())
        })?;
        (dir, guard.master.clone())
    };

    store::remove_account(&app_dir, &steam_id, master.as_deref())?;

    // Also remove from in-memory sessions.
    let mut guard = state.lock().await;
    guard.sessions.remove(&steam_id);

    Ok(())
}

/// Minimum auto-confirm poll interval (seconds). Enforced to keep the background
/// engine well within Steam's rate limit even if the user requests a smaller value.
pub const MIN_POLL_INTERVAL: u32 = 15;

/// Set per-account auto-confirm flags for a batch of accounts.
///
/// `market` / `trade` are optional: `null` leaves that flag untouched. Returns the
/// updated account list (camelCase) so the frontend can refresh in one round-trip.
#[tauri::command]
pub async fn set_auto_confirm(
    steam_ids: Vec<String>,
    market: Option<bool>,
    trade: Option<bool>,
    state: State<'_, Mutex<AppState>>,
) -> Result<Vec<AccountSummaryIpc>, AppError> {
    let (app_dir, master) = {
        let guard = state.lock().await;
        let dir = guard.app_data_dir.clone().ok_or_else(|| {
            AppError::SteamError("app data directory not initialised".into())
        })?;
        (dir, guard.master.clone())
    };

    let updated =
        store::set_auto_confirm(&app_dir, &steam_ids, market, trade, master.as_deref())?;
    Ok(updated.iter().map(AccountSummaryIpc::from).collect())
}

/// Persist the auto-confirm poll interval (seconds). Clamped to `MIN_POLL_INTERVAL`.
/// Returns the value actually stored.
#[tauri::command]
pub async fn set_poll_interval(
    seconds: u32,
    state: State<'_, Mutex<AppState>>,
) -> Result<u32, AppError> {
    let (app_dir, master) = {
        let guard = state.lock().await;
        let dir = guard.app_data_dir.clone().ok_or_else(|| {
            AppError::SteamError("app data directory not initialised".into())
        })?;
        (dir, guard.master.clone())
    };

    let clamped = seconds.max(MIN_POLL_INTERVAL);
    store::set_poll_interval(&app_dir, clamped, master.as_deref())?;
    Ok(clamped)
}

/// Read the current auto-confirm poll interval (seconds).
#[tauri::command]
pub async fn get_poll_interval(
    state: State<'_, Mutex<AppState>>,
) -> Result<u32, AppError> {
    let (app_dir, master) = {
        let guard = state.lock().await;
        let dir = guard.app_data_dir.clone().ok_or_else(|| {
            AppError::SteamError("app data directory not initialised".into())
        })?;
        (dir, guard.master.clone())
    };

    let vault = store::load_vault(&app_dir, master.as_deref())?;
    Ok(vault.poll_interval_secs)
}

/// List all account groups.
#[tauri::command]
pub async fn list_groups(
    state: State<'_, Mutex<AppState>>,
) -> Result<Vec<GroupIpc>, AppError> {
    let (app_dir, master) = app_dir_and_master(&state).await?;
    let groups = store::list_groups(&app_dir, master.as_deref())?;
    Ok(groups.iter().map(GroupIpc::from).collect())
}

/// Create an empty group (no-op if it already exists).
#[tauri::command]
pub async fn create_group(
    name: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<(), AppError> {
    let (app_dir, master) = app_dir_and_master(&state).await?;
    store::create_group(&app_dir, &name, master.as_deref())?;
    Ok(())
}

/// Delete a group.
#[tauri::command]
pub async fn delete_group(
    name: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<(), AppError> {
    let (app_dir, master) = app_dir_and_master(&state).await?;
    store::delete_group(&app_dir, &name, master.as_deref())?;
    Ok(())
}

/// Add an account to a group (creates the group if missing).
#[tauri::command]
pub async fn add_to_group(
    steam_id: String,
    name: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<(), AppError> {
    let (app_dir, master) = app_dir_and_master(&state).await?;
    store::add_to_group(&app_dir, &steam_id, &name, master.as_deref())?;
    Ok(())
}

/// Remove an account from a group.
#[tauri::command]
pub async fn remove_from_group(
    steam_id: String,
    name: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<(), AppError> {
    let (app_dir, master) = app_dir_and_master(&state).await?;
    store::remove_from_group(&app_dir, &steam_id, &name, master.as_deref())?;
    Ok(())
}

/// Export a standard maFile JSON for an account (explicit user action). Returns
/// secret material — the only command that does.
#[tauri::command]
pub async fn export_mafile(
    steam_id: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<String, AppError> {
    let (app_dir, master) = app_dir_and_master(&state).await?;
    let account = load_account(&steam_id, &app_dir, master.as_deref()).await?;
    Ok(crate::vault::mafile::export_json(&account))
}

/// Export one or more accounts' plaintext maFiles to `dest_path` (explicit user
/// action — returns secret material to disk, like `export_mafile`).
///
/// - Exactly one id → write the plaintext maFile JSON to `dest_path`.
/// - Many ids → write a single ZIP to `dest_path`, one `<mafile_name>.maFile`
///   entry per account.
///
/// Returns the written `dest_path` for the frontend to surface.
#[tauri::command]
pub async fn export_mafiles(
    steam_ids: Vec<String>,
    dest_path: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<String, AppError> {
    if steam_ids.is_empty() {
        return Err(AppError::SteamError("no accounts selected for export".into()));
    }
    let (app_dir, master) = app_dir_and_master(&state).await?;
    let dest = std::path::Path::new(&dest_path);

    // Single account → plaintext maFile straight to the chosen path.
    if steam_ids.len() == 1 {
        let account = load_account(&steam_ids[0], &app_dir, master.as_deref()).await?;
        crate::vault::export::write_single(dest, &account)
            .map_err(|e| AppError::SteamError(format!("cannot write maFile: {e}")))?;
        return Ok(dest_path);
    }

    // Many → resolve each account + its on-disk maFile name for ZIP entries.
    let vault = store::load_vault(&app_dir, master.as_deref())?;
    let mut items: Vec<(String, crate::vault::model::Account)> = Vec::with_capacity(steam_ids.len());
    for steam_id in &steam_ids {
        let account = load_account(steam_id, &app_dir, master.as_deref()).await?;
        let name = vault
            .accounts
            .iter()
            .find(|a| &a.steam_id == steam_id)
            .map(|a| a.mafile_name.clone())
            .filter(|n| !n.is_empty())
            .unwrap_or_else(|| format!("{steam_id}.maFile"));
        items.push((name, account));
    }
    crate::vault::export::write_zip(dest, &items)
        .map_err(|e| AppError::SteamError(format!("cannot write ZIP: {e}")))?;
    Ok(dest_path)
}

/// Read the persisted UI settings (works before the vault is unlocked).
#[tauri::command]
pub async fn get_settings(
    state: State<'_, Mutex<AppState>>,
) -> Result<AppSettingsIpc, AppError> {
    let (app_dir, _) = app_dir_and_master(&state).await?;
    let s = crate::settings::load(&app_dir);
    Ok((&s).into())
}

/// Update UI settings. Each `None` argument leaves that field unchanged. Also
/// refreshes the in-memory `minimize_to_tray` mirror. Returns the stored settings.
#[tauri::command]
pub async fn set_settings(
    language: Option<String>,
    minimize_to_tray: Option<bool>,
    accent_hue: Option<u32>,
    mafile_naming: Option<String>,
    common_mafile_format: Option<bool>,
    state: State<'_, Mutex<AppState>>,
) -> Result<AppSettingsIpc, AppError> {
    let (app_dir, _) = app_dir_and_master(&state).await?;
    let mut s = crate::settings::load(&app_dir);
    if let Some(l) = language {
        s.language = l;
    }
    if let Some(m) = minimize_to_tray {
        s.minimize_to_tray = m;
    }
    if let Some(h) = accent_hue {
        s.accent_hue = h;
    }
    if let Some(n) = mafile_naming {
        s.mafile_naming = n;
    }
    if let Some(c) = common_mafile_format {
        s.common_mafile_format = c;
    }
    crate::settings::save(&app_dir, &s)
        .map_err(|e| AppError::SteamError(e.to_string()))?;
    {
        let mut guard = state.lock().await;
        guard.minimize_to_tray = s.minimize_to_tray;
    }
    Ok((&s).into())
}

/// Quit the application (Menu → Quit).
#[tauri::command]
pub fn quit_app(app: tauri::AppHandle) {
    app.exit(0);
}

/// Import any maFile dropped into the folder. Returns the refreshed account list.
#[tauri::command]
pub async fn rescan_mafiles(
    state: State<'_, Mutex<AppState>>,
) -> Result<Vec<AccountSummaryIpc>, AppError> {
    let (app_dir, master) = app_dir_and_master(&state).await?;
    store::reconcile_folder(&app_dir, master.as_deref())?;
    let vault = store::load_vault(&app_dir, master.as_deref())?;
    Ok(vault.accounts.iter().map(AccountSummaryIpc::from).collect())
}

/// Rename all maFiles to match the current naming mode + format. Returns the list.
#[tauri::command]
pub async fn rename_mafiles(
    state: State<'_, Mutex<AppState>>,
) -> Result<Vec<AccountSummaryIpc>, AppError> {
    let (app_dir, master) = app_dir_and_master(&state).await?;
    let s = crate::settings::load(&app_dir);
    let ext = if s.common_mafile_format { "maFile" } else { "json" };
    store::rename_all_mafiles(&app_dir, &s.mafile_naming, ext, master.as_deref())?;
    let vault = store::load_vault(&app_dir, master.as_deref())?;
    Ok(vault.accounts.iter().map(AccountSummaryIpc::from).collect())
}

/// Return the maFiles folder path (created if missing) so the frontend can open it.
#[tauri::command]
pub async fn mafiles_dir(
    state: State<'_, Mutex<AppState>>,
) -> Result<String, AppError> {
    let (app_dir, _) = app_dir_and_master(&state).await?;
    let dir = crate::vault::mafiles::ensure_dir(&app_dir)
        .map_err(|e| AppError::SteamError(e.to_string()))?;
    Ok(dir.to_string_lossy().to_string())
}

#[tauri::command]
pub async fn list_proxies(
    state: State<'_, Mutex<AppState>>,
) -> Result<Vec<ProxyIpc>, AppError> {
    let (app_dir, master) = app_dir_and_master(&state).await?;
    let proxies = store::list_proxies(&app_dir, master.as_deref())?;
    Ok(proxies.iter().map(ProxyIpc::from).collect())
}

#[tauri::command]
pub async fn add_proxies(
    text: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<Vec<ProxyIpc>, AppError> {
    let (app_dir, master) = app_dir_and_master(&state).await?;
    let parsed = crate::steam::proxy::parse_proxy_lines(&text);
    let updated = store::add_proxies(&app_dir, &parsed, master.as_deref())?;
    Ok(updated.iter().map(ProxyIpc::from).collect())
}

#[tauri::command]
pub async fn delete_proxy(id: String, state: State<'_, Mutex<AppState>>) -> Result<(), AppError> {
    let (app_dir, master) = app_dir_and_master(&state).await?;
    store::delete_proxy(&app_dir, &id, master.as_deref())?;
    Ok(())
}

#[tauri::command]
pub async fn set_proxy_favorite(
    id: String,
    favorite: bool,
    state: State<'_, Mutex<AppState>>,
) -> Result<(), AppError> {
    let (app_dir, master) = app_dir_and_master(&state).await?;
    store::set_proxy_favorite(&app_dir, &id, favorite, master.as_deref())?;
    Ok(())
}

#[tauri::command]
pub async fn set_default_proxy(
    id: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<(), AppError> {
    let (app_dir, master) = app_dir_and_master(&state).await?;
    store::set_default_proxy(&app_dir, &id, master.as_deref())?;
    Ok(())
}

#[tauri::command]
pub async fn get_default_proxy(
    state: State<'_, Mutex<AppState>>,
) -> Result<String, AppError> {
    let (app_dir, master) = app_dir_and_master(&state).await?;
    Ok(store::load_vault(&app_dir, master.as_deref())?.default_proxy)
}

#[tauri::command]
pub async fn assign_proxy(
    steam_id: String,
    id: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<(), AppError> {
    let (app_dir, master) = app_dir_and_master(&state).await?;
    store::assign_proxy(&app_dir, &steam_id, &id, master.as_deref())?;
    Ok(())
}

#[tauri::command]
pub async fn bulk_assign_proxy(
    steam_ids: Vec<String>,
    proxy_id: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<(), AppError> {
    let (app_dir, master) = app_dir_and_master(&state).await?;
    store::bulk_assign_proxy(&app_dir, &steam_ids, &proxy_id, master.as_deref())?;
    Ok(())
}

#[tauri::command]
pub async fn unpin_proxy(
    steam_id: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<(), AppError> {
    let (app_dir, master) = app_dir_and_master(&state).await?;
    store::unpin_proxy(&app_dir, &steam_id, master.as_deref())?;
    Ok(())
}

#[tauri::command]
pub async fn distribute_proxies(
    steam_ids: Vec<String>,
    state: State<'_, Mutex<AppState>>,
) -> Result<(), AppError> {
    let (app_dir, master) = app_dir_and_master(&state).await?;
    store::distribute_proxies(&app_dir, &steam_ids, master.as_deref())?;
    Ok(())
}

#[tauri::command]
pub async fn assign_proxies_by_text(
    text: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<AssignReportIpc, AppError> {
    let (app_dir, master) = app_dir_and_master(&state).await?;
    let report = store::assign_by_text(&app_dir, &text, master.as_deref())?;
    Ok(AssignReportIpc { assigned: report.assigned, unmatched: report.unmatched })
}

/// Return the current resolved data directory (where maFiles/, sessions/,
/// vault.json and settings.json live).
#[tauri::command]
pub async fn get_data_dir(
    state: State<'_, Mutex<AppState>>,
) -> Result<String, AppError> {
    let guard = state.lock().await;
    let dir = guard.app_data_dir.clone().ok_or_else(|| {
        AppError::SteamError("app data directory not initialised".into())
    })?;
    Ok(dir.to_string_lossy().to_string())
}

/// Move all data (maFiles/, sessions/, vault.json, settings.json) to `new_path`,
/// update the bootstrap pointer, and switch `AppState.app_data_dir`. Returns the
/// new path. On any failure the old location and pointer are left untouched.
#[tauri::command]
pub async fn set_data_dir(
    new_path: String,
    app: tauri::AppHandle,
    state: State<'_, Mutex<AppState>>,
) -> Result<String, AppError> {
    use tauri::Manager;

    // Snapshot the current data dir; drop the guard before doing disk work.
    let current = {
        let guard = state.lock().await;
        guard.app_data_dir.clone().ok_or_else(|| {
            AppError::SteamError("app data directory not initialised".into())
        })?
    };

    let target = std::path::PathBuf::from(&new_path);

    // No-op if the target is the same directory we already use.
    if target == current {
        return Ok(new_path);
    }

    // The FIXED dir owns the pointer regardless of where data currently lives.
    let fixed_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| AppError::SteamError(format!("no app data dir: {e}")))?;

    // 1. Copy + verify only. On failure the old location and pointer are intact.
    crate::vault::data_location::migrate(&current, &target)
        .map_err(|e| AppError::SteamError(e.to_string()))?;

    // 2. Point the bootstrap at the new location BEFORE removing originals.
    //    If this write fails the originals are still in place → consistent.
    crate::vault::data_location::write_pointer(&fixed_dir, &target)
        .map_err(|e| AppError::SteamError(e.to_string()))?;

    // 3. Now that the pointer is durable, remove the originals.
    //    A failure here is non-fatal: data is safe at the new location.
    let _ = crate::vault::data_location::remove_migrated_originals(&current);

    // 4. Switch the in-memory data dir under the Mutex and restart the watcher.
    {
        let mut guard = state.lock().await;
        guard.app_data_dir = Some(target.clone());
        // Dropping the old watcher (via assignment) stops the old watch.
        // There is an accepted transient where both old and new watchers may
        // briefly emit `accounts-changed` concurrently; this is harmless because
        // the frontend re-list is idempotent.
        match crate::watch::start(app.clone(), &target) {
            Ok(w) => guard.watcher = Some(w),
            Err(e) => eprintln!("watch: failed to restart on new data dir: {e}"),
        }
    }

    Ok(new_path)
}

/// Test a proxy by making a timed request to Steam through it.
#[tauri::command]
pub async fn check_proxy(line: String) -> Result<ProxyCheckIpc, AppError> {
    let proxy = match crate::steam::proxy::parse_proxy_line(&line) {
        Some(p) => p,
        None => {
            return Ok(ProxyCheckIpc { ok: false, latency_ms: None, error: Some("invalid proxy".into()) })
        }
    };
    let mut b = reqwest::Client::builder().timeout(std::time::Duration::from_secs(8));
    if let Ok(mut rp) = reqwest::Proxy::all(format!("{}://{}:{}", proxy.scheme, proxy.host, proxy.port)) {
        if !proxy.user.is_empty() {
            rp = rp.basic_auth(&proxy.user, &proxy.pass);
        }
        b = b.proxy(rp);
    }
    let client = match b.build() {
        Ok(c) => c,
        Err(e) => return Ok(ProxyCheckIpc { ok: false, latency_ms: None, error: Some(e.to_string()) }),
    };
    let start = std::time::Instant::now();
    match client.get("https://steamcommunity.com/").send().await {
        Ok(resp) => {
            let ms = start.elapsed().as_millis() as u32;
            if resp.status().is_success() || resp.status().is_redirection() {
                Ok(ProxyCheckIpc { ok: true, latency_ms: Some(ms), error: None })
            } else {
                Ok(ProxyCheckIpc {
                    ok: false,
                    latency_ms: Some(ms),
                    error: Some(format!("HTTP {}", resp.status().as_u16())),
                })
            }
        }
        Err(e) => Ok(ProxyCheckIpc { ok: false, latency_ms: None, error: Some(e.to_string()) }),
    }
}

// ── Authenticator-linking DTOs + commands ─────────────────────────────────────

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkLoginResult {
    pub status: String, // "ok" | "bad_credentials" | "rate_limited"
    pub needs_email_guard: bool,
}

#[derive(Serialize)]
pub struct LinkStatusResult {
    pub status: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkStartResult {
    pub status: String, // "code" | "need_phone" | "already_linked" | "rate_limited"
    pub confirm_type: Option<String>, // "sms" | "email"
    pub phone_hint: Option<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkAwaitResult {
    pub still_waiting: bool,
    pub seconds: Option<u32>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LinkFinalizeResult {
    pub status: String, // "done" | "wrong_code" | "time_sync_failed"
    pub revocation_code: Option<String>,
    pub mafile_name: Option<String>,
}

/// Resolve an optional proxy by its `id()` from the stored proxy list.
async fn resolve_link_proxy(
    state: &State<'_, Mutex<AppState>>,
    proxy_id: Option<String>,
) -> Result<Option<crate::vault::model::Proxy>, AppError> {
    let Some(id) = proxy_id else { return Ok(None) };
    let (app_dir, master) = app_dir_and_master(state).await?;
    let proxies = store::list_proxies(&app_dir, master.as_deref())?;
    Ok(proxies.into_iter().find(|p| p.id() == id))
}

#[tauri::command]
pub async fn link_begin_login(
    username: String,
    password: String,
    proxy_id: Option<String>,
    state: State<'_, Mutex<AppState>>,
) -> Result<LinkLoginResult, AppError> {
    let proxy = resolve_link_proxy(&state, proxy_id).await?;

    let proxy_for_blocking = proxy.clone();
    let result = tokio::task::spawn_blocking(move || {
        crate::steam::link::start_login(proxy_for_blocking.as_ref(), &username, &password)
    })
    .await
    .map_err(|e| AppError::SteamError(e.to_string()))?;

    match result {
        Ok(r) => {
            let needs = r.needs_email_guard;
            let mut guard = state.lock().await;
            guard.link_session = Some(crate::steam::link::LinkSession {
                proxy,
                login: if needs { Some(r.login) } else { None },
                tokens: r.tokens,
                steam_id: r.steam_id,
                linked: None,
            });
            Ok(LinkLoginResult { status: "ok".into(), needs_email_guard: needs })
        }
        Err(crate::steam::link::LinkError::BadCredentials) => {
            Ok(LinkLoginResult { status: "bad_credentials".into(), needs_email_guard: false })
        }
        Err(crate::steam::link::LinkError::RateLimited) => {
            Ok(LinkLoginResult { status: "rate_limited".into(), needs_email_guard: false })
        }
        Err(crate::steam::link::LinkError::Network(m)) => Err(AppError::SteamError(m)),
    }
}

#[tauri::command]
pub async fn link_submit_email_guard(
    code: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<LinkStatusResult, AppError> {
    // Take the UserLogin + proxy out under the lock.
    let (login, _proxy) = {
        let mut guard = state.lock().await;
        let sess = guard
            .link_session
            .as_mut()
            .ok_or_else(|| AppError::SteamError("no linking session".into()))?;
        let login = sess
            .login
            .take()
            .ok_or_else(|| AppError::SteamError("login step not active".into()))?;
        (login, sess.proxy.clone())
    };

    let result = tokio::task::spawn_blocking(move || {
        crate::steam::link::submit_email_and_poll(login, &code)
    })
    .await
    .map_err(|e| AppError::SteamError(e.to_string()))?;

    match result {
        Ok((tokens, steam_id)) => {
            let mut guard = state.lock().await;
            if let Some(sess) = guard.link_session.as_mut() {
                sess.tokens = Some(tokens);
                sess.steam_id = Some(steam_id);
            }
            Ok(LinkStatusResult { status: "ok".into() })
        }
        Err(crate::steam::link::LinkError::BadCredentials) => {
            Ok(LinkStatusResult { status: "bad_code".into() })
        }
        Err(crate::steam::link::LinkError::RateLimited) => {
            Ok(LinkStatusResult { status: "rate_limited".into() })
        }
        Err(crate::steam::link::LinkError::Network(m)) => Err(AppError::SteamError(m)),
    }
}

#[tauri::command]
pub async fn link_start(
    state: State<'_, Mutex<AppState>>,
) -> Result<LinkStartResult, AppError> {
    let (app_dir, _master) = app_dir_and_master(&state).await?;
    let (tokens, proxy) = {
        let guard = state.lock().await;
        let sess = guard
            .link_session
            .as_ref()
            .ok_or_else(|| AppError::SteamError("no linking session".into()))?;
        let tokens = sess
            .tokens
            .clone()
            .ok_or_else(|| AppError::SteamError("not logged in".into()))?;
        (tokens, sess.proxy.clone())
    };

    let proxy_b = proxy.clone();
    let outcome = tokio::task::spawn_blocking(move || {
        crate::steam::link::add_authenticator(&tokens, proxy_b.as_ref())
    })
    .await
    .map_err(|e| AppError::SteamError(e.to_string()))?
    .map_err(|e| match e {
        crate::steam::link::LinkError::RateLimited => AppError::SteamError("rate limited".into()),
        crate::steam::link::LinkError::Network(m) => AppError::SteamError(m),
        crate::steam::link::LinkError::BadCredentials => AppError::SteamError("bad credentials".into()),
    })?;

    use crate::steam::link::AddOutcome;
    match outcome {
        AddOutcome::Code(data) => {
            // ANTI-LOCKOUT: write the maFile to disk NOW (secret + revocation code),
            // before finalize. It is NOT yet registered in the manifest; finalize
            // registers it. If the app closes here, the file survives and reconcile
            // will pick it up so the user is never locked out.
            let settings = crate::settings::load(&app_dir);
            let ext = if settings.common_mafile_format { "maFile" } else { "json" };
            crate::vault::mafiles::write(&app_dir, &data.account, &settings.mafile_naming, ext)
                .map_err(|e| AppError::SteamError(format!("write maFile: {e}")))?;

            let confirm = data.confirm_type.as_str().to_string();
            let hint = data.phone_hint.clone();
            {
                let mut guard = state.lock().await;
                if let Some(sess) = guard.link_session.as_mut() {
                    sess.linked = Some(data);
                }
            }
            Ok(LinkStartResult {
                status: "code".into(),
                confirm_type: Some(confirm),
                phone_hint: Some(hint),
            })
        }
        AddOutcome::NeedPhone => Ok(LinkStartResult {
            status: "need_phone".into(),
            confirm_type: None,
            phone_hint: None,
        }),
        AddOutcome::AlreadyLinked => Ok(LinkStartResult {
            status: "already_linked".into(),
            confirm_type: None,
            phone_hint: None,
        }),
        AddOutcome::RateLimited => Ok(LinkStartResult {
            status: "rate_limited".into(),
            confirm_type: None,
            phone_hint: None,
        }),
        AddOutcome::Failed(m) => Err(AppError::SteamError(m)),
    }
}

/// Snapshot tokens + proxy from the active linking session.
async fn link_tokens_proxy(
    state: &State<'_, Mutex<AppState>>,
) -> Result<(steamguard::token::Tokens, Option<crate::vault::model::Proxy>), AppError> {
    let guard = state.lock().await;
    let sess = guard
        .link_session
        .as_ref()
        .ok_or_else(|| AppError::SteamError("no linking session".into()))?;
    let tokens = sess
        .tokens
        .clone()
        .ok_or_else(|| AppError::SteamError("not logged in".into()))?;
    Ok((tokens, sess.proxy.clone()))
}

#[tauri::command]
pub async fn link_set_phone(
    phone: String,
    state: State<'_, Mutex<AppState>>,
) -> Result<LinkStatusResult, AppError> {
    let (tokens, proxy) = link_tokens_proxy(&state).await?;
    let ok = tokio::task::spawn_blocking(move || {
        crate::steam::link::set_phone(&tokens, proxy.as_ref(), &phone)
    })
    .await
    .map_err(|e| AppError::SteamError(e.to_string()))?
    .map_err(|e| AppError::SteamError(format!("{e:?}")))?;
    Ok(LinkStatusResult { status: if ok { "ok".into() } else { "failed".into() } })
}

#[tauri::command]
pub async fn link_await_phone_email(
    state: State<'_, Mutex<AppState>>,
) -> Result<LinkAwaitResult, AppError> {
    let (tokens, proxy) = link_tokens_proxy(&state).await?;
    let seconds = tokio::task::spawn_blocking(move || {
        crate::steam::link::await_phone_email(&tokens, proxy.as_ref())
    })
    .await
    .map_err(|e| AppError::SteamError(e.to_string()))?
    .map_err(|e| AppError::SteamError(format!("{e:?}")))?;
    Ok(LinkAwaitResult { still_waiting: seconds.is_some(), seconds })
}

#[tauri::command]
pub async fn link_send_sms(
    state: State<'_, Mutex<AppState>>,
) -> Result<LinkStatusResult, AppError> {
    let (tokens, proxy) = link_tokens_proxy(&state).await?;
    let ok = tokio::task::spawn_blocking(move || {
        crate::steam::link::send_sms(&tokens, proxy.as_ref())
    })
    .await
    .map_err(|e| AppError::SteamError(e.to_string()))?
    .map_err(|e| AppError::SteamError(format!("{e:?}")))?;
    Ok(LinkStatusResult { status: if ok { "ok".into() } else { "failed".into() } })
}

#[tauri::command]
pub async fn link_finalize(
    code: String,
    app: tauri::AppHandle,
    state: State<'_, Mutex<AppState>>,
) -> Result<LinkFinalizeResult, AppError> {
    let (app_dir, master) = app_dir_and_master(&state).await?;

    // Snapshot everything finalize needs.
    let (tokens, steam_id, secret_bytes, confirm_type, account, proxy) = {
        let guard = state.lock().await;
        let sess = guard
            .link_session
            .as_ref()
            .ok_or_else(|| AppError::SteamError("no linking session".into()))?;
        let tokens = sess.tokens.clone().ok_or_else(|| AppError::SteamError("not logged in".into()))?;
        let steam_id = sess.steam_id.ok_or_else(|| AppError::SteamError("no steam id".into()))?;
        let linked = sess.linked.as_ref().ok_or_else(|| AppError::SteamError("authenticator not created".into()))?;
        (
            tokens,
            steam_id,
            linked.shared_secret_bytes.clone(),
            linked.confirm_type,
            linked.account.clone(),
            sess.proxy.clone(),
        )
    };

    let tokens_b = tokens.clone();
    let proxy_b = proxy.clone();
    let result = tokio::task::spawn_blocking(move || {
        crate::steam::link::finalize(&tokens_b, steam_id, &secret_bytes, confirm_type, &code, proxy_b.as_ref())
    })
    .await
    .map_err(|e| AppError::SteamError(e.to_string()))?
    .map_err(|e| match e {
        crate::steam::link::LinkError::RateLimited => AppError::SteamError("rate limited".into()),
        crate::steam::link::LinkError::Network(m) => AppError::SteamError(m),
        crate::steam::link::LinkError::BadCredentials => AppError::SteamError("bad credentials".into()),
    })?;

    use crate::steam::link::FinalizeResult;
    match result {
        FinalizeResult::Done => {
            // Register the account in the manifest (writes/overwrites the maFile too).
            let settings = crate::settings::load(&app_dir);
            let ext = if settings.common_mafile_format { "maFile" } else { "json" };
            let mafile_name = store::add_account(&app_dir, &account, &settings.mafile_naming, ext, master.as_deref())?;

            // Persist the session refresh token so confirmations work immediately.
            let refresh = tokens.refresh_token().expose_secret().to_owned();
            let _ = crate::steam::session_store::save(&app_dir, &steam_id.to_string(), &refresh);

            // Clear the session and notify the UI.
            {
                let mut guard = state.lock().await;
                guard.link_session = None;
            }
            use tauri::Emitter;
            let _ = app.emit("accounts-changed", ());

            Ok(LinkFinalizeResult {
                status: "done".into(),
                revocation_code: Some(account.revocation_code.clone()),
                mafile_name: Some(mafile_name),
            })
        }
        FinalizeResult::WrongCode => Ok(LinkFinalizeResult {
            status: "wrong_code".into(),
            revocation_code: None,
            mafile_name: None,
        }),
        FinalizeResult::TimeSyncFailed => Ok(LinkFinalizeResult {
            status: "time_sync_failed".into(),
            revocation_code: None,
            mafile_name: None,
        }),
    }
}

#[tauri::command]
pub async fn link_cancel(state: State<'_, Mutex<AppState>>) -> Result<(), AppError> {
    let mut guard = state.lock().await;
    guard.link_session = None;
    Ok(())
}

/// Write a revocation code to a user-chosen path (from the Save dialog).
#[tauri::command]
pub async fn link_save_revocation(path: String, code: String) -> Result<(), AppError> {
    std::fs::write(&path, code).map_err(|e| AppError::SteamError(format!("write file: {e}")))
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seconds_remaining_at_exact_boundary() {
        // now % 30 == 0 → window just rotated → 30 seconds remain.
        assert_eq!(seconds_remaining(0), 30);
        assert_eq!(seconds_remaining(30), 30);
        assert_eq!(seconds_remaining(60), 30);
        // 1_600_000_020 / 30 = 53_333_334 exactly.
        assert_eq!(seconds_remaining(1_600_000_020), 30);
    }

    #[test]
    fn seconds_remaining_at_one_second_in() {
        // now % 30 == 1 → 29 seconds remain.
        assert_eq!(seconds_remaining(1), 29);
        assert_eq!(seconds_remaining(31), 29);
        // 1_600_000_001 % 30 = 11, so seconds_remaining = 19.
        assert_eq!(1_600_000_001u64 % 30, 11);
        assert_eq!(seconds_remaining(1_600_000_001), 19);
    }

    #[test]
    fn seconds_remaining_at_29_seconds_in() {
        // now % 30 == 29 → 1 second remains.
        assert_eq!(seconds_remaining(29), 1);
        assert_eq!(seconds_remaining(59), 1);
    }

    #[test]
    fn seconds_remaining_mid_window() {
        // 1_600_000_000 % 30 = 10 → 20 seconds remain.
        assert_eq!(1_600_000_000u64 % 30, 10);
        assert_eq!(seconds_remaining(1_600_000_000), 20);
    }

    #[test]
    fn seconds_remaining_always_in_range_1_to_30() {
        for i in 0u64..90 {
            let s = seconds_remaining(i);
            assert!(
                (1..=30).contains(&s),
                "seconds_remaining({i}) = {s} is out of range [1, 30]"
            );
        }
    }

}
