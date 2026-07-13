// SPDX-FileCopyrightText: 2026 shinyawave
// SPDX-License-Identifier: AGPL-3.0-or-later

//! AES-256-GCM encryption with Argon2id key derivation for the vault manifest.
//!
//! Layout on disk: `salt(16) || nonce(12) || ciphertext+tag`.

use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Key, Nonce,
};
use argon2::{Algorithm, Argon2, Params, Version};
use rand::RngCore;

/// Errors produced by [`encrypt`] and [`decrypt`].
#[derive(Debug)]
pub enum EncError {
    /// The password was wrong or the ciphertext was tampered with.
    WrongPasswordOrCorrupt,
    /// AES-GCM encryption failed.
    #[allow(dead_code)] // wired up in later tasks
    Encrypt,
    /// Argon2 key derivation failed.
    #[allow(dead_code)] // wired up in later tasks
    KeyDerivation,
}

impl std::fmt::Display for EncError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EncError::WrongPasswordOrCorrupt => write!(f, "wrong password or corrupt data"),
            EncError::Encrypt => write!(f, "encryption failed"),
            EncError::KeyDerivation => write!(f, "key derivation failed"),
        }
    }
}

impl std::error::Error for EncError {}

/// Derive a 32-byte AES key from `password` and `salt` using Argon2id.
fn derive_key(password: &str, salt: &[u8]) -> Result<[u8; 32], EncError> {
    let params = Params::new(
        64 * 1024, // m_cost: 64 MiB
        3,         // t_cost: 3 iterations
        1,         // p_cost: 1 lane
        Some(32),  // output length
    )
    .map_err(|_| EncError::KeyDerivation)?;
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut key = [0u8; 32];
    argon2
        .hash_password_into(password.as_bytes(), salt, &mut key)
        .map_err(|_| EncError::KeyDerivation)?;
    Ok(key)
}

/// Encrypt `plain` bytes with `password`.
///
/// Returns `salt(16) || nonce(12) || ciphertext+tag`.
#[allow(dead_code)] // wired up in later tasks
pub fn encrypt(plain: &[u8], password: &str) -> Result<Vec<u8>, EncError> {
    let mut salt = [0u8; 16];
    let mut nonce_bytes = [0u8; 12];
    rand::thread_rng().fill_bytes(&mut salt);
    rand::thread_rng().fill_bytes(&mut nonce_bytes);

    let key_bytes = derive_key(password, &salt)?;
    let key: &Key<Aes256Gcm> = (&key_bytes).into();
    let cipher = Aes256Gcm::new(key);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, plain)
        .map_err(|_| EncError::Encrypt)?;

    let mut output = Vec::with_capacity(16 + 12 + ciphertext.len());
    output.extend_from_slice(&salt);
    output.extend_from_slice(&nonce_bytes);
    output.extend_from_slice(&ciphertext);
    Ok(output)
}

/// Decrypt a blob produced by [`encrypt`].
///
/// Returns `Err(EncError::WrongPasswordOrCorrupt)` on any failure.
#[allow(dead_code)] // wired up in later tasks
pub fn decrypt(blob: &[u8], password: &str) -> Result<Vec<u8>, EncError> {
    if blob.len() < 16 + 12 + 16 {
        // minimum: salt(16) + nonce(12) + tag(16)
        return Err(EncError::WrongPasswordOrCorrupt);
    }

    let salt = &blob[..16];
    let nonce_bytes = &blob[16..28];
    let ciphertext = &blob[28..];

    let key_bytes = derive_key(password, salt)?;
    let key: &Key<Aes256Gcm> = (&key_bytes).into();
    let cipher = Aes256Gcm::new(key);
    let nonce = Nonce::from_slice(nonce_bytes);

    cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| EncError::WrongPasswordOrCorrupt)
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypt_then_decrypt_returns_original() {
        let blob = encrypt(b"secret manifest", "master-pw").unwrap();
        assert_eq!(decrypt(&blob, "master-pw").unwrap(), b"secret manifest");
    }

    #[test]
    fn wrong_master_password_fails() {
        let blob = encrypt(b"secret manifest", "master-pw").unwrap();
        assert!(decrypt(&blob, "nope").is_err());
    }

    #[test]
    fn truncated_blob_returns_error() {
        // Empty blob — below minimum length guard.
        assert!(decrypt(&[], "pw").is_err());
        // Short but non-empty blob — still below minimum.
        assert!(decrypt(&[0u8; 10], "pw").is_err());
    }
}
