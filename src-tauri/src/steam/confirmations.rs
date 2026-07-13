// SPDX-FileCopyrightText: 2026 shinyawave
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Steam mobile confirmations — our own `mobileconf` client.
//!
//! We deliberately do NOT use `steamguard`'s `Confirmer` here. That type
//! hard-codes `User-Agent: steamguard-cli`, sends only a minimal cookie set, and
//! issues a separate `get_server_time` request on *every* getlist. Steam's
//! `mobileconf/getlist` endpoint rate-limits/soft-blocks traffic that does not
//! look like the genuine Steam mobile app, so those choices make the endpoint
//! trip its limit far sooner.
//!
//! This module instead mimics the real Android app exactly — matching the
//! behaviour of the genuine Steam mobile client:
//!
//! * `User-Agent: okhttp/3.12.12` (the app's HTTP stack) + `X-Requested-With`,
//!   `Origin`, `Referer`, `Accept` headers.
//! * The full mobile cookie set (`mobileClient`, `mobileClientVersion`,
//!   `sessionid`, `Steam_Language`, `steamid`, `dob`, `steamLoginSecure`).
//! * A **cached** server-time offset (refreshed at most hourly) instead of a
//!   `get_server_time` round-trip per getlist.
//! * **Cached nonces** from the last getlist so `respond()` does not have to
//!   re-fetch the whole list just to map id → nonce.
//! * Bulk accept/deny via a single `mobileconf/multiajaxop` POST.
//!
//! `steamguard` is still used for login / TOTP; only the confirmation HTTP path
//! is ours.
//!
//! The getlist / multiajaxop query params are unchanged from the app:
//! `p`=device_id, `a`=steamid, `k`=HMAC-SHA1(be(time) ‖ tag), `t`=time,
//! `m`=`react`, `tag`=`conf` (list) / `allow` | `cancel` (respond).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{SystemTime, UNIX_EPOCH};

use base64::Engine;
use hmac::{Hmac, Mac};
use reqwest::header::{ACCEPT, COOKIE, ORIGIN, REFERER, USER_AGENT};
use serde::{Deserialize, Serialize};
use sha1::Sha1;

use crate::{steam::session::SteamSession, vault::model::Account};

// ── Error ─────────────────────────────────────────────────────────────────────

#[derive(Debug)]
pub enum ConfError {
    /// Session tokens are missing or rejected by Steam.
    InvalidSession,
    /// Network or HTTP failure.
    Network(String),
    /// Steam server returned a failure response.
    RemoteFailure(String),
    /// account.steam_id is not a valid u64.
    InvalidSteamId(String),
    /// Local configuration error (bad shared_secret, bad steam_id format, etc.).
    #[allow(dead_code)]
    Config(String),
}

impl std::fmt::Display for ConfError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfError::InvalidSession => write!(f, "invalid or missing session tokens"),
            ConfError::Network(e) => write!(f, "network error: {e}"),
            ConfError::RemoteFailure(e) => write!(f, "Steam remote failure: {e}"),
            ConfError::InvalidSteamId(e) => write!(f, "invalid steam_id: {e}"),
            ConfError::Config(e) => write!(f, "configuration error: {e}"),
        }
    }
}

impl std::error::Error for ConfError {}

// ── IPC-safe output type ──────────────────────────────────────────────────────

/// A single pending mobile confirmation — safe to send over IPC.
///
/// Contains only display fields and the `id` needed to call [`respond`].
/// No secrets, keys, or nonces are included.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ConfirmationItem {
    /// Confirmation ID (used to accept/deny via [`respond`]).
    pub id: String,
    /// Human-readable confirmation type (e.g. "Trade Offer", "Market Listing").
    pub kind: String,
    /// Short title line shown in the Steam mobile app (maps to `headline`).
    pub title: String,
    /// Joined summary lines, comma-separated.
    pub summary: String,
    /// Optional icon URL.
    pub icon: Option<String>,
    /// Stable machine category derived from the Steam confirmation type:
    /// `"trade"`, `"market"`, or `"other"`. Used for glyphs and auto-confirm
    /// filtering (independent of the human-readable `kind` string).
    pub category: String,
}

/// Map a numeric Steam confirmation `type` to a stable category string.
///
/// `2` (Trade) → `"trade"`, `3` (MarketListing) → `"market"`, else → `"other"`.
pub fn category_of(conf_type: i64) -> &'static str {
    match conf_type {
        2 => "trade",
        3 => "market",
        _ => "other",
    }
}

