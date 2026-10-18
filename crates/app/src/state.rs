//! Application state machine.
//!
//! This is the load-bearing part of the UI in Phase 1: the state machine
//! encodes the "unlock gates the app" rule. It is testable without GPUI.

use std::path::PathBuf;

use career_ops_core::error::Result;
use career_ops_core::keystore::{Keystore, UnlockOutcome};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Screen {
    Booting,
    FirstLaunch,
    Unlocking,
    KeychainUnavailable { reason: String },
    Recovery { reason: String },
    Ready,
}

pub struct AppState {
    data_dir: PathBuf,
    keystore: Option<Keystore>,
    screen: Screen,
}

impl AppState {
    /// Inspect the data directory and determine the initial screen.
    /// This is called once at startup and never again.
    pub fn bootstrap(data_dir: PathBuf) -> Result<Self> {
        let wrapped = data_dir.join(career_ops_core::keystore::WRAPPED_FILE);
        let db = data_dir.join(career_ops_core::keystore::DB_FILE);

        let (keystore, screen) = match (wrapped.exists(), db.exists()) {
            (false, false) => (None, Screen::FirstLaunch),
            (true, false) => (None, Screen::Recovery {
                reason: "wrapped key exists but database is missing".into(),
            }),
            (false, true) => (None, Screen::Recovery {
                reason: "database exists but wrapped key is missing. This database cannot be opened without its key.".into(),
            }),
            (true, true) => {
                let ks = Keystore::open(&data_dir)?;
                (Some(ks), Screen::Unlocking)
            }
        };

        Ok(Self { data_dir, keystore, screen })
    }

    pub fn current(&self) -> &Screen {
        &self.screen
    }

    pub fn data_dir(&self) -> &PathBuf {
        &self.data_dir
    }

    /// Attempt unlock via keychain. Only callable from `Unlocking`.
    pub fn try_keychain(&mut self) -> Result<()> {
        let ks = self.keystore.as_ref().expect("keystore present in Unlocking");
        match ks.unlock_with_keychain()? {
            UnlockOutcome::Unlocked(_dek) => {
                self.screen = Screen::Ready;
                Ok(())
            }
            UnlockOutcome::NeedsPassphrase => {
                // Stay on Unlocking; the UI should show the passphrase field.
                Ok(())
            }
            UnlockOutcome::KeychainUnavailable { reason } => {
                self.screen = Screen::KeychainUnavailable { reason };
                Ok(())
            }
            UnlockOutcome::RecoveryRequired { reason } => {
                self.screen = Screen::Recovery { reason };
                Ok(())
            }
        }
    }

    pub fn try_passphrase(&mut self, passphrase: &str) -> Result<()> {
        let ks = self.keystore.as_ref().expect("keystore present in Unlocking");
        match ks.unlock_with_passphrase(passphrase)? {
            UnlockOutcome::Unlocked(_dek) => {
                self.screen = Screen::Ready;
                Ok(())
            }
            UnlockOutcome::RecoveryRequired { reason } => {
                self.screen = Screen::Recovery { reason };
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn no_files_goes_to_first_launch() {
        let dir = tempdir().unwrap();
        let state = AppState::bootstrap(dir.path().to_path_buf()).unwrap();
        assert_eq!(state.current(), &Screen::FirstLaunch);
    }

    #[test]
    fn db_without_wrapped_goes_to_recovery() {
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("career-ops.db"), b"").unwrap();
        let state = AppState::bootstrap(dir.path().to_path_buf()).unwrap();
        assert!(matches!(state.current(), Screen::Recovery { .. }));
    }

    #[test]
    fn ready_is_unreachable_without_unlock() {
        // This is the core safety property. If `bootstrap` ever returned
        // `Ready` directly, the unlock gate would be broken.
        let dir = tempdir().unwrap();
        let state = AppState::bootstrap(dir.path().to_path_buf()).unwrap();
        assert_ne!(state.current(), &Screen::Ready);
    }
}