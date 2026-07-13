// SPDX-FileCopyrightText: 2026 shinyawave
// SPDX-License-Identifier: AGPL-3.0-or-later

// ─── steamguard 0.18.1 API surface (verified from cargo doc + source) ───────
//
// UserLogin<T: Transport + Clone>
//   ::new(transport: T, device_details: DeviceDetails) -> Self
//   .begin_auth_via_credentials(&mut self, account_name: &str, password: &str)
//       -> Result<Vec<AllowedConfirmation>, LoginError>
//   .submit_steam_guard_code(&mut self, guard_type: EAuthSessionGuardType, code: String)
//       -> Result<CAuthentication_UpdateAuthSessionWithSteamGuardCode_Response, UpdateAuthSessionError>
//   .poll_until_tokens(&mut self) -> anyhow::Result<Tokens>
//
// DeviceDetails { friendly_name: String, platform_type: EAuthTokenPlatformType,
//                 os_type: i32, gaming_device_type: u32 }
//
// EAuthTokenPlatformType::k_EAuthTokenPlatformType_MobileApp  (value = 3)
// EAuthSessionGuardType::k_EAuthSessionGuardType_DeviceCode   (value = 3)
//
// Tokens { access_token: Jwt, refresh_token: Jwt }
//   .access_token() -> &Jwt
//   .refresh_token() -> &Jwt
//
// Jwt
//   .decode() -> anyhow::Result<SteamJwtData>
//   .expose_secret() -> &str
//
// SteamJwtData { exp: u64, iat: u64, iss: String, aud: Vec<String>,
//                sub: String, jti: String }
//   .steam_id() -> u64        (parses sub)
//
// TokenRefresher<T: Transport>
//   ::new(client: AuthenticationClient<T>) -> Self
//   .refresh(&mut self, steam_id: u64, tokens: &Tokens) -> Result<Jwt, anyhow::Error>
//
// WebApiTransport::new(client: reqwest::blocking::Client) -> Self
//
// LoginError variants: BadCredentials, TooManyAttempts, SessionExpired,
//     UnknownEResult, AuthAlreadyStarted, TransportError, NetworkFailure, OtherFailure
//
// IMPORTANT: UserLogin uses reqwest::blocking — it must run inside
//            tokio::task::spawn_blocking to avoid blocking the async executor.
// ────────────────────────────────────────────────────────────────────────────

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use steamguard::{
    protobufs::steammessages_auth_steamclient::{EAuthSessionGuardType, EAuthTokenPlatformType},
    token::Tokens,
    transport::WebApiTransport,
    userlogin::{DeviceDetails, LoginError, UserLogin},
};

use crate::vault::{keychain, model::{Account, Proxy}};

// ── Error ─────────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum SessionError {
    BadCredentials,
    GuardRequired,
    RateLimited,
    Network(String),
}

impl std::fmt::Display for SessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SessionError::BadCredentials => write!(f, "bad credentials"),
            SessionError::GuardRequired => write!(f, "Steam Guard code required"),
            SessionError::RateLimited => write!(f, "rate limited by Steam"),
            SessionError::Network(e) => write!(f, "network error: {e}"),
        }
    }
}

impl std::error::Error for SessionError {}

impl From<LoginError> for SessionError {
    fn from(e: LoginError) -> Self {
        match e {
            LoginError::BadCredentials => SessionError::BadCredentials,
            LoginError::TooManyAttempts => SessionError::RateLimited,
            LoginError::NetworkFailure(e) => SessionError::Network(e.to_string()),
            LoginError::TransportError(e) => SessionError::Network(e.to_string()),
            other => SessionError::Network(format!("login error: {other:?}")),
        }
    }
}

// ── Session ───────────────────────────────────────────────────────────────────

/// Holds the Steam access + refresh tokens obtained after a successful login.
///
/// The refresh token string is persisted in the macOS Keychain under the key
/// `"<steam_id>:session"` so it can survive process restarts.
/// Raw token strings are never surfaced to the frontend.
#[allow(dead_code)] // wired up in later tasks
pub struct SteamSession {
    /// The raw `Tokens` from steamguard (access + refresh JWTs).
    tokens: Tokens,
    /// Unix timestamp at which the *access* token expires (from JWT `exp` field).
    access_expires_at: u64,
    /// SteamID64 of the authenticated account.
    steam_id: u64,
    /// Proxy this session routes through (None = direct). Reused by refresh.
    proxy: Option<Proxy>,
    /// App data dir — where refresh persists the token file.
    app_dir: PathBuf,
}

