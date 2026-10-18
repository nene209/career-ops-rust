use base64::Engine;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// A versioned bundle of wrapped DEK copies.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WrappedDek {
    pub version: u32,
    pub wraps: Vec<KeyWrap>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "method", rename_all = "snake_case")]
pub enum KeyWrap {
    Keychain {
        created_at: DateTime<Utc>,
        nonce: String,
        ciphertext: String,
    },
    Passphrase {
        created_at: DateTime<Utc>,
        kdf: KdfParams,
        nonce: String,
        ciphertext: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KdfParams {
    pub algo: String,
    pub salt: String,
    pub m_cost_kib: u32,
    pub t_cost: u32,
    pub p_cost: u32,
}

pub(crate) fn b64(bytes: &[u8]) -> String {
    base64::engine::general_purpose::STANDARD.encode(bytes)
}

pub(crate) fn unb64(s: &str) -> Result<Vec<u8>, base64::DecodeError> {
    base64::engine::general_purpose::STANDARD.decode(s)
}