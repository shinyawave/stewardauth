// SPDX-FileCopyrightText: 2026 shinyawave
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Explicit-export writers: a single plaintext maFile, or many bundled in a ZIP.
//!
//! These produce secret-bearing maFile JSON (shared_secret, identity_secret) and
//! are only ever driven by an explicit user "Export" action via `export_mafiles`.

use std::collections::HashMap;
use std::io::Write;
use std::path::Path;

use crate::vault::mafile::export_json;
use crate::vault::model::Account;

/// Write one account's plaintext maFile JSON to `dest`.
#[allow(dead_code)] // wired up by the export_mafiles command
pub fn write_single(dest: &Path, account: &Account) -> std::io::Result<()> {
    std::fs::write(dest, export_json(account))
}

/// Bundle many accounts into a ZIP at `dest`, one `<mafile_name>` entry each.
///
/// Entry names are de-duplicated (` (2)`, ` (3)`, …) so the archive is always
/// valid even if two accounts resolve to the same maFile name.
#[allow(dead_code)] // wired up by the export_mafiles command
pub fn write_zip(dest: &Path, items: &[(String, Account)]) -> std::io::Result<()> {
    let file = std::fs::File::create(dest)?;
    let mut zip = zip::ZipWriter::new(file);
    let options: zip::write::FileOptions<'_, ()> =
        zip::write::FileOptions::default().compression_method(zip::CompressionMethod::Deflated);

    let mut seen: HashMap<String, u32> = HashMap::new();
    for (name, account) in items {
        let entry_name = dedup_name(name, &mut seen);
        zip.start_file(entry_name, options)
            .map_err(|e| std::io::Error::other(e.to_string()))?;
        zip.write_all(export_json(account).as_bytes())?;
    }
    zip.finish().map_err(|e| std::io::Error::other(e.to_string()))?;
    Ok(())
}

/// Return `name` the first time it is seen; on collisions insert ` (N)` before
/// the final extension: `foo.maFile` → `foo (2).maFile`.
fn dedup_name(name: &str, seen: &mut HashMap<String, u32>) -> String {
    let count = seen.entry(name.to_string()).or_insert(0);
    *count += 1;
    if *count == 1 {
        return name.to_string();
    }
    match name.rfind('.') {
        Some(dot) => format!("{} ({}){}", &name[..dot], count, &name[dot..]),
        None => format!("{name} ({count})"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::mafile::parse_mafile;

    fn sample(steam_id: &str, account_name: &str) -> Account {
        Account {
            steam_id: steam_id.to_string(),
            account_name: account_name.to_string(),
            shared_secret: "SHARED==".to_string(),
            identity_secret: "IDENT==".to_string(),
            device_id: "android:xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx".to_string(),
            revocation_code: "R00000".to_string(),
        }
    }

    #[test]
    fn write_single_produces_valid_mafile_json() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("out.maFile");
        let acc = sample("76561198000000001", "alice");

        write_single(&dest, &acc).unwrap();

        let json = std::fs::read_to_string(&dest).unwrap();
        let parsed = parse_mafile(&json).unwrap();
        assert_eq!(parsed.steam_id, "76561198000000001");
        assert_eq!(parsed.account_name, "alice");
        assert_eq!(parsed.shared_secret, "SHARED==");
    }

    #[test]
    fn write_zip_has_expected_entries_that_parse_back() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("bundle.zip");
        let items = vec![
            ("76561198000000001.maFile".to_string(), sample("76561198000000001", "alice")),
            ("76561198000000002.maFile".to_string(), sample("76561198000000002", "bob")),
        ];

        write_zip(&dest, &items).unwrap();

        // Re-open the archive and assert entry names + round-trip parse.
        let file = std::fs::File::open(&dest).unwrap();
        let mut archive = zip::ZipArchive::new(file).unwrap();
        assert_eq!(archive.len(), 2);

        let mut names: Vec<String> = (0..archive.len())
            .map(|i| archive.by_index(i).unwrap().name().to_string())
            .collect();
        names.sort();
        assert_eq!(
            names,
            vec![
                "76561198000000001.maFile".to_string(),
                "76561198000000002.maFile".to_string(),
            ]
        );

        // Each entry parses back to the right account.
        use std::io::Read;
        let mut entry = archive.by_name("76561198000000002.maFile").unwrap();
        let mut body = String::new();
        entry.read_to_string(&mut body).unwrap();
        let parsed = parse_mafile(&body).unwrap();
        assert_eq!(parsed.account_name, "bob");
        assert_eq!(parsed.steam_id, "76561198000000002");
    }

    #[test]
    fn write_zip_dedups_colliding_entry_names() {
        let dir = tempfile::tempdir().unwrap();
        let dest = dir.path().join("dupe.zip");
        // Two items intentionally share the same maFile name.
        let items = vec![
            ("dup.maFile".to_string(), sample("76561198000000001", "alice")),
            ("dup.maFile".to_string(), sample("76561198000000002", "bob")),
        ];

        write_zip(&dest, &items).unwrap();

        let file = std::fs::File::open(&dest).unwrap();
        let mut archive = zip::ZipArchive::new(file).unwrap();
        assert_eq!(archive.len(), 2, "collision must not drop an entry");
        let mut names: Vec<String> = (0..archive.len())
            .map(|i| archive.by_index(i).unwrap().name().to_string())
            .collect();
        names.sort();
        assert_eq!(names, vec!["dup (2).maFile".to_string(), "dup.maFile".to_string()]);
    }
}
