// SPDX-FileCopyrightText: 2026 shinyawave
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Unified importer: classify input paths (file / zip / dir) into `ImportSource`s,
//! detect each source's format by content, decrypt/parse, and persist.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::error::AppError;
use crate::vault::mafile::{parse_mafile, MaFileError};
use crate::vault::model::Account;
use crate::vault::sda_crypto::{decrypt_cbc, parse_encrypted_mafile, EncryptedEntry, SdaError};
use crate::vault::sda_manifest::{parse_manifest, SdaEntry};
use crate::vault::store;

/// Result of importing a batch of paths. IPC-safe (no secrets).
#[derive(Debug, Default, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    pub imported: usize,
    pub skipped_existing: Vec<String>,
    pub failed: Vec<String>,
}

/// One maFile to import, plus (for raw-encrypted blobs) its manifest entry.
pub(crate) struct ImportSource {
    pub display_name: String,
    pub content: String,
    pub manifest_entry: Option<SdaEntry>,
}

pub(crate) enum ParseOutcome {
    Parsed(Box<Account>),
    Failed(String),
    WrongPassword,
}

fn is_base64_blob(s: &str) -> bool {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    s.len() >= 64 && STANDARD.decode(s).is_ok()
}

/// Detect the maFile format by content and turn it into an `Account`.
/// - `{`-prefixed JSON → plaintext, or SDA wrapper `{encrypted_data,salt,iv}`.
/// - long pure base64 → raw SDA-encrypted blob (needs `manifest_entry` iv/salt).
pub(crate) fn detect_and_parse(
    content: &str,
    manifest_entry: Option<&SdaEntry>,
    password: Option<&str>,
) -> ParseOutcome {
    let trimmed = content.trim();

    if trimmed.starts_with('{') {
        let v: serde_json::Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(_) => return ParseOutcome::Failed("invalid JSON".into()),
        };
        let str_field = |k: &str| v.get(k).and_then(|x| x.as_str());
        let is_wrapper =
            str_field("encrypted_data").is_some() && str_field("salt").is_some() && str_field("iv").is_some();
        if is_wrapper {
            let pw = match password {
                Some(p) if !p.is_empty() => p,
                _ => return ParseOutcome::WrongPassword,
            };
            let entry = EncryptedEntry {
                cipher_b64: str_field("encrypted_data").unwrap().to_string(),
                salt_b64: str_field("salt").unwrap().to_string(),
                iv_b64: str_field("iv").unwrap().to_string(),
            };
            return match parse_encrypted_mafile(&entry, pw) {
                Ok(a) => ParseOutcome::Parsed(Box::new(a)),
                Err(MaFileError::Decrypt) => ParseOutcome::WrongPassword,
                Err(e) => ParseOutcome::Failed(format!("bad maFile ({e:?})")),
            };
        }
        return match parse_mafile(trimmed) {
            Ok(a) => ParseOutcome::Parsed(Box::new(a)),
            Err(e) => ParseOutcome::Failed(format!("bad maFile ({e:?})")),
        };
    }

    if is_base64_blob(trimmed) {
        let entry = match manifest_entry {
            Some(e) if e.encryption_iv.is_some() && e.encryption_salt.is_some() => e,
            _ => return ParseOutcome::Failed("encrypted maFile without manifest.json".into()),
        };
        let pw = match password {
            Some(p) if !p.is_empty() => p,
            _ => return ParseOutcome::WrongPassword,
        };
        let iv = entry.encryption_iv.as_deref().unwrap();
        let salt = entry.encryption_salt.as_deref().unwrap();
        return match decrypt_cbc(trimmed, pw, salt, iv) {
            Ok(bytes) => match String::from_utf8(bytes) {
                Ok(json) => match parse_mafile(&json) {
                    Ok(a) => ParseOutcome::Parsed(Box::new(a)),
                    Err(e) => ParseOutcome::Failed(format!("bad maFile ({e:?})")),
                },
                Err(_) => ParseOutcome::WrongPassword,
            },
            Err(SdaError::WrongPasswordOrCorrupt) => ParseOutcome::WrongPassword,
            Err(SdaError::Base64) => ParseOutcome::Failed("bad base64 iv/salt/cipher".into()),
        };
    }

    ParseOutcome::Failed("not a valid maFile".into())
}

