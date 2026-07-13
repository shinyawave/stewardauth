// SPDX-FileCopyrightText: 2026 shinyawave
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Steam login-approval — approve/deny pending device-confirmation login
//! sessions via `steamguard::LoginApprover` (WebAPI `IAuthenticationService`).
//!
//! Mirrors the shape of `confirmations.rs`: an IPC-safe DTO, an error enum that
//! maps into `AppError`, and async `list_pending` / `respond` functions that run
//! the blocking `steamguard` calls inside `tokio::task::spawn_blocking` through
//! the account's proxy client.
//!
//! Unlike `confirmations.rs` (our own `mobileconf` client), approval endpoints
//! are the WebAPI `IAuthenticationService` methods `steamguard` already wraps, so
//! we reuse `LoginApprover` directly.

use serde::Serialize;
use steamguard::protobufs::steammessages_auth_steamclient::{
    CAuthentication_GetAuthSessionInfo_Response, EAuthTokenPlatformType,
};
use steamguard::ApproverError;

use crate::steam::session::SteamSession;
use crate::vault::model::Account;

// ── Error ─────────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum ApproveError {
    /// Session tokens are missing or rejected by Steam (Unauthorized).
    InvalidSession,
    /// Network / transport failure.
    Network(String),
    /// Steam server returned a non-OK EResult (expired/duplicate/unknown).
    RemoteFailure(String),
    /// client_id string is not a valid u64.
    #[allow(dead_code)]
    InvalidClientId(String),
    /// Local configuration error (bad shared_secret, bad steam_id format).
    #[allow(dead_code)]
    Config(String),
}

impl std::fmt::Display for ApproveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ApproveError::InvalidSession => write!(f, "invalid or missing session tokens"),
            ApproveError::Network(e) => write!(f, "network error: {e}"),
            ApproveError::RemoteFailure(e) => write!(f, "Steam remote failure: {e}"),
            ApproveError::InvalidClientId(e) => write!(f, "invalid client_id: {e}"),
            ApproveError::Config(e) => write!(f, "configuration error: {e}"),
        }
    }
}

impl std::error::Error for ApproveError {}

impl From<ApproverError> for ApproveError {
    fn from(e: ApproverError) -> Self {
        match e {
            ApproverError::Unauthorized => ApproveError::InvalidSession,
            ApproverError::Expired | ApproverError::DuplicateRequest => {
                ApproveError::RemoteFailure(e.to_string())
            }
            ApproverError::TransportError(t) => ApproveError::Network(t.to_string()),
            other => ApproveError::RemoteFailure(other.to_string()),
        }
    }
}

// ── IPC-safe output type ────────────────────────────────────────────────────────

/// A single pending login session — safe to send over IPC. No secrets.
///
/// `client_id` is a u64 serialized as a **string** to avoid JS 53-bit precision
/// loss; it is the token used to approve/deny via [`respond`].
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PendingLogin {
    /// Pending session client id (u64 as string).
    pub client_id: String,
    /// Device friendly name (may be empty).
    pub device: String,
    /// Best-effort "City, Country" (either part may be missing).
    pub location: String,
    /// Best-effort requestor IP (may be empty).
    pub ip: String,
    /// Human platform label: "Steam Client", "Web Browser", "Mobile App", "Unknown".
    pub platform: String,
}

/// Map the steamguard `EAuthTokenPlatformType` enum to a stable human label.
fn platform_label(p: EAuthTokenPlatformType) -> &'static str {
    match p {
        EAuthTokenPlatformType::k_EAuthTokenPlatformType_SteamClient => "Steam Client",
        EAuthTokenPlatformType::k_EAuthTokenPlatformType_WebBrowser => "Web Browser",
        EAuthTokenPlatformType::k_EAuthTokenPlatformType_MobileApp => "Mobile App",
        _ => "Unknown",
    }
}

/// Join city + country into a best-effort "City, Country", dropping empty parts.
fn location_of(city: &str, country: &str) -> String {
    match (city.trim(), country.trim()) {
        ("", "") => String::new(),
        (c, "") => c.to_owned(),
        ("", n) => n.to_owned(),
        (c, n) => format!("{c}, {n}"),
    }
}

/// Build an IPC-safe [`PendingLogin`] from a client_id + its session info.
fn to_pending(client_id: u64, info: &CAuthentication_GetAuthSessionInfo_Response) -> PendingLogin {
    PendingLogin {
        client_id: client_id.to_string(),
        device: info.device_friendly_name().to_owned(),
        location: location_of(info.city(), info.country()),
        ip: info.ip().to_owned(),
        platform: platform_label(info.platform_type()).to_owned(),
    }
}

