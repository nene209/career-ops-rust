//! Database round-trip tests.
//!
//! This is the highest-risk untested path in Phase 1: SQLCipher integration.
//! A wrong `PRAGMA key` or a feature-unification failure in
//! `libsqlite3-sys` manifests here, and here only.
//!
//! Uses `TestDb::new_raw()` — the keystore is not involved. These tests
//! verify that the *database layer* is correct, independently of how the
//! DEK was obtained.

mod common;

use career_ops_core::db;
use chrono::Utc;

/// Create, write, close, reopen with the same DEK, verify the row is there.
#[tokio::test]
async fn reopen_with_correct_key_preserves_data() {
    let db = common::TestDb::new_raw().await;

    // Write a row through the profile repository.
    career_ops_core::repo::profile::ensure_singleton(&db.pool)
        .await
        .expect("ensure profile");
    career_ops_core::repo::profile::update_identity(
        &db.pool,
        "Ada Lovelace",
        "ada@example.com",
        Some("London"),
    )
    .await
    .expect("update profile");

    // Close the pool before reopening. SQLite permits concurrent connections
    // to the same file, but closing first removes ambiguity about which
    // connection sees the write.
    db.pool.close().await;

    // Reopen with the same DEK.
    let pool = db::open(db.path(), &db.dek)
        .await
        .expect("reopen with correct key");

    let profile = career_ops_core::repo::profile::get(&pool)
        .await
        .expect("get profile")
        .expect("profile row exists");

    assert_eq!(profile.name, "Ada Lovelace");
    assert_eq!(profile.email, "ada@example.com");
    assert_eq!(profile.location.as_deref(), Some("London"));

    pool.close().await;
    db.close().await;
}

/// Reopen with a different DEK. The consistency check in `db::open` must
/// fail before any query can touch the schema.
#[tokio::test]
async fn reopen_with_wrong_key_fails_before_write() {
    let db = common::TestDb::new_raw().await;

    career_ops_core::repo::profile::ensure_singleton(&db.pool)
        .await
        .expect("ensure profile");

    db.pool.close().await;

    // A different DEK.
    let mut wrong_dek = [0u8; 32];
    for (i, b) in wrong_dek.iter_mut().enumerate() {
        // Deliberately different from db.dek. Any non-identical key works,
        // but flip all bytes so it is definitely not a key with a single
        // bit in common.
        *b = db.dek[i] ^ 0xFF;
    }

    let result = db::open(db.path(), &wrong_dek).await;
    assert!(
        result.is_err(),
        "reopening with a wrong key must fail at the consistency check"
    );

    db.close().await;
}

/// The `sqlite_master` query on a freshly-created database succeeds; this is
/// the check that runs immediately after `PRAGMA key` in `db::open`. A
/// regression here would mean the consistency check is not actually being
/// executed.
#[tokio::test]
async fn all_schema_tables_exist() {
    let db = common::TestDb::new_raw().await;

    let expected = [
        "profile",
        "cv_entries",
        "job_descriptions",
        "applications",
        "evaluation_reports",
        "cover_letters",
        "templates",
        "document_renders",
        "portals",
        "prompts",
        "settings",
    ];

    for table in expected {
        let exists: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM sqlite_master
             WHERE type = 'table' AND name = ?",
        )
        .bind(table)
        .fetch_one(&db.pool)
        .await
        .unwrap_or_else(|e| panic!("query sqlite_master for {table}: {e}"));

        assert_eq!(exists, 1, "table `{table}` missing from schema");
    }

    db.close().await;
}

/// Migrations must be idempotent: running them twice against the same
/// database is a no-op. This catches a released migration being edited
/// (checksum mismatch) and confirms that `db::open` is safe to call on
/// every startup.
#[tokio::test]
async fn migrations_are_idempotent() {
    let db = common::TestDb::new_raw().await;

    // The DB was created and migrated by `create_and_migrate`. Close it and
    // reopen, which runs migrations a second time via `db::open`.
    db.pool.close().await;

    let pool = db::open(db.path(), &db.dek)
        .await
        .expect("reopen");

    // Second run of the migration should not have changed anything. A
    // concrete assertion: the profile row we insert now is still the only
    // row when we close and reopen a third time.
    let now = Utc::now();
    sqlx::query(
        "INSERT INTO profile (id, name, email, created_at, updated_at)
         VALUES (1, 'test', 'test@example.com', ?, ?)
         ON CONFLICT(id) DO NOTHING",
    )
    .bind(now)
    .bind(now)
    .execute(&pool)
    .await
    .expect("insert profile");

    pool.close().await;

    let pool = db::open(db.path(), &db.dek).await.expect("third open");
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM profile")
        .fetch_one(&pool)
        .await
        .expect("count profiles");
    assert_eq!(count, 1);

    pool.close().await;
    db.close().await;
}