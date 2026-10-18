//! OS keychain integration for the KEK-keychain wrap.
//!
//! Uses the `keyring` crate with explicit platform features. A missing feature
//! would cause every read to return `NoEntry` silently — see the sentinel test
//! in `tests/keystore_safety.rs`.

use chrono::Utc;
use chacha20poly1305::aead::Aead;
use chacha20poly1305::{KeyInit, XChaCha20Poly1305, XNonce};
use rand::RngCore;

use super::wrap::{b64, unb64, KeyWrap, WrappedDek};
use crate::error::{CoreError, Result};

const SERVICE: &str = "com.career-ops.app";
const ACCOUNT_KEK: &str = "kek-keychain-v1";

#[cfg(feature = "keychain")]
use keyring::Entry;

#[cfg(feature = "keychain")]
pub fn create_and_wrap(dek: &[u8; 32]) -> Result<KeyWrap> {
    // Generate a fresh 32-byte KEK and store it in the keychain.
    let mut kek = [0u8; 32];
    rand::thread_rng().fill_bytes(&mut kek);

    let entry = Entry::new(SERVICE, ACCOUNT_KEK)
        .map_err(|e| CoreError::KeychainUnavailable(e.to_string()))?;
    entry
        .set_password(&b64(&kek))
        .map_err(|e| CoreError::KeychainUnavailable(e.to_string()))?;

    wrap_with_kek(dek, &kek)
}

#[cfg(feature = "keychain")]
pub fn unwrap(wrapped: &WrappedDek) -> Result<[u8; 32]> {
    let wrap = wrapped
        .wraps
        .iter()
        .find_map(|w| match w {
            KeyWrap::Keychain { nonce, ciphertext, .. } => Some((nonce, ciphertext)),
            _ => None,
        })
        .ok_or_else(|| CoreError::CorruptWrap("no keychain wrap present".into()))?;

    let entry = Entry::new(SERVICE, ACCOUNT_KEK)
        .map_err(|e| CoreError::KeychainUnavailable(e.to_string()))?;

    let kek_b64 = match entry.get_password() {
        Ok(v) => v,
        Err(keyring::Error::NoEntry) => return Err(CoreError::KeychainNoEntry),
        Err(e) => return Err(CoreError::KeychainUnavailable(e.to_string())),
    };

    let kek_bytes = unb64(&kek_b64).map_err(|e| CoreError::Crypto(e.to_string()))?;
    if kek_bytes.len() != 32 {
        return Err(CoreError::CorruptWrap("keychain KEK wrong length".into()));
    }
    let mut kek = [0u8; 32];
    kek.copy_from_slice(&kek_bytes);

    unwrap_with_kek(&unwrap_nonce(wrap.0)?, &unwrap_ciphertext(wrap.1)?, &kek)
}

#[cfg(not(feature = "keychain"))]
pub fn create_and_wrap(_dek: &[u8; 32]) -> Result<KeyWrap> {
    Err(CoreError::KeychainUnavailable(
        "built without `keychain` feature".into(),
    ))
}

#[cfg(not(feature = "keychain"))]
pub fn unwrap(_wrapped: &WrappedDek) -> Result<[u8; 32]> {
    Err(CoreError::KeychainUnavailable(
        "built without `keychain` feature".into(),
    ))
}

fn wrap_with_kek(dek: &[u8; 32], kek: &[u8; 32]) -> Result<KeyWrap> {
    let cipher = XChaCha20Poly1305::new_from_slice(kek).map_err(|_| CoreError::Internal("kek"))?;
    let mut nonce = [0u8; 24];
    rand::thread_rng().fill_bytes(&mut nonce);
    let ct = cipher
        .encrypt(&XNonce::from(nonce), dek.as_slice())
        .map_err(|_| CoreError::Crypto("keychain wrap encrypt failed".into()))?;
    Ok(KeyWrap::Keychain {
        created_at: Utc::now(),
        nonce: b64(&nonce),
        ciphertext: b64(&ct),
    })
}

fn unwrap_with_kek(nonce: &[u8; 24], ct: &[u8], kek: &[u8; 32]) -> Result<[u8; 32]> {
    let cipher = XChaCha20Poly1305::new_from_slice(kek).map_err(|_| CoreError::Internal("kek"))?;
    let pt = cipher
        .decrypt(&XNonce::from(*nonce), ct)
        .map_err(|_| CoreError::Crypto("keychain unwrap failed".into()))?;
    if pt.len() != 32 {
        return Err(CoreError::CorruptWrap("unwrap produced wrong length".into()));
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&pt);
    Ok(out)
}

fn unwrap_nonce(s: &str) -> Result<[u8; 24]> {
    let v = unb64(s).map_err(|e| CoreError::Crypto(e.to_string()))?;
    if v.len() != 24 {
        return Err(CoreError::CorruptWrap("nonce wrong length".into()));
    }
    let mut out = [0u8; 24];
    out.copy_from_slice(&v);
    Ok(out)
}

fn unwrap_ciphertext(s: &str) -> Result<Vec<u8>> {
    unb64(s).map_err(|e| CoreError::Crypto(e.to_string()))
}