fn file_label(p: &Path) -> String {
    p.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| p.to_string_lossy().into_owned())
}

fn is_mafile_ext(p: &Path) -> bool {
    let ext_ok = p
        .extension()
        .map(|e| e.eq_ignore_ascii_case("mafile") || e.eq_ignore_ascii_case("json"))
        .unwrap_or(false);
    let not_manifest = p
        .file_name()
        .map(|n| !n.eq_ignore_ascii_case("manifest.json"))
        .unwrap_or(true);
    ext_ok && not_manifest
}

/// Gather sources from a directory. If `manifest.json` is present, use it (encrypted
/// entries carry iv/salt). Otherwise scan (non-recursively) for loose `.maFile`/`.json`
/// treated as plaintext.
fn gather_from_dir(dir: &Path) -> (Vec<ImportSource>, Vec<String>) {
    let mut sources = Vec::new();
    let mut fails = Vec::new();

    if let Ok(mtext) = std::fs::read_to_string(dir.join("manifest.json")) {
        match parse_manifest(&mtext) {
            Ok((_encrypted, entries)) => {
                for e in entries {
                    match std::fs::read_to_string(dir.join(&e.filename)) {
                        Ok(content) => sources.push(ImportSource {
                            display_name: e.filename.clone(),
                            content,
                            manifest_entry: Some(e),
                        }),
                        Err(err) => fails.push(format!("{}: cannot read ({err})", e.filename)),
                    }
                }
            }
            Err(err) => fails.push(format!("manifest.json: invalid ({err:?})")),
        }
        return (sources, fails);
    }

    match std::fs::read_dir(dir) {
        Ok(rd) => {
            for entry in rd.flatten() {
                let p = entry.path();
                if p.is_file() && is_mafile_ext(&p) {
                    match std::fs::read_to_string(&p) {
                        Ok(content) => sources.push(ImportSource {
                            display_name: file_label(&p),
                            content,
                            manifest_entry: None,
                        }),
                        Err(err) => fails.push(format!("{}: cannot read ({err})", file_label(&p))),
                    }
                }
            }
        }
        Err(err) => fails.push(format!("{}: cannot read dir ({err})", file_label(dir))),
    }
    (sources, fails)
}

/// If `root` has no importable content of its own but contains exactly one
/// subdirectory, return that subdirectory (handles zips wrapped in a single folder).
fn single_child_dir(root: &Path) -> Option<PathBuf> {
    if root.join("manifest.json").is_file() {
        return None;
    }
    let entries: Vec<_> = std::fs::read_dir(root).ok()?.flatten().collect();
    let has_mafile = entries.iter().any(|e| {
        let p = e.path();
        p.is_file() && is_mafile_ext(&p)
    });
    if has_mafile {
        return None;
    }
    let dirs: Vec<PathBuf> = entries.iter().map(|e| e.path()).filter(|p| p.is_dir()).collect();
    if dirs.len() == 1 {
        Some(dirs[0].clone())
    } else {
        None
    }
}

/// Extract a ZIP into a temp dir (with a zip-slip guard) then gather from it.
/// File contents are read into memory, so the temp dir is safe to drop on return.
fn gather_from_zip(zip_path: &Path) -> Result<(Vec<ImportSource>, Vec<String>), String> {
    let file = std::fs::File::open(zip_path).map_err(|e| format!("cannot open zip ({e})"))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("bad zip ({e})"))?;
    let tmp = tempfile::TempDir::new().map_err(|e| format!("temp dir ({e})"))?;

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| format!("zip entry ({e})"))?;
        // zip-slip guard: enclosed_name() is None for paths that escape the root.
        let rel = match entry.enclosed_name() {
            Some(r) => r,
            None => continue,
        };
        let out = tmp.path().join(rel);
        if entry.is_dir() {
            let _ = std::fs::create_dir_all(&out);
            continue;
        }
        if let Some(parent) = out.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let mut outfile =
            std::fs::File::create(&out).map_err(|e| format!("extract ({e})"))?;
        std::io::copy(&mut entry, &mut outfile).map_err(|e| format!("extract ({e})"))?;
    }

    let root = single_child_dir(tmp.path()).unwrap_or_else(|| tmp.path().to_path_buf());
    Ok(gather_from_dir(&root))
}

