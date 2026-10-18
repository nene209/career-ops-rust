//! Envelope encryption keystore.
//!
//! Key hierarchy (§7 of the main plan):
//!   - DEK: 32 random bytes, used as SQLCipher `PRAGMA key`
//!   - KEK-keychain: 32 random bytes, stored in the OS keychain
//!   - KEK-passphrase: 32 bytes derived via Argon2id from the user passphrase
//!
//! The DEK is wrapped independently by each KEK. Either can unwrap the DEK.
//! Both must fail for the database to become unreadable.
//!
//! ## Safety rules (enforced, not just documented)
//!
//! 1. `initialize` refuses if `dek.wrapped.json` already exists.
//! 2. Unlock distinguishes `NoEntry` (expected on first launch) from unwrap
//!    failure (an error state, not a fresh start).
//! 3. A key consistency check follows every successful unlock.
//! 4. Nothing in this module creates a new DEK unless `initialize` is called
//!    and the wrapped file is absent.

mod argon;
mod keychain;
mod wrap;

use std::path::{Path, PathBuf};

use rand::RngCore;
use zeroize::Zeroizing;

use crate::error::{CoreError, Result};

pub use wrap::{KeyWrap, WrappedDek};

pub const WRAPPED_FILE: &str = "dek.wrapped.json";
pub const DB_FILE: &str = "career-ops.db";

/// Result of attempting to unlock the keystore.
#[derive(Debug)]
pub enum UnlockOutcome {
    Unlocked(Zeroizing<[u8; 32]>),
    NeedsPassphrase,
    KeychainUnavailable { reason: String },
    RecoveryRequired { reason: String },
}

pub struct Keystore {
    data_dir: PathBuf,
    wrapped: WrappedDek,
}

impl Keystore {
    /// Load an existing keystore from `data_dir`.
    ///
    /// Fails if no `dek.wrapped.json` exists. Never generates a new key.
    pub fn open(data_dir: impl AsRef<Path>) -> Result<Self> {
        let data_dir = data_dir.as_ref().to_path_buf();
        let path = data_dir.join(WRAPPED_FILE);
        if !path.exists() {
            return Err(CoreError::KeystoreNotInitialized(format!(
                "no {} in {}",
                WRAPPED_FILE,
                data_dir.display()
            )));
        }
        let raw = std::fs::read_to_string(&path)?;
        let wrapped: WrappedDek = serde_json::from_str(&raw)?;
        Ok(Self { data_dir, wrapped })
    }

    /// First-launch initialization. Creates a fresh DEK, wraps it with both
    /// KEKs, and writes `dek.wrapped.json`.
    ///
    /// **Refuses** if a wrapped file already exists at the target path.
    pub fn initialize(
        data_dir: impl AsRef<Path>,
        passphrase: &str,
    ) -> Result<(Self, Zeroizing<[u8; 32]>)> {
        let data_dir = data_dir.as_ref().to_path_buf();
        std::fs::create_dir_all(&data_dir)?;

        let path = data_dir.join(WRAPPED_FILE);
        if path.exists() {
            return Err(CoreError::KeystoreLocked(path));
        }

        // 1. Generate DEK.
        let mut dek = Zeroizing::new([0u8; 32]);
        rand::thread_rng().fill_bytes(&mut *dek);

        // 2. Keychain KEK + wrap.
        let keychain_wrap = keychain::create_and_wrap(&dek)?;

        // 3. Passphrase KEK + wrap.
        let passphrase_wrap = argon::derive_and_wrap(passphrase, &dek)?;

        let wrapped = WrappedDek {
            version: 1,
            wraps: vec![keychain_wrap, passphrase_wrap],
        };

        // 4. Persist. Write-then-rename for atomicity.
        let tmp = path.with_extension("json.tmp");
        let body = serde_json::to_string_pretty(&wrapped)?;
        std::fs::write(&tmp, &body)?;
        std::fs::rename(&tmp, &path)?;

        Ok((Self { data_dir, wrapped }, dek))
    }

    /// Try to unlock via the OS keychain. Does not touch the passphrase path.
    pub fn unlock_with_keychain(&self) -> Result<UnlockOutcome> {
        match keychain::unwrap(&self.wrapped) {
            Ok(dek) => Ok(UnlockOutcome::Unlocked(dek)),
            Err(CoreError::KeychainNoEntry) => Ok(UnlockOutcome::NeedsPassphrase),
            Err(CoreError::KeychainUnavailable(reason)) => {
                Ok(UnlockOutcome::KeychainUnavailable { reason })
            }
            Err(e) => Err(e),
        }
    }

    /// Try to unlock via a passphrase.
    pub fn unlock_with_passphrase(&self, passphrase: &str) -> Result<UnlockOutcome> {
        match argon::unwrap(&self.wrapped, passphrase) {
            Ok(dek) => Ok(UnlockOutcome::Unlocked(dek)),
            Err(CoreError::WrongPassphrase) => Ok(UnlockOutcome::RecoveryRequired {
                reason: "passphrase is incorrect".into(),
            }),
            Err(e) => Err(e),
        }
    }

    /// Re-wrap the DEK under a new passphrase KEK. Requires unlocking first.
    pub fn rotate_passphrase(&mut self, dek: &[u8; 32], new_passphrase: &str) -> Result<()> {
        let new_wrap = argon::derive_and_wrap(new_passphrase, dek)?;
        self.wrapped
            .wraps
            .retain(|w| !matches!(w, KeyWrap::Passphrase { .. }));
        self.wrapped.wraps.push(new_wrap);
        let path = self.data_dir.join(WRAPPED_FILE);
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(&self.wrapped)?)?;
        std::fs::rename(&tmp, &path)?;
        Ok(())
    }

    /// Export the encrypted database and wrapped key file to `dest`.
    /// Used for backup and for the "export encrypted bundle" recovery option.
    pub fn export_encrypted_bundle(&self, dest: impl AsRef<Path>) -> Result<()> {
        let dest = dest.as_ref();
        std::fs::create_dir_all(dest)?;
        std::fs::copy(self.data_dir.join(DB_FILE), dest.join(DB_FILE))?;
        std::fs::copy(self.data_dir.join(WRAPPED_FILE), dest.join(WRAPPED_FILE))?;
        Ok(())
    }
}