// ── Build helpers ───────────────────────────────────────────────────────────────

/// Extract the data needed to drive `LoginApprover` from an account + session.
/// Analogous to `build_ctx` in `confirmations.rs`.
#[allow(dead_code)]
struct ApproverCtx {
    steam_id: u64,
    shared_secret: steamguard::token::TwoFactorSecret,
    tokens: steamguard::token::Tokens,
    proxy: Option<crate::vault::model::Proxy>,
}

#[allow(dead_code)]
fn build_approver_ctx(account: &Account, session: &SteamSession) -> Result<ApproverCtx, ApproveError> {
    let steam_id: u64 = account
        .steam_id
        .parse()
        .map_err(|e| ApproveError::Config(format!("{}: {e}", account.steam_id)))?;

    let shared_secret =
        steamguard::token::TwoFactorSecret::parse_shared_secret(account.shared_secret.clone())
            .map_err(|e| ApproveError::Config(format!("bad shared_secret: {e}")))?;

    Ok(ApproverCtx {
        steam_id,
        shared_secret,
        tokens: session.tokens().clone(),
        proxy: session.proxy().cloned(),
    })
}

// ── Public API ──────────────────────────────────────────────────────────────────

/// Fetch all pending login-approval sessions for the account.
///
/// Returns a list of [`PendingLogin`] structs — safe to send over IPC.
/// No secrets (tokens, shared_secret) are included.
pub async fn list_pending(
    account: &Account,
    session: &SteamSession,
) -> Result<Vec<PendingLogin>, ApproveError> {
    let ctx = build_approver_ctx(account, session)?;

    tokio::task::spawn_blocking(move || -> Result<Vec<PendingLogin>, ApproveError> {
        let client = crate::steam::proxy::build_blocking_client(ctx.proxy.as_ref());
        let transport = steamguard::transport::WebApiTransport::new(client);
        let approver = steamguard::approver::LoginApprover::new(transport, &ctx.tokens);

        let client_ids = approver.list_auth_sessions().map_err(ApproveError::from)?;

        let mut results = Vec::with_capacity(client_ids.len());
        for cid in client_ids {
            match approver.get_auth_session_info(cid) {
                Ok(info) => results.push(to_pending(cid, &info)),
                Err(e) => {
                    // Info lookup failed — push a degraded row so the user can still see
                    // and act on the pending session (respond() only needs client_id).
                    eprintln!("login_approve: get_auth_session_info({cid}) failed: {e}");
                    results.push(PendingLogin {
                        client_id: cid.to_string(),
                        device: String::new(),
                        location: String::new(),
                        ip: String::new(),
                        platform: String::new(),
                    });
                }
            }
        }
        Ok(results)
    })
    .await
    .map_err(|e| ApproveError::Network(format!("spawn_blocking panic: {e}")))?
}

/// Approve or deny a pending login session identified by `client_id_str`.
///
/// `client_id_str` is the string that was serialised in [`PendingLogin::client_id`].
/// `approve = true` → approve the session; `false` → deny it.
pub async fn respond(
    account: &Account,
    session: &SteamSession,
    client_id_str: &str,
    approve: bool,
) -> Result<(), ApproveError> {
    let client_id: u64 = client_id_str
        .parse()
        .map_err(|e| ApproveError::InvalidClientId(format!("{client_id_str}: {e}")))?;

    let ctx = build_approver_ctx(account, session)?;

    tokio::task::spawn_blocking(move || -> Result<(), ApproveError> {
        let client = crate::steam::proxy::build_blocking_client(ctx.proxy.as_ref());
        let transport = steamguard::transport::WebApiTransport::new(client);
        let mut approver = steamguard::approver::LoginApprover::new(transport, &ctx.tokens);

        // Build the SteamGuardAccount needed to sign the approval.
        let mut sga = steamguard::SteamGuardAccount::new();
        sga.steam_id = ctx.steam_id;
        sga.shared_secret = ctx.shared_secret;
        sga.tokens = Some(ctx.tokens.clone());

        let challenge = steamguard::approver::Challenge::new(1, client_id);

        if approve {
            approver
                .approve(
                    &sga,
                    challenge,
                    steamguard::protobufs::enums::ESessionPersistence::k_ESessionPersistence_Persistent,
                )
                .map_err(ApproveError::from)
        } else {
            approver.deny(&sga, challenge).map_err(ApproveError::from)
        }
    })
    .await
    .map_err(|e| ApproveError::Network(format!("spawn_blocking panic: {e}")))?
}

