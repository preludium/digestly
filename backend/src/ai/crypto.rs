//! Authenticated encryption for secrets at rest (prompt.md §6, §11).
//!
//! Provider API keys (and, later, ntfy tokens) are encrypted with a key derived from
//! `SECRET_KEY` and stored as a BLOB. Plaintext is **never** returned by any endpoint or logged.
//! Format: `nonce (12 bytes) || ChaCha20-Poly1305 ciphertext+tag`.

use anyhow::{anyhow, Result};
use argon2::password_hash::rand_core::{OsRng, RngCore};
use chacha20poly1305::aead::Aead;
use chacha20poly1305::{ChaCha20Poly1305, Key, KeyInit, Nonce};

const NONCE_LEN: usize = 12;

/// Encrypt `plaintext` with the instance AEAD key. Returns `nonce || ciphertext`.
pub fn encrypt(enc_key: &[u8; 32], plaintext: &str) -> Result<Vec<u8>> {
    let cipher = ChaCha20Poly1305::new(Key::from_slice(enc_key));
    let mut nonce_bytes = [0u8; NONCE_LEN];
    OsRng.fill_bytes(&mut nonce_bytes);
    let nonce = Nonce::from_slice(&nonce_bytes);

    let ciphertext = cipher
        .encrypt(nonce, plaintext.as_bytes())
        .map_err(|_| anyhow!("encryption failed"))?;

    let mut out = Vec::with_capacity(NONCE_LEN + ciphertext.len());
    out.extend_from_slice(&nonce_bytes);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

/// Decrypt a blob produced by [`encrypt`]. Fails if the key changed or the data is corrupt.
pub fn decrypt(enc_key: &[u8; 32], blob: &[u8]) -> Result<String> {
    if blob.len() <= NONCE_LEN {
        return Err(anyhow!("ciphertext too short"));
    }
    let cipher = ChaCha20Poly1305::new(Key::from_slice(enc_key));
    let (nonce_bytes, ciphertext) = blob.split_at(NONCE_LEN);
    let nonce = Nonce::from_slice(nonce_bytes);

    let plaintext = cipher
        .decrypt(nonce, ciphertext)
        .map_err(|_| anyhow!("decryption failed (wrong SECRET_KEY or corrupt data)"))?;
    String::from_utf8(plaintext).map_err(|_| anyhow!("decrypted key is not valid UTF-8"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key() -> [u8; 32] {
        [7u8; 32]
    }

    #[test]
    fn round_trips() {
        let k = key();
        let secret = "sk-super-secret-api-key-123";
        let blob = encrypt(&k, secret).unwrap();
        // Ciphertext must not contain the plaintext (encrypted at rest, §11).
        assert!(!blob.windows(secret.len()).any(|w| w == secret.as_bytes()));
        assert_eq!(decrypt(&k, &blob).unwrap(), secret);
    }

    #[test]
    fn wrong_key_fails() {
        let blob = encrypt(&key(), "secret").unwrap();
        assert!(decrypt(&[9u8; 32], &blob).is_err());
    }

    #[test]
    fn distinct_nonces_yield_distinct_ciphertext() {
        let k = key();
        let a = encrypt(&k, "same").unwrap();
        let b = encrypt(&k, "same").unwrap();
        assert_ne!(a, b); // random nonce → non-deterministic ciphertext
    }
}