// ── Raw getlist response (mirrors the mobileconf JSON) ──────────────────────────

#[derive(Debug, Deserialize)]
struct GetListResponse {
    #[serde(default)]
    success: bool,
    #[serde(default)]
    needauth: bool,
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    conf: Vec<RawConfirmation>,
}

#[derive(Debug, Clone, Deserialize)]
struct RawConfirmation {
    #[serde(rename = "type")]
    conf_type: i64,
    #[serde(default)]
    type_name: String,
    id: String,
    nonce: String,
    #[serde(default)]
    headline: String,
    #[serde(default)]
    summary: Vec<String>,
    #[serde(default)]
    icon: Option<String>,
}

fn to_item(c: &RawConfirmation) -> ConfirmationItem {
    ConfirmationItem {
        id: c.id.clone(),
        kind: c.type_name.clone(),
        title: c.headline.clone(),
        summary: c.summary.join(", "),
        icon: c.icon.clone(),
        category: category_of(c.conf_type).to_string(),
    }
}

// ── Mobile-app fingerprint ──────────────────────────────────────────────────────

/// The Steam Android app's HTTP stack. Presenting this (instead of
/// `steamguard-cli`) is the single biggest factor in not tripping the
/// `mobileconf/getlist` rate limit.
const MOBILE_UA: &str = "okhttp/3.12.12";

/// Build the mobile cookie header. `steamLoginSecure` carries the access token;
/// the `mobileClient*` cookies mark the request as coming from the app.
fn cookie_header(steam_id: u64, access_token: &str, sessionid: &str) -> String {
    format!(
        "mobileClientVersion=0 (2.1.3); mobileClient=android; steamid={sid}; \
         steamLoginSecure={sid}||{tok}; Steam_Language=english; dob=; sessionid={sess}",
        sid = steam_id,
        tok = access_token,
        sess = sessionid,
    )
}

/// Attach the headers the real app sends on every `mobileconf` request.
fn mobile_headers(
    req: reqwest::blocking::RequestBuilder,
    cookie: &str,
) -> reqwest::blocking::RequestBuilder {
    req.header(USER_AGENT, MOBILE_UA)
        .header(COOKIE, cookie)
        .header("X-Requested-With", "com.valvesoftware.android.steam.community")
        .header(
            ACCEPT,
            "application/json, text/javascript, text/html, application/xml, text/xml, */*",
        )
        .header(REFERER, "https://steamcommunity.com/mobileconf/conf")
        .header(ORIGIN, "https://steamcommunity.com")
}

// ── Process-wide caches (avoid redundant Steam requests) ────────────────────────

/// Cached server-time offset `(server - local, computed_at_local)`. Time is
/// universal, so one offset is shared across accounts/proxies and refreshed at
/// most hourly — replacing a `QueryTime` round-trip on every getlist.
static TIME_OFFSET: OnceLock<Mutex<Option<(i64, u64)>>> = OnceLock::new();

/// Per-account random `sessionid` cookie, generated once and reused (the app
/// keeps a stable sessionid for the life of a session).
static SESSIONID_CACHE: OnceLock<Mutex<HashMap<u64, String>>> = OnceLock::new();

/// Per-account `id → nonce` map from the last getlist, so `respond()` need not
/// re-fetch the whole list just to resolve nonces.
static NONCE_CACHE: OnceLock<Mutex<HashMap<u64, HashMap<String, String>>>> = OnceLock::new();

fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

/// Current Steam server time, using a cached offset (refreshed hourly). Falls
/// back to local time if the one-time `QueryTime` call fails.
fn server_time(client: &reqwest::blocking::Client) -> u64 {
    let local = now_unix();
    let cell = TIME_OFFSET.get_or_init(|| Mutex::new(None));
    let mut guard = cell.lock().unwrap();
    if let Some((offset, at)) = *guard {
        if local.saturating_sub(at) < 3600 {
            return (local as i64 + offset).max(0) as u64;
        }
    }
    match query_server_time(client) {
        Some(server) => {
            *guard = Some((server as i64 - local as i64, local));
            server
        }
        None => local,
    }
}

fn query_server_time(client: &reqwest::blocking::Client) -> Option<u64> {
    let resp = client
        .post("https://api.steampowered.com/ITwoFactorService/QueryTime/v1/")
        .header("Content-Length", "0")
        .send()
        .ok()?;
    let v: serde_json::Value = resp.json().ok()?;
    v.get("response")?.get("server_time")?.as_str()?.parse().ok()
}

