//! Application state machine.
//!
//! Phase 1's load-bearing UI concern: the state machine encodes the
//! "unlock gates the app" rule, and now owns the database pool and the
//! seeding of system prompts on every successful unlock.

use std::path::PathBuf;

use career_ops_core::db;
use career_ops_core::error::{CoreError, Result};
use career_ops_core::keystore::{Keystore, UnlockOutcome};
use career_ops_core::prompts_seed;
use sqlx::SqlitePool;
use zeroize::Zeroizing;

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
    pool: Option<SqlitePool>,
    screen: Screen,
}

impl AppState {
    pub fn bootstrap(data_dir: PathBuf) -> Result<Self> {
        let wrapped = data_dir.join(career_ops_core::keystore::WRAPPED_FILE);
        let db_path = data_dir.join(career_ops_core::keystore::DB_FILE);

        let (keystore, screen) = match (wrapped.exists(), db_path.exists()) {
            (false, false) => (None, Screen::FirstLaunch),
            (true, false) => (
                None,
                Screen::Recovery {
                    reason: "wrapped key exists but database is missing".into(),
                },
            ),
            (false, true) => (
                None,
                Screen::Recovery {
                    reason: "database exists but wrapped key is missing. \
                             This database cannot be opened without its key."
                        .into(),
                },
            ),
            (true, true) => {
                let ks = Keystore::open(&data_dir)?;
                (Some(ks), Screen::Unlocking)
            }
        };

        Ok(Self {
            data_dir,
            keystore,
            pool: None,
            screen,
        })
    }

    pub fn current(&self) -> &Screen {
        &self.screen
    }

    pub fn data_dir(&self) -> &PathBuf {
        &self.data_dir
    }

    /// The pool is `Some` only when `screen == Ready`. Callers in Phase 3+
    /// use this to run repository methods; Phase 1 callers do not.
    pub fn pool(&self) -> Option<&SqlitePool> {
        self.pool.as_ref()
    }

    // ------------------------------------------------------------------
    // Unlock paths
    // ------------------------------------------------------------------

    pub async fn try_keychain(&mut self) -> Result<()> {
        let ks = self
            .keystore
            .as_ref()
            .ok_or(CoreError::Internal("keystore missing in Unlocking state"))?;

        match ks.unlock_with_keychain()? {
            UnlockOutcome::Unlocked(dek) => self.post_unlock(dek).await,
            UnlockOutcome::NeedsPassphrase => Ok(()),
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

    pub async fn try_passphrase(&mut self, passphrase: &str) -> Result<()> {
        let ks = self
            .keystore
            .as_ref()
            .ok_or(CoreError::Internal("keystore missing in Unlocking state"))?;

        match ks.unlock_with_passphrase(passphrase)? {
            UnlockOutcome::Unlocked(dek) => self.post_unlock(dek).await,
            UnlockOutcome::RecoveryRequired { reason } => {
                self.screen = Screen::Recovery { reason };
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// Called by the first-launch wizard once the user has set a passphrase.
    /// Creates the keystore and database, seeds prompts, and transitions to
    /// Ready in one step — there is no intermediate "unlocking" state on the
    /// first-launch path.
    pub async fn complete_first_launch(&mut self, passphrase: &str) -> Result<()> {
        let (keystore, dek) = Keystore::initialize(&self.data_dir, passphrase)?;
        let pool = db::create_and_migrate(&self.data_dir, &dek).await?;

        self.run_seeding(&pool).await;

        self.keystore = Some(keystore);
        self.pool = Some(pool);
        self.screen = Screen::Ready;
        Ok(())
    }

    async fn post_unlock(&mut self, dek: Zeroizing<[u8; 32]>) -> Result<()> {
        let pool = db::open(&self.data_dir, &dek).await?;

        self.run_seeding(&pool).await;

        self.pool = Some(pool);
        self.screen = Screen::Ready;
        Ok(())
    }

    /// Seed system prompts. Non-fatal on failure: the app continues to Ready
    /// so the pipeline UI is usable; only Phase 3 evaluation and cover letter
    /// generation would be affected. The next launch retries.
    async fn run_seeding(&self, pool: &SqlitePool) {
        if let Err(e) = prompts_seed::seed_or_update_system_prompts(pool).await {
            tracing::error!(error = %e, "prompt seeding failed; app continues");
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
        assert!(state.pool().is_none());
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
        let dir = tempdir().unwrap();
        let state = AppState::bootstrap(dir.path().to_path_buf()).unwrap();
        assert_ne!(state.current(), &Screen::Ready);
        assert!(state.pool().is_none());
    }
}