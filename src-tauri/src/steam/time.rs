// SPDX-FileCopyrightText: 2026 shinyawave
// SPDX-License-Identifier: AGPL-3.0-or-later

use serde::Deserialize;

#[allow(dead_code)] // wired up in later tasks
#[derive(Debug, Clone, Copy)]
pub struct TimeSync {
    pub offset_secs: i64,
}

#[allow(dead_code)] // wired up in later tasks
#[derive(Debug)]
pub enum TimeError {
    Network,
    Parse,
}

/// Return local unix time corrected by the Steam server offset.
#[allow(dead_code)] // wired up in later tasks
pub fn steam_now(sync: &TimeSync, local_unix: u64) -> u64 {
    (local_unix as i64 + sync.offset_secs).max(0) as u64
}

#[derive(Deserialize)]
struct TimeResponse {
    response: TimeInner,
}
#[derive(Deserialize)]
struct TimeInner {
    server_time: String,
}

/// Query Steam's server time and return (server - local) in seconds.
#[allow(dead_code)] // wired up in later tasks
pub async fn fetch_offset(client: &reqwest::Client, local_unix: u64) -> Result<i64, TimeError> {
    let resp = client
        .post("https://api.steampowered.com/ITwoFactorService/QueryTime/v1/")
        .header("Content-Length", "0")
        .send()
        .await
        .map_err(|_| TimeError::Network)?
        .json::<TimeResponse>()
        .await
        .map_err(|_| TimeError::Parse)?;
    let server: i64 = resp.response.server_time.parse().map_err(|_| TimeError::Parse)?;
    Ok(server - local_unix as i64)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn applies_offset_to_local_time() {
        let sync = TimeSync { offset_secs: 7 };
        assert_eq!(steam_now(&sync, 1_000), 1_007);
    }
    #[test]
    fn negative_offset_moves_time_back() {
        let sync = TimeSync { offset_secs: -3 };
        assert_eq!(steam_now(&sync, 1_000), 997);
    }
}