// ── Tests ───────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn location_of_joins_and_drops_empties() {
        assert_eq!(location_of("Berlin", "Germany"), "Berlin, Germany");
        assert_eq!(location_of("Berlin", ""), "Berlin");
        assert_eq!(location_of("", "Germany"), "Germany");
        assert_eq!(location_of("", ""), "");
        assert_eq!(location_of("  ", "  "), "");
    }

    #[test]
    fn platform_label_maps_known_variants() {
        assert_eq!(
            platform_label(EAuthTokenPlatformType::k_EAuthTokenPlatformType_MobileApp),
            "Mobile App"
        );
        assert_eq!(
            platform_label(EAuthTokenPlatformType::k_EAuthTokenPlatformType_WebBrowser),
            "Web Browser"
        );
        assert_eq!(
            platform_label(EAuthTokenPlatformType::k_EAuthTokenPlatformType_Unknown),
            "Unknown"
        );
    }

    #[test]
    fn to_pending_maps_fields_and_stringifies_client_id() {
        let mut info = CAuthentication_GetAuthSessionInfo_Response::new();
        info.set_ip("203.0.113.7".to_owned());
        info.set_city("Berlin".to_owned());
        info.set_country("Germany".to_owned());
        info.set_device_friendly_name("Chrome on macOS".to_owned());
        info.set_platform_type(EAuthTokenPlatformType::k_EAuthTokenPlatformType_WebBrowser);

        // A value that overflows JS's 53-bit safe integer range.
        let client_id: u64 = 2372462679780599330;
        let p = to_pending(client_id, &info);

        assert_eq!(p.client_id, "2372462679780599330");
        assert_eq!(p.device, "Chrome on macOS");
        assert_eq!(p.location, "Berlin, Germany");
        assert_eq!(p.ip, "203.0.113.7");
        assert_eq!(p.platform, "Web Browser");
    }

    #[test]
    fn client_id_string_round_trips_full_u64() {
        let raw: u64 = u64::MAX - 3;
        let p = to_pending(raw, &CAuthentication_GetAuthSessionInfo_Response::new());
        let parsed: u64 = p.client_id.parse().expect("client_id must round-trip");
        assert_eq!(parsed, raw, "u64 client_id must survive the string round-trip");
    }

    #[test]
    fn approver_unauthorized_maps_to_invalid_session() {
        let mapped: ApproveError = ApproverError::Unauthorized.into();
        assert!(matches!(mapped, ApproveError::InvalidSession));
    }

    /// Live integration test — skipped in CI; run manually with:
    ///   STEAM_STEAM_ID=<steamid64> STEAM_APP_DIR=<path> \
    ///     cargo test --lib login_approve -- --ignored
    ///
    /// Restores the file-based session for the given SteamID, loads the account
    /// from the vault maFile, calls `list_pending`, and prints the count.
    /// Never approves or denies anything.
    #[tokio::test]
    #[ignore = "requires live Steam session"]
    async fn live_list_pending() {
        let steam_id_str = match std::env::var("STEAM_STEAM_ID") {
            Ok(v) if !v.is_empty() => v,
            _ => {
                println!("live_list_pending: STEAM_STEAM_ID not set — skipping");
                return;
            }
        };
        let app_dir_str = match std::env::var("STEAM_APP_DIR") {
            Ok(v) if !v.is_empty() => v,
            _ => {
                println!("live_list_pending: STEAM_APP_DIR not set — skipping");
                return;
            }
        };
        let app_dir = std::path::Path::new(&app_dir_str);

        let session = match crate::steam::session::restore_session(&steam_id_str, None, app_dir).await {
            Ok(s) => s,
            Err(e) => {
                println!("live_list_pending: restore_session failed: {e} — skipping");
                return;
            }
        };

        // Locate the account maFile via the vault manifest.
        let vault = match crate::vault::store::load_vault(app_dir, None) {
            Ok(v) => v,
            Err(e) => {
                println!("live_list_pending: load_vault failed: {e} — skipping");
                return;
            }
        };
        let summary = match vault.accounts.iter().find(|a| a.steam_id == steam_id_str) {
            Some(s) => s,
            None => {
                println!("live_list_pending: steam_id {steam_id_str} not found in vault — skipping");
                return;
            }
        };
        let account = match crate::vault::mafiles::read(app_dir, &summary.mafile_name) {
            Ok(a) => a,
            Err(e) => {
                println!("live_list_pending: mafile read failed: {e:?} — skipping");
                return;
            }
        };

        match super::list_pending(&account, &session).await {
            Ok(pending) => println!("live_list_pending: {} pending login(s)", pending.len()),
            Err(e) => println!("live_list_pending: list_pending error: {e}"),
        }
    }
}
