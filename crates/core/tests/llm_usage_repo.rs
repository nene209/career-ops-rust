//! Phase 3.1: the usage ledger's queries and its migration.
//!
//! The reads are the ones the **Usage** screen needs: totals by provider/model,
//! totals by task, and the recent calls. They are exercised here, without egui,
//! so the arithmetic is pinned independently of the view.

mod common;

use chrono::{Duration, Utc};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions};
use sqlx::ConnectOptions;

use career_ops_core::repo::job_descriptions as job_repo;
use career_ops_core::repo::llm_usage::{self, NewLlmUsage};

fn row<'a>(
    provider: &'a str,
    model: &'a str,
    task: &'a str,
    input: Option<i64>,
    output: Option<i64>,
    cost: Option<f64>,
) -> NewLlmUsage<'a> {
    NewLlmUsage {
        provider,
        model,
        task,
        application_id: None,
        job_description_id: None,
        input_tokens: input,
        output_tokens: output,
        total_tokens: input.zip(output).map(|(input, output)| input + output),
        cached_input_tokens: None,
        reasoning_tokens: None,
        source: "reported",
        cost_usd: cost,
        cost_complete: cost.map(|_| true),
        outcome: "ok",
        error_kind: None,
        duration_ms: 12,
    }
}

#[tokio::test]
async fn totals_group_by_provider_model_and_task() {
    let db = common::TestDb::new_raw().await;
    llm_usage::insert_many(
        &db.pool,
        &[
            row(
                "deepseek",
                "deepseek-flash",
                "evaluation",
                Some(100),
                Some(20),
                Some(0.001),
            ),
            row(
                "deepseek",
                "deepseek-flash",
                "cover_letter",
                Some(50),
                Some(10),
                Some(0.0005),
            ),
            row("openai", "gpt-x", "evaluation", Some(10), Some(5), None),
        ],
    )
    .await
    .expect("insert");

    let by_model = llm_usage::totals_by_provider_model(&db.pool, None)
        .await
        .expect("totals");
    assert_eq!(by_model.len(), 2);

    let deepseek = by_model
        .iter()
        .find(|group| group.model == "deepseek-flash")
        .expect("the deepseek group");
    assert_eq!(deepseek.calls, 2);
    assert_eq!(deepseek.input_tokens, 150);
    assert_eq!(deepseek.output_tokens, 30);
    assert_eq!(deepseek.total_tokens, 180);
    assert_eq!(deepseek.cost_usd, Some(0.0015));
    assert_eq!(deepseek.unpriced_calls, 0);
    assert_eq!(deepseek.reported_calls, 2);
    assert_eq!(deepseek.estimated_calls, 0);

    let openai = by_model
        .iter()
        .find(|group| group.model == "gpt-x")
        .expect("the unpriced group");
    assert_eq!(openai.cost_usd, None, "no price is not a zero");
    assert_eq!(openai.unpriced_calls, 1);

    let by_task = llm_usage::totals_by_task(&db.pool, None)
        .await
        .expect("tasks");
    assert_eq!(by_task.len(), 2);
    let evaluation = by_task
        .iter()
        .find(|group| group.task == "evaluation")
        .expect("evaluation");
    assert_eq!(evaluation.calls, 2, "one per provider");
    assert_eq!(evaluation.total_tokens, 135);
    assert_eq!(evaluation.unpriced_calls, 1);

    db.close().await;
}

#[tokio::test]
async fn a_missing_cost_is_counted_but_never_summed_as_zero() {
    let db = common::TestDb::new_raw().await;
    llm_usage::insert_many(
        &db.pool,
        &[
            row(
                "deepseek",
                "deepseek-flash",
                "evaluation",
                Some(10),
                Some(5),
                Some(2.0),
            ),
            row(
                "deepseek",
                "deepseek-flash",
                "evaluation",
                Some(10),
                Some(5),
                None,
            ),
        ],
    )
    .await
    .expect("insert");

    let groups = llm_usage::totals_by_provider_model(&db.pool, None)
        .await
        .expect("totals");
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].cost_usd, Some(2.0), "only priced rows are summed");
    assert_eq!(groups[0].unpriced_calls, 1, "the unpriced call is counted");

    db.close().await;
}

