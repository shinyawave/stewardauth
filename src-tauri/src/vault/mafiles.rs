// SPDX-FileCopyrightText: 2026 shinyawave
// SPDX-License-Identifier: AGPL-3.0-or-later

//! On-disk maFile folder storage (`<app_dir>/maFiles/`).
//!
//! Plaintext standard `.maFile` JSON. This is the source of truth for account
//! secrets (Keychain is retired after migration).

use std::path::{Path, PathBuf};

use crate::vault::mafile::{self, MaFileError};
use crate::vault::model::Account;

/// The maFiles directory: `<app_dir>/maFiles`.
pub fn dir(app_dir: &Path) -> PathBuf {
    app_dir.join("maFiles")
}

/// Ensure the maFiles directory exists, returning its path.
pub fn ensure_dir(app_dir: &Path) -> std::io::Result<PathBuf> {
    let d = dir(app_dir);
    std::fs::create_dir_all(&d)?;
    Ok(d)
}

fn sanitize(s: &str) -> String {
    let cleaned: String = s
        .chars()
        .map(|c| if c.is_alphanumeric() || c == '-' || c == '_' { c } else { '_' })
        .collect();
    if cleaned.is_empty() { "account".to_string() } else { cleaned }
}

/// Compute the file name for an account under the given naming mode + extension.
/// `naming`: "login" → sanitized account_name, anything else → steam_id.
pub fn file_name(account: &Account, naming: &str, ext: &str) -> String {
    let base = if naming == "login" {
        sanitize(&account.account_name)
    } else {
        account.steam_id.clone()
    };
    format!("{base}.{ext}")
}

/// Write an account as a plaintext maFile; returns the file name.
pub fn write(app_dir: &Path, account: &Account, naming: &str, ext: &str) -> std::io::Result<String> {
    let d = ensure_dir(app_dir)?;
    let name = file_name(account, naming, ext);
    std::fs::write(d.join(&name), mafile::export_json(account))?;
    Ok(name)
}

/// Read + parse an account from a maFile by file name.
pub fn read(app_dir: &Path, file_name: &str) -> Result<Account, MaFileError> {
    let path = dir(app_dir).join(file_name);
    let s = std::fs::read_to_string(path).map_err(|_| MaFileError::Json)?;
    mafile::parse_mafile(&s)
}

/// Parse every `*.maFile` / `*.json` in the folder, skipping unparseable files.
/// Returns `(file_name, account)` pairs.
pub fn scan(app_dir: &Path) -> Vec<(String, Account)> {
    let d = dir(app_dir);
    let mut out = Vec::new();
    let entries = match std::fs::read_dir(&d) {
        Ok(e) => e,
        Err(_) => return out,
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let ext_ok = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("mafile") || e.eq_ignore_ascii_case("json"))
            .unwrap_or(false);
        if !ext_ok {
            continue;
        }
        let name = match path.file_name().and_then(|n| n.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        if let Ok(s) = std::fs::read_to_string(&path) {
            if let Ok(account) = mafile::parse_mafile(&s) {
                out.push((name, account));
            }
        }
    }
    out
}

/// Delete a maFile by name (no error if absent).
pub fn delete(app_dir: &Path, file_name: &str) -> std::io::Result<()> {
    let path = dir(app_dir).join(file_name);
    match std::fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// Rename a maFile.
pub fn rename(app_dir: &Path, old: &str, new: &str) -> std::io::Result<()> {
    let d = dir(app_dir);
    std::fs::rename(d.join(old), d.join(new))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_account() -> Account {
        Account {
            steam_id: "76561198000000001".into(),
            account_name: "my user!".into(),
            shared_secret: "SS==".into(),
            identity_secret: "IS==".into(),
            device_id: "android:abc".into(),
            revocation_code: "R00000".into(),
        }
    }

    #[test]
    fn file_name_modes_and_sanitize() {
        let a = sample_account();
        assert_eq!(file_name(&a, "steamid", "maFile"), "76561198000000001.maFile");
        assert_eq!(file_name(&a, "login", "maFile"), "my_user_.maFile");
        assert_eq!(file_name(&a, "login", "json"), "my_user_.json");
    }

    #[test]
    fn write_then_read_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let a = sample_account();
        let name = write(dir.path(), &a, "steamid", "maFile").unwrap();
        assert_eq!(name, "76561198000000001.maFile");
        let back = read(dir.path(), &name).unwrap();
        assert_eq!(back.steam_id, a.steam_id);
        assert_eq!(back.shared_secret, a.shared_secret);
        assert_eq!(back.identity_secret, a.identity_secret);
    }

    #[test]
    fn scan_finds_valid_skips_garbage() {
        let dir = tempfile::tempdir().unwrap();
        write(dir.path(), &sample_account(), "steamid", "maFile").unwrap();
        // a garbage file with a matching extension
        std::fs::write(dir.path().join("maFiles").join("bad.maFile"), b"not json").unwrap();
        // an unrelated extension
        std::fs::write(dir.path().join("maFiles").join("note.txt"), b"{}").unwrap();
        let found = scan(dir.path());
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].1.steam_id, "76561198000000001");
    }

    #[test]
    fn delete_missing_is_ok() {
        let dir = tempfile::tempdir().unwrap();
        ensure_dir(dir.path()).unwrap();
        assert!(delete(dir.path(), "nope.maFile").is_ok());
    }
}
