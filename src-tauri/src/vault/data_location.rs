// SPDX-FileCopyrightText: 2026 shinyawave
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Bootstrap pointer for a user-configurable data directory.
//!
//! A tiny `location.json` lives at the FIXED OS app-data dir
//! (`app.path().app_data_dir()`). It records where the real data dir is:
//! `{ "data_dir": "<absolute path>" }`. Absent/empty → the fixed dir is used.
//! Only this pointer stays fixed; `maFiles/`, `sessions/`, `vault.json`, and
//! `settings.json` all move with the resolved data dir.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The pointer file name at the fixed app-data dir.
pub const POINTER_FILE: &str = "location.json";

/// The previous OS app-data directory name, keyed by the pre-rename bundle id
/// (`com.macsda.authenticator`). The app was rebranded MacSDA → StewardAuth
/// (bundle id `com.stewardauth.app`), which moves the fixed app-data dir; this
/// lets a one-time migration find the old bootstrap pointer / data.
pub const LEGACY_BUNDLE_DIR: &str = "com.macsda.authenticator";

/// On-disk shape of `location.json`.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Pointer {
    /// Absolute path to the data directory. Empty → fall back to the fixed dir.
    #[serde(default)]
    data_dir: String,
}

/// Errors produced while reading/writing the pointer or migrating data.
#[derive(Debug)]
pub enum DataLocError {
    Io(std::io::Error),
    Json(serde_json::Error),
    /// The destination already contains a `vault.json` — refuse to clobber.
    DestinationNotEmpty,
    /// A copied item failed post-copy verification.
    VerifyFailed(String),
}

impl std::fmt::Display for DataLocError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DataLocError::Io(e) => write!(f, "data-location io error: {e}"),
            DataLocError::Json(e) => write!(f, "data-location json error: {e}"),
            DataLocError::DestinationNotEmpty => {
                write!(f, "destination already contains a vault.json")
            }
            DataLocError::VerifyFailed(what) => {
                write!(f, "migration verification failed for {what}")
            }
        }
    }
}

impl std::error::Error for DataLocError {}

impl From<std::io::Error> for DataLocError {
    fn from(e: std::io::Error) -> Self {
        DataLocError::Io(e)
    }
}

impl From<serde_json::Error> for DataLocError {
    fn from(e: serde_json::Error) -> Self {
        DataLocError::Json(e)
    }
}

/// Read the data-dir pointer at `<fixed_dir>/location.json`.
/// Returns `None` when the file is absent, empty, unparseable, or the
/// `data_dir` field is blank (all of which mean "use the fixed dir").
pub fn read_pointer(fixed_dir: &Path) -> Option<PathBuf> {
    let path = fixed_dir.join(POINTER_FILE);
    let raw = std::fs::read_to_string(path).ok()?;
    let ptr: Pointer = serde_json::from_str(&raw).ok()?;
    let trimmed = ptr.data_dir.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(PathBuf::from(trimmed))
    }
}

/// Resolve the effective data directory: the pointer target if present and
/// non-empty, otherwise the fixed app-data dir itself.
pub fn resolve(fixed_dir: &Path) -> PathBuf {
    read_pointer(fixed_dir).unwrap_or_else(|| fixed_dir.to_path_buf())
}

/// The four items that live under a data directory.
const DIR_ITEMS: [&str; 2] = ["maFiles", "sessions"];
const FILE_ITEMS: [&str; 2] = ["vault.json", "settings.json"];

/// Migrate a data directory from `from` to `to` (copy + verify only).
///
/// Order: refuse-if-dest-has-vault → copy all present items → verify each copy.
/// Returns `Ok` once the target is a verified full copy of the source.
/// **Does NOT remove the originals** — call [`remove_migrated_originals`] after
/// the pointer has been written so that a pointer-write failure leaves the old
/// location and pointer fully intact and consistent.
pub fn migrate(from: &Path, to: &Path) -> Result<(), DataLocError> {
    // 1. Clobber guard: never overwrite an existing vault at the destination.
    if to.join("vault.json").exists() {
        return Err(DataLocError::DestinationNotEmpty);
    }
    std::fs::create_dir_all(to)?;

    // 2. Copy every present item. Missing sources are skipped silently.
    for name in DIR_ITEMS {
        let src = from.join(name);
        if src.is_dir() {
            copy_dir_all(&src, &to.join(name))?;
        }
    }
    for name in FILE_ITEMS {
        let src = from.join(name);
        if src.is_file() {
            std::fs::copy(&src, to.join(name))?;
        }
    }

    // 3. Verify: every item that existed in the source now exists in the dest.
    for name in DIR_ITEMS {
        if from.join(name).is_dir() && !to.join(name).is_dir() {
            return Err(DataLocError::VerifyFailed(name.to_string()));
        }
    }
    for name in FILE_ITEMS {
        if from.join(name).is_file() {
            verify_copy(&from.join(name), &to.join(name), name)?;
        }
    }

    Ok(())
}