#[tokio::test]
async fn a_partial_cost_is_counted_as_a_lower_bound() {
    let db = common::TestDb::new_raw().await;
    let mut partial = row(
        "deepseek",
        "deepseek-flash",
        "evaluation",
        Some(100),
        Some(20),
        Some(1.0),
    );
    partial.cost_complete = Some(false);
    llm_usage::insert_many(&db.pool, &[partial])
        .await
        .expect("insert");

    let groups = llm_usage::totals_by_provider_model(&db.pool, None)
        .await
        .expect("totals");
    assert_eq!(groups[0].cost_usd, Some(1.0));
    assert_eq!(
        groups[0].unpriced_calls, 1,
        "an incomplete price is a lower bound and is counted"
    );

    db.close().await;
}

#[tokio::test]
async fn the_range_filter_excludes_rows_before_since() {
    let db = common::TestDb::new_raw().await;
    llm_usage::insert_many(
        &db.pool,
        &[
            row("old", "m", "evaluation", Some(1), Some(1), None),
            row("new", "m", "evaluation", Some(1), Some(1), None),
        ],
    )
    .await
    .expect("insert");

    // Move one row into the past without touching the repository's clock.
    sqlx::query(
        "UPDATE llm_usage SET created_at = '2020-01-01T00:00:00+00:00' WHERE provider = 'old'",
    )
    .execute(&db.pool)
    .await
    .expect("age the row");

    let since = Utc::now() - Duration::days(1);
    let recent_groups = llm_usage::totals_by_provider_model(&db.pool, Some(since))
        .await
        .expect("totals");
    assert_eq!(recent_groups.len(), 1);
    assert_eq!(recent_groups[0].provider, "new");

    let all_groups = llm_usage::totals_by_provider_model(&db.pool, None)
        .await
        .expect("all totals");
    assert_eq!(all_groups.len(), 2);

    let recent = llm_usage::recent(&db.pool, Some(since), 10)
        .await
        .expect("recent");
    assert_eq!(recent.len(), 1);
    assert_eq!(recent[0].provider, "new");

    db.close().await;
}

#[tokio::test]
async fn an_unknown_task_is_stored_and_read_back() {
    let db = common::TestDb::new_raw().await;
    llm_usage::insert_many(
        &db.pool,
        &[
            row(
                "deepseek",
                "deepseek-flash",
                "triage",
                Some(1),
                Some(1),
                None,
            ),
            row(
                "deepseek",
                "deepseek-flash",
                "a_later_task",
                Some(1),
                Some(1),
                None,
            ),
        ],
    )
    .await
    .expect("insert");

    let by_task = llm_usage::totals_by_task(&db.pool, None)
        .await
        .expect("tasks");
    let names: Vec<&str> = by_task.iter().map(|group| group.task.as_str()).collect();
    assert!(names.contains(&"triage"));
    assert!(names.contains(&"a_later_task"));

    db.close().await;
}

#[tokio::test]
async fn a_call_can_be_found_by_the_job_it_concerned() {
    let db = common::TestDb::new_raw().await;
    let job = common::phase3::store_job(&db, common::phase3::Fixture::by_name("strong")).await;
    let mut usage = row(
        "deepseek",
        "deepseek-flash",
        "evaluation",
        Some(10),
        Some(5),
        None,
    );
    usage.job_description_id = Some(job.id);
    llm_usage::insert_many(&db.pool, &[usage])
        .await
        .expect("insert");

    let rows = llm_usage::recent(&db.pool, None, 10).await.expect("recent");
    assert_eq!(rows[0].job_title.as_deref(), Some(job.title.as_str()));
    assert_eq!(rows[0].company.as_deref(), Some("Acme"));

    db.close().await;
}

