// SPDX-FileCopyrightText: 2026 shinyawave
// SPDX-License-Identifier: AGPL-3.0-or-later

use crate::vault::model::{Account, AccountSummary};
use serde_json::Value;

#[allow(unused_imports)] // re-export enables spec-required path vault::mafile::parse_encrypted_mafile
pub use crate::vault::sda_crypto::parse_encrypted_mafile;

#[allow(dead_code)] // wired up in later tasks
#[derive(Debug, PartialEq)]
pub enum MaFileError {
    Json,
    MissingField(&'static str),
    Decrypt,
}

fn field<'a>(v: &'a Value, key: &'static str) -> Result<&'a str, MaFileError> {
    v.get(key)
        .and_then(Value::as_str)
        .ok_or(MaFileError::MissingField(key))
}

#[allow(dead_code)] // wired up in later tasks
pub fn parse_mafile(json: &str) -> Result<Account, MaFileError> {
    let v: Value = serde_json::from_str(json).map_err(|_| MaFileError::Json)?;
    let steam_id = v
        .get("Session")
        .and_then(|s| s.get("SteamID"))
        .map(|id| id.to_string().trim_matches('"').to_string())
        .ok_or(MaFileError::MissingField("Session.SteamID"))?;
    Ok(Account {
        steam_id,
        account_name: field(&v, "account_name")?.to_string(),
        shared_secret: field(&v, "shared_secret")?.to_string(),
        identity_secret: field(&v, "identity_secret")?.to_string(),
        device_id: field(&v, "device_id")?.to_string(),
        revocation_code: v
            .get("revocation_code")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string(),
    })
}

#[allow(dead_code)] // wired up in later tasks
pub fn summary(a: &Account) -> AccountSummary {
    AccountSummary {
        steam_id: a.steam_id.clone(),
        account_name: a.account_name.clone(),
        status: "active".to_string(),
        ..Default::default()
    }
}

/// Reconstruct a standard maFile JSON for export (copy-to-clipboard). Contains
/// secret material (shared_secret, identity_secret) — only ever produced on an
/// explicit user "Copy maFile" action.
#[allow(dead_code)]
pub fn export_json(a: &Account) -> String {
    let steamid: u64 = a.steam_id.parse().unwrap_or(0);
    serde_json::json!({
        "account_name": a.account_name,
        "shared_secret": a.shared_secret,
        "identity_secret": a.identity_secret,
        "device_id": a.device_id,
        "revocation_code": a.revocation_code,
        "Session": { "SteamID": steamid }
    })
    .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    const SAMPLE: &str = include_str!("../../tests/fixtures/sample.maFile");

    #[test]
    fn parses_all_required_fields() {
        let a = parse_mafile(SAMPLE).unwrap();
        assert_eq!(a.account_name, "tester");
        assert_eq!(a.steam_id, "76561190000000000");
        assert_eq!(a.shared_secret, "cnOgv/KdpLoP6Nbh0GMkXkPnNqmc0Q=");
        assert!(a.device_id.starts_with("android:"));
    }

    #[test]
    fn rejects_json_missing_shared_secret() {
        let bad = r#"{"account_name":"x","Session":{"SteamID":1}}"#;
        assert!(matches!(parse_mafile(bad), Err(MaFileError::MissingField(_))));
    }

    #[test]
    fn summary_has_no_secrets() {
        let a = parse_mafile(SAMPLE).unwrap();
        let s = summary(&a);
        assert_eq!(s.account_name, "tester");
        // Compile-time guarantee: AccountSummary simply has no secret fields.
    }

    #[test]
    fn export_json_contains_core_fields() {
        let a = Account {
            steam_id: "76561198000000001".into(),
            account_name: "user".into(),
            shared_secret: "SS".into(),
            identity_secret: "IS".into(),
            device_id: "android:x".into(),
            revocation_code: "R00000".into(),
        };
        let json = export_json(&a);
        let v: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(v["account_name"], "user");
        assert_eq!(v["shared_secret"], "SS");
        assert_eq!(v["identity_secret"], "IS");
        assert_eq!(v["Session"]["SteamID"], 76561198000000001u64);
    }
}
