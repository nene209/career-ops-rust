//! Shared test harness for `career-ops-core` integration tests.
//!
//! Provides a fresh, migrated, encrypted SQLite database in a tempdir.
//! Two constructors exist because different test suites need different
//! things:
//!
//!   - `TestDb::new_raw()` — generates a DEK directly, no keystore. Works
//!     without the `keychain` feature. Use for tests that need a database
//!     but not the full unlock flow (prompt seeding, repositories).
//!
//!   - `TestDb::new_via_keystore()` — runs through `Keystore::initialize`.
//!     Requires the `keychain` feature. Use for tests that need to verify
//!     the integration between keystore and DB (unlock flows, key rotation).
//!
//! # Cleanup
//!
//! Always call `db.close().await` at the end of a test. `SqlitePool` is
//! `Drop`, but `TempDir` cleanup can fail on Windows if the file is still
//! held; `close()` awaits `pool.close()` first, which releases the handle.

#![allow(dead_code)]

use std::path::Path;
use std::sync::Once;

use career_ops_core::db;
use rand::RngCore;
use sqlx::SqlitePool;
use tempfile::TempDir;
use zeroize::Zeroizing;

#[cfg(feature = "keychain")]
use career_ops_core::keystore::Keystore;

static KEYRING_INIT: Once = Once::new();

pub const TEST_PASSPHRASE: &str = "test-correct-horse-battery-staple";

/// Set up the in-process keychain mock. Idempotent: safe to call from every
/// test binary that uses the keystore.
pub fn setup_keyring_mock() {
    KEYRING_INIT.call_once(|| {
        keyring::set_default_credential_builder(keyring::mock::default_credential_builder());
    });
}

pub struct TestDb {
    pub pool: SqlitePool,
    pub dir: TempDir,
    pub dek: Zeroizing<[u8; 32]>,
}

impl TestDb {
    /// Fresh encrypted DB with a random DEK. No keystore, no keychain.
    pub async fn new_raw() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut dek = Zeroizing::new([0u8; 32]);
        rand::thread_rng().fill_bytes(&mut *dek);

        let pool = db::create_and_migrate(dir.path(), &dek)
            .await
            .expect("db::create_and_migrate");

        Self { pool, dir, dek }
    }

    /// Fresh encrypted DB created through the full keystore flow.
    /// Requires the `keychain` feature.
    #[cfg(feature = "keychain")]
    pub async fn new_via_keystore() -> Self {
        setup_keyring_mock();
        let dir = tempfile::tempdir().expect("tempdir");

        let (_ks, dek) =
            Keystore::initialize(dir.path(), TEST_PASSPHRASE).expect("Keystore::initialize");

        let pool = db::create_and_migrate(dir.path(), &dek)
            .await
            .expect("db::create_and_migrate");

        Self { pool, dir, dek }
    }

    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    pub async fn close(self) {
        self.pool.close().await;
        drop(self.dir);
    }

    /// Reopen the encrypted database at `self.path()` with `self.dek`.
    /// Returns a fresh pool. The original pool stays open; callers should
    /// either close it first or accept two connections to the same file
    /// (SQLite permits this — WAL mode is set in `db::create_and_migrate`).
    pub async fn reopen(&self) -> Result<SqlitePool, career_ops_core::error::CoreError> {
        career_ops_core::db::open(self.path(), &self.dek).await
    }
}
