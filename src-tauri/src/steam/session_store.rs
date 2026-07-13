// SPDX-FileCopyrightText: 2026 shinyawave
// SPDX-License-Identifier: AGPL-3.0-or-later

//! File-based Steam session refresh-token storage.
//!
//! Replaces the macOS Keychain for session tokens: one plaintext token per
//! account at `<app_dir>/sessions/<steam_id>.token`. Consistent with the
//! file-based maFile model; means no Keychain password prompts.

use std::path::{Path, PathBuf};

fn dir(app_dir: &Path) -> PathBuf {
    app_dir.join("sessions")
}

/// Path to an account's session-token file.
pub fn token_path(app_dir: &Path, steam_id: &str) -> PathBuf {
    dir(app_dir).join(format!("{steam_id}.token"))
}

/// Persist the refresh token (creates the sessions dir).
pub fn save(app_dir: &Path, steam_id: &str, token: &str) -> std::io::Result<()> {
    std::fs::create_dir_all(dir(app_dir))?;
    std::fs::write(token_path(app_dir, steam_id), token)
}

/// Load a stored token; None if absent or empty.
pub fn load(app_dir: &Path, steam_id: &str) -> Option<String> {
    match std::fs::read_to_string(token_path(app_dir, steam_id)) {
        Ok(s) => {
            let t = s.trim().to_string();
            if t.is_empty() { None } else { Some(t) }
        }
        Err(_) => None,
    }
}

/// Delete a stored token (no error if missing).
pub fn delete(app_dir: &Path, steam_id: &str) -> std::io::Result<()> {
    match std::fs::remove_file(token_path(app_dir, steam_id)) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn save_load_delete_round_trip() {
        let d = tempfile::tempdir().unwrap();
        assert!(load(d.path(), "123").is_none());
        save(d.path(), "123", "REFRESH_TOKEN_XYZ").unwrap();
        assert_eq!(load(d.path(), "123").as_deref(), Some("REFRESH_TOKEN_XYZ"));
        delete(d.path(), "123").unwrap();
        assert!(load(d.path(), "123").is_none());
        // delete of a missing file is Ok
        assert!(delete(d.path(), "123").is_ok());
    }

    #[test]
    fn empty_file_reads_as_none() {
        let d = tempfile::tempdir().unwrap();
        save(d.path(), "9", "").unwrap();
        assert!(load(d.path(), "9").is_none());
    }
}
