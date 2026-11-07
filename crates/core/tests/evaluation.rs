//! The evaluation pipeline (Phase 3, WS3).
//!
//! Everything here is offline. A [`FakeBackend`] stands in for the model, so the
//! tests pin what the pipeline *does* — normalizing, truncating, prompting,
//! validating, and writing one transaction — without a network or a cassette.
//!
//! The migration test runs against a database that is otherwise at 0005, the
//! schema a Phase 2.9 install has.

mod common;

use std::collections::VecDeque;
use std::sync::Mutex;

use async_trait::async_trait;
use career_ops_core::acquisition::router::FetchSource;
use career_ops_core::evaluation::{
    run_evaluation, CompletionBackend, EvaluationRequest, Recommendation,
};
use career_ops_core::llm::{DeltaSink, LlmError};
use career_ops_core::model::{
    Application, ApplicationStatus, FetchTier, JobDescription, QueueState,
};
use career_ops_core::prompts_seed::seed_or_update_system_prompts;
use career_ops_core::repo::applications;
use career_ops_core::repo::evaluation_reports::{self, NewEvaluationReport};
use career_ops_core::repo::job_descriptions as job_repo;
use career_ops_core::repo::job_descriptions::NewJobDescription;
use career_ops_core::repo::profile::{self, ProfileDetails};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions};
use sqlx::ConnectOptions;

// ----------------------------------------------------------------------
// Migration 0006
// ----------------------------------------------------------------------

const MIGRATIONS_BEFORE_0006: &[&str] = &[
    include_str!("../migrations/0001_initial.sql"),
    include_str!("../migrations/0002_portal_fetch_status.sql"),
    include_str!("../migrations/0003_job_identity.sql"),
    include_str!("../migrations/0004_portal_cursor.sql"),
    include_str!("../migrations/0005_job_queue.sql"),
];

const MIGRATION_0006: &str = include_str!("../migrations/0006_evaluation_provenance.sql");

async fn pool_at_0005() -> (tempfile::TempDir, SqlitePool) {
    let dir = tempfile::tempdir().expect("tempdir");
    let options = SqliteConnectOptions::new()
        .filename(dir.path().join("evaluation-test.db"))
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

    for sql in MIGRATIONS_BEFORE_0006 {
        sqlx::raw_sql(sql).execute(&pool).await.expect("migrate");
    }

    (dir, pool)
}