impl SteamSession {
    /// Expose the raw [`Tokens`] so other steam modules (e.g. `confirmations`)
    /// can build a `SteamGuardAccount` with `.tokens = Some(...)`.
    ///
    /// Tokens contain secrets — callers must NOT send them to the frontend.
    pub(crate) fn tokens(&self) -> &Tokens {
        &self.tokens
    }

    pub(crate) fn proxy(&self) -> Option<&Proxy> {
        self.proxy.as_ref()
    }

    /// App data dir for this session — used by `confirmations` to write a
    /// best-effort raw-response log when Steam rejects a request.
    pub(crate) fn app_dir(&self) -> &Path {
        &self.app_dir
    }

    /// Returns `true` if the access token has not yet expired.
    ///
    /// Uses wall clock (`SystemTime::now()`). Adds a 30-second margin so callers
    /// can refresh proactively before the token becomes invalid mid-request.
    #[allow(dead_code)] // wired up in later tasks
    pub fn is_valid(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        // 30-second safety margin
        self.access_expires_at > now.saturating_add(30)
    }

    /// Attempt to refresh the access token using the stored refresh token.
    ///
    /// On success the new access token is stored internally and the Keychain
    /// entry is updated.
    #[allow(dead_code)] // wired up in later tasks
    pub async fn refresh(&mut self) -> Result<(), SessionError> {
        let steam_id = self.steam_id;
        // Clone tokens so they can move into the blocking closure.
        let tokens_clone = self.tokens.clone();

        let proxy = self.proxy.clone();
        let new_tokens = tokio::task::spawn_blocking(move || -> Result<Tokens, SessionError> {
            let client = crate::steam::proxy::build_blocking_client(proxy.as_ref());
            let transport = WebApiTransport::new(client);

            let mut refresher = steamguard::refresher::TokenRefresher::new(
                steamguard::steamapi::AuthenticationClient::new(transport),
            );

            let new_access = refresher
                .refresh(steam_id, &tokens_clone)
                .map_err(|e| SessionError::Network(e.to_string()))?;

            // Build a new Tokens with the refreshed access token
            let mut updated = tokens_clone.clone();
            updated.set_access_token(new_access);
            Ok(updated)
        })
        .await
        .map_err(|e| SessionError::Network(e.to_string()))??;

        // Update expiry from the new access token
        let new_exp = new_tokens
            .access_token()
            .decode()
            .map(|d| d.exp)
            .unwrap_or(0);

        self.tokens = new_tokens;
        self.access_expires_at = new_exp;

        // Persist the refreshed token to the session file (best-effort).
        let refresh_str = self.tokens.refresh_token().expose_secret().to_owned();
        let _ = crate::steam::session_store::save(&self.app_dir, &steam_id.to_string(), &refresh_str);

        Ok(())
    }
}

// ── Login ─────────────────────────────────────────────────────────────────────