/// Search for a `manifest.json` in the file's own dir, walking up ≤2 levels,
/// and return the entry matching this file's name.
fn find_manifest_entry_for(file: &Path) -> Option<SdaEntry> {
    let fname = file.file_name()?.to_string_lossy().to_string();
    let mut dir = file.parent()?.to_path_buf();
    for _ in 0..3 {
        if let Ok(mtext) = std::fs::read_to_string(dir.join("manifest.json")) {
            if let Ok((_e, entries)) = parse_manifest(&mtext) {
                if let Some(entry) = entries.into_iter().find(|e| {
                    Path::new(&e.filename)
                        .file_name()
                        .map(|n| n.to_string_lossy() == fname)
                        .unwrap_or(false)
                }) {
                    return Some(entry);
                }
            }
        }
        match dir.parent() {
            Some(p) => dir = p.to_path_buf(),
            None => break,
        }
    }
    None
}

/// Gather sources from individually-selected files. Plaintext/wrapper files need no
/// manifest; raw-encrypted blobs look up a sibling `manifest.json`.
fn gather_from_files(files: &[PathBuf]) -> (Vec<ImportSource>, Vec<String>) {
    let mut sources = Vec::new();
    let mut fails = Vec::new();
    for f in files {
        let content = match std::fs::read_to_string(f) {
            Ok(c) => c,
            Err(e) => {
                fails.push(format!("{}: cannot read ({e})", file_label(f)));
                continue;
            }
        };
        let manifest_entry = if content.trim().starts_with('{') {
            None
        } else {
            find_manifest_entry_for(f)
        };
        sources.push(ImportSource {
            display_name: file_label(f),
            content,
            manifest_entry,
        });
    }
    (sources, fails)
}

