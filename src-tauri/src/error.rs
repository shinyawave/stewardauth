// SPDX-FileCopyrightText: 2026 shinyawave
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Typed IPC error that crosses the Tauri command boundary.
//!
//! `AppError` serialises to `{ "kind": "VariantName", "message": "..." }` so
//! the TypeScript frontend can `switch (err.kind)` on the discriminant.

use serde::Serialize;

use crate::steam::{
    confirmations::ConfError,
    login_approve::ApproveError,
    session::SessionError,
    time::TimeError,
    totp::TotpError,
};
use crate::vault::{
    encryption::EncError,
    mafile::MaFileError,
    store::VaultError,
};

// ── AppError enum ─────────────────────────────────────────────────────────────

/// All errors that can be returned from a Tauri IPC command.
#[derive(Debug)]
pub enum AppError {
    /// Network / HTTP failure (reqwest, Steam connectivity).
    Network,
    /// Wrong password, bad credentials, or failed decryption.
    InvalidPassword,
    /// The .maFile could not be parsed (missing fields, bad JSON).
    CorruptMaFile,
    /// The Steam session has expired or was revoked.
    SessionExpired,
    /// Steam has rate-limited this account.
    RateLimited,
    /// A Steam-side error with a descriptive message.
    SteamError(String),
}

// ── Serialize → { "kind": "...", "message": "..." } ──────────────────────────

impl Serialize for AppError {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        let mut map = s.serialize_map(Some(2))?;
        let (kind, message): (&str, String) = match self {
            AppError::Network => ("Network", "network error".into()),
            AppError::InvalidPassword => ("InvalidPassword", "invalid password or bad credentials".into()),
            AppError::CorruptMaFile => ("CorruptMaFile", "corrupt or unreadable .maFile".into()),
            AppError::SessionExpired => ("SessionExpired", "Steam session has expired".into()),
            AppError::RateLimited => ("RateLimited", "rate limited by Steam".into()),
            AppError::SteamError(msg) => ("SteamError", msg.clone()),
        };
        map.serialize_entry("kind", kind)?;
        map.serialize_entry("message", &message)?;
        map.end()
    }
}

// ── From impls ────────────────────────────────────────────────────────────────

impl From<TotpError> for AppError {
    fn from(e: TotpError) -> Self {
        match e {
            TotpError::Decode => AppError::CorruptMaFile,
        }
    }
}

impl From<MaFileError> for AppError {
    fn from(e: MaFileError) -> Self {
        match e {
            MaFileError::Json | MaFileError::MissingField(_) => AppError::CorruptMaFile,
            MaFileError::Decrypt => AppError::InvalidPassword,
        }
    }
}

impl From<SessionError> for AppError {
    fn from(e: SessionError) -> Self {
        match e {
            SessionError::BadCredentials => AppError::InvalidPassword,
            SessionError::GuardRequired => {
                AppError::SteamError("Steam Guard code required".into())
            }
            SessionError::RateLimited => AppError::RateLimited,
            SessionError::Network(msg) => AppError::SteamError(msg),
        }
    }
}

impl From<ConfError> for AppError {
    fn from(e: ConfError) -> Self {
        match e {
            ConfError::InvalidSession => AppError::SessionExpired,
            ConfError::Network(msg) => AppError::SteamError(msg),
            ConfError::RemoteFailure(msg) => AppError::SteamError(msg),
            ConfError::InvalidSteamId(msg) => AppError::SteamError(msg),
            ConfError::Config(msg) => AppError::SteamError(msg),
        }
    }
}

impl From<ApproveError> for AppError {
    fn from(e: ApproveError) -> Self {
        match e {
            ApproveError::InvalidSession => AppError::SessionExpired,
            ApproveError::Network(msg) => AppError::SteamError(msg),
            ApproveError::RemoteFailure(msg) => AppError::SteamError(msg),
            ApproveError::InvalidClientId(msg) => AppError::SteamError(msg),
            ApproveError::Config(msg) => AppError::SteamError(msg),
        }
    }
}

impl From<VaultError> for AppError {
    fn from(e: VaultError) -> Self {
        match e {
            // Both "wrong password" and "no password supplied for an encrypted
            // vault" should trigger the frontend unlock gate (kind=InvalidPassword).
            VaultError::WrongPasswordOrCorrupt => AppError::InvalidPassword,
            VaultError::MasterPasswordRequired => AppError::InvalidPassword,
            VaultError::Io(e) => AppError::SteamError(format!("vault io error: {e}")),
            VaultError::Json(e) => AppError::SteamError(format!("vault json error: {e}")),
            VaultError::NoProxies => AppError::SteamError("no proxies available".into()),
        }
    }
}

