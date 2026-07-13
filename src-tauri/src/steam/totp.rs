// SPDX-FileCopyrightText: 2026 shinyawave
// SPDX-License-Identifier: AGPL-3.0-or-later

use base64::{engine::general_purpose::STANDARD, Engine};
use hmac::{Hmac, Mac};
use sha1::Sha1;

#[allow(dead_code)] // wired up in later tasks
const STEAM_ALPHABET: &[u8] = b"23456789BCDFGHJKMNPQRTVWXY";

#[allow(dead_code)] // wired up in later tasks
#[derive(Debug, PartialEq)]
pub enum TotpError {
    Decode,
}

/// Decode a maFile `shared_secret`: 40-char hex, otherwise standard base64.
#[allow(dead_code)] // wired up in later tasks
pub fn decode_shared_secret(s: &str) -> Result<Vec<u8>, TotpError> {
    let s = s.trim();
    if s.len() == 40 && s.bytes().all(|b| b.is_ascii_hexdigit()) {
        (0..40)
            .step_by(2)
            .map(|i| u8::from_str_radix(&s[i..i + 2], 16).map_err(|_| TotpError::Decode))
            .collect()
    } else {
        STANDARD.decode(s).map_err(|_| TotpError::Decode)
    }
}

/// Generate the 5-character Steam Guard code for a given unix time.
#[allow(dead_code)] // wired up in later tasks
pub fn generate_steam_code(shared_secret: &[u8], unix_time: u64) -> String {
    let counter = (unix_time / 30).to_be_bytes();
    let mut mac = Hmac::<Sha1>::new_from_slice(shared_secret).expect("HMAC accepts any key length");
    mac.update(&counter);
    let digest = mac.finalize().into_bytes();

    let offset = (digest[19] & 0x0F) as usize;
    let mut point = ((digest[offset] as u32 & 0x7f) << 24)
        | ((digest[offset + 1] as u32) << 16)
        | ((digest[offset + 2] as u32) << 8)
        | (digest[offset + 3] as u32);

    let mut code = String::with_capacity(5);
    for _ in 0..5 {
        code.push(STEAM_ALPHABET[(point % 26) as usize] as char);
        point /= 26;
    }
    code
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_is_five_chars_from_steam_alphabet() {
        let secret = decode_shared_secret("AQIDBAUGBwgJCgsMDQ4PEBESExQ=").unwrap();
        let code = generate_steam_code(&secret, 1_600_000_000);
        assert_eq!(code.len(), 5);
        assert!(code.bytes().all(|b| STEAM_ALPHABET.contains(&b)));
    }

    #[test]
    fn code_is_stable_within_a_30s_window_and_changes_across_windows() {
        let secret = decode_shared_secret("AQIDBAUGBwgJCgsMDQ4PEBESExQ=").unwrap();
        // Use 1_599_999_990 which is exactly on a 30s boundary (1599999990 / 30 = 53333333).
        // Same 30s bucket (offset +0 and +19 both land in bucket 53333333) -> identical code.
        assert_eq!(
            generate_steam_code(&secret, 1_599_999_990),
            generate_steam_code(&secret, 1_600_000_009)
        );
        // Next bucket (offset +30) -> (almost surely) different code.
        assert_ne!(
            generate_steam_code(&secret, 1_599_999_990),
            generate_steam_code(&secret, 1_600_000_020)
        );
    }

    #[test]
    fn hex_and_base64_secrets_decode_to_same_bytes() {
        let b64 = decode_shared_secret("AAAAAAAAAAAAAAAAAAAAAAAAAAA=").unwrap();
        assert_eq!(b64.len(), 20);
        let hex = decode_shared_secret("0000000000000000000000000000000000000000").unwrap();
        assert_eq!(hex, b64);
    }

    #[test]
    fn totp_matches_steamguard() {
        // Cross-validate against steamguard::token::TwoFactorSecret.
        // API: TwoFactorSecret::parse_shared_secret(String) -> Result<TwoFactorSecret>
        //      TwoFactorSecret::generate_code(&self, time: u64) -> String
        // Both accept a standard base64 shared_secret and a unix timestamp.
        use steamguard::token::TwoFactorSecret;

        let b64 = "AQIDBAUGBwgJCgsMDQ4PEBESExQ=";
        let sg_secret = TwoFactorSecret::parse_shared_secret(b64.to_owned()).unwrap();
        let our_secret = decode_shared_secret(b64).unwrap();

        for &ts in &[1_600_000_000u64, 1_616_374_841u64, 1_700_000_000u64] {
            let sg_code = sg_secret.generate_code(ts);
            let our_code = generate_steam_code(&our_secret, ts);
            assert_eq!(
                our_code, sg_code,
                "Code mismatch at unix_time={ts}: ours={our_code}, steamguard={sg_code}"
            );
        }
    }
}
