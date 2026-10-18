//! Argon2id-based passphrase wrap.
//!
//! Parameters are the RustCrypto Book minimums. They are safe but may need
//! tuning to hit the 250–500 ms target on representative hardware (Q-3).
//! The parameters are stored alongside the wrapped DEK so tuning does not
//! break existing databases.

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::aead::Aead;
use chacha20poly1305::{KeyInit, XChaCha20Poly1305, XNonce};
use chrono::Utc;
use rand::RngCore;

use super::wrap::{b64, unb64, KdfParams, KeyWrap, WrappedDek};
use crate::error::{CoreError, Result};

const M_COST_KIB: u32 = 19_456; // 19 MiB
const T_COST: u32 = 2;
const P_COST: u32 = 1;

pub fn derive_and_wrap(passphrase: &str, dek: &[u8; 32]) -> Result<KeyWrap> {
    let mut salt = [0u8; 16];
    rand::thread_rng().fill_bytes(&mut salt);

    let kdf = KdfParams {
        algo: "argon2id".into(),
        salt: b64(&salt),
        m_cost_kib: M_COST_KIB,
        t_cost: T_COST,
        p_cost: P_COST,
    };
    let kek = derive(passphrase, &kdf)?;

    let cipher = XChaCha20Poly1305::new_from_slice(&kek).map_err(|_| CoreError::Internal("kek"))?;
    let mut nonce = [0u8; 24];
    rand::thread_rng().fill_bytes(&mut nonce);
    let ct = cipher
        .encrypt(&XNonce::from(nonce), dek.as_slice())
        .map_err(|_| CoreError::Crypto("passphrase wrap encrypt failed".into()))?;

    Ok(KeyWrap::Passphrase {
        created_at: Utc::now(),
        kdf,
        nonce: b64(&nonce),
        ciphertext: b64(&ct),
    })
}

pub fn unwrap(wrapped: &WrappedDek, passphrase: &str) -> Result<[u8; 32]> {
    let (kdf, nonce, ct) = wrapped
        .wraps
        .iter()
        .find_map(|w| match w {
            KeyWrap::Passphrase { kdf, nonce, ciphertext, .. } => Some((kdf, nonce, ciphertext)),
            _ => None,
        })
        .ok_or_else(|| CoreError::CorruptWrap("no passphrase wrap present".into()))?;

    let kek = derive(passphrase, kdf)?;
    let cipher = XChaCha20Poly1305::new_from_slice(&kek).map_err(|_| CoreError::Internal("kek"))?;

    let nonce_bytes = unb64(nonce).map_err(|e| CoreError::Crypto(e.to_string()))?;
    let ct_bytes = unb64(ct).map_err(|e| CoreError::Crypto(e.to_string()))?;

    if nonce_bytes.len() != 24 {
        return Err(CoreError::CorruptWrap("nonce wrong length".into()));
    }
    let nonce_arr: [u8; 24] = nonce_bytes.as_slice().try_into().unwrap();

    let pt = cipher
        .decrypt(&XNonce::from(nonce_arr), ct_bytes.as_slice())
        .map_err(|_| CoreError::WrongPassphrase)?;

    if pt.len() != 32 {
        return Err(CoreError::CorruptWrap("unwrap produced wrong length".into()));
    }
    let mut out = [0u8; 32];
    out.copy_from_slice(&pt);
    Ok(out)
}

fn derive(passphrase: &str, kdf: &KdfParams) -> Result<[u8; 32]> {
    let params = Params::new(kdf.m_cost_kib, kdf.t_cost, kdf.p_cost, Some(32))
        .map_err(|e| CoreError::Crypto(format!("argon2 params: {e}")))?;
    let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let salt = unb64(&kdf.salt).map_err(|e| CoreError::Crypto(e.to_string()))?;
    let mut out = [0u8; 32];
    argon
        .hash_password_into(passphrase.as_bytes(), &salt, &mut out)
        .map_err(|e| CoreError::Crypto(format!("argon2 hash: {e}")))?;
    Ok(out)
}