fn sessionid_for(steam_id: u64) -> String {
    let cell = SESSIONID_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = cell.lock().unwrap();
    guard
        .entry(steam_id)
        .or_insert_with(|| {
            use rand::Rng;
            let mut rng = rand::thread_rng();
            (0..24)
                .map(|_| std::char::from_digit(rng.gen_range(0..16), 16).unwrap())
                .collect()
        })
        .clone()
}

fn cache_nonces(steam_id: u64, confs: &[RawConfirmation]) {
    let cell = NONCE_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = cell.lock().unwrap();
    let map = guard.entry(steam_id).or_default();
    map.clear();
    for c in confs {
        map.insert(c.id.clone(), c.nonce.clone());
    }
}

fn take_cached_nonces(steam_id: u64, ids: &[String]) -> (Vec<(String, String)>, bool) {
    let cell = NONCE_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let guard = cell.lock().unwrap();
    let map = guard.get(&steam_id);
    let mut pairs = Vec::new();
    let mut any_missing = false;
    for id in ids {
        match map.and_then(|m| m.get(id)) {
            Some(nonce) => pairs.push((id.clone(), nonce.clone())),
            None => any_missing = true,
        }
    }
    (pairs, any_missing)
}

fn forget_nonces(steam_id: u64, ids: &[String]) {
    let cell = NONCE_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let mut guard = cell.lock().unwrap();
    if let Some(map) = guard.get_mut(&steam_id) {
        for id in ids {
            map.remove(id);
        }
    }
}

// ── Confirmation hash ───────────────────────────────────────────────────────────

/// `k` param: base64(HMAC-SHA1(big-endian u64 time ‖ tag)) keyed by the
/// base64-decoded `identity_secret`.
fn confirmation_hash(identity_secret: &str, time: u64, tag: &str) -> Result<String, ConfError> {
    let key = base64::engine::general_purpose::STANDARD
        .decode(identity_secret.trim())
        .map_err(|e| ConfError::Config(format!("bad identity_secret base64: {e}")))?;
    let mut mac = <Hmac<Sha1>>::new_from_slice(&key)
        .map_err(|e| ConfError::Config(format!("hmac init: {e}")))?;
    mac.update(&time.to_be_bytes());
    mac.update(tag.as_bytes());
    Ok(base64::engine::general_purpose::STANDARD.encode(mac.finalize().into_bytes()))
}

/// Best-effort: append a rate-limit / failure diagnostic to
/// `<app_dir>/getlist-debug.log`. The getlist body contains only confirmation
/// metadata and success flags — no secrets — so it is safe to persist. Helps
/// diagnose *what* Steam actually returned (HTTP 429, HTML challenge, JSON
/// error) instead of a swallowed generic error.
fn log_failure(app_dir: &Path, endpoint: &str, status: u16, body: &str) {
    let snippet: String = body.chars().take(4000).collect();
    let line = format!(
        "[{}] {} HTTP {} :: {}\n",
        now_unix(),
        endpoint,
        status,
        snippet.replace('\n', " ")
    );
    let path = app_dir.join("getlist-debug.log");
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let _ = f.write_all(line.as_bytes());
    }
}

// ── Request context (moved into the blocking closure) ───────────────────────────

struct ReqCtx {
    steam_id: u64,
    device_id: String,
    identity_secret: String,
    access_token: String,
    app_dir: PathBuf,
}

fn build_ctx(account: &Account, session: &SteamSession) -> Result<ReqCtx, ConfError> {
    let steam_id: u64 = account
        .steam_id
        .parse()
        .map_err(|e| ConfError::InvalidSteamId(format!("{}: {e}", account.steam_id)))?;
    Ok(ReqCtx {
        steam_id,
        device_id: account.device_id.clone(),
        identity_secret: account.identity_secret.clone(),
        access_token: session.tokens().access_token().expose_secret().to_owned(),
        app_dir: session.app_dir().to_path_buf(),
    })
}

// ── Blocking core ───────────────────────────────────────────────────────────────