/// Remove the original files/dirs after a successful [`migrate`] + pointer write.
///
/// Called only after the pointer has been durably written so that any earlier
/// failure leaves the old location and its pointer consistent.
pub fn remove_migrated_originals(from: &Path) -> Result<(), DataLocError> {
    for name in DIR_ITEMS {
        let src = from.join(name);
        if src.exists() {
            std::fs::remove_dir_all(&src)?;
        }
    }
    for name in FILE_ITEMS {
        let src = from.join(name);
        if src.exists() {
            std::fs::remove_file(&src)?;
        }
    }
    Ok(())
}

/// One-time bootstrap migration after the bundle-id rename
/// (`com.macsda.authenticator` → `com.stewardauth.app`), which relocates the
/// FIXED OS app-data dir. Brings the previous install's bootstrap state into the
/// new fixed dir so no data is lost:
///   * If the old fixed dir has a `location.json` pointer (data was moved
///     elsewhere, e.g. `~/Desktop/sda`) → copy just the pointer. The real data
///     dir is untouched by the rename.
///   * Else if the old fixed dir was itself the data dir (`vault.json` present)
///     → copy the data items into the new fixed dir.
///
/// No-op when the new dir is already established (has a pointer or vault) or the
/// old dir is absent. **Copy-only** — the old dir is left intact as a backup.
/// Best-effort: any error is swallowed by the caller (never blocks startup).
pub fn migrate_from_old_bundle(
    new_fixed_dir: &Path,
    old_fixed_dir: &Path,
) -> Result<(), DataLocError> {
    // New dir already established? Nothing to do.
    if new_fixed_dir.join(POINTER_FILE).exists() || new_fixed_dir.join("vault.json").exists() {
        return Ok(());
    }
    if !old_fixed_dir.is_dir() {
        return Ok(());
    }

    // Case 1: old dir holds a pointer → copy just the pointer. The pointed-at
    // data dir (e.g. ~/Desktop/sda) is not keyed by bundle id, so it stays put.
    let old_ptr = old_fixed_dir.join(POINTER_FILE);
    if old_ptr.is_file() {
        std::fs::create_dir_all(new_fixed_dir)?;
        std::fs::copy(&old_ptr, new_fixed_dir.join(POINTER_FILE))?;
        return Ok(());
    }

    // Case 2: old fixed dir was used as the data dir directly → copy the data.
    if old_fixed_dir.join("vault.json").is_file() {
        migrate(old_fixed_dir, new_fixed_dir)?;
    }

    Ok(())
}

/// Recursively copy a directory tree.
fn copy_dir_all(src: &Path, dst: &Path) -> Result<(), DataLocError> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let path = entry.path();
        let target = dst.join(entry.file_name());
        if path.is_dir() {
            copy_dir_all(&path, &target)?;
        } else {
            std::fs::copy(&path, &target)?;
        }
    }
    Ok(())
}

/// Confirm a copied file exists and its byte length matches the source.
fn verify_copy(src: &Path, dst: &Path, name: &str) -> Result<(), DataLocError> {
    let src_len = std::fs::metadata(src)?.len();
    let dst_len = std::fs::metadata(dst)
        .map_err(|_| DataLocError::VerifyFailed(name.to_string()))?
        .len();
    if src_len != dst_len {
        return Err(DataLocError::VerifyFailed(name.to_string()));
    }
    Ok(())
}

