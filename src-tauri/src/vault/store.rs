// SPDX-FileCopyrightText: 2026 shinyawave
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Vault manifest stored at `<app_dir>/vault.json`.
//!
//! The manifest holds `encrypted: bool` + `Vec<AccountSummary>`.
//! Per-account secrets go to the macOS Keychain (Task 6), never into this file.
//!
//! When `encrypted` is true, the manifest bytes on disk are wrapped with
//! [`encryption::encrypt`] under the master password.

// ── Stubs for TDD RED phase ────────────────────────────────────────────────────

use std::collections::HashSet;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::vault::model::{AccountSummary, Group, Proxy};

/// The in-memory representation of the vault manifest.
#[allow(dead_code)] // wired up in later tasks
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Vault {
    pub encrypted: bool,
    pub accounts: Vec<AccountSummary>,
    /// Auto-confirm background poll interval, in seconds. Applies per account.
    #[serde(default = "default_poll_interval")]
    pub poll_interval_secs: u32,
    /// Named account groups.
    #[serde(default)]
    pub groups: Vec<Group>,
    /// True once the one-time Keychain→file migration has run.
    #[serde(default)]
    pub migrated: bool,
    /// Stored proxies.
    #[serde(default)]
    pub proxies: Vec<crate::vault::model::Proxy>,
    /// Default proxy id used by accounts with no assignment (empty = direct).
    #[serde(default)]
    pub default_proxy: String,
}

/// Default auto-confirm poll interval (seconds). Chosen conservatively to stay
/// well within Steam's `mobileconf/getlist` rate limit.
pub(crate) fn default_poll_interval() -> u32 {
    60
}

/// Errors produced by vault operations.
#[allow(dead_code)] // wired up in later tasks
#[derive(Debug)]
pub enum VaultError {
    Io(std::io::Error),
    Json(serde_json::Error),
    WrongPasswordOrCorrupt,
    MasterPasswordRequired,
    NoProxies,
}

impl std::fmt::Display for VaultError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            VaultError::Io(e) => write!(f, "vault io error: {e}"),
            VaultError::Json(e) => write!(f, "vault json error: {e}"),
            VaultError::WrongPasswordOrCorrupt => write!(f, "wrong master password or corrupt vault"),
            VaultError::MasterPasswordRequired => write!(f, "master password required for encrypted vault"),
            VaultError::NoProxies => write!(f, "no proxies available to distribute"),
        }
    }
}

impl std::error::Error for VaultError {}

impl From<std::io::Error> for VaultError {
    fn from(e: std::io::Error) -> Self {
        VaultError::Io(e)
    }
}

impl From<serde_json::Error> for VaultError {
    fn from(e: serde_json::Error) -> Self {
        VaultError::Json(e)
    }
}

// ── Pure manifest serialization helpers (testable without Keychain) ────────────

/// Serialize a [`Vault`] to JSON bytes.
pub(crate) fn serialize_manifest(vault: &Vault) -> Result<Vec<u8>, VaultError> {
    Ok(serde_json::to_vec_pretty(vault)?)
}

/// Deserialize a [`Vault`] from JSON bytes.
pub(crate) fn deserialize_manifest(bytes: &[u8]) -> Result<Vault, VaultError> {
    Ok(serde_json::from_slice(bytes)?)
}

/// Write the vault to `<app_dir>/vault.json`, optionally encrypting under `master`.
pub(crate) fn write_manifest(app_dir: &Path, vault: &Vault, master: Option<&str>) -> Result<(), VaultError> {
    // Reject: encrypted vault with no master password would silently write plaintext.
    if vault.encrypted && master.is_none() {
        return Err(VaultError::MasterPasswordRequired);
    }

    let json = serialize_manifest(vault)?;
    let blob = match master {
        Some(pw) if vault.encrypted => crate::vault::encryption::encrypt(&json, pw)
            .map_err(|e| VaultError::Io(std::io::Error::other(e.to_string())))?,
        _ => json,
    };
    let path = app_dir.join("vault.json");
    std::fs::write(&path, blob)?;
    Ok(())
}

/// Read the vault from `<app_dir>/vault.json`, optionally decrypting under `master`.
pub(crate) fn read_manifest(app_dir: &Path, master: Option<&str>) -> Result<Vault, VaultError> {
    let path = app_dir.join("vault.json");
    let blob = std::fs::read(&path)?;

    // Peek: try parsing as plain JSON first; fall back to decryption.
    // The encrypted flag tells us how the file was written, but we don't know
    // until we read it. We try JSON first; if that fails and we have a master
    // password, we try decryption.
    if let Ok(v) = serde_json::from_slice::<Vault>(&blob) {
        if v.encrypted {
            // The file claims to be encrypted but is readable as plain JSON.
            // This indicates the manifest is corrupt or tampered with — a vault
            // flagged `encrypted=true` must never be readable without decryption.
            // Treat it as corrupt to prevent fail-open behaviour.
            return Err(VaultError::WrongPasswordOrCorrupt);
        }
        return Ok(v);
    }

    // Not plain JSON → must be encrypted blob.
    let pw = master.ok_or(VaultError::MasterPasswordRequired)?;
    let json = crate::vault::encryption::decrypt(&blob, pw)
        .map_err(|_| VaultError::WrongPasswordOrCorrupt)?;
    deserialize_manifest(&json)
}

// ── Public API ─────────────────────────────────────────────────────────────────

/// Load the vault manifest from `<app_dir>/vault.json`.
///
/// If the vault does not exist yet, returns an empty unencrypted [`Vault`].
#[allow(dead_code)] // wired up in later tasks
pub fn load_vault(app_dir: &Path, master: Option<&str>) -> Result<Vault, VaultError> {
    let path = app_dir.join("vault.json");
    if !path.exists() {
        return Ok(Vault {
            encrypted: false,
            accounts: Vec::new(),
            poll_interval_secs: default_poll_interval(),
            groups: Vec::new(),
            migrated: false,
            proxies: Vec::new(),
            default_proxy: String::new(),
        });
    }
    read_manifest(app_dir, master)
}

