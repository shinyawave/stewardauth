// SPDX-FileCopyrightText: 2026 shinyawave
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Parser for the classic Windows Steam Desktop Authenticator `manifest.json`.
//!
//! Format (encrypted):
//! ```json
//! { "encrypted": true,
//!   "entries": [ { "filename": "1234.maFile", "steamid": 7656..,
//!                  "encryption_iv": "b64", "encryption_salt": "b64" } ] }
//! ```
//! When `encrypted` is false, each `<filename>` is plaintext maFile JSON and the
//! per-entry iv/salt fields are absent.

use serde_json::Value;

#[allow(dead_code)] // wired up in later tasks
#[derive(Debug, Clone, PartialEq)]
pub struct SdaEntry {
    pub filename: String,
    pub steamid: String,
    pub encryption_iv: Option<String>,
    pub encryption_salt: Option<String>,
}

#[allow(dead_code)] // wired up in later tasks
#[derive(Debug, PartialEq)]
pub enum ManifestError {
    Json,
    MissingEntries,
    MissingFilename,
}

#[allow(dead_code)] // wired up in later tasks
pub fn parse_manifest(json: &str) -> Result<(bool, Vec<SdaEntry>), ManifestError> {
    let root: Value = serde_json::from_str(json).map_err(|_| ManifestError::Json)?;
    let encrypted = root.get("encrypted").and_then(Value::as_bool).unwrap_or(false);
    let arr = root
        .get("entries")
        .and_then(Value::as_array)
        .ok_or(ManifestError::MissingEntries)?;

    let mut entries = Vec::with_capacity(arr.len());
    for e in arr {
        let filename = e
            .get("filename")
            .and_then(Value::as_str)
            .ok_or(ManifestError::MissingFilename)?
            .to_string();
        // steamid may be a JSON number or a quoted string — normalise to String.
        let steamid = match e.get("steamid") {
            Some(Value::Number(n)) => n.to_string(),
            Some(Value::String(s)) => s.clone(),
            _ => String::new(),
        };
        let encryption_iv = e.get("encryption_iv").and_then(Value::as_str).map(str::to_string);
        let encryption_salt = e.get("encryption_salt").and_then(Value::as_str).map(str::to_string);
        entries.push(SdaEntry { filename, steamid, encryption_iv, encryption_salt });
    }
    Ok((encrypted, entries))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_encrypted_manifest() {
        let json = r#"{
            "encrypted": true,
            "entries": [
                { "filename": "1234.maFile", "steamid": 76561190000000123,
                  "encryption_iv": "IV==", "encryption_salt": "SALT==" }
            ]
        }"#;
        let (encrypted, entries) = parse_manifest(json).unwrap();
        assert!(encrypted);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].filename, "1234.maFile");
        assert_eq!(entries[0].steamid, "76561190000000123");
        assert_eq!(entries[0].encryption_iv.as_deref(), Some("IV=="));
        assert_eq!(entries[0].encryption_salt.as_deref(), Some("SALT=="));
    }

    #[test]
    fn parses_plaintext_manifest_without_iv_salt() {
        let json = r#"{
            "encrypted": false,
            "entries": [ { "filename": "9999.maFile", "steamid": 76561190000000999 } ]
        }"#;
        let (encrypted, entries) = parse_manifest(json).unwrap();
        assert!(!encrypted);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].filename, "9999.maFile");
        assert_eq!(entries[0].steamid, "76561190000000999");
        assert!(entries[0].encryption_iv.is_none());
        assert!(entries[0].encryption_salt.is_none());
    }

    #[test]
    fn steamid_accepts_string_form() {
        // Some SDA exports quote steamid as a string; accept both.
        let json = r#"{ "encrypted": false,
            "entries": [ { "filename": "a.maFile", "steamid": "76561190000000001" } ] }"#;
        let (_e, entries) = parse_manifest(json).unwrap();
        assert_eq!(entries[0].steamid, "76561190000000001");
    }

    #[test]
    fn rejects_garbage() {
        assert_eq!(parse_manifest("not json"), Err(ManifestError::Json));
    }

    #[test]
    fn rejects_missing_entries() {
        assert_eq!(parse_manifest(r#"{"encrypted":true}"#), Err(ManifestError::MissingEntries));
    }

    #[test]
    fn rejects_entry_without_filename() {
        let json = r#"{"encrypted":true,"entries":[{"steamid":1}]}"#;
        assert_eq!(parse_manifest(json), Err(ManifestError::MissingFilename));
    }

    #[test]
    fn parses_fixture_manifest() {
        // Uses the committed deterministic SDA fixture from Task 2.
        let json = include_str!("../../tests/fixtures/sda/manifest.json");
        let (encrypted, entries) = parse_manifest(json).unwrap();
        assert!(encrypted);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].filename, "1234.maFile");
        assert_eq!(entries[0].steamid, "76561190000000123");
        assert!(entries[0].encryption_iv.is_some());
        assert!(entries[0].encryption_salt.is_some());
    }
}
