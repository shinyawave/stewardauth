// SPDX-FileCopyrightText: 2026 shinyawave
// SPDX-License-Identifier: AGPL-3.0-or-later

use serde::{Deserialize, Serialize};

#[allow(dead_code)] // wired up in later tasks
#[derive(Debug, Clone)]
pub struct Account {
    pub steam_id: String,
    pub account_name: String,
    pub shared_secret: String,
    pub identity_secret: String,
    pub device_id: String,
    pub revocation_code: String,
}

#[allow(dead_code)] // wired up in later tasks
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AccountSummary {
    pub steam_id: String,
    pub account_name: String,
    pub status: String,
    /// Auto-accept market-listing confirmations for this account (persisted).
    #[serde(default)]
    pub auto_confirm_market: bool,
    /// Auto-accept trade confirmations for this account (persisted).
    #[serde(default)]
    pub auto_confirm_trade: bool,
    /// Name of the account's `.maFile` in the maFiles folder (empty = legacy Keychain).
    #[serde(default)]
    pub mafile_name: String,
    /// Assigned proxy id (empty = none; falls back to the vault default).
    #[serde(default)]
    pub proxy: String,
}

/// A named collection of accounts (by steam_id). Persisted in the manifest.
#[allow(dead_code)]
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct Group {
    pub name: String,
    #[serde(default)]
    pub members: Vec<String>,
}

/// A stored proxy. `id()` is the stable key used for assignment/dedup.
#[allow(dead_code)]
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Proxy {
    pub scheme: String, // http | https | socks5
    pub host: String,
    pub port: u16,
    #[serde(default)]
    pub user: String,
    #[serde(default)]
    pub pass: String,
    #[serde(default)]
    pub favorite: bool,
}

impl Proxy {
    /// Stable identity string used as the assignment key and for dedup.
    pub fn id(&self) -> String {
        if self.user.is_empty() {
            format!("{}://{}:{}", self.scheme, self.host, self.port)
        } else {
            format!("{}://{}:{}@{}:{}", self.scheme, self.user, self.pass, self.host, self.port)
        }
    }
}