/// Add an account to the vault.
///
/// Writes the account as a plaintext maFile and appends a secret-free
/// [`AccountSummary`] to the manifest. Returns the maFile name.
#[allow(dead_code)]
pub fn add_account(
    app_dir: &Path,
    account: &crate::vault::model::Account,
    naming: &str,
    ext: &str,
    master: Option<&str>,
) -> Result<String, VaultError> {
    // Load the manifest FIRST (fail fast before writing files).
    let mut vault = load_vault(app_dir, master)?;
    vault.accounts.retain(|a| a.steam_id != account.steam_id);

    // Write the plaintext maFile.
    let mafile_name = crate::vault::mafiles::write(app_dir, account, naming, ext)
        .map_err(VaultError::Io)?;

    vault.accounts.push(AccountSummary {
        steam_id: account.steam_id.clone(),
        account_name: account.account_name.clone(),
        status: "active".to_string(),
        mafile_name: mafile_name.clone(),
        ..Default::default()
    });

    // On manifest failure, roll back the file.
    if let Err(e) = write_manifest(app_dir, &vault, master) {
        let _ = crate::vault::mafiles::delete(app_dir, &mafile_name);
        return Err(e);
    }
    Ok(mafile_name)
}

/// Remove an account from the vault (manifest + maFile + legacy Keychain).
#[allow(dead_code)]
pub fn remove_account(
    app_dir: &Path,
    steam_id: &str,
    master: Option<&str>,
) -> Result<(), VaultError> {
    let mut vault = load_vault(app_dir, master)?;
    // Delete the maFile (and legacy Keychain entry, if any).
    if let Some(s) = vault.accounts.iter().find(|a| a.steam_id == steam_id) {
        if !s.mafile_name.is_empty() {
            let _ = crate::vault::mafiles::delete(app_dir, &s.mafile_name);
        }
    }
    let _ = crate::steam::session_store::delete(app_dir, steam_id);

    vault.accounts.retain(|a| a.steam_id != steam_id);
    for g in vault.groups.iter_mut() {
        g.members.retain(|m| m != steam_id);
    }
    write_manifest(app_dir, &vault, master)
}


/// Rename every file-backed account's maFile to match the current naming/ext.
#[allow(dead_code)]
pub fn rename_all_mafiles(
    app_dir: &Path,
    naming: &str,
    ext: &str,
    master: Option<&str>,
) -> Result<(), VaultError> {
    let mut vault = load_vault(app_dir, master)?;
    for summary in vault.accounts.iter_mut() {
        if summary.mafile_name.is_empty() {
            continue;
        }
        let account = match crate::vault::mafiles::read(app_dir, &summary.mafile_name) {
            Ok(a) => a,
            Err(_) => continue,
        };
        let new_name = crate::vault::mafiles::file_name(&account, naming, ext);
        if new_name != summary.mafile_name
            && crate::vault::mafiles::rename(app_dir, &summary.mafile_name, &new_name).is_ok()
        {
            summary.mafile_name = new_name;
        }
    }
    write_manifest(app_dir, &vault, master)
}

#[allow(dead_code)]
pub fn list_proxies(app_dir: &Path, master: Option<&str>) -> Result<Vec<Proxy>, VaultError> {
    Ok(load_vault(app_dir, master)?.proxies)
}

#[allow(dead_code)]
pub fn add_proxies(
    app_dir: &Path,
    proxies: &[Proxy],
    master: Option<&str>,
) -> Result<Vec<Proxy>, VaultError> {
    let mut vault = load_vault(app_dir, master)?;
    for p in proxies {
        if !vault.proxies.iter().any(|e| e.id() == p.id()) {
            vault.proxies.push(p.clone());
        }
    }
    write_manifest(app_dir, &vault, master)?;
    Ok(vault.proxies.clone())
}

#[allow(dead_code)]
pub fn delete_proxy(app_dir: &Path, id: &str, master: Option<&str>) -> Result<(), VaultError> {
    let mut vault = load_vault(app_dir, master)?;
    vault.proxies.retain(|p| p.id() != id);
    if vault.default_proxy == id {
        vault.default_proxy.clear();
    }
    for a in vault.accounts.iter_mut() {
        if a.proxy == id {
            a.proxy.clear();
        }
    }
    write_manifest(app_dir, &vault, master)
}

#[allow(dead_code)]
pub fn set_proxy_favorite(
    app_dir: &Path,
    id: &str,
    favorite: bool,
    master: Option<&str>,
) -> Result<(), VaultError> {
    let mut vault = load_vault(app_dir, master)?;
    if let Some(p) = vault.proxies.iter_mut().find(|p| p.id() == id) {
        p.favorite = favorite;
    }
    write_manifest(app_dir, &vault, master)
}

#[allow(dead_code)]
pub fn set_default_proxy(app_dir: &Path, id: &str, master: Option<&str>) -> Result<(), VaultError> {
    let mut vault = load_vault(app_dir, master)?;
    vault.default_proxy = id.to_string();
    write_manifest(app_dir, &vault, master)
}

#[allow(dead_code)]
pub fn assign_proxy(
    app_dir: &Path,
    steam_id: &str,
    id: &str,
    master: Option<&str>,
) -> Result<(), VaultError> {
    let mut vault = load_vault(app_dir, master)?;
    if let Some(a) = vault.accounts.iter_mut().find(|a| a.steam_id == steam_id) {
        a.proxy = id.to_string();
    }
    write_manifest(app_dir, &vault, master)
}

/// Assign `proxy_id` to every account whose steam_id is in `steam_ids`, in a
/// single manifest load/write. Ids not present in the vault are ignored.
#[allow(dead_code)]
pub fn bulk_assign_proxy(
    app_dir: &Path,
    steam_ids: &[String],
    proxy_id: &str,
    master: Option<&str>,
) -> Result<(), VaultError> {
    let mut vault = load_vault(app_dir, master)?;
    for a in vault.accounts.iter_mut() {
        if steam_ids.iter().any(|id| id == &a.steam_id) {
            a.proxy = proxy_id.to_string();
        }
    }
    write_manifest(app_dir, &vault, master)
}

/// Result of a text-based `login:proxy` bulk assignment.
#[allow(dead_code)]
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct AssignReport {
    pub assigned: usize,
    pub unmatched: Vec<String>,
}

