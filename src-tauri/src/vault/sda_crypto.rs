// SPDX-FileCopyrightText: 2026 shinyawave
// SPDX-License-Identifier: AGPL-3.0-or-later

// SDA crypto: PBKDF2-HMAC-SHA1 (50 000 iters, 32-byte key) → AES-256-CBC / PKCS7.
// These parameters match the documented SDA maFile format.
// IMPORTANT: validate against a real SDA-exported maFile during manual QA before release.

use crate::vault::model::Account;
use crate::vault::mafile::{MaFileError, parse_mafile};

#[allow(dead_code)] // wired up in later tasks
#[derive(Debug)]
pub enum SdaError {
    Base64,
    WrongPasswordOrCorrupt,
}

/// Holds the three base64-encoded fields stored alongside an SDA encrypted maFile entry.
#[allow(dead_code)] // wired up in later tasks
pub struct EncryptedEntry {
    pub cipher_b64: String,
    pub salt_b64: String,
    pub iv_b64: String,
}

/// Core AES-256-CBC decrypt. PBKDF2-HMAC-SHA1 (50 000 iters) → AES-256-CBC/PKCS7.
/// Returns raw plaintext bytes. Argument order mirrors the manifest layout:
/// ciphertext first, then the shared password, then salt + iv.
pub fn decrypt_cbc(
    cipher_b64: &str,
    password: &str,
    salt_b64: &str,
    iv_b64: &str,
) -> Result<Vec<u8>, SdaError> {
    use base64::{engine::general_purpose::STANDARD, Engine as _};
    use pbkdf2::pbkdf2_hmac;
    use sha1::Sha1;
    use aes::Aes256;
    use cbc::Decryptor;
    use aes::cipher::{BlockDecryptMut, KeyIvInit, block_padding::Pkcs7};

    let salt = STANDARD.decode(salt_b64).map_err(|_| SdaError::Base64)?;
    let iv = STANDARD.decode(iv_b64).map_err(|_| SdaError::Base64)?;
    let mut cipher_bytes = STANDARD.decode(cipher_b64).map_err(|_| SdaError::Base64)?;

    let mut key = [0u8; 32];
    pbkdf2_hmac::<Sha1>(password.as_bytes(), &salt, 50_000, &mut key);

    let iv_arr: [u8; 16] = iv.try_into().map_err(|_| SdaError::WrongPasswordOrCorrupt)?;
    let decryptor = Decryptor::<Aes256>::new(&key.into(), &iv_arr.into());
    let plaintext = decryptor
        .decrypt_padded_mut::<Pkcs7>(&mut cipher_bytes)
        .map_err(|_| SdaError::WrongPasswordOrCorrupt)?;

    Ok(plaintext.to_vec())
}

/// Decrypt an SDA-encrypted maFile, returning the plaintext JSON string.
///
/// Parameters match SDA's scheme: PBKDF2-HMAC-SHA1 (50 000 iters, 32-byte key) for
/// key derivation, then AES-256-CBC with PKCS7 padding for decryption.
#[allow(dead_code)] // wired up in later tasks
pub fn decrypt_sda(
    cipher_b64: &str,
    salt_b64: &str,
    iv_b64: &str,
    password: &str,
) -> Result<String, SdaError> {
    let bytes = decrypt_cbc(cipher_b64, password, salt_b64, iv_b64)?;
    String::from_utf8(bytes).map_err(|_| SdaError::WrongPasswordOrCorrupt)
}

