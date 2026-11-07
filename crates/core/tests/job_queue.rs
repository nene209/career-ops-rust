//! The pre-evaluation queue (migration 0005).
//!
//! Two things are under test here and they fail differently:
//!
//! * the migration itself, applied to a database that is otherwise at 0004 —
//!   the schema change must be additive and must not touch existing rows;
//! * the repository write path, which the Pipeline's three queue actions use.

mod common;

use career_ops_core::model::{FetchTier, QueueState};
use career_ops_core::repo::job_descriptions as job_repo;
use career_ops_core::repo::job_descriptions::NewJobDescription;
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions};
use sqlx::ConnectOptions;

/// Every migration that predates the queue, in order.
const MIGRATIONS_BEFORE_0005: &[&str] = &[
    include_str!("../migrations/0001_initial.sql"),
    include_str!("../migrations/0002_portal_fetch_status.sql"),
    include_str!("../migrations/0003_job_identity.sql"),
    include_str!("../migrations/0004_portal_cursor.sql"),
];

const MIGRATION_0005: &str = include_str!("../migrations/0005_job_queue.sql");

/// An encrypted pool at the 0004 schema, with no migrator involved.
///
/// Reusing `db::create_and_migrate` would run 0005 as well, which is exactly
/// what this test needs to do by hand.
async fn pool_at_0004() -> (tempfile::TempDir, SqlitePool) {
    let dir = tempfile::tempdir().expect("tempdir");
    let options = SqliteConnectOptions::new()
        .filename(dir.path().join("queue-test.db"))
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .pragma("key", format!("\"x'{}'\"", "ab".repeat(32)))
        .pragma("foreign_keys", "ON")
        .disable_statement_logging();

    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .expect("open");

    for sql in MIGRATIONS_BEFORE_0005 {
        sqlx::raw_sql(sql).execute(&pool).await.expect("migrate");
    }

    (dir, pool)
}

#[tokio::test]
async fn migration_0005_applies_to_a_0004_database_and_preserves_rows() {
    let (_dir, pool) = pool_at_0004().await;

    // A row written by a build that has never heard of the queue.
    sqlx::query(
        "INSERT INTO job_descriptions
         (title, company, source_url, raw_text, fetched_at, fetch_tier)
         VALUES ('Engineer', 'Acme', 'https://example.com/jobs/1', 'body',
                 '2026-01-01T00:00:00Z', 'tier1b:html')",
    )
    .execute(&pool)
    .await
    .expect("insert a pre-queue row");

    sqlx::raw_sql(MIGRATION_0005)
        .execute(&pool)
        .await
        .expect("apply 0005");

    let (title, state, reason, queued_at): (String, String, Option<String>, Option<String>) =
        sqlx::query_as(
            "SELECT title, queue_state, queue_reason, queued_at
             FROM job_descriptions WHERE id = 1",
        )
        .fetch_one(&pool)
        .await
        .expect("the row still exists");

    assert_eq!(title, "Engineer", "the migration must not rewrite rows");
    assert_eq!(state, "new", "0005's default backfills existing rows");
    assert_eq!(reason, None);
    assert_eq!(queued_at, None, "an untouched row has no queue timestamp");

    let index: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM sqlite_master
         WHERE type = 'index' AND name = 'ix_job_descriptions_queue_state'",
    )
    .fetch_one(&pool)
    .await
    .expect("query sqlite_master");
    assert_eq!(index, 1, "the queue filter needs its index");

    pool.close().await;
}

#[tokio::test]
async fn a_fresh_job_starts_in_the_new_state() {
    let db = common::TestDb::new_raw().await;

    let stored = job_repo::upsert(&db.pool, new_job("Engineer", "jobs/1"))
        .await
        .expect("store");

    assert_eq!(stored.queue_state, QueueState::New.as_str());
    assert_eq!(stored.queue_reason, None);
    assert_eq!(stored.queued_at, None);

    db.close().await;
}