fn get_confirmations_blocking(
    client: &reqwest::blocking::Client,
    ctx: &ReqCtx,
) -> Result<Vec<RawConfirmation>, ConfError> {
    let time = server_time(client);
    let tag = "conf";
    let k = confirmation_hash(&ctx.identity_secret, time, tag)?;
    let sid = ctx.steam_id.to_string();
    let sessionid = sessionid_for(ctx.steam_id);
    let cookie = cookie_header(ctx.steam_id, &ctx.access_token, &sessionid);
    let time_str = time.to_string();

    let req = client
        .get("https://steamcommunity.com/mobileconf/getlist")
        .query(&[
            ("p", ctx.device_id.as_str()),
            ("a", sid.as_str()),
            ("k", k.as_str()),
            ("t", time_str.as_str()),
            ("m", "react"),
            ("tag", tag),
        ]);
    let resp = mobile_headers(req, &cookie)
        .send()
        .map_err(|e| ConfError::Network(e.to_string()))?;

    let status = resp.status();
    let body = resp.text().map_err(|e| ConfError::Network(e.to_string()))?;

    if !status.is_success() {
        log_failure(&ctx.app_dir, "getlist", status.as_u16(), &body);
        return Err(ConfError::RemoteFailure(format!(
            "getlist HTTP {} (likely rate limit)",
            status.as_u16()
        )));
    }

    let parsed: GetListResponse = match serde_json::from_str(&body) {
        Ok(p) => p,
        Err(e) => {
            // A non-JSON body here is the tell-tale sign of a rate-limit / block
            // (Steam serves an HTML page or empty response).
            log_failure(&ctx.app_dir, "getlist", status.as_u16(), &body);
            return Err(ConfError::RemoteFailure(format!(
                "getlist returned non-JSON (likely rate limit): {e}"
            )));
        }
    };

    if parsed.needauth {
        return Err(ConfError::InvalidSession);
    }
    if !parsed.success {
        return Err(ConfError::RemoteFailure(
            parsed.message.unwrap_or_else(|| "getlist success=false".to_owned()),
        ));
    }

    cache_nonces(ctx.steam_id, &parsed.conf);
    Ok(parsed.conf)
}

fn respond_blocking(
    client: &reqwest::blocking::Client,
    ctx: &ReqCtx,
    ids: &[String],
    accept: bool,
) -> Result<(), ConfError> {
    // Resolve id → nonce from the cache populated by the last getlist. Only if a
    // nonce is missing do we pay for one refresh getlist.
    let (mut pairs, missing) = take_cached_nonces(ctx.steam_id, ids);
    if missing {
        get_confirmations_blocking(client, ctx)?;
        let (refreshed, _) = take_cached_nonces(ctx.steam_id, ids);
        pairs = refreshed;
    }
    if pairs.is_empty() {
        // Nothing to do — the confirmations are already gone.
        return Ok(());
    }

    let time = server_time(client);
    let tag = if accept { "allow" } else { "cancel" };
    let k = confirmation_hash(&ctx.identity_secret, time, tag)?;
    let sid = ctx.steam_id.to_string();
    let sessionid = sessionid_for(ctx.steam_id);
    let cookie = cookie_header(ctx.steam_id, &ctx.access_token, &sessionid);

    // Single POST to multiajaxop handles one or many confirmations.
    let mut form: Vec<(String, String)> = vec![
        ("op".into(), tag.into()),
        ("p".into(), ctx.device_id.clone()),
        ("a".into(), sid),
        ("k".into(), k),
        ("t".into(), time.to_string()),
        ("m".into(), "react".into()),
        ("tag".into(), tag.into()),
    ];
    for (id, nonce) in &pairs {
        form.push(("cid[]".into(), id.clone()));
        form.push(("ck[]".into(), nonce.clone()));
    }

    let req = client
        .post("https://steamcommunity.com/mobileconf/multiajaxop")
        .form(&form);
    let resp = mobile_headers(req, &cookie)
        .send()
        .map_err(|e| ConfError::Network(e.to_string()))?;

    let status = resp.status();
    let body = resp.text().unwrap_or_default();
    if !status.is_success() {
        log_failure(&ctx.app_dir, "multiajaxop", status.as_u16(), &body);
        return Err(ConfError::RemoteFailure(format!(
            "multiajaxop HTTP {}",
            status.as_u16()
        )));
    }

    let v: serde_json::Value = serde_json::from_str(&body).map_err(|e| {
        log_failure(&ctx.app_dir, "multiajaxop", status.as_u16(), &body);
        ConfError::RemoteFailure(format!("multiajaxop returned non-JSON: {e}"))
    })?;

    if v.get("success").and_then(|s| s.as_bool()).unwrap_or(false) {
        let responded: Vec<String> = pairs.into_iter().map(|(id, _)| id).collect();
        forget_nonces(ctx.steam_id, &responded);
        Ok(())
    } else {
        Err(ConfError::RemoteFailure(
            v.get("message")
                .and_then(|m| m.as_str())
                .unwrap_or("multiajaxop success=false")
                .to_owned(),
        ))
    }
}

