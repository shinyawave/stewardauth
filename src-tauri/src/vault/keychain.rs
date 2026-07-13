// SPDX-FileCopyrightText: 2026 shinyawave
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Keychain-backed secret store for MacSDA.
//!
//! Stores per-account secrets (serialised JSON) in the macOS Keychain under
//! the service name `"MacSDA"` keyed by `steam_id`.

use keyring::Entry;

// ── Error type ────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum KeychainError {
    /// The requested item does not exist in the Keychain.
    NotFound,
    /// Any other Keychain / OS-level error.
    Other(keyring::Error),
}

impl std::fmt::Display for KeychainError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KeychainError::NotFound => write!(f, "keychain entry not found"),
            KeychainError::Other(e) => write!(f, "keychain error: {e}"),
        }
    }
}

impl std::error::Error for KeychainError {}

fn map_err(e: keyring::Error) -> KeychainError {
    match e {
        keyring::Error::NoEntry => KeychainError::NotFound,
        other => KeychainError::Other(other),
    }
}

// ── Public API ────────────────────────────────────────────────────────────────

const SERVICE: &str = "MacSDA";

/// Store `secrets_json` in the macOS Keychain for `steam_id`.
#[allow(dead_code)] // wired up in later tasks
pub fn store_secrets(steam_id: &str, secrets_json: &str) -> Result<(), KeychainError> {
    let entry = Entry::new(SERVICE, steam_id).map_err(map_err)?;
    entry.set_password(secrets_json).map_err(map_err)
}

/// Load the secrets JSON string for `steam_id` from the macOS Keychain.
#[allow(dead_code)] // wired up in later tasks
pub fn load_secrets(steam_id: &str) -> Result<String, KeychainError> {
    let entry = Entry::new(SERVICE, steam_id).map_err(map_err)?;
    entry.get_password().map_err(map_err)
}

/// Delete the Keychain entry for `steam_id`.
#[allow(dead_code)] // wired up in later tasks
pub fn delete_secrets(steam_id: &str) -> Result<(), KeychainError> {
    let entry = Entry::new(SERVICE, steam_id).map_err(map_err)?;
    entry.delete_credential().map_err(map_err)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    #[ignore = "touches the real macOS Keychain; run manually"]
    fn store_load_delete_round_trip() {
        let id = "76561190000000099";
        store_secrets(id, r#"{"shared_secret":"x"}"#).unwrap();
        assert_eq!(load_secrets(id).unwrap(), r#"{"shared_secret":"x"}"#);
        delete_secrets(id).unwrap();
        assert!(load_secrets(id).is_err());
    }
}