/// Evenly distribute the selected accounts across all stored proxies using a
/// least-loaded-first strategy. Existing proxy pins across the whole vault seed
/// the starting load so distribution respects them. Deterministic: ties are
/// broken by proxy order. Errors if there are no proxies.
#[allow(dead_code)]
pub fn distribute_proxies(
    app_dir: &Path,
    steam_ids: &[String],
    master: Option<&str>,
) -> Result<(), VaultError> {
    let mut vault = load_vault(app_dir, master)?;
    if vault.proxies.is_empty() {
        return Err(VaultError::NoProxies);
    }

    // proxy id → current load, seeded from accounts NOT in the selection.
    let selected: std::collections::HashSet<&String> = steam_ids.iter().collect();
    let proxy_ids: Vec<String> = vault.proxies.iter().map(|p| p.id()).collect();
    let mut load: std::collections::HashMap<String, usize> =
        proxy_ids.iter().map(|id| (id.clone(), 0usize)).collect();
    for a in &vault.accounts {
        if !selected.contains(&a.steam_id) && !a.proxy.is_empty() {
            if let Some(c) = load.get_mut(&a.proxy) {
                *c += 1;
            }
        }
    }

    // Assign each selected account to the currently least-loaded proxy.
    for id in steam_ids {
        // Pick least-loaded, ties broken by first-declared proxy order.
        let target = proxy_ids
            .iter()
            .min_by_key(|pid| load.get(*pid).copied().unwrap_or(0))
            .cloned();
        let Some(target) = target else { continue };
        if let Some(a) = vault.accounts.iter_mut().find(|a| &a.steam_id == id) {
            a.proxy = target.clone();
            *load.entry(target).or_default() += 1;
        }
    }

    write_manifest(app_dir, &vault, master)
}

/// Assign proxies from `login:proxy` text. `login` matches `account_name`;
/// `proxy` is a stored proxy id or a line parseable by `parse_proxy_line`
/// (added to the vault if new). One manifest load/write. Reports how many were
/// assigned and which logins could not be matched/parsed.
#[allow(dead_code)]
pub fn assign_by_text(
    app_dir: &Path,
    text: &str,
    master: Option<&str>,
) -> Result<AssignReport, VaultError> {
    let mut vault = load_vault(app_dir, master)?;
    let mut report = AssignReport::default();

    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() {
            continue;
        }
        // Split on the FIRST ':' — proxy values themselves contain ':'.
        let Some((login, proxy_val)) = line.split_once(':') else {
            report.unmatched.push(line.to_string());
            continue;
        };
        let login = login.trim();
        let proxy_val = proxy_val.trim();

        // Resolve the target proxy id: an existing id, else a parseable line.
        let proxy_id = if vault.proxies.iter().any(|p| p.id() == proxy_val) {
            Some(proxy_val.to_string())
        } else if let Some(parsed) = crate::steam::proxy::parse_proxy_line(proxy_val) {
            let id = parsed.id();
            if !vault.proxies.iter().any(|p| p.id() == id) {
                vault.proxies.push(parsed);
            }
            Some(id)
        } else {
            None
        };

        match proxy_id {
            Some(id) => {
                if let Some(a) = vault.accounts.iter_mut().find(|a| a.account_name == login) {
                    a.proxy = id;
                    report.assigned += 1;
                } else {
                    report.unmatched.push(login.to_string());
                }
            }
            None => report.unmatched.push(login.to_string()),
        }
    }

    write_manifest(app_dir, &vault, master)?;
    Ok(report)
}

#[allow(dead_code)]
pub fn unpin_proxy(app_dir: &Path, steam_id: &str, master: Option<&str>) -> Result<(), VaultError> {
    let mut vault = load_vault(app_dir, master)?;
    if let Some(a) = vault.accounts.iter_mut().find(|a| a.steam_id == steam_id) {
        a.proxy.clear();
    }
    write_manifest(app_dir, &vault, master)
}

/// Resolve the effective proxy for an account: its assignment, else the default.
#[allow(dead_code)]
pub fn resolve_proxy(
    app_dir: &Path,
    steam_id: &str,
    master: Option<&str>,
) -> Result<Option<Proxy>, VaultError> {
    let vault = load_vault(app_dir, master)?;
    let assigned = vault
        .accounts
        .iter()
        .find(|a| a.steam_id == steam_id)
        .map(|a| a.proxy.clone())
        .filter(|p| !p.is_empty());
    let id = assigned.or_else(|| {
        if vault.default_proxy.is_empty() {
            None
        } else {
            Some(vault.default_proxy.clone())
        }
    });
    Ok(id.and_then(|id| vault.proxies.iter().find(|p| p.id() == id).cloned()))
}

/// Set the per-account auto-confirm flags for a batch of accounts.
///
/// `market` / `trade` are optional: `None` leaves that flag untouched, `Some(v)`
/// sets it. Only accounts whose `steam_id` appears in `steam_ids` are modified.
/// Returns the full, updated account list so callers can refresh their view.
#[allow(dead_code)] // wired up in later tasks
pub fn set_auto_confirm(
    app_dir: &Path,
    steam_ids: &[String],
    market: Option<bool>,
    trade: Option<bool>,
    master: Option<&str>,
) -> Result<Vec<AccountSummary>, VaultError> {
    let mut vault = load_vault(app_dir, master)?;
    for acc in vault.accounts.iter_mut() {
        if steam_ids.iter().any(|id| id == &acc.steam_id) {
            if let Some(m) = market {
                acc.auto_confirm_market = m;
            }
            if let Some(t) = trade {
                acc.auto_confirm_trade = t;
            }
        }
    }
    write_manifest(app_dir, &vault, master)?;
    Ok(vault.accounts.clone())
}

/// Persist the global auto-confirm poll interval (seconds). Callers should clamp
/// to a safe minimum before calling.
#[allow(dead_code)] // wired up in later tasks
pub fn set_poll_interval(
    app_dir: &Path,
    seconds: u32,
    master: Option<&str>,
) -> Result<(), VaultError> {
    let mut vault = load_vault(app_dir, master)?;
    vault.poll_interval_secs = seconds;
    write_manifest(app_dir, &vault, master)
}

/// List all groups.
#[allow(dead_code)]
pub fn list_groups(app_dir: &Path, master: Option<&str>) -> Result<Vec<Group>, VaultError> {
    Ok(load_vault(app_dir, master)?.groups)
}

/// Create an empty group. No-op if a group with the (trimmed) name already exists
/// or the name is empty.
#[allow(dead_code)]
pub fn create_group(app_dir: &Path, name: &str, master: Option<&str>) -> Result<(), VaultError> {
    let name = name.trim();
    if name.is_empty() {
        return Ok(());
    }
    let mut vault = load_vault(app_dir, master)?;
    if !vault.groups.iter().any(|g| g.name == name) {
        vault.groups.push(Group {
            name: name.to_string(),
            members: Vec::new(),
        });
        write_manifest(app_dir, &vault, master)?;
    }
    Ok(())
}