/// Perform a Steam credential login using the 2026 `IAuthenticationService`
/// protobuf protocol.
///
/// `guard_code` is the 5-character Steam Guard (TOTP) code generated from
/// `account.shared_secret`.  Pass an empty string only if the account has no
/// authenticator — Steam will then return an error which maps to
/// [`SessionError::GuardRequired`].
///
/// On success the refresh token is persisted to the macOS Keychain under
/// `"<steam_id>:session"` and the resulting [`SteamSession`] is returned.
///
/// # Security
/// Passwords and token strings are never logged or exposed to the frontend.
#[allow(dead_code)] // wired up in later tasks
pub async fn login(
    account: &Account,
    username: &str,
    password: &str,
    guard_code: &str,
    proxy: Option<&Proxy>,
    app_dir: &Path,
) -> Result<SteamSession, SessionError> {
    let username = username.to_owned();
    let password = password.to_owned();
    let guard_code = guard_code.to_owned();
    let device_id = account.device_id.clone();
    let proxy_owned = proxy.cloned();

    let tokens = tokio::task::spawn_blocking(move || -> Result<Tokens, SessionError> {
        let client = crate::steam::proxy::build_blocking_client(proxy_owned.as_ref());
        let transport = WebApiTransport::new(client);

        let device_details = DeviceDetails {
            // Use the account device_id as the human-readable name so sessions
            // are identifiable in the Steam app's "Manage Devices" list.
            friendly_name: format!("StewardAuth-{}", device_id),
            platform_type: EAuthTokenPlatformType::k_EAuthTokenPlatformType_MobileApp,
            // EOSType: -500 is the value Steam's mobile app uses for iOS/macOS.
            os_type: -500,
            // EGamingDeviceType: 1 = phone/mobile, matching MobileApp platform.
            gaming_device_type: 1,
        };

        let mut user_login = UserLogin::new(transport, device_details);

        // Step 1 — RSA encrypt + begin auth session.
        user_login
            .begin_auth_via_credentials(&username, &password)
            .map_err(SessionError::from)?;

        // Step 2 — Submit the TOTP guard code if one was provided.
        if !guard_code.is_empty() {
            user_login
                .submit_steam_guard_code(
                    EAuthSessionGuardType::k_EAuthSessionGuardType_DeviceCode,
                    guard_code,
                )
                .map_err(|e| match e {
                    steamguard::userlogin::UpdateAuthSessionError::TooManyAttempts => {
                        SessionError::RateLimited
                    }
                    steamguard::userlogin::UpdateAuthSessionError::SessionNotStarted => {
                        SessionError::GuardRequired
                    }
                    steamguard::userlogin::UpdateAuthSessionError::IncorrectSteamGuardCode => {
                        SessionError::BadCredentials
                    }
                    other => SessionError::Network(format!("guard code error: {other:?}")),
                })?;
        }

        // Step 3 — Poll until Steam issues the tokens.
        let tokens = user_login
            .poll_until_tokens()
            .map_err(|e| SessionError::Network(e.to_string()))?;

        Ok(tokens)
    })
    .await
    .map_err(|e| SessionError::Network(e.to_string()))??;

    // Decode the access token to extract the expiry and steam_id.
    let jwt_data = tokens
        .access_token()
        .decode()
        .map_err(|e| SessionError::Network(format!("failed to decode access token: {e}")))?;

    let steam_id = jwt_data.steam_id();
    let access_expires_at = jwt_data.exp;

    // Persist the refresh token to the session file (best-effort).
    let refresh_str = tokens.refresh_token().expose_secret().to_owned();
    let _ = crate::steam::session_store::save(app_dir, &steam_id.to_string(), &refresh_str);

    Ok(SteamSession {
        tokens,
        access_expires_at,
        steam_id,
        proxy: proxy.cloned(),
        app_dir: app_dir.to_path_buf(),
    })
}

// ── Session restore (survive process restarts) ─────────────────────────────────