#[tokio::test]
#[allow(clippy::type_complexity)]
async fn migration_0006_applies_to_a_0005_database_and_preserves_rows() {
    let (_dir, pool) = pool_at_0005().await;

    // A job, its application, a report, and a cover letter, all written by a
    // build that has never heard of the provenance columns.
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

    sqlx::raw_sql(MIGRATION_0006)
        .execute(&pool)
        .await
        .expect("apply 0006");

    let (content, model, score, recommendation, input, source, truncated): (
        String,
        String,
        Option<f64>,
        Option<String>,
        Option<i64>,
        Option<i64>,
        bool,
    ) = sqlx::query_as(
        "SELECT content, model_used, match_score, recommendation, input_chars,
                source_chars, truncated
         FROM evaluation_reports WHERE id = 1",
    )
    .fetch_one(&pool)
    .await
    .expect("the report still exists");

    assert_eq!(content, "old report", "the migration must not rewrite rows");
    assert_eq!(model, "old-model");
    assert_eq!(score, None, "a pre-0006 report has no score");
    assert_eq!(recommendation, None);
    assert_eq!(input, None);
    assert_eq!(source, None);
    assert!(!truncated, "the default is not truncated");

    let letter_input: Option<i64> =
        sqlx::query_scalar("SELECT input_chars FROM cover_letters WHERE id = 1")
            .fetch_one(&pool)
            .await
            .expect("the letter still exists");
    assert_eq!(letter_input, None);

    let index: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM sqlite_master
         WHERE type = 'index' AND name = 'ux_prompts_name_owner_version'",
    )
    .fetch_one(&pool)
    .await
    .expect("query sqlite_master");
    assert_eq!(index, 1, "the version guard needs its index");

    // Two owners may share a name and version: the sequences are independent.
    sqlx::query(
        "INSERT INTO prompts
         (name, category, content, is_system, version, content_hash, created_at, updated_at)
         VALUES ('evaluate', 'evaluation', 'system', 1, 1, 'h1',
                 '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z'),
                ('evaluate', 'evaluation', 'user', 0, 1, 'h2',
                 '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
    )
    .execute(&pool)
    .await
    .expect("two owners may share a version");

    // The same owner may not.
    let duplicate = sqlx::query(
        "INSERT INTO prompts
         (name, category, content, is_system, version, content_hash, created_at, updated_at)
         VALUES ('evaluate', 'evaluation', 'again', 1, 1, 'h3',
                 '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
    )
    .execute(&pool)
    .await;
    assert!(
        duplicate.is_err(),
        "same owner, name, and version must collide"
    );

    pool.close().await;
}

// ----------------------------------------------------------------------
// Repository writes
// ----------------------------------------------------------------------

#[tokio::test]
async fn finish_evaluation_tx_links_the_report_and_optionally_moves_the_status() {
    let db = common::TestDb::new_raw().await;
    let job = store_job(&db, "Engineer", &long_text()).await;
    let application = applications::ensure(&db.pool, job.id)
        .await
        .expect("ensure");

    let mut tx = db.pool.begin().await.expect("begin");
    let report_id = evaluation_reports::insert_tx(
        &mut tx,
        NewEvaluationReport {
            application_id: application.id,
            content: "report",
            model_used: "deepseek-chat",
            prompt_version: Some(1),
            prompt_hash: Some("hash"),
            match_score: Some(0.82),
            recommendation: Some("match"),
            input_chars: Some(100),
            source_chars: Some(200),
            truncated: false,
        },
    )
    .await
    .expect("insert report");
    applications::finish_evaluation_tx(
        &mut tx,
        application.id,
        report_id,
        0.82,
        Some(ApplicationStatus::Ready),
    )
    .await
    .expect("finish");
    tx.commit().await.expect("commit");

    let after = applications::get(&db.pool, application.id)
        .await
        .expect("get")
        .expect("row");
    assert_eq!(after.status, ApplicationStatus::Ready.as_str());
    assert_eq!(after.evaluation_report_id, Some(report_id));
    assert_eq!(after.match_score, Some(0.82));

    // `None` leaves a submitted application where it is, which is the Q-43
    // rule stated at the writer.
    let applied = store_applied(&db, "Applied role").await;
    let mut tx = db.pool.begin().await.expect("begin");
    applications::finish_evaluation_tx(&mut tx, applied.id, report_id, 0.5, None)
        .await
        .expect("finish without a status");
    tx.commit().await.expect("commit");

    let after = applications::get(&db.pool, applied.id)
        .await
        .expect("get")
        .expect("row");
    assert_eq!(after.status, ApplicationStatus::Applied.as_str());
    assert_eq!(after.evaluation_report_id, Some(report_id));

    // The range is checked before any write.
    let error = applications::finish_evaluation_tx(
        &mut db.pool.begin().await.expect("begin"),
        applied.id,
        report_id,
        1.5,
        None,
    )
    .await
    .expect_err("out of range");
    assert!(error.to_string().contains("0.0, 1.0"), "{error}");

    db.close().await;
}

#[tokio::test]
async fn set_queue_state_tx_writes_inside_the_callers_transaction() {
    let db = common::TestDb::new_raw().await;
    let job = store_job(&db, "Engineer", &long_text()).await;

    let mut tx = db.pool.begin().await.expect("begin");
    job_repo::set_queue_state_tx(
        &mut tx,
        job.id,
        QueueState::Evaluated,
        Some("report written"),
    )
    .await
    .expect("set");
    // Uncommitted: a separate connection still sees the old value.
    let before = job_repo::get(&db.pool, job.id)
        .await
        .expect("get")
        .expect("row");
    assert_eq!(before.queue_state, QueueState::New.as_str());
    tx.commit().await.expect("commit");

    let after = job_repo::get(&db.pool, job.id)
        .await
        .expect("get")
        .expect("row");
    assert_eq!(after.queue_state, QueueState::Evaluated.as_str());
    assert_eq!(after.queue_reason.as_deref(), Some("report written"));

    db.close().await;
}

#[tokio::test]
async fn deleting_a_report_clears_the_applications_latest_pointer() {
    let db = common::TestDb::new_raw().await;
    let job = store_job(&db, "Engineer", &long_text()).await;
    let application = applications::ensure(&db.pool, job.id)
        .await
        .expect("ensure");

    let mut tx = db.pool.begin().await.expect("begin");
    let report_id = evaluation_reports::insert_tx(
        &mut tx,
        NewEvaluationReport {
            application_id: application.id,
            content: "report",
            model_used: "deepseek-chat",
            prompt_version: None,
            prompt_hash: None,
            match_score: None,
            recommendation: None,
            input_chars: None,
            source_chars: None,
            truncated: false,
        },
    )
    .await
    .expect("insert");
    applications::finish_evaluation_tx(&mut tx, application.id, report_id, 0.5, None)
        .await
        .expect("finish");
    tx.commit().await.expect("commit");

    evaluation_reports::delete(&db.pool, report_id)
        .await
        .expect("delete");
    let after = applications::get(&db.pool, application.id)
        .await
        .expect("get")
        .expect("row");
    assert_eq!(after.evaluation_report_id, None, "the link must not dangle");

    db.close().await;
}

#[tokio::test]
async fn a_report_alone_makes_a_job_count_as_user_touched() {
    let db = common::TestDb::new_raw().await;
    let job = store_job(&db, "Engineer", &long_text()).await;
    let application = applications::ensure(&db.pool, job.id)
        .await
        .expect("ensure");
    assert!(
        !job_repo::user_touched_ids(&db.pool)
            .await
            .expect("touched")
            .contains(&job.id),
        "a discovered row with no report is not gated"
    );

    // A report on a row that stayed `discovered`: the status alone would not
    // gate it, the report union must.
    let mut tx = db.pool.begin().await.expect("begin");
    evaluation_reports::insert_tx(
        &mut tx,
        NewEvaluationReport {
            application_id: application.id,
            content: "report",
            model_used: "deepseek-chat",
            prompt_version: None,
            prompt_hash: None,
            match_score: None,
            recommendation: None,
            input_chars: None,
            source_chars: None,
            truncated: false,
        },
    )
    .await
    .expect("insert");
    tx.commit().await.expect("commit");

    assert!(
        job_repo::user_touched_ids(&db.pool)
            .await
            .expect("touched")
            .contains(&job.id),
        "deleting a job with a report needs the typed gate"
    );

    db.close().await;
}

// ----------------------------------------------------------------------
// The pipeline, with a fake model
// ----------------------------------------------------------------------

#[tokio::test]
async fn an_evaluation_stores_provenance_and_moves_discovered_to_ready() {
    let db = common::TestDb::new_raw().await;
    seed_profile_and_prompts(&db).await;
    let job = store_job(&db, "Rust Engineer", &long_text()).await;

    let backend = FakeBackend::with_json(json_reply(0.9, "strong_match", "A strong fit."));
    let result = run_evaluation(
        &db.pool,
        &EvaluationRequest::new(job.id),
        &backend,
        None,
        None,
    )
    .await
    .expect("evaluate");

    assert_eq!(result.match_score, 0.9);
    assert_eq!(result.recommendation, Recommendation::StrongMatch);
    assert_eq!(result.model_used, "deepseek-chat");
    assert_eq!(result.prompt_version, 1, "the seeded prompt is version 1");
    assert!(!result.prompt_hash.is_empty());
    assert!(result.input_chars > 0, "the prompt carried the description");
    assert_eq!(
        result.source_chars,
        long_text().chars().count(),
        "the raw size is recorded beside the normalized one"
    );
    assert!(!result.truncated);

    let stored = evaluation_reports::get(&db.pool, result.report_id)
        .await
        .expect("get")
        .expect("report");
    assert!(
        (stored.match_score.expect("score") - 0.9).abs() < 1e-6,
        "f32 to f64 conversion keeps the score"
    );
    assert_eq!(stored.recommendation.as_deref(), Some("strong_match"));
    assert_eq!(stored.input_chars, Some(result.input_chars as i64));
    assert_eq!(stored.source_chars, Some(result.source_chars as i64));
    assert!(!stored.truncated);
    assert_eq!(stored.prompt_version, Some(1));
    assert_eq!(
        stored.prompt_hash.as_deref(),
        Some(result.prompt_hash.as_str())
    );

    let application = applications::get(&db.pool, result.application_id)
        .await
        .expect("get")
        .expect("row");
    assert_eq!(application.status, ApplicationStatus::Ready.as_str());
    assert_eq!(application.evaluation_report_id, Some(result.report_id));
    assert!(
        (application.match_score.expect("score") - 0.9).abs() < 1e-6,
        "the application carries the score"
    );

    let stored_job = job_repo::get(&db.pool, job.id)
        .await
        .expect("get")
        .expect("row");
    assert_eq!(stored_job.queue_state, QueueState::Evaluated.as_str());

    // The prompt sent to the model is the rendered one, with no placeholders
    // and no markup from the stored row.
    let prompts = backend.prompts();
    assert_eq!(prompts.len(), 1, "a usable reply needs no repair call");
    assert!(!prompts[0].contains("{{"), "every variable was substituted");
    assert!(!prompts[0].contains("<p>"), "normalization ran first");
    assert!(
        prompts[0].contains("Ada Lovelace"),
        "the CV summary is present"
    );

    db.close().await;
}

#[tokio::test]
async fn a_backend_failure_restores_the_previous_status_and_stores_nothing() {
    let db = common::TestDb::new_raw().await;
    seed_profile_and_prompts(&db).await;
    let job = store_job(&db, "Rust Engineer", &long_text()).await;

    let backend = FakeBackend::failing();
    let error = run_evaluation(
        &db.pool,
        &EvaluationRequest::new(job.id),
        &backend,
        None,
        None,
    )
    .await
    .expect_err("the call failed");

    assert!(
        error.to_string().contains("fake transport failure"),
        "{error}"
    );

    let application = applications::ensure(&db.pool, job.id)
        .await
        .expect("ensure");
    assert_eq!(
        application.status,
        ApplicationStatus::Discovered.as_str(),
        "a failed call must not leave the row in `evaluating`"
    );
    assert_eq!(application.evaluation_report_id, None);

    let reports = evaluation_reports::list_for_application(&db.pool, application.id)
        .await
        .expect("list");
    assert!(
        reports.is_empty(),
        "a failure never stores a partial report"
    );

    let stored_job = job_repo::get(&db.pool, job.id)
        .await
        .expect("get")
        .expect("row");
    assert_eq!(stored_job.queue_state, QueueState::New.as_str());

    db.close().await;
}

#[tokio::test]
async fn an_unusable_reply_is_repaired_once() {
    let db = common::TestDb::new_raw().await;
    seed_profile_and_prompts(&db).await;
    let job = store_job(&db, "Rust Engineer", &long_text()).await;

    let backend = FakeBackend::sequence([
        "I cannot answer in that format.".to_string(),
        json_reply(0.75, "weak_match", "Partial fit."),
    ]);
    let result = run_evaluation(
        &db.pool,
        &EvaluationRequest::new(job.id),
        &backend,
        None,
        None,
    )
    .await
    .expect("the repair succeeded");

    assert_eq!(result.recommendation, Recommendation::WeakMatch);
    let prompts = backend.prompts();
    assert_eq!(prompts.len(), 2, "one repair attempt");
    assert!(
        prompts[1].contains("could not be used"),
        "the repair restates the contract"
    );

    db.close().await;
}

#[tokio::test]
async fn a_reply_that_stays_unusable_is_an_error_with_no_report() {
    let db = common::TestDb::new_raw().await;
    seed_profile_and_prompts(&db).await;
    let job = store_job(&db, "Rust Engineer", &long_text()).await;

    let backend = FakeBackend::sequence(["not json".to_string(), "still not json".to_string()]);
    let error = run_evaluation(
        &db.pool,
        &EvaluationRequest::new(job.id),
        &backend,
        None,
        None,
    )
    .await
    .expect_err("two unusable replies");

    assert!(error.to_string().contains("not usable"), "{error}");
    let application = applications::ensure(&db.pool, job.id)
        .await
        .expect("ensure");
    assert_eq!(application.status, ApplicationStatus::Discovered.as_str());
    let reports = evaluation_reports::list_for_application(&db.pool, application.id)
        .await
        .expect("list");
    assert!(reports.is_empty());

    db.close().await;
}

#[tokio::test]
async fn an_out_of_range_score_is_rejected_before_anything_is_written() {
    let db = common::TestDb::new_raw().await;
    seed_profile_and_prompts(&db).await;
    let job = store_job(&db, "Rust Engineer", &long_text()).await;

    let backend = FakeBackend::sequence([
        json_reply(1.4, "strong_match", "Impossible."),
        json_reply(1.4, "strong_match", "Impossible."),
    ]);
    let error = run_evaluation(
        &db.pool,
        &EvaluationRequest::new(job.id),
        &backend,
        None,
        None,
    )
    .await
    .expect_err("out of range");

    assert!(error.to_string().contains("not usable"), "{error}");
    let application = applications::ensure(&db.pool, job.id)
        .await
        .expect("ensure");
    assert_eq!(application.status, ApplicationStatus::Discovered.as_str());

    db.close().await;
}

#[tokio::test]
async fn a_no_match_report_marks_a_ready_application_not_a_match() {
    let db = common::TestDb::new_raw().await;
    seed_profile_and_prompts(&db).await;
    let job = store_job(&db, "Rust Engineer", &long_text()).await;

    let backend = FakeBackend::with_json(json_reply(0.2, "no_match", "Wrong stack."));
    let result = run_evaluation(
        &db.pool,
        &EvaluationRequest::new(job.id),
        &backend,
        None,
        None,
    )
    .await
    .expect("evaluate");

    let application = applications::get(&db.pool, result.application_id)
        .await
        .expect("get")
        .expect("row");
    assert_eq!(application.status, ApplicationStatus::NotAMatch.as_str());
    assert_eq!(application.evaluation_report_id, Some(result.report_id));
    assert!(
        (application.match_score.expect("score") - 0.2).abs() < 1e-6,
        "the score is stored even for a rejected match"
    );

    db.close().await;
}

#[tokio::test]
async fn a_no_match_report_on_a_submitted_application_leaves_its_status_alone() {
    let db = common::TestDb::new_raw().await;
    seed_profile_and_prompts(&db).await;
    let application = store_applied(&db, "Applied role").await;

    let backend = FakeBackend::with_json(json_reply(0.1, "no_match", "Wrong stack."));
    let result = run_evaluation(
        &db.pool,
        &EvaluationRequest::new(application.job_description_id),
        &backend,
        None,
        None,
    )
    .await
    .expect("the report is stored even when the status change is refused");

    let after = applications::get(&db.pool, application.id)
        .await
        .expect("get")
        .expect("row");
    assert_eq!(
        after.status,
        ApplicationStatus::Applied.as_str(),
        "a submitted application is never moved backwards"
    );
    assert_eq!(after.evaluation_report_id, Some(result.report_id));

    db.close().await;
}

#[tokio::test]
async fn a_match_report_on_a_submitted_application_leaves_its_status_alone() {
    let db = common::TestDb::new_raw().await;
    seed_profile_and_prompts(&db).await;
    let application = store_applied(&db, "Applied role").await;

    let backend = FakeBackend::with_json(json_reply(0.88, "match", "Good fit."));
    let result = run_evaluation(
        &db.pool,
        &EvaluationRequest::new(application.job_description_id),
        &backend,
        None,
        None,
    )
    .await
    .expect("evaluate");

    let after = applications::get(&db.pool, application.id)
        .await
        .expect("get")
        .expect("row");
    assert_eq!(after.status, ApplicationStatus::Applied.as_str());
    assert_eq!(after.evaluation_report_id, Some(result.report_id));

    db.close().await;
}

#[tokio::test]
async fn a_missing_cv_summary_is_an_actionable_error() {
    let db = common::TestDb::new_raw().await;
    seed_or_update_system_prompts(&db.pool)
        .await
        .expect("seed prompts");
    let job = store_job(&db, "Rust Engineer", &long_text()).await;

    let backend = FakeBackend::with_json(json_reply(0.8, "match", "Fine."));
    let error = run_evaluation(
        &db.pool,
        &EvaluationRequest::new(job.id),
        &backend,
        None,
        None,
    )
    .await
    .expect_err("no CV");

    assert!(
        error.to_string().contains("Add your CV"),
        "the message must say what to do: {error}"
    );
    assert!(backend.prompts().is_empty(), "no call without a prompt");

    db.close().await;
}

#[tokio::test]
async fn a_model_that_cannot_return_structured_output_is_refused_before_the_call() {
    let db = common::TestDb::new_raw().await;
    seed_profile_and_prompts(&db).await;
    let job = store_job(&db, "Rust Engineer", &long_text()).await;

    let backend = FakeBackend::with_json(json_reply(0.8, "match", "Fine."));
    let mut request = EvaluationRequest::new(job.id);
    request.model_override = Some("deepseek-reasoner".into());
    let error = run_evaluation(&db.pool, &request, &backend, None, None)
        .await
        .expect_err("the reasoner has no structured output");

    assert!(
        error
            .to_string()
            .contains("cannot return structured output"),
        "{error}"
    );
    assert!(backend.prompts().is_empty());

    db.close().await;
}

#[tokio::test]
async fn a_thin_row_is_fetched_first_when_the_request_asks() {
    let db = common::TestDb::new_raw().await;
    seed_profile_and_prompts(&db).await;
    let job = store_job(&db, "Engineer", "teaser").await;

    let source = common::MockFetchSource::default().pages([Ok(detail_page())]);
    let source_ref: &dyn FetchSource = &source;
    let backend = FakeBackend::with_json(json_reply(0.8, "match", "Fine."));
    let mut request = EvaluationRequest::new(job.id);
    request.fetch_description_if_thin = true;

    let result = run_evaluation(&db.pool, &request, &backend, Some(source_ref), None)
        .await
        .expect("evaluate");

    assert_eq!(source.page_calls().len(), 1, "one fetch for the thin row");
    assert!(
        result.source_chars > 400,
        "the fetched description replaced the teaser: {}",
        result.source_chars
    );
    let stored = job_repo::get(&db.pool, job.id)
        .await
        .expect("get")
        .expect("row");
    assert!(
        stored.raw_text.contains("full description"),
        "the fetch is stored"
    );

    db.close().await;
}

#[tokio::test]
async fn a_thin_row_is_not_fetched_when_the_request_does_not_ask() {
    let db = common::TestDb::new_raw().await;
    seed_profile_and_prompts(&db).await;
    let job = store_job(&db, "Engineer", "teaser").await;

    let source = common::MockFetchSource::default().pages([Ok(detail_page())]);
    let source_ref: &dyn FetchSource = &source;
    let backend = FakeBackend::with_json(json_reply(0.8, "match", "Fine."));
    let result = run_evaluation(
        &db.pool,
        &EvaluationRequest::new(job.id),
        &backend,
        Some(source_ref),
        None,
    )
    .await
    .expect("evaluate");

    assert!(source.page_calls().is_empty(), "the fetch is opt-in");
    assert_eq!(result.source_chars, "teaser".chars().count());

    db.close().await;
}

#[tokio::test]
async fn a_prompt_over_the_context_window_is_refused_before_the_call() {
    let db = common::TestDb::new_raw().await;
    seed_profile_and_prompts(&db).await;
    sqlx::query("INSERT INTO settings (key, value) VALUES ('llm.max_job_chars', '400000')")
        .execute(&db.pool)
        .await
        .expect("insert setting");
    let huge = format!("<p>{}</p>", "x".repeat(300_000));
    let job = store_job(&db, "Engineer", &huge).await;

    let backend = FakeBackend::with_json(json_reply(0.8, "match", "Fine."));
    let error = run_evaluation(
        &db.pool,
        &EvaluationRequest::new(job.id),
        &backend,
        None,
        None,
    )
    .await
    .expect_err("300k characters do not fit 64k tokens");

    assert!(error.to_string().contains("context window"), "{error}");
    assert!(backend.prompts().is_empty(), "no call was made");
    let application = applications::ensure(&db.pool, job.id)
        .await
        .expect("ensure");
    assert_eq!(application.status, ApplicationStatus::Discovered.as_str());

    db.close().await;
}

#[tokio::test]
async fn a_truncated_input_is_recorded_on_the_report() {
    let db = common::TestDb::new_raw().await;
    seed_profile_and_prompts(&db).await;
    // A budget that cannot hold the text, set through the same settings key the
    // UI writes.
    sqlx::query("INSERT INTO settings (key, value) VALUES ('llm.max_job_chars', '1000')")
        .execute(&db.pool)
        .await
        .expect("insert setting");
    let job = store_job(&db, "Engineer", &long_text()).await;

    let backend = FakeBackend::with_json(json_reply(0.8, "match", "Fine."));
    let result = run_evaluation(
        &db.pool,
        &EvaluationRequest::new(job.id),
        &backend,
        None,
        None,
    )
    .await
    .expect("evaluate");

    assert!(result.truncated);
    assert_eq!(result.input_chars, 1_000);
    let stored = evaluation_reports::get(&db.pool, result.report_id)
        .await
        .expect("get")
        .expect("report");
    assert!(stored.truncated, "the flag is a column, not inferred");

    db.close().await;
}

// ----------------------------------------------------------------------
// Helpers
// ----------------------------------------------------------------------

/// A reply queue plus a prompt log, standing in for a provider.
struct FakeBackend {
    replies: Mutex<VecDeque<Reply>>,
    prompts: Mutex<Vec<String>>,
}

enum Reply {
    Text(String),
    Fail,
}

impl FakeBackend {
    fn with_json(json: String) -> Self {
        Self::sequence([json])
    }

    fn sequence(replies: impl IntoIterator<Item = String>) -> Self {
        Self {
            replies: Mutex::new(replies.into_iter().map(Reply::Text).collect()),
            prompts: Mutex::new(Vec::new()),
        }
    }

    fn failing() -> Self {
        Self {
            replies: Mutex::new(VecDeque::from([Reply::Fail])),
            prompts: Mutex::new(Vec::new()),
        }
    }

    fn prompts(&self) -> Vec<String> {
        self.prompts.lock().unwrap().clone()
    }
}

#[async_trait]
impl CompletionBackend for FakeBackend {
    async fn complete(
        &self,
        prompt: &str,
        _model: Option<&str>,
        _json_mode: bool,
        deltas: Option<&DeltaSink>,
    ) -> Result<String, LlmError> {
        self.prompts.lock().unwrap().push(prompt.to_string());
        if let Some(sink) = deltas {
            let _ = sink.send("fragment".into());
        }
        match self.replies.lock().unwrap().pop_front() {
            Some(Reply::Text(text)) => Ok(text),
            Some(Reply::Fail) => Err(LlmError::Transport("fake transport failure".into())),
            None => Err(LlmError::Transport("the fake ran out of replies".into())),
        }
    }
}

fn json_reply(score: f32, recommendation: &str, rationale: &str) -> String {
    format!(
        r###"{{"match_score": {score},
            "recommendation": "{recommendation}",
            "strengths": ["a", "b", "c"],
            "gaps": ["x"],
            "rationale": "{rationale}",
            "full_report_markdown": "## Report\n{n}"}}"###,
        n = "Body of the report.".repeat(4)
    )
}