/// Delete a group (its members are unaffected — only the grouping is removed).
#[allow(dead_code)]
pub fn delete_group(app_dir: &Path, name: &str, master: Option<&str>) -> Result<(), VaultError> {
    let mut vault = load_vault(app_dir, master)?;
    vault.groups.retain(|g| g.name != name);
    write_manifest(app_dir, &vault, master)
}

/// Add an account to a group, creating the group if it does not exist. Dedups.
#[allow(dead_code)]
pub fn add_to_group(
    app_dir: &Path,
    steam_id: &str,
    name: &str,
    master: Option<&str>,
) -> Result<(), VaultError> {
    let name = name.trim();
    if name.is_empty() {
        return Ok(());
    }
    let mut vault = load_vault(app_dir, master)?;
    match vault.groups.iter_mut().find(|g| g.name == name) {
        Some(g) => {
            if !g.members.iter().any(|m| m == steam_id) {
                g.members.push(steam_id.to_string());
            }
        }
        None => vault.groups.push(Group {
            name: name.to_string(),
            members: vec![steam_id.to_string()],
        }),
    }
    write_manifest(app_dir, &vault, master)
}

/// Remove an account from a specific group.
#[allow(dead_code)]
pub fn remove_from_group(
    app_dir: &Path,
    steam_id: &str,
    name: &str,
    master: Option<&str>,
) -> Result<(), VaultError> {
    let mut vault = load_vault(app_dir, master)?;
    if let Some(g) = vault.groups.iter_mut().find(|g| g.name == name) {
        g.members.retain(|m| m != steam_id);
    }
    write_manifest(app_dir, &vault, master)
}