/// Classify each path (dir / zip / file), gather sources, skip duplicates, then
/// decrypt+parse+persist each. A wrong/missing shared password aborts with
/// `AppError::InvalidPassword`.
pub fn import_paths_inner(
    app_dir: &Path,
    paths: &[String],
    password: Option<&str>,
    master: Option<&str>,
    naming: &str,
    ext: &str,
) -> Result<ImportReport, AppError> {
    let mut sources: Vec<ImportSource> = Vec::new();
    let mut report = ImportReport::default();
    let mut loose_files: Vec<PathBuf> = Vec::new();

    for p in paths {
        let path = Path::new(p);
        let is_zip = path
            .extension()
            .map(|e| e.eq_ignore_ascii_case("zip"))
            .unwrap_or(false);
        if path.is_dir() {
            let (mut s, mut f) = gather_from_dir(path);
            sources.append(&mut s);
            report.failed.append(&mut f);
        } else if is_zip && path.is_file() {
            match gather_from_zip(path) {
                Ok((mut s, mut f)) => {
                    sources.append(&mut s);
                    report.failed.append(&mut f);
                }
                Err(e) => report.failed.push(format!("{}: {e}", file_label(path))),
            }
        } else if path.is_file() {
            loose_files.push(path.to_path_buf());
        } else {
            report.failed.push(format!("{}: not found", file_label(path)));
        }
    }

    if !loose_files.is_empty() {
        let (mut s, mut f) = gather_from_files(&loose_files);
        sources.append(&mut s);
        report.failed.append(&mut f);
    }

    let mut existing: HashSet<String> = store::load_vault(app_dir, master)?
        .accounts
        .into_iter()
        .map(|a| a.steam_id)
        .collect();

    for src in sources {
        // Cheap skip: manifest already tells us the steamid → avoid needless decrypt.
        if let Some(entry) = &src.manifest_entry {
            if !entry.steamid.is_empty() && existing.contains(&entry.steamid) {
                report.skipped_existing.push(entry.steamid.clone());
                continue;
            }
        }
        match detect_and_parse(&src.content, src.manifest_entry.as_ref(), password) {
            ParseOutcome::Parsed(account) => {
                if existing.contains(&account.steam_id) {
                    report.skipped_existing.push(account.steam_id.clone());
                    continue;
                }
                match store::add_account(app_dir, &account, naming, ext, master) {
                    Ok(_) => {
                        report.imported += 1;
                        existing.insert(account.steam_id.clone());
                    }
                    Err(e) => report
                        .failed
                        .push(format!("{}: store error ({e})", src.display_name)),
                }
            }
            ParseOutcome::Failed(msg) => {
                report.failed.push(format!("{}: {msg}", src.display_name))
            }
            ParseOutcome::WrongPassword => return Err(AppError::InvalidPassword),
        }
    }

    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    // A committed plaintext maFile fixture (has Session.SteamID etc.).
    const PLAIN: &str = include_str!("../../tests/fixtures/sample.maFile");
    // Raw SDA-encrypted blob fixture (base64) + its known iv/salt/password.
    const BLOB: &str = include_str!("../../tests/fixtures/sda/1234.maFile");
    const BLOB_SALT: &str = "oaGhoaGhoaGhoaGhoaGhoQ==";
    const BLOB_IV: &str = "srKysrKysrKysrKysrKysg==";

    fn blob_entry() -> SdaEntry {
        SdaEntry {
            filename: "1234.maFile".into(),
            steamid: "76561190000000123".into(),
            encryption_iv: Some(BLOB_IV.into()),
            encryption_salt: Some(BLOB_SALT.into()),
        }
    }

    #[test]
    fn plaintext_json_parses() {
        match detect_and_parse(PLAIN, None, None) {
            ParseOutcome::Parsed(a) => assert_eq!(a.account_name, "tester"),
            _ => panic!("expected Parsed"),
        }
    }

    #[test]
    fn blob_with_manifest_and_password_parses() {
        let e = blob_entry();
        match detect_and_parse(BLOB, Some(&e), Some("hunter2")) {
            ParseOutcome::Parsed(a) => assert_eq!(a.steam_id, "76561190000000123"),
            _ => panic!("expected Parsed"),
        }
    }

    #[test]
    fn blob_wrong_password_is_wrongpassword() {
        let e = blob_entry();
        assert!(matches!(
            detect_and_parse(BLOB, Some(&e), Some("nope")),
            ParseOutcome::WrongPassword
        ));
    }

    #[test]
    fn blob_without_manifest_fails_clearly() {
        match detect_and_parse(BLOB, None, Some("hunter2")) {
            ParseOutcome::Failed(msg) => assert!(msg.contains("manifest")),
            _ => panic!("expected Failed"),
        }
    }

    #[test]
    fn garbage_fails() {
        assert!(matches!(detect_and_parse("not a mafile", None, None), ParseOutcome::Failed(_)));
    }

    use std::fs;

    #[test]
    fn dir_with_manifest_yields_entry_sources() {
        let td = tempfile::TempDir::new().unwrap();
        fs::write(td.path().join("manifest.json"),
            r#"{"encrypted":true,"entries":[{"filename":"1234.maFile","steamid":76561190000000123,"encryption_iv":"srKysrKysrKysrKysrKysg==","encryption_salt":"oaGhoaGhoaGhoaGhoaGhoQ=="}]}"#).unwrap();
        fs::write(td.path().join("1234.maFile"), BLOB).unwrap();
        let (sources, fails) = gather_from_dir(td.path());
        assert!(fails.is_empty());
        assert_eq!(sources.len(), 1);
        assert!(sources[0].manifest_entry.is_some());
    }

    #[test]
    fn dir_without_manifest_scans_loose_plaintext() {
        let td = tempfile::TempDir::new().unwrap();
        fs::write(td.path().join("a.maFile"), PLAIN).unwrap();
        fs::write(td.path().join("b.json"), PLAIN).unwrap();
        fs::write(td.path().join("ignore.txt"), "nope").unwrap();
        let (sources, fails) = gather_from_dir(td.path());
        assert!(fails.is_empty());
        assert_eq!(sources.len(), 2); // .maFile + .json, not .txt
        assert!(sources.iter().all(|s| s.manifest_entry.is_none()));
    }

    #[test]
    fn dir_with_manifest_missing_file_records_failure() {
        let td = tempfile::TempDir::new().unwrap();
        fs::write(td.path().join("manifest.json"),
            r#"{"encrypted":false,"entries":[{"filename":"gone.maFile","steamid":1}]}"#).unwrap();
        let (sources, fails) = gather_from_dir(td.path());
        assert!(sources.is_empty());
        assert_eq!(fails.len(), 1);
        assert!(fails[0].contains("gone.maFile"));
    }

    fn make_zip(entries: &[(&str, &str)]) -> tempfile::TempDir {
        let td = tempfile::TempDir::new().unwrap();
        let zpath = td.path().join("bundle.zip");
        let f = fs::File::create(&zpath).unwrap();
        let mut zw = zip::ZipWriter::new(f);
        let opts: zip::write::FileOptions<'_, ()> =
            zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Stored);
        for (name, body) in entries {
            zw.start_file(*name, opts).unwrap();
            zw.write_all(body.as_bytes()).unwrap();
        }
        zw.finish().unwrap();
        td
    }

    #[test]
    fn zip_with_manifest_and_encrypted_file() {
        let manifest = r#"{"encrypted":true,"entries":[{"filename":"1234.maFile","steamid":76561190000000123,"encryption_iv":"srKysrKysrKysrKysrKysg==","encryption_salt":"oaGhoaGhoaGhoaGhoaGhoQ=="}]}"#;
        let td = make_zip(&[("manifest.json", manifest), ("1234.maFile", BLOB)]);
        let (sources, fails) = gather_from_zip(&td.path().join("bundle.zip")).unwrap();
        assert!(fails.is_empty());
        assert_eq!(sources.len(), 1);
        assert!(sources[0].manifest_entry.is_some());
    }

    #[test]
    fn zip_with_loose_plaintext() {
        let td = make_zip(&[("a.maFile", PLAIN), ("b.maFile", PLAIN)]);
        let (sources, fails) = gather_from_zip(&td.path().join("bundle.zip")).unwrap();
        assert!(fails.is_empty());
        assert_eq!(sources.len(), 2);
    }

    #[test]
    fn zip_slip_entry_is_skipped() {
        // A malicious entry name that tries to escape the extraction root.
        let td = make_zip(&[("../evil.maFile", PLAIN), ("ok.maFile", PLAIN)]);
        let (sources, _fails) = gather_from_zip(&td.path().join("bundle.zip")).unwrap();
        // The traversal entry is dropped; only ok.maFile survives.
        assert_eq!(sources.len(), 1);
        assert_eq!(sources[0].display_name, "ok.maFile");
    }

    #[test]
    fn zip_wrapped_in_single_folder() {
        let td = make_zip(&[("wrapper/a.maFile", PLAIN)]);
        let (sources, fails) = gather_from_zip(&td.path().join("bundle.zip")).unwrap();
        assert!(fails.is_empty());
        assert_eq!(sources.len(), 1);
    }

    #[test]
    fn loose_plaintext_files_no_manifest_needed() {
        let td = tempfile::TempDir::new().unwrap();
        let a = td.path().join("a.maFile");
        fs::write(&a, PLAIN).unwrap();
        let (sources, fails) = gather_from_files(&[a]);
        assert!(fails.is_empty());
        assert_eq!(sources.len(), 1);
        assert!(sources[0].manifest_entry.is_none());
    }

    #[test]
    fn loose_encrypted_file_finds_sibling_manifest() {
        let td = tempfile::TempDir::new().unwrap();
        fs::write(td.path().join("manifest.json"),
            r#"{"encrypted":true,"entries":[{"filename":"1234.maFile","steamid":76561190000000123,"encryption_iv":"srKysrKysrKysrKysrKysg==","encryption_salt":"oaGhoaGhoaGhoaGhoaGhoQ=="}]}"#).unwrap();
        let blob = td.path().join("1234.maFile");
        fs::write(&blob, BLOB).unwrap();
        let (sources, _fails) = gather_from_files(&[blob]);
        assert_eq!(sources.len(), 1);
        assert!(sources[0].manifest_entry.is_some(), "should attach sibling manifest entry");
    }

    fn import(app_dir: &Path, paths: &[PathBuf], pw: Option<&str>) -> ImportReport {
        let strs: Vec<String> = paths.iter().map(|p| p.to_string_lossy().into_owned()).collect();
        import_paths_inner(app_dir, &strs, pw, None, "steamid", "maFile").unwrap()
    }

    #[test]
    fn folder_of_loose_files_imports() {
        // The reported bug: a folder of loose .maFiles with NO manifest.json.
        let app = tempfile::TempDir::new().unwrap();
        let src = tempfile::TempDir::new().unwrap();
        fs::write(src.path().join("acc.maFile"), PLAIN).unwrap();
        let report = import(app.path(), &[src.path().to_path_buf()], None);
        assert_eq!(report.imported, 1);
        assert!(report.failed.is_empty());
    }

    #[test]
    fn duplicate_steamid_is_skipped() {
        let app = tempfile::TempDir::new().unwrap();
        let f = tempfile::TempDir::new().unwrap();
        let file = f.path().join("acc.maFile");
        fs::write(&file, PLAIN).unwrap();
        let first = import(app.path(), &[file.clone()], None);
        assert_eq!(first.imported, 1);
        let second = import(app.path(), &[file], None);
        assert_eq!(second.imported, 0);
        assert_eq!(second.skipped_existing.len(), 1);
    }

    #[test]
    fn intra_batch_duplicate_steamid_counted_once() {
        // Two loose files with the SAME account (same steamid) in one batch.
        let app = tempfile::TempDir::new().unwrap();
        let src = tempfile::TempDir::new().unwrap();
        fs::write(src.path().join("a.maFile"), PLAIN).unwrap();
        fs::write(src.path().join("b.maFile"), PLAIN).unwrap();
        let report = import(app.path(), &[src.path().to_path_buf()], None);
        assert_eq!(report.imported, 1, "same steamid must import once");
        assert_eq!(report.skipped_existing.len(), 1, "the duplicate is skipped");
    }

    #[test]
    fn wrong_password_aborts_with_invalid_password() {
        let app = tempfile::TempDir::new().unwrap();
        let src = tempfile::TempDir::new().unwrap();
        fs::write(src.path().join("manifest.json"),
            r#"{"encrypted":true,"entries":[{"filename":"1234.maFile","steamid":76561190000000123,"encryption_iv":"srKysrKysrKysrKysrKysg==","encryption_salt":"oaGhoaGhoaGhoaGhoaGhoQ=="}]}"#).unwrap();
        fs::write(src.path().join("1234.maFile"), BLOB).unwrap();
        let strs = vec![src.path().to_string_lossy().into_owned()];
        let err = import_paths_inner(app.path(), &strs, Some("wrong"), None, "steamid", "maFile");
        assert!(matches!(err, Err(AppError::InvalidPassword)));
    }

    #[test]
    fn mixed_batch_partial_success() {
        let app = tempfile::TempDir::new().unwrap();
        let src = tempfile::TempDir::new().unwrap();
        fs::write(src.path().join("good.maFile"), PLAIN).unwrap();
        fs::write(src.path().join("bad.maFile"), "{ not a mafile }").unwrap();
        let report = import(app.path(), &[src.path().to_path_buf()], None);
        assert_eq!(report.imported, 1);
        assert_eq!(report.failed.len(), 1);
    }
}