fn long_text() -> String {
    let mut text = String::from("<p>We are hiring a Rust engineer.</p>");
    while text.chars().count() < 2_400 {
        text.push_str("<p>You will build offline-first desktop software with SQLite.</p>");
    }
    text
}

/// A JSON-LD posting page the generic detail extractor accepts.
fn detail_page() -> String {
    format!(
        r#"<html><head><script type="application/ld+json">
        {{"@type":"JobPosting","title":"Engineer","description":"<p>{}</p>"}}
        </script></head><body></body></html>"#,
        "This is the full description of the role. ".repeat(20)
    )
}

async fn seed_profile_and_prompts(db: &common::TestDb) {
    profile::update_details(
        &db.pool,
        &ProfileDetails {
            name: "Ada Lovelace".into(),
            email: "ada@example.com".into(),
            phone: None,
            location: Some("London".into()),
            summary: Some("Mathematician and first programmer.".into()),
        },
    )
    .await
    .expect("profile");
    seed_or_update_system_prompts(&db.pool)
        .await
        .expect("seed prompts");
}

async fn store_job(db: &common::TestDb, title: &str, raw_text: &str) -> JobDescription {
    job_repo::upsert(
        &db.pool,
        NewJobDescription {
            title: title.into(),
            company: "Acme".into(),
            source_url: "https://example.com/jobs/1".into(),
            raw_text: raw_text.into(),
            fetch_tier: FetchTier::Tier1bHtml,
            external_id: None,
        },
    )
    .await
    .expect("store job")
}

/// A job whose application is already submitted, so the status writer must
/// refuse to move it.
async fn store_applied(db: &common::TestDb, title: &str) -> Application {
    let job = store_job(db, title, &long_text()).await;
    let application = applications::ensure(&db.pool, job.id)
        .await
        .expect("ensure");
    applications::transition(&db.pool, application.id, ApplicationStatus::Ready)
        .await
        .expect("ready");
    applications::mark_applied(&db.pool, application.id)
        .await
        .expect("applied")
}
