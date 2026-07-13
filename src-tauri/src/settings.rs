// SPDX-FileCopyrightText: 2026 shinyawave
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Unencrypted UI settings stored at `<app_dir>/settings.json`.
//!
//! Kept OUTSIDE the encrypted vault so the language (and accent/theme) can be
//! applied before the vault is unlocked. Contains no secrets.

use std::path::Path;

use serde::{Deserialize, Serialize};

fn default_language() -> String {
    "en".to_string()
}
fn default_accent_hue() -> u32 {
    250
}
fn default_mafile_naming() -> String {
    "steamid".to_string()
}
fn default_common_format() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppSettings {
    #[serde(default = "default_language")]
    pub language: String,
    #[serde(default)]
    pub minimize_to_tray: bool,
    #[serde(default = "default_accent_hue")]
    pub accent_hue: u32,
    #[serde(default = "default_mafile_naming")]
    pub mafile_naming: String,
    #[serde(default = "default_common_format")]
    pub common_mafile_format: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            language: default_language(),
            minimize_to_tray: false,
            accent_hue: default_accent_hue(),
            mafile_naming: default_mafile_naming(),
            common_mafile_format: default_common_format(),
        }
    }
}

/// Load settings; returns defaults if the file is missing or unparsable.
pub fn load(app_dir: &Path) -> AppSettings {
    let path = app_dir.join("settings.json");
    match std::fs::read(&path) {
        Ok(bytes) => serde_json::from_slice(&bytes).unwrap_or_default(),
        Err(_) => AppSettings::default(),
    }
}

/// Persist settings to `<app_dir>/settings.json` (pretty JSON).
pub fn save(app_dir: &Path, settings: &AppSettings) -> std::io::Result<()> {
    let path = app_dir.join("settings.json");
    let json = serde_json::to_vec_pretty(settings)
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    std::fs::write(path, json)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_en_notray_hue250() {
        let s = AppSettings::default();
        assert_eq!(s.language, "en");
        assert!(!s.minimize_to_tray);
        assert_eq!(s.accent_hue, 250);
    }

    #[test]
    fn load_missing_returns_defaults() {
        let dir = tempfile::tempdir().unwrap();
        let s = load(dir.path());
        assert_eq!(s.language, "en");
    }

    #[test]
    fn save_then_load_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let s = AppSettings {
            language: "ru".into(),
            minimize_to_tray: true,
            accent_hue: 120,
            mafile_naming: "login".into(),
            common_mafile_format: false,
        };
        save(dir.path(), &s).unwrap();
        let loaded = load(dir.path());
        assert_eq!(loaded.language, "ru");
        assert!(loaded.minimize_to_tray);
        assert_eq!(loaded.accent_hue, 120);
        assert_eq!(loaded.mafile_naming, "login");
        assert!(!loaded.common_mafile_format);
    }

    #[test]
    fn partial_json_uses_defaults_for_missing_fields() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("settings.json"), br#"{"language":"fr"}"#).unwrap();
        let s = load(dir.path());
        assert_eq!(s.language, "fr");
        assert!(!s.minimize_to_tray);
        assert_eq!(s.accent_hue, 250);
    }
}