/// Write the data-dir pointer to `<fixed_dir>/location.json` (pretty JSON).
/// Creates `fixed_dir` if needed.
pub fn write_pointer(fixed_dir: &Path, data_dir: &Path) -> Result<(), DataLocError> {
    std::fs::create_dir_all(fixed_dir)?;
    let ptr = Pointer {
        data_dir: data_dir.to_string_lossy().to_string(),
    };
    let json = serde_json::to_string_pretty(&ptr)?;
    std::fs::write(fixed_dir.join(POINTER_FILE), json)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // ---- migrate tests ----

    /// Build a populated source data dir with all four movable items.
    fn seed_source(dir: &Path) {
        std::fs::create_dir_all(dir.join("maFiles")).unwrap();
        std::fs::write(dir.join("maFiles").join("7656.maFile"), br#"{"a":1}"#).unwrap();
        std::fs::create_dir_all(dir.join("sessions")).unwrap();
        std::fs::write(dir.join("sessions").join("7656.token"), b"TOKEN").unwrap();
        std::fs::write(dir.join("vault.json"), br#"{"encrypted":false,"accounts":[]}"#).unwrap();
        std::fs::write(dir.join("settings.json"), br#"{"language":"en"}"#).unwrap();
    }

    #[test]
    fn migrate_copies_all_four_but_does_not_remove_originals() {
        let from = tempfile::tempdir().unwrap();
        let to = tempfile::tempdir().unwrap();
        seed_source(from.path());

        migrate(from.path(), to.path()).unwrap();

        // Destination has everything.
        assert_eq!(
            std::fs::read(to.path().join("maFiles").join("7656.maFile")).unwrap(),
            br#"{"a":1}"#
        );
        assert_eq!(
            std::fs::read(to.path().join("sessions").join("7656.token")).unwrap(),
            b"TOKEN"
        );
        assert!(to.path().join("vault.json").exists());
        assert!(to.path().join("settings.json").exists());

        // Originals are still present (migrate no longer removes them).
        assert!(from.path().join("maFiles").exists());
        assert!(from.path().join("sessions").exists());
        assert!(from.path().join("vault.json").exists());
        assert!(from.path().join("settings.json").exists());
    }

    #[test]
    fn full_sequence_removes_originals_and_pointer_points_at_target() {
        let fixed = tempfile::tempdir().unwrap();
        let from = tempfile::tempdir().unwrap();
        let to = tempfile::tempdir().unwrap();
        seed_source(from.path());

        // Simulate set_data_dir's safe order: migrate → write_pointer → remove.
        migrate(from.path(), to.path()).unwrap();
        write_pointer(fixed.path(), to.path()).unwrap();
        remove_migrated_originals(from.path()).unwrap();

        // Pointer now points at the new location.
        assert_eq!(
            read_pointer(fixed.path()).as_deref(),
            Some(to.path())
        );

        // Originals are gone.
        assert!(!from.path().join("maFiles").exists());
        assert!(!from.path().join("sessions").exists());
        assert!(!from.path().join("vault.json").exists());
        assert!(!from.path().join("settings.json").exists());

        // Target has everything.
        assert!(to.path().join("vault.json").exists());
        assert!(to.path().join("maFiles").exists());
    }

    #[test]
    fn pointer_write_fail_leaves_originals_intact() {
        // After migrate succeeds but before the pointer is written, a hypothetical
        // failure must leave originals intact. We model this by calling migrate but
        // NOT calling write_pointer or remove_migrated_originals.
        let from = tempfile::tempdir().unwrap();
        let to = tempfile::tempdir().unwrap();
        seed_source(from.path());

        migrate(from.path(), to.path()).unwrap();
        // (pointer write "failed" — we skip it)

        // Originals must still be intact.
        assert!(from.path().join("maFiles").exists());
        assert!(from.path().join("vault.json").exists());
    }

    #[test]
    fn migrate_refuses_when_destination_has_vault() {
        let from = tempfile::tempdir().unwrap();
        let to = tempfile::tempdir().unwrap();
        seed_source(from.path());
        std::fs::write(to.path().join("vault.json"), b"{}").unwrap();

        let err = migrate(from.path(), to.path()).unwrap_err();
        assert!(matches!(err, DataLocError::DestinationNotEmpty));
        // Source is untouched by a refused migration.
        assert!(from.path().join("vault.json").exists());
        assert!(from.path().join("maFiles").exists());
    }

    #[test]
    fn migrate_with_no_source_data_is_ok_and_noop() {
        // Fresh install: source dir exists but has no vault/maFiles yet.
        let from = tempfile::tempdir().unwrap();
        let to = tempfile::tempdir().unwrap();
        migrate(from.path(), to.path()).unwrap();
        // Nothing created spuriously in the destination.
        assert!(!to.path().join("vault.json").exists());
    }

    // ---- bundle-rename migration tests ----

    #[test]
    fn old_bundle_pointer_is_copied_to_new_fixed_dir() {
        // Old fixed dir holds only a pointer to an external data dir.
        let old = tempfile::tempdir().unwrap();
        let new = tempfile::tempdir().unwrap();
        let external = tempfile::tempdir().unwrap();
        write_pointer(old.path(), external.path()).unwrap();

        migrate_from_old_bundle(new.path(), old.path()).unwrap();

        // New dir now resolves to the same external data dir; old pointer intact.
        assert_eq!(read_pointer(new.path()).as_deref(), Some(external.path()));
        assert!(old.path().join(POINTER_FILE).exists());
    }

    #[test]
    fn old_bundle_inline_data_is_copied_to_new_fixed_dir() {
        // Old fixed dir was used as the data dir directly (no pointer).
        let old = tempfile::tempdir().unwrap();
        let new = tempfile::tempdir().unwrap();
        seed_source(old.path());

        migrate_from_old_bundle(new.path(), old.path()).unwrap();

        assert!(new.path().join("vault.json").exists());
        assert!(new.path().join("maFiles").join("7656.maFile").exists());
        // Old dir kept as a backup (copy-only).
        assert!(old.path().join("vault.json").exists());
    }

    #[test]
    fn bundle_migration_is_noop_when_new_established_or_old_absent() {
        // New already has a vault → skip entirely.
        let old = tempfile::tempdir().unwrap();
        let new = tempfile::tempdir().unwrap();
        seed_source(old.path());
        std::fs::write(new.path().join("vault.json"), b"{}").unwrap();
        migrate_from_old_bundle(new.path(), old.path()).unwrap();
        assert!(!new.path().join("maFiles").exists()); // nothing copied

        // Old dir absent → no-op, no error.
        let new2 = tempfile::tempdir().unwrap();
        let missing = new2.path().join("does-not-exist");
        migrate_from_old_bundle(new2.path(), &missing).unwrap();
        assert!(!new2.path().join("vault.json").exists());
    }

    // ---- existing pointer tests ----

    #[test]
    fn write_then_read_pointer_round_trips() {
        let fixed = tempfile::tempdir().unwrap();
        let target = tempfile::tempdir().unwrap();
        assert!(read_pointer(fixed.path()).is_none());
        write_pointer(fixed.path(), target.path()).unwrap();
        assert_eq!(
            read_pointer(fixed.path()).as_deref(),
            Some(target.path())
        );
        // The pointer file lives at the fixed dir, not the target.
        assert!(fixed.path().join(POINTER_FILE).exists());
    }

    #[test]
    fn read_pointer_absent_or_empty_is_none() {
        let fixed = tempfile::tempdir().unwrap();
        // Absent file.
        assert!(read_pointer(fixed.path()).is_none());
        // Present but empty data_dir.
        std::fs::write(fixed.path().join(POINTER_FILE), br#"{"data_dir":""}"#).unwrap();
        assert!(read_pointer(fixed.path()).is_none());
        // Garbage JSON → None (never panics).
        std::fs::write(fixed.path().join(POINTER_FILE), b"not json").unwrap();
        assert!(read_pointer(fixed.path()).is_none());
    }

    #[test]
    fn resolve_falls_back_to_fixed_when_pointer_absent() {
        let fixed = tempfile::tempdir().unwrap();
        assert_eq!(resolve(fixed.path()), fixed.path().to_path_buf());
    }

    #[test]
    fn resolve_returns_pointer_target_when_present() {
        let fixed = tempfile::tempdir().unwrap();
        let target = tempfile::tempdir().unwrap();
        write_pointer(fixed.path(), target.path()).unwrap();
        assert_eq!(resolve(fixed.path()), target.path().to_path_buf());
    }
}