// ── Public API ──────────────────────────────────────────────────────────────────

/// Fetch all pending mobile confirmations for the account.
///
/// # Security
/// The returned [`ConfirmationItem`] values contain no secrets or keys.
pub async fn fetch(
    account: &Account,
    session: &SteamSession,
) -> Result<Vec<ConfirmationItem>, ConfError> {
    let ctx = build_ctx(account, session)?;
    let proxy = session.proxy().cloned();

    let items = tokio::task::spawn_blocking(move || -> Result<Vec<ConfirmationItem>, ConfError> {
        let client = crate::steam::proxy::build_blocking_client(proxy.as_ref());
        let confs = get_confirmations_blocking(&client, &ctx)?;
        Ok(confs.iter().map(to_item).collect())
    })
    .await
    .map_err(|e| ConfError::Network(format!("spawn_blocking panic: {e}")))??;

    Ok(items)
}

/// Accept or deny a batch of confirmations by their IDs.
///
/// Nonces are resolved from the cache filled by the most recent [`fetch`], so
/// the common path issues a single `multiajaxop` POST with **no** extra getlist.
///
/// # Security
/// Nonces (confirmation keys) never leave the backend.
pub async fn respond(
    account: &Account,
    session: &SteamSession,
    ids: &[String],
    accept: bool,
) -> Result<(), ConfError> {
    let ctx = build_ctx(account, session)?;
    let ids: Vec<String> = ids.to_vec();
    let proxy = session.proxy().cloned();

    tokio::task::spawn_blocking(move || -> Result<(), ConfError> {
        let client = crate::steam::proxy::build_blocking_client(proxy.as_ref());
        respond_blocking(&client, &ctx, &ids, accept)
    })
    .await
    .map_err(|e| ConfError::Network(format!("spawn_blocking panic: {e}")))??;

    Ok(())
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn make_confirmation_json(
        id: &str,
        nonce: &str,
        type_num: i64,
        type_name: &str,
        headline: &str,
        summary: &[&str],
        icon: Option<&str>,
    ) -> String {
        let summary_arr = summary
            .iter()
            .map(|s| format!("\"{}\"", s))
            .collect::<Vec<_>>()
            .join(",");
        let icon_val = match icon {
            Some(u) => format!("\"{}\"", u),
            None => "null".to_owned(),
        };
        format!(
            r#"{{
                "type": {type_num},
                "type_name": "{type_name}",
                "id": "{id}",
                "creator_id": "999",
                "nonce": "{nonce}",
                "creation_time": 1700000000,
                "cancel": "Cancel",
                "accept": "Accept",
                "icon": {icon_val},
                "multi": false,
                "headline": "{headline}",
                "summary": [{summary_arr}]
            }}"#
        )
    }

    #[test]
    fn to_item_maps_fields_correctly() {
        let json = make_confirmation_json(
            "12345",
            "abc_nonce",
            2,
            "Trade Offer",
            "Trade with SomeUser",
            &["1 item", "Worth ~$5.00"],
            Some("https://cdn.steamcommunity.com/icon.png"),
        );
        let conf: RawConfirmation =
            serde_json::from_str(&json).expect("fixture should deserialize");

        let item = to_item(&conf);

        assert_eq!(item.id, "12345");
        assert_eq!(item.kind, "Trade Offer");
        assert_eq!(item.title, "Trade with SomeUser");
        assert_eq!(item.summary, "1 item, Worth ~$5.00");
        assert_eq!(item.category, "trade");
        assert_eq!(
            item.icon,
            Some("https://cdn.steamcommunity.com/icon.png".to_owned())
        );
    }

    #[test]
    fn to_item_handles_no_icon_and_market_category() {
        let json = make_confirmation_json(
            "99999",
            "xyz_nonce",
            3,
            "Market Listing",
            "Listed an item",
            &["Dragon Lore"],
            None,
        );
        let conf: RawConfirmation =
            serde_json::from_str(&json).expect("fixture should deserialize");

        let item = to_item(&conf);

        assert_eq!(item.id, "99999");
        assert_eq!(item.category, "market");
        assert_eq!(item.summary, "Dragon Lore");
        assert!(item.icon.is_none());
    }

    #[test]
    fn to_item_empty_summary_is_other_category() {
        let json = make_confirmation_json("11111", "empty_nonce", 6, "Unknown", "Confirm", &[], None);
        let conf: RawConfirmation =
            serde_json::from_str(&json).expect("fixture should deserialize");

        let item = to_item(&conf);
        assert_eq!(item.summary, "");
        assert_eq!(item.category, "other");
    }

    #[test]
    fn getlist_response_parses_success_list() {
        let body = r#"{"success":true,"conf":[
            {"type":2,"type_name":"Trade Offer","id":"1","creator_id":"9","nonce":"n1",
             "creation_time":1,"cancel":"Cancel","accept":"Accept","icon":null,"multi":false,
             "headline":"h","summary":["s"]}
        ]}"#;
        let parsed: GetListResponse = serde_json::from_str(body).unwrap();
        assert!(parsed.success);
        assert!(!parsed.needauth);
        assert_eq!(parsed.conf.len(), 1);
        assert_eq!(parsed.conf[0].nonce, "n1");
    }

    #[test]
    fn getlist_response_parses_needauth() {
        let body = r#"{"success":false,"needauth":true}"#;
        let parsed: GetListResponse = serde_json::from_str(body).unwrap();
        assert!(!parsed.success);
        assert!(parsed.needauth);
        assert!(parsed.conf.is_empty());
    }

    #[test]
    fn confirmation_hash_is_deterministic_and_tag_sensitive() {
        // Valid base64 identity_secret (20 bytes → 28 b64 chars).
        let secret = base64::engine::general_purpose::STANDARD.encode([7u8; 20]);
        let a = confirmation_hash(&secret, 1_700_000_000, "conf").unwrap();
        let b = confirmation_hash(&secret, 1_700_000_000, "conf").unwrap();
        let c = confirmation_hash(&secret, 1_700_000_000, "allow").unwrap();
        assert_eq!(a, b, "same inputs → same hash");
        assert_ne!(a, c, "different tag → different hash");
        // base64 of a 20-byte SHA1 HMAC is 28 chars.
        assert_eq!(a.len(), 28);
    }

    #[test]
    fn confirmation_hash_rejects_bad_secret() {
        assert!(confirmation_hash("!!!not base64!!!", 1, "conf").is_err());
    }

    #[test]
    fn cookie_header_contains_mobile_markers() {
        let c = cookie_header(76561190000000001, "TOKEN123", "deadbeef");
        assert!(c.contains("mobileClient=android"));
        assert!(c.contains("steamLoginSecure=76561190000000001||TOKEN123"));
        assert!(c.contains("sessionid=deadbeef"));
        assert!(c.contains("Steam_Language=english"));
    }

    #[test]
    fn sessionid_is_stable_per_account() {
        let a = sessionid_for(111);
        let b = sessionid_for(111);
        assert_eq!(a, b, "sessionid is cached per account");
        assert_eq!(a.len(), 24);
        assert!(a.chars().all(|ch| ch.is_ascii_hexdigit()));
    }

    #[test]
    fn nonce_cache_round_trip() {
        let sid = 222;
        let confs = vec![
            RawConfirmation {
                conf_type: 2,
                type_name: "Trade".into(),
                id: "A".into(),
                nonce: "na".into(),
                headline: String::new(),
                summary: vec![],
                icon: None,
            },
            RawConfirmation {
                conf_type: 3,
                type_name: "Market".into(),
                id: "B".into(),
                nonce: "nb".into(),
                headline: String::new(),
                summary: vec![],
                icon: None,
            },
        ];
        cache_nonces(sid, &confs);

        let (pairs, missing) = take_cached_nonces(sid, &["A".into(), "B".into()]);
        assert!(!missing);
        assert_eq!(pairs.len(), 2);

        let (_, missing2) = take_cached_nonces(sid, &["A".into(), "C".into()]);
        assert!(missing2, "unknown id should report missing");

        forget_nonces(sid, &["A".into()]);
        let (_, missing3) = take_cached_nonces(sid, &["A".into()]);
        assert!(missing3, "forgotten id should now be missing");
    }
}