#[tokio::test]
async fn every_queue_state_round_trips_through_the_repository() {
    let db = common::TestDb::new_raw().await;
    let stored = job_repo::upsert(&db.pool, new_job("Engineer", "jobs/1"))
        .await
        .expect("store");

    let cases = [
        QueueState::DetailsNeeded,
        QueueState::Evaluate,
        QueueState::Dismissed,
        QueueState::Evaluated,
        // `New` is the default, but it is still writable: a user can undo a
        // decision by returning a row to the queue's start.
        QueueState::New,
    ];

    for state in cases {
        job_repo::set_queue_state(&db.pool, stored.id, state, Some("reason"))
            .await
            .expect("set queue state");

        let row = job_repo::get(&db.pool, stored.id)
            .await
            .expect("get")
            .expect("row");
        assert_eq!(row.queue_state, state.as_str());
        assert_eq!(row.queue_reason.as_deref(), Some("reason"));
        assert!(row.queued_at.is_some(), "a write records when it happened");
    }

    db.close().await;
}

#[tokio::test]
async fn set_queue_state_errors_when_the_row_is_gone() {
    let db = common::TestDb::new_raw().await;

    // A stale list is not a write to swallow: the app reloads rather than
    // showing a badge the database does not have.
    let result = job_repo::set_queue_state(&db.pool, 404, QueueState::Evaluate, None).await;
    assert!(result.is_err(), "a missing row must be reported");

    db.close().await;
}

#[tokio::test]
async fn set_queue_states_updates_several_rows_and_reports_the_count() {
    let db = common::TestDb::new_raw().await;
    let first = job_repo::upsert(&db.pool, new_job("Engineer", "jobs/1"))
        .await
        .expect("store");
    let second = job_repo::upsert(&db.pool, new_job("Designer", "jobs/2"))
        .await
        .expect("store");

    let updated = job_repo::set_queue_states(
        &db.pool,
        &[first.id, second.id],
        QueueState::DetailsNeeded,
        Some("wants details"),
    )
    .await
    .expect("batch");
    assert_eq!(updated, 2);

    // A batch that includes a vanished row counts what it actually changed
    // rather than failing the whole transaction.
    let updated =
        job_repo::set_queue_states(&db.pool, &[first.id, 404], QueueState::Evaluate, None)
            .await
            .expect("batch with a stale id");
    assert_eq!(updated, 1);

    let rows = job_repo::list_recent(&db.pool, 10).await.expect("list");
    let by_id = |id: i64| {
        rows.iter()
            .find(|row| row.id == id)
            .expect("row in the listing")
    };
    assert_eq!(by_id(first.id).queue_state, QueueState::Evaluate.as_str());
    assert_eq!(
        by_id(second.id).queue_state,
        QueueState::DetailsNeeded.as_str()
    );

    db.close().await;
}

/// The column is free text, so a build that does not know a value must not
/// guess at it.
#[test]
fn queue_state_parsing_is_exact_and_labels_are_unique() {
    let all = [
        QueueState::New,
        QueueState::DetailsNeeded,
        QueueState::Evaluate,
        QueueState::Dismissed,
        QueueState::Evaluated,
    ];

    for state in all {
        assert_eq!(QueueState::parse(state.as_str()), Some(state));
        assert!(!state.label().is_empty());
    }

    assert_eq!(
        QueueState::parse("  evaluate  "),
        Some(QueueState::Evaluate)
    );
    assert_eq!(QueueState::parse("details-needed"), None);
    assert_eq!(QueueState::parse(""), None);

    let labels: Vec<&str> = all.iter().map(|state| state.label()).collect();
    let unique: std::collections::HashSet<&str> = labels.iter().copied().collect();
    assert_eq!(labels.len(), unique.len(), "duplicate labels: {labels:?}");

    let values: Vec<&str> = all.iter().map(|state| state.as_str()).collect();
    let unique: std::collections::HashSet<&str> = values.iter().copied().collect();
    assert_eq!(values.len(), unique.len(), "duplicate storage values");
}

fn new_job(title: &str, path: &str) -> NewJobDescription {
    NewJobDescription {
        title: title.into(),
        company: "Acme".into(),
        source_url: format!("https://example.com/{path}"),
        raw_text: "body".into(),
        fetch_tier: FetchTier::Tier1bHtml,
        external_id: None,
    }
}