/// Rebuild a [`SteamSession`] from the refresh token persisted in the Keychain
/// under `"<steam_id>:session"`, obtaining a fresh access token.
///
/// This is what lets a login survive an app restart: the user logs in once, and
/// on subsequent launches the stored refresh token is exchanged for a new access
/// token instead of prompting for a password again.
///
/// Fails (so the caller falls back to a fresh login) when:
/// - there is no saved refresh token for this account, or
/// - the stored refresh token is expired/revoked (e.g. the user signed the
///   session out, or it is genuinely stale) and the refresh call is rejected.
pub async fn restore_session(steam_id_str: &str, proxy: Option<&Proxy>, app_dir: &Path) -> Result<SteamSession, SessionError> {
    let steam_id: u64 = steam_id_str
        .parse()
        .map_err(|_| SessionError::Network("invalid steam_id".into()))?;

    // Prefer the session file; fall back to the Keychain ONCE and migrate it to a
    // file (so the user keeps their session without re-logging in, and the Keychain
    // is never read again after this).
    let refresh_str = match crate::steam::session_store::load(app_dir, steam_id_str) {
        Some(t) => t,
        None => {
            let key = format!("{steam_id}:session");
            let migrated = keychain::load_secrets(&key).ok();
            match migrated {
                Some(t) => {
                    let _ = crate::steam::session_store::save(app_dir, steam_id_str, &t);
                    t
                }
                None => return Err(SessionError::Network("no saved session".into())),
            }
        }
    };

    // Build Tokens with the stored refresh token. The access token is a
    // placeholder that `refresh()` immediately replaces with a real one.
    let refresh: steamguard::token::Jwt = refresh_str.clone().into();
    let access_placeholder: steamguard::token::Jwt = refresh_str.into();
    let tokens = Tokens::new(access_placeholder, refresh);

    let mut session = SteamSession {
        tokens,
        access_expires_at: 0, // expired → forces the refresh below
        steam_id,
        proxy: proxy.cloned(),
        app_dir: app_dir.to_path_buf(),
    };

    // Exchange the refresh token for a fresh access token. If the refresh token
    // is no longer valid, this errors and the caller requires a fresh login.
    session.refresh().await?;

    Ok(session)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Construct a SteamSession with a synthetic expiry and verify is_valid().
    ///
    /// We build the Tokens from raw strings because Jwt implements From<String>.
    fn make_session_with_exp(exp: u64) -> SteamSession {
        // Minimal valid-looking JWT strings (not actually decodable — we bypass
        // decode() and set access_expires_at directly in the struct).
        let access: steamguard::token::Jwt = "header.payload.sig".to_owned().into();
        let refresh: steamguard::token::Jwt = "header.payload.sig".to_owned().into();
        let tokens = Tokens::new(access, refresh);
        SteamSession {
            tokens,
            access_expires_at: exp,
            steam_id: 76561190000000001,
            proxy: None,
            app_dir: PathBuf::new(),
        }
    }

    #[test]
    fn is_valid_returns_true_for_far_future_expiry() {
        // Year 2100 in Unix time ≈ 4_102_444_800
        let session = make_session_with_exp(4_102_444_800);
        assert!(session.is_valid(), "session with year-2100 expiry should be valid");
    }

    #[test]
    fn is_valid_returns_false_for_past_expiry() {
        // Unix epoch — long expired.
        let session = make_session_with_exp(0);
        assert!(!session.is_valid(), "session with zero expiry should be invalid");
    }

    #[test]
    fn is_valid_returns_false_within_safety_margin() {
        // Expiry = now + 10s, which is inside the 30-second safety margin.
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let session = make_session_with_exp(now + 10);
        assert!(
            !session.is_valid(),
            "session expiring in 10s should be invalid due to 30s safety margin"
        );
    }

    #[test]
    fn is_valid_returns_true_beyond_safety_margin() {
        // Expiry = now + 120s, which is outside the 30-second safety margin.
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let session = make_session_with_exp(now + 120);
        assert!(
            session.is_valid(),
            "session expiring in 120s should be valid (beyond 30s margin)"
        );
    }

    /// Live-network login test — requires real Steam credentials and network.
    /// Gated behind #[ignore] so CI never executes it.
    #[tokio::test]
    #[ignore = "requires real Steam credentials and live network — run manually"]
    async fn live_login_round_trip() {
        // Fill in real credentials to test manually.
        let account = crate::vault::model::Account {
            steam_id: "76561190000000000".to_owned(),
            account_name: "testaccount".to_owned(),
            shared_secret: "AAAAAAAAAAAAAAAAAAAAAAAAAAAA".to_owned(),
            identity_secret: "AAAAAAAAAAAAAAAAAAAAAAAAAAAA".to_owned(),
            device_id: "android:00000000-0000-0000-0000-000000000000".to_owned(),
            revocation_code: "RAAAAA".to_owned(),
        };
        let username = std::env::var("STEAM_USERNAME").expect("STEAM_USERNAME not set");
        let password = std::env::var("STEAM_PASSWORD").expect("STEAM_PASSWORD not set");
        let guard_code = std::env::var("STEAM_GUARD_CODE").unwrap_or_default();

        let session = login(&account, &username, &password, &guard_code, None, std::path::Path::new("."))
            .await
            .expect("login should succeed");

        assert!(session.is_valid(), "freshly obtained session should be valid");
        assert!(session.steam_id > 0, "steam_id should be set");
    }
}
