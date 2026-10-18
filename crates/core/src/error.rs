use thiserror::Error;

pub type Result<T> = std::result::Result<T, CoreError>;

#[derive(Debug, Error)]
pub enum CoreError {
    #[error("keystore not initialized: {0}")]
    KeystoreNotInitialized(String),

    #[error("keystore lock file exists at {0}, but the key is unavailable. This database cannot be opened.")]
    KeystoreLocked(std::path::PathBuf),

    #[error("passphrase is incorrect")]
    WrongPassphrase,

    #[error("keychain entry unavailable: {0}")]
    KeychainUnavailable(String),

    #[error("keychain entry missing (NoEntry)")]
    KeychainNoEntry,

    #[error("corrupt wrapped key: {0}")]
    CorruptWrap(String),

    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),

    #[error("migration error: {0}")]
    Migration(#[from] sqlx::migrate::MigrateError),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("serde error: {0}")]
    Serde(#[from] serde_json::Error),

    #[error("crypto error: {0}")]
    Crypto(String),

    #[error("backend unsupported: {0}")]
    BackendUnsupported(&'static str),

    #[error("internal invariant violated: {0}")]
    Internal(&'static str),
}

impl From<argon2::password_hash::Error> for CoreError {
    fn from(e: argon2::password_hash::Error) -> Self {
        CoreError::Crypto(format!("argon2: {e}"))
    }
}

impl From<chacha20poly1305::aead::Error> for CoreError {
    fn from(_: chacha20poly1305::aead::Error) -> Self {
        CoreError::Crypto("aead authentication failed".into())
    }
}