/// Decrypt an `EncryptedEntry` with the given password then parse the resulting JSON
/// into an `Account`.
#[allow(dead_code)] // wired up in later tasks
pub fn parse_encrypted_mafile(
    entry: &EncryptedEntry,
    password: &str,
) -> Result<Account, MaFileError> {
    let json = decrypt_sda(&entry.cipher_b64, &entry.salt_b64, &entry.iv_b64, password)
        .map_err(|_| MaFileError::Decrypt)?;
    parse_mafile(&json)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Encrypt `plain` with `password` using the same scheme as `decrypt_sda`
    /// (PBKDF2-HMAC-SHA1, 50 000 iters, AES-256-CBC, PKCS7) and return
    /// `(cipher_b64, salt_b64, iv_b64)`.
    fn test_encrypt(plain: &str, password: &str) -> (String, String, String) {
        use base64::{engine::general_purpose::STANDARD, Engine as _};
        use pbkdf2::pbkdf2_hmac;
        use sha1::Sha1;
        use aes::Aes256;
        use cbc::Encryptor;
        use aes::cipher::{BlockEncryptMut, KeyIvInit, block_padding::Pkcs7};

        // Fixed salt and IV so the test is deterministic
        let salt = [0u8; 16];
        let iv   = [1u8; 16];

        let mut key = [0u8; 32];
        pbkdf2_hmac::<Sha1>(password.as_bytes(), &salt, 50_000, &mut key);

        let mut buf = vec![0u8; plain.len() + 16]; // enough room for padding
        buf[..plain.len()].copy_from_slice(plain.as_bytes());

        let encryptor = Encryptor::<Aes256>::new(&key.into(), &iv.into());
        let ciphertext = encryptor
            .encrypt_padded_mut::<Pkcs7>(&mut buf, plain.len())
            .expect("encrypt");

        (
            STANDARD.encode(ciphertext),
            STANDARD.encode(salt),
            STANDARD.encode(iv),
        )
    }

    #[test]
    fn round_trips_plaintext() {
        let plain = r#"{"account_name":"tester"}"#;
        let (cipher, salt, iv) = test_encrypt(plain, "hunter2");
        let out = decrypt_sda(&cipher, &salt, &iv, "hunter2").unwrap();
        assert_eq!(out, plain);
    }

    #[test]
    fn wrong_password_fails() {
        let (cipher, salt, iv) = test_encrypt("{}", "right");
        assert!(decrypt_sda(&cipher, &salt, &iv, "wrong").is_err());
    }

    #[test]
    fn decrypt_cbc_round_trips_raw_bytes() {
        let plain = r#"{"account_name":"raw"}"#;
        let (cipher, salt, iv) = test_encrypt(plain, "pw123");
        // Note the argument ORDER: (cipher, password, salt, iv)
        let out = decrypt_cbc(&cipher, "pw123", &salt, &iv).unwrap();
        assert_eq!(out, plain.as_bytes());
    }

    #[test]
    fn decrypt_cbc_wrong_password_errors() {
        let (cipher, salt, iv) = test_encrypt("{}", "right");
        assert!(matches!(
            decrypt_cbc(&cipher, "wrong", &salt, &iv),
            Err(SdaError::WrongPasswordOrCorrupt)
        ));
    }

    /// Run with: cargo test --lib vault::sda_crypto::tests::gen_sda_fixture -- --ignored --nocapture
    /// Copies the printed base64 into tests/fixtures/sda/1234.maFile.
    #[test]
    #[ignore]
    fn gen_sda_fixture() {
        use base64::{engine::general_purpose::STANDARD, Engine as _};
        use pbkdf2::pbkdf2_hmac;
        use sha1::Sha1;
        use aes::Aes256;
        use cbc::Encryptor;
        use aes::cipher::{BlockEncryptMut, KeyIvInit, block_padding::Pkcs7};

        let plain = r#"{"account_name":"sdauser","shared_secret":"cnOgv/KdpLoP6Nbh0GMkXkPnNqmc0Q=","identity_secret":"AAAAAAAAAAAAAAAAAAAAAAAAAAA=","device_id":"android:00000000-0000-0000-0000-000000000000","revocation_code":"R54321","Session":{"SteamID":76561190000000123}}"#;
        let salt = [0xA1u8; 16];
        let iv = [0xB2u8; 16];
        let mut key = [0u8; 32];
        pbkdf2_hmac::<Sha1>(b"hunter2", &salt, 50_000, &mut key);
        let mut buf = vec![0u8; plain.len() + 16];
        buf[..plain.len()].copy_from_slice(plain.as_bytes());
        let ct = Encryptor::<Aes256>::new(&key.into(), &iv.into())
            .encrypt_padded_mut::<Pkcs7>(&mut buf, plain.len())
            .expect("encrypt");
        println!("SALT_B64={}", STANDARD.encode(salt));
        println!("IV_B64={}", STANDARD.encode(iv));
        println!("CIPHER_B64={}", STANDARD.encode(ct));
    }

    /// Verifies the committed fixture (tests/fixtures/sda/1234.maFile) round-trips
    /// through decrypt_cbc with the known password, salt, and iv.
    #[test]
    fn fixture_round_trip() {
        let cipher_b64 = include_str!("../../tests/fixtures/sda/1234.maFile").trim();
        let salt_b64 = "oaGhoaGhoaGhoaGhoaGhoQ==";
        let iv_b64 = "srKysrKysrKysrKysrKysg==";
        let expected = r#"{"account_name":"sdauser","shared_secret":"cnOgv/KdpLoP6Nbh0GMkXkPnNqmc0Q=","identity_secret":"AAAAAAAAAAAAAAAAAAAAAAAAAAA=","device_id":"android:00000000-0000-0000-0000-000000000000","revocation_code":"R54321","Session":{"SteamID":76561190000000123}}"#;

        let plaintext = decrypt_cbc(cipher_b64, "hunter2", salt_b64, iv_b64)
            .expect("decrypt_cbc failed");
        assert_eq!(String::from_utf8(plaintext).unwrap(), expected);
    }
}
