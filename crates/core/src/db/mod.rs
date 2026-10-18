//! Encrypted SQLite setup via SQLCipher.
//!
//! The DEK from the keystore is passed directly to SQLCipher as a raw key
//! (`PRAGMA key = "x'<hex>'"`), which skips SQLCipher's internal PBKDF2
//! step — we already generated the DEK randomly.
//!
//! After opening, we run a consistency check (`SELECT count(*) FROM sqlite_master`)
//! before allowing any other query. A wrong key causes the check to fail with
//! a clear error rather than silently corrupting writes.

use std::path::Path;

use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions};
use sqlx::ConnectOptions;

use crate::error::Result;
use crate::keystore::DB_FILE;

/// Open the encrypted database at `data_dir/DB_FILE` with `dek`.
pub async fn open(data_dir: impl AsRef<Path>, dek: &[u8; 32]) -> Result<SqlitePool> {
    let path = data_dir.as_ref().join(DB_FILE);
    let key_hex = hex::encode(dek);

    let options = SqliteConnectOptions::new()
        .filename(&path)
        .create_if_missing(false)
        .journal_mode(SqliteJournalMode::Wal)
        .pragma("key", format!("\"x'{}'\"", key_hex))
        .pragma("foreign_keys", "ON")
        .disable_statement_logging();

    let pool = SqlitePoolOptions::new()
        .max_connections(4)
        .connect_with(options)
        .await?;

    let check = sqlx::query("SELECT count(*) FROM sqlite_master")
        .fetch_one(&pool)
        .await;
    if let Err(e) = check {
        pool.close().await;
        return Err(crate::error::CoreError::Database(e));
    }

    // Run pending migrations. Idempotent: a no-op if all are applied.
    // A checksum mismatch (a released migration was edited) errors here,
    // enforcing the append-only rule at runtime.
    sqlx::migrate!("./migrations").run(&pool).await?;

    Ok(pool)
}

/// Create a fresh encrypted database and run migrations.
/// Called only from `initialize` in the app orchestrator; never called if a
/// database file already exists.
pub async fn create_and_migrate(data_dir: impl AsRef<Path>, dek: &[u8; 32]) -> Result<SqlitePool> {
    let path = data_dir.as_ref().join(DB_FILE);
    if path.exists() {
        return Err(crate::error::CoreError::KeystoreLocked(path));
    }

    let key_hex = hex::encode(dek);
    let options = SqliteConnectOptions::new()
        .filename(&path)
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .pragma("key", format!("\"x'{}'\"", key_hex))
        .pragma("foreign_keys", "ON")
        .disable_statement_logging();

    let pool = SqlitePoolOptions::new()
        .max_connections(4)
        .connect_with(options)
        .await?;

    sqlx::migrate!("./migrations").run(&pool).await?;
    Ok(pool)
}
