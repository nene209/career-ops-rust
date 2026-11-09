//! The Pipeline load path (Phase 3.3, WS1).
//!
//! The Pipeline loads every job row in one query, ordered newest first with an
//! `id DESC` tie-break, and paginates the result in the view model. These tests
//! pin the query, the order, and the index migration; the page math lives in
//! the app's view-model tests.

mod common;

use career_ops_core::model::FetchTier;
use career_ops_core::repo::job_descriptions as job_repo;
use chrono::{DateTime, TimeZone, Utc};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions};
use sqlx::ConnectOptions;

use common::TestDb;

fn at(year: i32) -> DateTime<Utc> {
    Utc.with_ymd_and_hms(year, 1, 1, 0, 0, 0)
        .single()
        .expect("a valid timestamp")
}

async fn store(db: &TestDb, index: usize, fetched_at: DateTime<Utc>) -> i64 {
    let job = job_repo::upsert(
        &db.pool,
        job_repo::NewJobDescription {
            title: format!("Engineer {index}"),
            company: "Acme".into(),
            source_url: format!("https://example.com/jobs/{index}"),
            raw_text: format!("A posting for role {index}."),
            fetch_tier: FetchTier::Tier1bHtml,
            external_id: None,
        },
    )
    .await
    .expect("store job");

    // `upsert` uses `Utc::now()`; force the value so the order is explicit.
    sqlx::query("UPDATE job_descriptions SET fetched_at = ? WHERE id = ?")
        .bind(fetched_at)
        .bind(job.id)
        .execute(&db.pool)
        .await
        .expect("set fetched_at");

    job.id
}

#[tokio::test]
async fn list_all_is_not_a_bounded_page() {
    let db = TestDb::new_raw().await;

    for index in 0..501 {
        store(&db, index, at(2026)).await;
    }

    let all = job_repo::list_all(&db.pool).await.expect("list all");
    assert_eq!(all.len(), 501, "the Pipeline must see every stored job");

    let recent = job_repo::list_recent(&db.pool, 500)
        .await
        .expect("list recent");
    assert_eq!(
        recent.len(),
        500,
        "the bounded helper still honours its limit"
    );

    db.close().await;
}

#[tokio::test]
async fn equal_discovery_times_are_ordered_by_id_descending() {
    let db = TestDb::new_raw().await;

    let mut ids = Vec::new();
    for index in 0..3 {
        ids.push(store(&db, index, at(2026)).await);
    }

    let rows = job_repo::list_all(&db.pool).await.expect("list");
    let order: Vec<i64> = rows.iter().map(|row| row.id).collect();
    ids.reverse();

    assert_eq!(order, ids, "a timestamp tie must fall back to `id DESC`");

    // The newer row still sorts first when the timestamps differ.
    let newest = store(&db, 99, at(2027)).await;
    let rows = job_repo::list_all(&db.pool).await.expect("list");
    assert_eq!(rows.first().map(|row| row.id), Some(newest));

    db.close().await;
}

// ----------------------------------------------------------------------
// Migration 0009
// ----------------------------------------------------------------------

const MIGRATIONS_BEFORE_0009: [&str; 8] = [
    include_str!("../migrations/0001_initial.sql"),
    include_str!("../migrations/0002_portal_fetch_status.sql"),
    include_str!("../migrations/0003_job_identity.sql"),
    include_str!("../migrations/0004_portal_cursor.sql"),
    include_str!("../migrations/0005_job_queue.sql"),
    include_str!("../migrations/0006_evaluation_provenance.sql"),
    include_str!("../migrations/0007_llm_usage.sql"),
    include_str!("../migrations/0008_triage_provenance.sql"),
];

const MIGRATION_0009: &str = include_str!("../migrations/0009_job_fetched_index.sql");

async fn pool_at_0008() -> (tempfile::TempDir, SqlitePool) {
    let dir = tempfile::tempdir().expect("tempdir");
    let options = SqliteConnectOptions::new()
        .filename(dir.path().join("pipeline-load-test.db"))
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .pragma("key", format!("\"x'{}'\"", "cd".repeat(32)))
        .pragma("foreign_keys", "ON")
        .disable_statement_logging();

    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .expect("open");

    for sql in MIGRATIONS_BEFORE_0009 {
        sqlx::raw_sql(sql).execute(&pool).await.expect("migrate");
    }

    (dir, pool)
}

#[tokio::test]
async fn migration_0009_adds_the_order_index_and_preserves_rows() {
    let (_dir, pool) = pool_at_0008().await;

    sqlx::query(
        "INSERT INTO job_descriptions
         (id, title, company, source_url, raw_text, fetched_at, fetch_tier, queue_state)
         VALUES (1, 'Engineer', 'Acme', 'https://example.com/jobs/1', 'body',
                 '2026-01-01T00:00:00Z', 'tier1b:html', 'evaluate')",
    )
    .execute(&pool)
    .await
    .expect("insert job");

    sqlx::raw_sql(MIGRATION_0009)
        .execute(&pool)
        .await
        .expect("apply 0009");

    let (title, state): (String, String) =
        sqlx::query_as("SELECT title, queue_state FROM job_descriptions WHERE id = 1")
            .fetch_one(&pool)
            .await
            .expect("the job still exists");
    assert_eq!(
        title, "Engineer",
        "an index migration must not rewrite rows"
    );
    assert_eq!(state, "evaluate", "the user's queue choice survives");

    let index: Option<(String,)> = sqlx::query_as(
        "SELECT name FROM sqlite_master
         WHERE type = 'index' AND name = 'ix_job_descriptions_fetched_at'",
    )
    .fetch_optional(&pool)
    .await
    .expect("read the schema");
    assert!(index.is_some(), "0009 must create the order index");

    pool.close().await;
}
