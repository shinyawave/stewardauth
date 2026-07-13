// SPDX-FileCopyrightText: 2026 shinyawave
// SPDX-License-Identifier: AGPL-3.0-or-later

//! Proxy parsing + reqwest client construction.

use crate::vault::model::Proxy;

/// Normalize a scheme; unknown schemes are rejected by the caller.
fn norm_scheme(s: &str) -> Option<String> {
    match s.to_lowercase().as_str() {
        "http" => Some("http".into()),
        "https" => Some("https".into()),
        "socks5" | "socks" => Some("socks5".into()),
        _ => None,
    }
}

/// Parse a single proxy line in any supported format. Returns None if invalid.
/// Supported: `IP:PORT`, `IP:PORT:USER:PASS`, `SCHEME://IP:PORT[:USER:PASS]`,
/// `SCHEME://USER:PASS@IP:PORT`. A trailing `{ID}` stays in the password.
pub fn parse_proxy_line(line: &str) -> Option<Proxy> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }
    let (scheme, rest) = match line.find("://") {
        Some(i) => (norm_scheme(&line[..i])?, &line[i + 3..]),
        None => ("http".to_string(), line),
    };

    // Optional `user:pass@host:port`.
    let (host, port, user, pass) = if let Some(at) = rest.rfind('@') {
        let creds = &rest[..at];
        let hostport = &rest[at + 1..];
        let (h, p) = hostport.rsplit_once(':')?;
        let (u, pw) = creds.split_once(':')?;
        (h.to_string(), p.to_string(), u.to_string(), pw.to_string())
    } else {
        let parts: Vec<&str> = rest.split(':').collect();
        match parts.len() {
            2 => (parts[0].to_string(), parts[1].to_string(), String::new(), String::new()),
            4 => (
                parts[0].to_string(),
                parts[1].to_string(),
                parts[2].to_string(),
                parts[3].to_string(),
            ),
            _ => return None,
        }
    };

    let port: u16 = port.parse().ok()?;
    if host.is_empty() {
        return None;
    }
    Some(Proxy { scheme, host, port, user, pass, favorite: false })
}

/// Parse many lines, dropping blanks/invalid.
pub fn parse_proxy_lines(text: &str) -> Vec<Proxy> {
    text.lines().filter_map(parse_proxy_line).collect()
}

/// Build a blocking reqwest client optionally routed through `proxy`.
/// Falls back to a direct client if the proxy URL cannot be built.
pub fn build_blocking_client(proxy: Option<&Proxy>) -> reqwest::blocking::Client {
    // Always bound the request so a slow/dead proxy or a stalled Steam endpoint
    // can never hang the "Loading…" state forever — it fails fast instead.
    let mut builder = reqwest::blocking::Client::builder()
        .connect_timeout(std::time::Duration::from_secs(12))
        .timeout(std::time::Duration::from_secs(20));
    if let Some(p) = proxy {
        if let Ok(mut rp) = reqwest::Proxy::all(format!("{}://{}:{}", p.scheme, p.host, p.port)) {
            if !p.user.is_empty() {
                rp = rp.basic_auth(&p.user, &p.pass);
            }
            builder = builder.proxy(rp);
        }
    }
    builder
        .build()
        .unwrap_or_else(|_| reqwest::blocking::Client::new())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ip_port_defaults_http() {
        let p = parse_proxy_line("1.2.3.4:8080").unwrap();
        assert_eq!(p.scheme, "http");
        assert_eq!(p.host, "1.2.3.4");
        assert_eq!(p.port, 8080);
        assert_eq!(p.id(), "http://1.2.3.4:8080");
    }

    #[test]
    fn parses_ip_port_user_pass() {
        let p = parse_proxy_line("1.2.3.4:8080:bob:secret").unwrap();
        assert_eq!(p.user, "bob");
        assert_eq!(p.pass, "secret");
        assert_eq!(p.id(), "http://bob:secret@1.2.3.4:8080");
    }

    #[test]
    fn parses_scheme_colon_form() {
        let p = parse_proxy_line("socks5://1.2.3.4:1080:u:p").unwrap();
        assert_eq!(p.scheme, "socks5");
        assert_eq!(p.user, "u");
    }

    #[test]
    fn parses_scheme_at_form() {
        let p = parse_proxy_line("https://u:p@1.2.3.4:3128").unwrap();
        assert_eq!(p.scheme, "https");
        assert_eq!(p.host, "1.2.3.4");
        assert_eq!(p.port, 3128);
        assert_eq!(p.user, "u");
    }

    #[test]
    fn keeps_session_id_in_pass() {
        let p = parse_proxy_line("1.2.3.4:8080:user:pass{sess-42}").unwrap();
        assert_eq!(p.pass, "pass{sess-42}");
    }

    #[test]
    fn socks_alias_normalized() {
        assert_eq!(parse_proxy_line("socks://h:1").unwrap().scheme, "socks5");
    }

    #[test]
    fn rejects_garbage_and_bad_port() {
        assert!(parse_proxy_line("nonsense").is_none());
        assert!(parse_proxy_line("1.2.3.4:notaport").is_none());
        assert!(parse_proxy_line("ftp://1.2.3.4:21").is_none());
    }

    #[test]
    fn parse_many_drops_invalid() {
        let v = parse_proxy_lines("1.2.3.4:8080\n\nbad\n5.6.7.8:9090:u:p\n");
        assert_eq!(v.len(), 2);
    }

    #[test]
    fn build_client_ok_for_socks_and_http() {
        let http = Proxy { scheme: "http".into(), host: "1.2.3.4".into(), port: 8080, ..Default::default() };
        let socks = Proxy { scheme: "socks5".into(), host: "1.2.3.4".into(), port: 1080, user: "u".into(), pass: "p".into(), favorite: false };
        // Should not panic and should return a usable client.
        let _ = build_blocking_client(Some(&http));
        let _ = build_blocking_client(Some(&socks));
        let _ = build_blocking_client(None);
    }
}