impl From<TimeError> for AppError {
    fn from(e: TimeError) -> Self {
        match e {
            TimeError::Network => AppError::Network,
            TimeError::Parse => AppError::SteamError("failed to parse Steam time response".into()),
        }
    }
}

impl From<EncError> for AppError {
    fn from(e: EncError) -> Self {
        match e {
            EncError::WrongPasswordOrCorrupt => AppError::InvalidPassword,
            EncError::Encrypt | EncError::KeyDerivation => {
                AppError::SteamError(e.to_string())
            }
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn ser(e: AppError) -> Value {
        serde_json::to_value(e).expect("AppError must serialize")
    }

    #[test]
    fn session_expired_has_correct_kind() {
        let v = ser(AppError::SessionExpired);
        assert_eq!(v["kind"], "SessionExpired");
        assert_eq!(v["message"], "Steam session has expired");
    }

    #[test]
    fn network_error_has_correct_kind() {
        let v = ser(AppError::Network);
        assert_eq!(v["kind"], "Network");
    }

    #[test]
    fn invalid_password_has_correct_kind() {
        let v = ser(AppError::InvalidPassword);
        assert_eq!(v["kind"], "InvalidPassword");
    }

    #[test]
    fn corrupt_mafile_has_correct_kind() {
        let v = ser(AppError::CorruptMaFile);
        assert_eq!(v["kind"], "CorruptMaFile");
    }

    #[test]
    fn rate_limited_has_correct_kind() {
        let v = ser(AppError::RateLimited);
        assert_eq!(v["kind"], "RateLimited");
    }

    #[test]
    fn steam_error_carries_message() {
        let v = ser(AppError::SteamError("something went wrong".into()));
        assert_eq!(v["kind"], "SteamError");
        assert_eq!(v["message"], "something went wrong");
    }

    #[test]
    fn serialized_shape_is_two_field_object() {
        // Must have exactly "kind" and "message" — no extra fields.
        let v = ser(AppError::Network);
        let obj = v.as_object().expect("must be a JSON object");
        assert_eq!(obj.len(), 2);
        assert!(obj.contains_key("kind"));
        assert!(obj.contains_key("message"));
    }

    // ── From mapping tests ─────────────────────────────────────────────────────

    #[test]
    fn totp_decode_maps_to_corrupt_mafile() {
        let e: AppError = TotpError::Decode.into();
        assert!(matches!(e, AppError::CorruptMaFile));
    }

    #[test]
    fn mafile_json_maps_to_corrupt_mafile() {
        let e: AppError = MaFileError::Json.into();
        assert!(matches!(e, AppError::CorruptMaFile));
    }

    #[test]
    fn mafile_missing_field_maps_to_corrupt_mafile() {
        let e: AppError = MaFileError::MissingField("shared_secret").into();
        assert!(matches!(e, AppError::CorruptMaFile));
    }

    #[test]
    fn mafile_decrypt_maps_to_invalid_password() {
        let e: AppError = MaFileError::Decrypt.into();
        assert!(matches!(e, AppError::InvalidPassword));
    }

    #[test]
    fn session_bad_credentials_maps_to_invalid_password() {
        let e: AppError = SessionError::BadCredentials.into();
        assert!(matches!(e, AppError::InvalidPassword));
    }

    #[test]
    fn session_rate_limited_maps_to_rate_limited() {
        let e: AppError = SessionError::RateLimited.into();
        assert!(matches!(e, AppError::RateLimited));
    }

    #[test]
    fn conf_invalid_session_maps_to_session_expired() {
        let e: AppError = ConfError::InvalidSession.into();
        assert!(matches!(e, AppError::SessionExpired));
    }

    #[test]
    fn vault_wrong_password_maps_to_invalid_password() {
        let e: AppError = VaultError::WrongPasswordOrCorrupt.into();
        assert!(matches!(e, AppError::InvalidPassword));
    }

    #[test]
    fn time_network_maps_to_network() {
        let e: AppError = TimeError::Network.into();
        assert!(matches!(e, AppError::Network));
    }

    #[test]
    fn enc_wrong_password_maps_to_invalid_password() {
        let e: AppError = EncError::WrongPasswordOrCorrupt.into();
        assert!(matches!(e, AppError::InvalidPassword));
    }

    #[test]
    fn approve_invalid_session_maps_to_session_expired() {
        let e: AppError = ApproveError::InvalidSession.into();
        assert!(matches!(e, AppError::SessionExpired));
    }

    #[test]
    fn approve_network_maps_to_steam_error() {
        let e: AppError = ApproveError::Network("timeout".into()).into();
        assert!(matches!(e, AppError::SteamError(_)));
    }

    #[test]
    fn approve_remote_failure_maps_to_steam_error() {
        let e: AppError = ApproveError::RemoteFailure("expired".into()).into();
        assert!(matches!(e, AppError::SteamError(_)));
    }
}