#[tokio::test]
async fn deleting_a_job_keeps_its_usage_rows() {
    let db = common::TestDb::new_raw().await;
    let job = common::phase3::store_job(&db, common::phase3::Fixture::by_name("strong")).await;
    let mut usage = row(
        "deepseek",
        "deepseek-flash",
        "evaluation",
        Some(10),
        Some(5),
        None,
    );
    usage.job_description_id = Some(job.id);
    llm_usage::insert_many(&db.pool, &[usage])
        .await
        .expect("insert");

    job_repo::delete(&db.pool, job.id)
        .await
        .expect("delete the job");

    let rows = llm_usage::recent(&db.pool, None, 10).await.expect("recent");
    assert_eq!(rows.len(), 1, "the ledger is not a child of the job");
    assert_eq!(rows[0].total_tokens, Some(15));

    db.close().await;
}

// ----------------------------------------------------------------------
// Migration 0007
// ----------------------------------------------------------------------

const MIGRATIONS_BEFORE_0007: [&str; 6] = [
    include_str!("../migrations/0001_initial.sql"),
    include_str!("../migrations/0002_portal_fetch_status.sql"),
    include_str!("../migrations/0003_job_identity.sql"),
    include_str!("../migrations/0004_portal_cursor.sql"),
    include_str!("../migrations/0005_job_queue.sql"),
    include_str!("../migrations/0006_evaluation_provenance.sql"),
];

const MIGRATION_0007: &str = include_str!("../migrations/0007_llm_usage.sql");

async fn pool_at_0006() -> (tempfile::TempDir, SqlitePool) {
    let dir = tempfile::tempdir().expect("tempdir");
    let options = SqliteConnectOptions::new()
        .filename(dir.path().join("llm-usage-test.db"))
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

    for sql in MIGRATIONS_BEFORE_0007 {
        sqlx::raw_sql(sql).execute(&pool).await.expect("migrate");
    }

    (dir, pool)
}

#[tokio::test]
async fn migration_0007_applies_to_a_0006_database_and_preserves_rows() {
    let (_dir, pool) = pool_at_0006().await;

    sqlx::query(
        "INSERT INTO job_descriptions
         (id, title, company, source_url, raw_text, fetched_at, fetch_tier)
         VALUES (1, 'Engineer', 'Acme', 'https://example.com/jobs/1', 'body',
                 '2026-01-01T00:00:00Z', 'tier1b:html')",
    )
    .execute(&pool)
    .await
    .expect("insert job");
    sqlx::query(
        "INSERT INTO applications (id, job_description_id, status, created_at, updated_at)
         VALUES (1, 1, 'ready', '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
    )
    .execute(&pool)
    .await
    .expect("insert application");
    sqlx::query(
        "INSERT INTO evaluation_reports
         (id, application_id, content, generated_at, model_used, prompt_version, prompt_hash)
         VALUES (1, 1, 'old report', '2026-01-01T00:00:00Z', 'old-model', 1, 'abc')",
    )
    .execute(&pool)
    .await
    .expect("insert report");
    sqlx::query(
        "INSERT INTO cover_letters
         (id, application_id, content, generated_at, model_used)
         VALUES (1, 1, 'old letter', '2026-01-01T00:00:00Z', 'old-model')",
    )
    .execute(&pool)
    .await
    .expect("insert letter");

    sqlx::raw_sql(MIGRATION_0007)
        .execute(&pool)
        .await
        .expect("apply 0007");

    let report: String = sqlx::query_scalar("SELECT content FROM evaluation_reports WHERE id = 1")
        .fetch_one(&pool)
        .await
        .expect("the report still exists");
    assert_eq!(report, "old report", "the migration must not rewrite rows");

    let letters: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM cover_letters")
        .fetch_one(&pool)
        .await
        .expect("count letters");
    assert_eq!(letters, 1);

    let usage: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM llm_usage")
        .fetch_one(&pool)
        .await
        .expect("count usage");
    assert_eq!(usage, 0, "the ledger starts empty; nothing is backfilled");

    // The new table accepts a row through the repository, with the join the
    // screen uses.
    let mut first = row(
        "deepseek",
        "deepseek-flash",
        "evaluation",
        Some(100),
        Some(20),
        Some(0.001),
    );
    first.job_description_id = Some(1);
    first.application_id = Some(1);
    llm_usage::insert_many(&pool, &[first])
        .await
        .expect("insert after migrating");

    let rows = llm_usage::recent(&pool, None, 10).await.expect("recent");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].job_title.as_deref(), Some("Engineer"));

    pool.close().await;
}