/// Reconcile the manifest with the maFiles directory: add newly-found maFiles and
/// prune manifest accounts whose backing maFile has vanished. Returns whether the
/// manifest changed.
///
/// SAFEGUARD: if the maFiles directory is missing or unreadable, treat it as a
/// transient condition and make NO changes (returns `Ok(false)`). Accounts with an
/// empty `mafile_name` (legacy/Keychain-backed) are never pruned.
pub fn reconcile_folder(app_dir: &Path, master: Option<&str>) -> Result<bool, VaultError> {
    // Build the raw filename set from the actual on-disk directory entries.
    // This MUST be independent of parsing so that a file that physically EXISTS
    // but is momentarily unreadable/corrupt (e.g. caught mid-write when the
    // file watcher fires) is NOT treated as vanished and pruned.
    // Safeguard: if the dir is missing or unreadable, make NO changes.
    let present: HashSet<String> = match std::fs::read_dir(crate::vault::mafiles::dir(app_dir)) {
        Ok(rd) => rd
            .flatten()
            .filter_map(|e| e.file_name().into_string().ok())
            .collect(),
        Err(_) => return Ok(false), // safeguard: dir missing/unreadable → no changes
    };

    let mut vault = load_vault(app_dir, master)?;
    // Use scan (parsed accounts) only for the ADD step — adding a new account
    // legitimately requires a parseable file.
    let scanned = crate::vault::mafiles::scan(app_dir); // Vec<(String, Account)>
    let known: HashSet<String> = vault.accounts.iter().map(|a| a.steam_id.clone()).collect();

    let mut changed = false;

    // Add newly-found maFiles.
    for (file_name, account) in &scanned {
        if !known.contains(&account.steam_id) {
            vault.accounts.push(AccountSummary {
                steam_id: account.steam_id.clone(),
                account_name: account.account_name.clone(),
                status: "active".to_string(),
                mafile_name: file_name.clone(),
                ..Default::default()
            });
            changed = true;
        }
    }

    // Prune accounts whose backing maFile vanished (skip legacy empty mafile_name).
    let mut pruned: Vec<String> = Vec::new();
    vault.accounts.retain(|a| {
        let vanished = !a.mafile_name.is_empty() && !present.contains(&a.mafile_name);
        if vanished {
            pruned.push(a.steam_id.clone());
        }
        !vanished
    });
    if !pruned.is_empty() {
        changed = true;
        for sid in &pruned {
            for g in vault.groups.iter_mut() {
                g.members.retain(|m| m != sid);
            }
            let _ = crate::steam::session_store::delete(app_dir, sid);
        }
    }

    if changed {
        write_manifest(app_dir, &vault, master)?;
    }
    Ok(changed)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── Pure manifest + encryption logic (no Keychain) ────────────────────────

    /// Round-trip: serialize → write unencrypted → read back → assert summary present.
    /// Also asserts that `vault.json` does NOT contain the shared_secret string.
    #[test]
    fn unencrypted_manifest_round_trip_no_secrets_on_disk() {
        let dir = tempfile::tempdir().expect("tempdir");
        let app_dir = dir.path();

        // Build a vault with one account summary (no secrets — this is what the manifest holds).
        let summary = AccountSummary {
            steam_id: "76561198000000001".to_string(),
            account_name: "testuser".to_string(),
            status: "active".to_string(),
            ..Default::default()
        };
        let vault = Vault {
            encrypted: false,
            accounts: vec![summary],
            poll_interval_secs: default_poll_interval(),
            groups: Vec::new(),
            migrated: false,
            proxies: Vec::new(),
            default_proxy: String::new(),
        };

        // Write the manifest.
        write_manifest(app_dir, &vault, None).expect("write_manifest");

        // Read it back and assert the summary is present.
        let loaded = read_manifest(app_dir, None).expect("read_manifest");
        assert_eq!(loaded.accounts.len(), 1);
        assert_eq!(loaded.accounts[0].steam_id, "76561198000000001");
        assert_eq!(loaded.accounts[0].account_name, "testuser");

        // Assert the on-disk file does NOT contain any secret strings.
        let raw = std::fs::read_to_string(app_dir.join("vault.json")).expect("read raw");
        assert!(!raw.contains("shared_secret"), "manifest must not contain shared_secret");
        assert!(!raw.contains("identity_secret"), "manifest must not contain identity_secret");
    }

    /// Round-trip: write encrypted vault → read with correct password succeeds.
    #[test]
    fn encrypted_manifest_round_trip() {
        let dir = tempfile::tempdir().expect("tempdir");
        let app_dir = dir.path();
        let pw = "s3cr3t-master";

        let summary = AccountSummary {
            steam_id: "76561198000000002".to_string(),
            account_name: "cryptouser".to_string(),
            status: "active".to_string(),
            ..Default::default()
        };
        let vault = Vault {
            encrypted: true,
            accounts: vec![summary],
            poll_interval_secs: default_poll_interval(),
            groups: Vec::new(),
            migrated: false,
            proxies: Vec::new(),
            default_proxy: String::new(),
        };

        write_manifest(app_dir, &vault, Some(pw)).expect("write encrypted");

        // Correct password → succeeds.
        let loaded = read_manifest(app_dir, Some(pw)).expect("read encrypted");
        assert_eq!(loaded.accounts.len(), 1);
        assert_eq!(loaded.accounts[0].steam_id, "76561198000000002");

        // Wrong password → fails.
        assert!(read_manifest(app_dir, Some("wrong-pw")).is_err());
    }

    /// Assert that the encrypted blob on disk is NOT plain JSON (i.e. is truly encrypted).
    #[test]
    fn encrypted_vault_is_not_plain_json_on_disk() {
        let dir = tempfile::tempdir().expect("tempdir");
        let app_dir = dir.path();
        let pw = "master";

        let vault = Vault {
            encrypted: true,
            accounts: vec![AccountSummary {
                steam_id: "76561198000000003".to_string(),
                account_name: "user3".to_string(),
                status: "active".to_string(),
                ..Default::default()
            }],
            poll_interval_secs: default_poll_interval(),
            groups: Vec::new(),
            migrated: false,
            proxies: Vec::new(),
            default_proxy: String::new(),
        };
        write_manifest(app_dir, &vault, Some(pw)).expect("write");

        let raw = std::fs::read(app_dir.join("vault.json")).expect("read raw bytes");
        // Cannot be parsed as JSON.
        assert!(serde_json::from_slice::<serde_json::Value>(&raw).is_err());
    }

    // ── Full add_account / remove_account round-trip (touches Keychain) ───────

    #[test]
    fn add_remove_account_round_trip_unencrypted() {
        let dir = tempfile::tempdir().expect("tempdir");
        let app_dir = dir.path();

        let account = crate::vault::model::Account {
            steam_id: "76561198000000099".to_string(),
            account_name: "keychain_test".to_string(),
            shared_secret: "SHAREDSECRET_XYZ".to_string(),
            identity_secret: "IDENTITYSECRET_ABC".to_string(),
            device_id: "android:xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx".to_string(),
            revocation_code: "Rxxxxx".to_string(),
        };

        add_account(app_dir, &account, "steamid", "maFile", None).expect("add_account");

        // Reload and assert summary present.
        let vault = load_vault(app_dir, None).expect("load_vault");
        assert_eq!(vault.accounts.len(), 1);
        assert_eq!(vault.accounts[0].steam_id, "76561198000000099");
        assert_eq!(vault.accounts[0].mafile_name, "76561198000000099.maFile");

        // Assert the manifest does NOT contain secrets.
        let raw = std::fs::read_to_string(app_dir.join("vault.json")).expect("read raw");
        assert!(!raw.contains("SHAREDSECRET_XYZ"), "manifest must not contain shared_secret value");
        assert!(!raw.contains("IDENTITYSECRET_ABC"), "manifest must not contain identity_secret value");

        // Remove.
        remove_account(app_dir, "76561198000000099", None).expect("remove");
        let vault2 = load_vault(app_dir, None).expect("load after remove");
        assert_eq!(vault2.accounts.len(), 0);
    }

    #[test]
    fn add_account_encrypted_wrong_master_fails() {
        let dir = tempfile::tempdir().expect("tempdir");
        let app_dir = dir.path();
        let pw = "correct-pw";

        // Pre-create an encrypted empty vault.
        let empty = Vault {
            encrypted: true,
            accounts: vec![],
            poll_interval_secs: default_poll_interval(),
            groups: Vec::new(),
            migrated: false,
            proxies: Vec::new(),
            default_proxy: String::new(),
        };
        write_manifest(app_dir, &empty, Some(pw)).expect("write initial encrypted vault");

        let account = crate::vault::model::Account {
            steam_id: "76561198000000077".to_string(),
            account_name: "enc_test".to_string(),
            shared_secret: "SECRET77".to_string(),
            identity_secret: "IDENTITY77".to_string(),
            device_id: "android:xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx".to_string(),
            revocation_code: "Rxxxxx".to_string(),
        };
        add_account(app_dir, &account, "steamid", "maFile", Some(pw)).expect("add_account");

        // Correct password works.
        let v = load_vault(app_dir, Some(pw)).expect("load correct pw");
        assert_eq!(v.accounts.len(), 1);

        // Wrong password fails.
        assert!(load_vault(app_dir, Some("wrong")).is_err());
    }

    // ── Regression test: encrypted vault with no master → MasterPasswordRequired ─

    /// Regression test for Fix 2 (unlock gate precondition).
    ///
    /// Writes an encrypted manifest with a known master password, then attempts
    /// to load it with `master: None`.  Asserts that the result is
    /// `VaultError::MasterPasswordRequired` — which error.rs maps to
    /// `AppError::InvalidPassword`, triggering the frontend unlock gate.
    ///
    /// This test is CI-runnable: it exercises only the manifest encrypt/decrypt
    /// layer and does not touch the macOS Keychain.
    #[test]
    fn encrypted_vault_with_no_master_returns_master_password_required() {
        let dir = tempfile::tempdir().expect("tempdir");
        let app_dir = dir.path();
        let pw = "unlock-gate-test-pw";

        // Write an encrypted vault with one account summary.
        let vault = Vault {
            encrypted: true,
            accounts: vec![AccountSummary {
                steam_id: "76561198000000042".to_string(),
                account_name: "gatetest".to_string(),
                status: "active".to_string(),
                ..Default::default()
            }],
            poll_interval_secs: default_poll_interval(),
            groups: Vec::new(),
            migrated: false,
            proxies: Vec::new(),
            default_proxy: String::new(),
        };
        write_manifest(app_dir, &vault, Some(pw)).expect("write encrypted manifest");

        // Attempt to load with master=None.  Must return MasterPasswordRequired
        // so the frontend unlock gate is triggered (via AppError::InvalidPassword).
        let err = load_vault(app_dir, None)
            .expect_err("load_vault with no master on encrypted vault must fail");
        assert!(
            matches!(err, VaultError::MasterPasswordRequired),
            "expected MasterPasswordRequired, got: {err:?}"
        );

        // Sanity: correct master should succeed.
        let loaded = load_vault(app_dir, Some(pw))
            .expect("load_vault with correct master must succeed");
        assert_eq!(loaded.accounts.len(), 1);
        assert_eq!(loaded.accounts[0].steam_id, "76561198000000042");
    }

    #[test]
    fn group_crud_round_trip() {
        let dir = tempfile::tempdir().expect("tempdir");
        let app_dir = dir.path();

        // create + add
        create_group(app_dir, "Traders", None).expect("create");
        add_to_group(app_dir, "111", "Traders", None).expect("add 111");
        add_to_group(app_dir, "222", "Traders", None).expect("add 222");
        // dedup
        add_to_group(app_dir, "111", "Traders", None).expect("add dup");

        let groups = list_groups(app_dir, None).expect("list");
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].name, "Traders");
        assert_eq!(groups[0].members, vec!["111".to_string(), "222".to_string()]);

        // add_to_group creates a missing group
        add_to_group(app_dir, "333", "Sellers", None).expect("auto-create");
        assert_eq!(list_groups(app_dir, None).unwrap().len(), 2);

        // remove member
        remove_from_group(app_dir, "111", "Traders", None).expect("remove");
        let g = list_groups(app_dir, None).unwrap();
        let traders = g.iter().find(|g| g.name == "Traders").unwrap();
        assert_eq!(traders.members, vec!["222".to_string()]);

        // delete group
        delete_group(app_dir, "Traders", None).expect("delete");
        assert_eq!(list_groups(app_dir, None).unwrap().len(), 1);
    }

    #[test]
    fn create_group_is_idempotent_and_trims() {
        let dir = tempfile::tempdir().expect("tempdir");
        let app_dir = dir.path();
        create_group(app_dir, "  Dup  ", None).expect("create");
        create_group(app_dir, "Dup", None).expect("create again");
        let groups = list_groups(app_dir, None).unwrap();
        assert_eq!(groups.len(), 1);
        assert_eq!(groups[0].name, "Dup");
    }

    #[test]
    fn remove_account_strips_group_membership_pure() {
        // Pure manifest-level check: build a vault with a group, write it, then
        // simulate removal by editing + rewriting (no Keychain).
        let dir = tempfile::tempdir().expect("tempdir");
        let app_dir = dir.path();
        let mut vault = Vault {
            encrypted: false,
            accounts: vec![],
            poll_interval_secs: default_poll_interval(),
            groups: vec![Group { name: "G".into(), members: vec!["a".into(), "b".into()] }],
            migrated: false,
            proxies: Vec::new(),
            default_proxy: String::new(),
        };
        write_manifest(app_dir, &vault, None).unwrap();

        // Emulate the cleanup remove_account performs:
        let steam_id = "a";
        for g in vault.groups.iter_mut() {
            g.members.retain(|m| m != steam_id);
        }
        write_manifest(app_dir, &vault, None).unwrap();

        let loaded = load_vault(app_dir, None).unwrap();
        assert_eq!(loaded.groups[0].members, vec!["b".to_string()]);
    }

    #[test]
    fn rename_all_applies_naming_mode() {
        let dir = tempfile::tempdir().unwrap();
        let account = crate::vault::model::Account {
            steam_id: "76561198000000901".into(),
            account_name: "renameme".into(),
            shared_secret: "SS==".into(),
            identity_secret: "IS==".into(),
            device_id: "android:x".into(),
            revocation_code: "R00000".into(),
        };
        add_account(dir.path(), &account, "steamid", "maFile", None).unwrap();
        rename_all_mafiles(dir.path(), "login", "maFile", None).unwrap();
        let vault = load_vault(dir.path(), None).unwrap();
        assert_eq!(vault.accounts[0].mafile_name, "renameme.maFile");
    }

    #[test]
    fn add_account_writes_file_and_summary() {
        let dir = tempfile::tempdir().unwrap();
        let account = crate::vault::model::Account {
            steam_id: "76561198000000123".into(),
            account_name: "fileuser".into(),
            shared_secret: "SS==".into(),
            identity_secret: "IS==".into(),
            device_id: "android:x".into(),
            revocation_code: "R00000".into(),
        };
        let name = add_account(dir.path(), &account, "steamid", "maFile", None).unwrap();
        assert_eq!(name, "76561198000000123.maFile");
        let vault = load_vault(dir.path(), None).unwrap();
        assert_eq!(vault.accounts.len(), 1);
        assert_eq!(vault.accounts[0].mafile_name, "76561198000000123.maFile");
        // secrets are in the file, not the manifest
        let raw = std::fs::read_to_string(dir.path().join("vault.json")).unwrap();
        assert!(!raw.contains("SS=="));
    }

    #[test]
    fn bulk_assign_proxy_sets_all_selected_in_one_pass() {
        use crate::vault::model::Proxy;
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path();

        let p = Proxy { scheme: "http".into(), host: "9.9.9.9".into(), port: 8080, ..Default::default() };
        add_proxies(app, &[p.clone()], None).unwrap();

        for (i, name) in ["a", "b", "c"].iter().enumerate() {
            let account = crate::vault::model::Account {
                steam_id: format!("7656119800000000{i}"),
                account_name: (*name).into(),
                shared_secret: "SS==".into(),
                identity_secret: "IS==".into(),
                device_id: "android:x".into(),
                revocation_code: "R00000".into(),
            };
            add_account(app, &account, "steamid", "maFile", None).unwrap();
        }

        let ids: Vec<String> = (0..3).map(|i| format!("7656119800000000{i}")).collect();
        bulk_assign_proxy(app, &ids, &p.id(), None).unwrap();

        let vault = load_vault(app, None).unwrap();
        assert!(vault.accounts.iter().all(|a| a.proxy == p.id()));

        // An id not in the batch is untouched (only the three we added exist, so
        // assert a bogus id in the batch is silently ignored — no panic, no error).
        bulk_assign_proxy(app, &["does-not-exist".into()], &p.id(), None).unwrap();
        assert_eq!(load_vault(app, None).unwrap().accounts.len(), 3);
    }

    #[test]
    fn distribute_balances_evenly_and_errors_without_proxies() {
        use crate::vault::model::Proxy;
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path();

        // No proxies yet → error.
        let ids0: Vec<String> = vec!["1".into()];
        assert!(matches!(
            distribute_proxies(app, &ids0, None),
            Err(VaultError::NoProxies)
        ));

        // 3 proxies.
        let proxies: Vec<Proxy> = (0..3)
            .map(|i| Proxy { scheme: "http".into(), host: format!("10.0.0.{i}"), port: 80, ..Default::default() })
            .collect();
        add_proxies(app, &proxies, None).unwrap();

        // 6 accounts.
        let mut ids = Vec::new();
        for i in 0..6 {
            let account = crate::vault::model::Account {
                steam_id: format!("acc{i}"),
                account_name: format!("name{i}"),
                shared_secret: "SS==".into(),
                identity_secret: "IS==".into(),
                device_id: "android:x".into(),
                revocation_code: "R00000".into(),
            };
            add_account(app, &account, "steamid", "maFile", None).unwrap();
            ids.push(format!("acc{i}"));
        }

        distribute_proxies(app, &ids, None).unwrap();

        // 6 accounts / 3 proxies → exactly 2 each.
        let vault = load_vault(app, None).unwrap();
        let mut counts: std::collections::HashMap<String, usize> = std::collections::HashMap::new();
        for a in &vault.accounts {
            *counts.entry(a.proxy.clone()).or_default() += 1;
        }
        assert_eq!(counts.len(), 3, "all three proxies used");
        assert!(counts.values().all(|&c| c == 2), "even split: {counts:?}");
    }

    #[test]
    fn assign_by_text_matches_by_account_name_adds_unknown_and_reports() {
        use crate::vault::model::Proxy;
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path();

        // A pre-stored proxy, referenced by id.
        let known = Proxy { scheme: "http".into(), host: "1.1.1.1".into(), port: 80, ..Default::default() };
        add_proxies(app, &[known.clone()], None).unwrap();

        // Two accounts by name.
        for (i, name) in [("0", "alice"), ("1", "bob")] {
            let account = crate::vault::model::Account {
                steam_id: format!("id{i}"),
                account_name: name.into(),
                shared_secret: "SS==".into(),
                identity_secret: "IS==".into(),
                device_id: "android:x".into(),
                revocation_code: "R00000".into(),
            };
            add_account(app, &account, "steamid", "maFile", None).unwrap();
        }

        // alice → known proxy by id; bob → a brand-new proxy URL; carol → unmatched.
        let text = format!(
            "alice:{}\nbob:2.2.2.2:9090\ncarol:3.3.3.3:80\n\n",
            known.id()
        );
        let report = assign_by_text(app, &text, None).unwrap();

        assert_eq!(report.assigned, 2);
        assert_eq!(report.unmatched, vec!["carol".to_string()]);

        let vault = load_vault(app, None).unwrap();
        let alice = vault.accounts.iter().find(|a| a.account_name == "alice").unwrap();
        assert_eq!(alice.proxy, known.id());

        // bob's new proxy was added to the vault and assigned.
        let new_id = crate::steam::proxy::parse_proxy_line("2.2.2.2:9090").unwrap().id();
        assert!(vault.proxies.iter().any(|p| p.id() == new_id), "new proxy stored");
        let bob = vault.accounts.iter().find(|a| a.account_name == "bob").unwrap();
        assert_eq!(bob.proxy, new_id);
    }

    #[test]
    fn assign_by_text_reports_unparseable_proxy_as_unmatched() {
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path();
        let account = crate::vault::model::Account {
            steam_id: "idz".into(),
            account_name: "zed".into(),
            shared_secret: "SS==".into(),
            identity_secret: "IS==".into(),
            device_id: "android:x".into(),
            revocation_code: "R00000".into(),
        };
        add_account(app, &account, "steamid", "maFile", None).unwrap();

        // zed exists but "garbage" is neither a stored id nor a parseable proxy.
        let report = assign_by_text(app, "zed:garbage", None).unwrap();
        assert_eq!(report.assigned, 0);
        assert_eq!(report.unmatched, vec!["zed".to_string()]);
    }

    #[test]
    fn distribute_respects_existing_pins_when_balancing() {
        use crate::vault::model::Proxy;
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path();
        let a = Proxy { scheme: "http".into(), host: "a".into(), port: 80, ..Default::default() };
        let b = Proxy { scheme: "http".into(), host: "b".into(), port: 80, ..Default::default() };
        add_proxies(app, &[a.clone(), b.clone()], None).unwrap();

        // 3 accounts; pin the first two to proxy `a`, then distribute only the third.
        for i in 0..3 {
            let account = crate::vault::model::Account {
                steam_id: format!("s{i}"),
                account_name: format!("n{i}"),
                shared_secret: "SS==".into(),
                identity_secret: "IS==".into(),
                device_id: "android:x".into(),
                revocation_code: "R00000".into(),
            };
            add_account(app, &account, "steamid", "maFile", None).unwrap();
        }
        assign_proxy(app, "s0", &a.id(), None).unwrap();
        assign_proxy(app, "s1", &a.id(), None).unwrap();

        distribute_proxies(app, &["s2".into()], None).unwrap();

        // `a` already had 2; `b` had 0 → the least-loaded is `b`, so s2 → b.
        let vault = load_vault(app, None).unwrap();
        let s2 = vault.accounts.iter().find(|x| x.steam_id == "s2").unwrap();
        assert_eq!(s2.proxy, b.id());
    }

    #[test]
    fn proxy_crud_and_assignment() {
        use crate::vault::model::Proxy;
        let dir = tempfile::tempdir().unwrap();
        let app = dir.path();
        let p1 = Proxy { scheme: "http".into(), host: "1.1.1.1".into(), port: 80, ..Default::default() };
        let p2 = Proxy { scheme: "socks5".into(), host: "2.2.2.2".into(), port: 1080, user: "u".into(), pass: "p".into(), favorite: false };
        add_proxies(app, &[p1.clone(), p2.clone(), p1.clone()], None).unwrap();
        assert_eq!(list_proxies(app, None).unwrap().len(), 2); // dedup

        // assign to an account present in the manifest
        let account = crate::vault::model::Account {
            steam_id: "76561198000000700".into(),
            account_name: "px".into(),
            shared_secret: "SS==".into(),
            identity_secret: "IS==".into(),
            device_id: "android:x".into(),
            revocation_code: "R00000".into(),
        };
        add_account(app, &account, "steamid", "maFile", None).unwrap();
        assign_proxy(app, "76561198000000700", &p2.id(), None).unwrap();
        let resolved = resolve_proxy(app, "76561198000000700", None).unwrap().unwrap();
        assert_eq!(resolved.host, "2.2.2.2");

        // default fallback for a different (unassigned) account id
        set_default_proxy(app, &p1.id(), None).unwrap();
        // unpin → falls back to default
        unpin_proxy(app, "76561198000000700", None).unwrap();
        let resolved2 = resolve_proxy(app, "76561198000000700", None).unwrap().unwrap();
        assert_eq!(resolved2.host, "1.1.1.1");

        // delete clears default + assignment references
        delete_proxy(app, &p1.id(), None).unwrap();
        assert!(list_proxies(app, None).unwrap().iter().all(|p| p.id() != p1.id()));
    }

    // ── reconcile_folder tests ────────────────────────────────────────────────

    fn mk_account(steam_id: &str, name: &str) -> crate::vault::model::Account {
        crate::vault::model::Account {
            steam_id: steam_id.into(),
            account_name: name.into(),
            shared_secret: "cnOgv/KdpLoP6Nbh0GMkXkPnNqmc0Q=".into(),
            identity_secret: "AAAAAAAAAAAAAAAAAAAAAAAAAAA=".into(),
            device_id: "android:00000000-0000-0000-0000-000000000000".into(),
            revocation_code: "R00000".into(),
        }
    }

    #[test]
    fn reconcile_prunes_vanished_mafile() {
        let td = tempfile::TempDir::new().unwrap();
        let dir = td.path();
        add_account(dir, &mk_account("76561190000000001", "a"), "steamid", "maFile", None).unwrap();
        // Delete the backing maFile out from under the manifest.
        crate::vault::mafiles::delete(dir, "76561190000000001.maFile").unwrap();
        let changed = reconcile_folder(dir, None).unwrap();
        assert!(changed);
        let vault = load_vault(dir, None).unwrap();
        assert!(vault.accounts.is_empty(), "pruned account should be gone");
    }

    #[test]
    fn reconcile_safeguard_when_dir_missing() {
        let td = tempfile::TempDir::new().unwrap();
        let dir = td.path();
        add_account(dir, &mk_account("76561190000000002", "b"), "steamid", "maFile", None).unwrap();
        // Remove the whole maFiles directory → transient/unavailable.
        std::fs::remove_dir_all(crate::vault::mafiles::dir(dir)).unwrap();
        let changed = reconcile_folder(dir, None).unwrap();
        assert!(!changed, "must not change manifest when dir is unreadable");
        let vault = load_vault(dir, None).unwrap();
        assert_eq!(vault.accounts.len(), 1, "account must be preserved");
    }

    #[test]
    fn reconcile_adds_new_mafile() {
        let td = tempfile::TempDir::new().unwrap();
        let dir = td.path();
        // Write a maFile directly, not yet in the manifest.
        crate::vault::mafiles::write(dir, &mk_account("76561190000000003", "c"), "steamid", "maFile").unwrap();
        let changed = reconcile_folder(dir, None).unwrap();
        assert!(changed);
        let vault = load_vault(dir, None).unwrap();
        assert_eq!(vault.accounts.len(), 1);
        assert_eq!(vault.accounts[0].steam_id, "76561190000000003");
    }

    #[test]
    fn reconcile_cleans_groups_of_pruned() {
        let td = tempfile::TempDir::new().unwrap();
        let dir = td.path();
        add_account(dir, &mk_account("76561190000000004", "d"), "steamid", "maFile", None).unwrap();
        // Put the account into a group.
        let mut vault = load_vault(dir, None).unwrap();
        vault.groups.push(crate::vault::model::Group {
            name: "g1".into(),
            members: vec!["76561190000000004".into()],
        });
        write_manifest(dir, &vault, None).unwrap();
        crate::vault::mafiles::delete(dir, "76561190000000004.maFile").unwrap();
        reconcile_folder(dir, None).unwrap();
        let vault = load_vault(dir, None).unwrap();
        assert!(vault.groups.iter().all(|g| g.members.is_empty()), "group member must be removed");
    }

    #[test]
    fn reconcile_deletes_session_of_pruned() {
        let td = tempfile::TempDir::new().unwrap();
        let dir = td.path();
        add_account(dir, &mk_account("76561190000000005", "e"), "steamid", "maFile", None).unwrap();
        crate::steam::session_store::save(dir, "76561190000000005", "tok").unwrap();
        assert!(crate::steam::session_store::load(dir, "76561190000000005").is_some());
        crate::vault::mafiles::delete(dir, "76561190000000005.maFile").unwrap();
        reconcile_folder(dir, None).unwrap();
        assert!(crate::steam::session_store::load(dir, "76561190000000005").is_none(), "session token must be deleted");
    }

    #[test]
    fn reconcile_noop_when_in_sync() {
        let td = tempfile::TempDir::new().unwrap();
        let dir = td.path();
        add_account(dir, &mk_account("76561190000000006", "f"), "steamid", "maFile", None).unwrap();
        let changed = reconcile_folder(dir, None).unwrap();
        assert!(!changed, "no disk/manifest drift → no change");
    }

    #[test]
    fn reconcile_keeps_legacy_empty_mafile_name() {
        let td = tempfile::TempDir::new().unwrap();
        let dir = td.path();
        crate::vault::mafiles::ensure_dir(dir).unwrap(); // dir must exist (safeguard passes)
        // A legacy account with no backing maFile name.
        let mut vault = load_vault(dir, None).unwrap();
        vault.accounts.push(AccountSummary {
            steam_id: "76561190000000007".into(),
            account_name: "legacy".into(),
            status: "active".into(),
            mafile_name: String::new(),
            ..Default::default()
        });
        write_manifest(dir, &vault, None).unwrap();
        let changed = reconcile_folder(dir, None).unwrap();
        assert!(!changed, "legacy account must not be pruned");
        let vault = load_vault(dir, None).unwrap();
        assert_eq!(vault.accounts.len(), 1);
    }

    #[test]
    fn reconcile_keeps_present_but_unparseable_mafile() {
        let td = tempfile::TempDir::new().unwrap();
        let dir = td.path();
        add_account(dir, &mk_account("76561190000000008", "corrupt"), "steamid", "maFile", None).unwrap();
        crate::steam::session_store::save(dir, "76561190000000008", "tok").unwrap();
        // Corrupt the on-disk maFile so it no longer parses, but the file still EXISTS.
        std::fs::write(crate::vault::mafiles::dir(dir).join("76561190000000008.maFile"), "{ garbage not json").unwrap();
        let changed = reconcile_folder(dir, None).unwrap();
        assert!(!changed, "a present-but-unparseable file must NOT be pruned");
        let vault = load_vault(dir, None).unwrap();
        assert_eq!(vault.accounts.len(), 1, "account must survive");
        assert!(crate::steam::session_store::load(dir, "76561190000000008").is_some(), "session must survive");
    }
}
