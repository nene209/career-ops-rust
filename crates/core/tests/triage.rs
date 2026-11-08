//! The triage batch service (Phase 3.2, WS2).
//!
//! Offline: a scripted backend returns one canned reply, and the real service,
//! repositories, queue write, and usage ledger do the work.

mod common;

use std::collections::VecDeque;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use career_ops_core::llm::{Completion, CompletionBackend, DeltaSink, LlmError, Usage};
use career_ops_core::model::{FetchTier, JobDescription, QueueState};
use career_ops_core::repo::job_descriptions::{self as job_repo, NewJobDescription};
use career_ops_core::repo::llm_usage;
use career_ops_core::triage::{
    estimate, run_triage, TriageChoice, TriageError, TriageRequest, DEFAULT_BATCH_LIMIT,
    ESTIMATED_OUTPUT_TOKENS_PER_JOB, MAX_BATCH_LIMIT,
};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions};
use sqlx::ConnectOptions;

use common::phase3;

type BackendResult = std::result::Result<Completion, LlmError>;

struct ScriptedBackend {
    replies: Mutex<VecDeque<BackendResult>>,
    prompts: Mutex<Vec<String>>,
}

impl ScriptedBackend {
    fn with_reply(reply: impl Into<String>) -> Self {
        Self::new([Ok(Completion::text_only(reply))])
    }

    fn with_completion(completion: Completion) -> Self {
        Self::new([Ok(completion)])
    }

    fn failing(error: LlmError) -> Self {
        Self::new([Err(error)])
    }

    fn new(replies: impl IntoIterator<Item = BackendResult>) -> Self {
        Self {
            replies: Mutex::new(replies.into_iter().collect()),
            prompts: Mutex::new(Vec::new()),
        }
    }

    fn prompts(&self) -> Vec<String> {
        self.prompts.lock().unwrap().clone()
    }
}

#[async_trait]
impl CompletionBackend for ScriptedBackend {
    async fn complete(
        &self,
        prompt: &str,
        _model: Option<&str>,
        json_mode: bool,
        _deltas: Option<&DeltaSink>,
    ) -> BackendResult {
        assert!(json_mode, "triage asks for JSON mode");
        self.prompts.lock().unwrap().push(prompt.to_string());
        self.replies
            .lock()
            .unwrap()
            .pop_front()
            .expect("the script ran out of replies")
    }
}

/// A reply with `(choice, job_id, reason)` per decision.
fn reply(decisions: &[(&str, i64, &str)]) -> String {
    let entries: Vec<String> = decisions
        .iter()
        .map(|(choice, id, reason)| {
            format!(r#"{{"job_id": {id}, "decision": "{choice}", "reason": "{reason}"}}"#)
        })
        .collect();
    format!(r#"{{"decisions": [{}]}}"#, entries.join(", "))
}

async fn seeded_jobs(count: usize) -> (common::TestDb, Vec<JobDescription>) {
    let db = common::TestDb::new_raw().await;
    phase3::seed(&db).await;

    let mut jobs = Vec::with_capacity(count);
    for index in 0..count {
        let job = job_repo::upsert(
            &db.pool,
            NewJobDescription {
                title: format!("Engineer {index}"),
                company: "Acme".into(),
                source_url: format!("https://example.com/jobs/{index}"),
                raw_text: format!(
                    "<p>We build offline-first desktop software in Rust. Role {index}.</p>"
                ),
                fetch_tier: FetchTier::Tier1bHtml,
                external_id: None,
            },
        )
        .await
        .expect("store job");
        jobs.push(job);
    }

    (db, jobs)
}

fn ids(jobs: &[JobDescription]) -> Vec<i64> {
    jobs.iter().map(|job| job.id).collect()
}

#[tokio::test]
async fn a_batch_applies_every_decision_and_records_provenance() {
    let (db, jobs) = seeded_jobs(4).await;
    let cases = [
        (
            "evaluate",
            jobs[0].id,
            "Rust and desktop work match the summary.",
            QueueState::Evaluate,
        ),
        (
            "worth_details",
            jobs[1].id,
            "Plausible; the full description would clarify.",
            QueueState::DetailsNeeded,
        ),
        (
            "skip",
            jobs[2].id,
            "This is not an engineering posting.",
            QueueState::Dismissed,
        ),
        (
            "unsure",
            jobs[3].id,
            "The brief is too thin to judge.",
            QueueState::Evaluate,
        ),
    ];
    let reply = reply(&cases.map(|(choice, id, reason, _)| (choice, id, reason)));
    let backend = ScriptedBackend::with_reply(reply);

    let report = run_triage(&db.pool, &TriageRequest::new(ids(&jobs)), &backend)
        .await
        .expect("triage");

    assert_eq!(report.decisions.len(), 4);
    assert_eq!(report.model_used, "deepseek-flash");
    assert_eq!(report.prompt_version, 1);
    assert!(!report.prompt_hash.is_empty());
    assert!(report.input_chars > 0);
    assert!(
        report.estimated_cost_usd.is_some_and(|cost| cost > 0.0),
        "the pre-call estimate is priced"
    );
    assert_eq!(
        report.actual_cost_usd, None,
        "the fake reported no cost of its own"
    );

    for (choice, id, reason, state) in cases {
        let row = job_repo::get(&db.pool, id)
            .await
            .expect("get")
            .expect("row");
        assert_eq!(row.queue_state, state.as_str());
        assert_eq!(row.queue_reason.as_deref(), Some(reason));
        assert_eq!(row.triage_decision.as_deref(), Some(choice));
        assert_eq!(row.triage_reason.as_deref(), Some(reason));
        assert_eq!(row.triage_model.as_deref(), Some("deepseek-flash"));
        assert_eq!(
            row.triage_prompt_hash.as_deref(),
            Some(report.prompt_hash.as_str())
        );
        assert!(row.triaged_at.is_some());
        assert!(row.queued_at.is_some());
    }

    let usage = llm_usage::recent(&db.pool, None, 10).await.expect("recent");
    assert_eq!(usage.len(), 1, "one call, one ledger row");
    assert_eq!(usage[0].task, "triage");
    assert_eq!(usage[0].outcome, "ok");
    assert_eq!(
        usage[0].job_title, None,
        "a triage call covers a batch, not one posting"
    );

    // The batch was one call, and the prompt carried the postings.
    let prompts = backend.prompts();
    assert_eq!(prompts.len(), 1);
    assert!(prompts[0].contains(&format!("{} | Engineer 0 | Acme", jobs[0].id)));
    assert!(prompts[0].contains("Rust"));

    db.close().await;
}

#[tokio::test]
async fn the_recorded_cassette_covers_every_decision() {
    let db = common::TestDb::new_raw().await;
    phase3::seed(&db).await;
    let jobs = phase3::store_triage_samples(&db).await;
    let backend = phase3::TriageCassetteBackend::new();

    let report = run_triage(&db.pool, &TriageRequest::new(ids(&jobs)), &backend)
        .await
        .expect("triage");

    // The cassette is the output contract the live test records: one decision
    // per posting, every choice represented.
    assert_eq!(report.decisions.len(), jobs.len());
    let mut choices: Vec<&str> = report
        .decisions
        .iter()
        .map(|decision| decision.decision.as_str())
        .collect();
    choices.sort_unstable();
    assert_eq!(choices, vec!["evaluate", "skip", "unsure", "worth_details"]);

    for decision in &report.decisions {
        assert!(!decision.reason.trim().is_empty());
        let row = job_repo::get(&db.pool, decision.job_id)
            .await
            .expect("get")
            .expect("row");
        assert_eq!(
            row.triage_decision.as_deref(),
            Some(decision.decision.as_str())
        );
        assert_eq!(row.triage_reason.as_deref(), Some(decision.reason.as_str()));
        assert_eq!(row.queue_state, decision.decision.queue_state().as_str());
    }

    // The prompt saw the four briefs, not markup or file paths.
    let prompts = backend.prompts();
    assert_eq!(prompts.len(), 1);
    for sample in phase3::TRIAGE_SAMPLES {
        assert!(prompts[0].contains(sample.title), "{}", sample.title);
    }

    db.close().await;
}

#[tokio::test]
async fn a_batch_over_the_limit_is_refused_before_the_call() {
    let (db, jobs) = seeded_jobs(DEFAULT_BATCH_LIMIT + 1).await;
    let backend = ScriptedBackend::with_reply("{}");

    let error = run_triage(&db.pool, &TriageRequest::new(ids(&jobs)), &backend)
        .await
        .expect_err("too large");

    match error {
        TriageError::BatchTooLarge { requested, limit } => {
            assert_eq!(requested, DEFAULT_BATCH_LIMIT + 1);
            assert_eq!(limit, DEFAULT_BATCH_LIMIT);
        }
        other => panic!("expected BatchTooLarge, got {other:?}"),
    }
    assert!(backend.prompts().is_empty(), "no request was sent");

    let untouched = job_repo::get(&db.pool, jobs[0].id)
        .await
        .expect("get")
        .expect("row");
    assert_eq!(untouched.queue_state, QueueState::New.as_str());

    db.close().await;
}

#[tokio::test]
async fn the_ceiling_cannot_be_raised_past_its_maximum() {
    let db = common::TestDb::new_raw().await;
    let backend = ScriptedBackend::with_reply("{}");
    let mut request = TriageRequest::new((0..=MAX_BATCH_LIMIT as i64).collect());
    request.limit = 10_000;

    let error = run_triage(&db.pool, &request, &backend)
        .await
        .expect_err("over the ceiling");

    assert!(matches!(
        error,
        TriageError::BatchTooLarge {
            requested,
            limit
        } if requested == MAX_BATCH_LIMIT + 1 && limit == MAX_BATCH_LIMIT
    ));
    assert!(backend.prompts().is_empty());

    db.close().await;
}

#[tokio::test]
async fn a_missing_decision_applies_nothing() {
    let (db, jobs) = seeded_jobs(3).await;
    let partial = reply(&[("evaluate", jobs[0].id, "Fits.")]);
    let backend = ScriptedBackend::with_reply(partial);

    let error = run_triage(&db.pool, &TriageRequest::new(ids(&jobs)), &backend)
        .await
        .expect_err("a decision is missing");
    assert!(
        matches!(error, TriageError::InvalidResponse(ref reason) if reason.contains("no decision")),
        "{error:?}"
    );

    for job in &jobs {
        let row = job_repo::get(&db.pool, job.id)
            .await
            .expect("get")
            .expect("row");
        assert_eq!(row.queue_state, QueueState::New.as_str());
        assert_eq!(row.triage_decision, None);
    }

    // The call happened, so its tokens are recorded even though nothing applied.
    let usage = llm_usage::recent(&db.pool, None, 10).await.expect("recent");
    assert_eq!(usage.len(), 1);
    assert_eq!(usage[0].task, "triage");

    db.close().await;
}

#[tokio::test]
async fn an_unknown_choice_applies_nothing() {
    let (db, jobs) = seeded_jobs(2).await;
    let backend = ScriptedBackend::with_reply(reply(&[
        ("evaluate", jobs[0].id, "Fits."),
        ("maybe", jobs[1].id, "Not sure."),
    ]));

    let error = run_triage(&db.pool, &TriageRequest::new(ids(&jobs)), &backend)
        .await
        .expect_err("unknown choice");
    assert!(
        matches!(error, TriageError::InvalidResponse(ref reason) if reason.contains("invalid JSON")),
        "{error:?}"
    );

    for job in &jobs {
        let row = job_repo::get(&db.pool, job.id)
            .await
            .expect("get")
            .expect("row");
        assert_eq!(row.queue_state, QueueState::New.as_str());
    }

    db.close().await;
}

#[tokio::test]
async fn a_failed_call_is_recorded_and_changes_nothing() {
    let (db, jobs) = seeded_jobs(2).await;
    let backend = ScriptedBackend::failing(LlmError::Transport("offline".into()));

    let error = run_triage(&db.pool, &TriageRequest::new(ids(&jobs)), &backend)
        .await
        .expect_err("the provider failed");
    assert!(matches!(error, TriageError::Llm(_)), "{error:?}");

    let usage = llm_usage::recent(&db.pool, None, 10).await.expect("recent");
    assert_eq!(usage.len(), 1);
    assert_eq!(usage[0].task, "triage");
    assert_eq!(usage[0].outcome, "error");
    assert_eq!(usage[0].error_kind.as_deref(), Some("transport"));

    for job in &jobs {
        let row = job_repo::get(&db.pool, job.id)
            .await
            .expect("get")
            .expect("row");
        assert_eq!(row.queue_state, QueueState::New.as_str());
    }

    db.close().await;
}

#[tokio::test]
async fn a_cancelled_call_is_recorded_and_changes_nothing() {
    let (db, jobs) = seeded_jobs(2).await;
    let backend = ScriptedBackend::new([]);
    let mut request = TriageRequest::new(ids(&jobs));
    request.cancel = career_ops_core::llm::CancelToken::new(Arc::new(AtomicBool::new(true)));

    let error = run_triage(&db.pool, &request, &backend)
        .await
        .expect_err("cancelled before the call");
    assert!(matches!(error, TriageError::Llm(LlmError::Cancelled)));

    assert!(backend.prompts().is_empty(), "no request was sent");
    let usage = llm_usage::recent(&db.pool, None, 10).await.expect("recent");
    assert_eq!(usage.len(), 1);
    assert_eq!(usage[0].outcome, "cancelled");

    for job in &jobs {
        let row = job_repo::get(&db.pool, job.id)
            .await
            .expect("get")
            .expect("row");
        assert_eq!(row.queue_state, QueueState::New.as_str());
    }

    db.close().await;
}

#[tokio::test]
async fn the_estimate_prices_the_prompt_without_a_call() {
    let (db, jobs) = seeded_jobs(2).await;
    let backend = ScriptedBackend::with_reply("{}");

    let estimate = estimate(&db.pool, &TriageRequest::new(ids(&jobs)))
        .await
        .expect("estimate");

    assert_eq!(estimate.jobs, 2);
    assert_eq!(estimate.model, "deepseek-flash");
    assert!(estimate.input_chars > 0);
    assert_eq!(estimate.input_tokens, estimate.input_chars.div_ceil(4));
    assert_eq!(
        estimate.estimated_output_tokens,
        2 * ESTIMATED_OUTPUT_TOKENS_PER_JOB
    );
    assert!(
        estimate.estimated_cost_usd.is_some_and(|cost| cost > 0.0),
        "Flash is in the catalog"
    );
    assert!(backend.prompts().is_empty(), "the estimate sends nothing");

    db.close().await;
}

#[tokio::test]
async fn a_reported_cost_reaches_the_report_and_the_ledger() {
    let (db, jobs) = seeded_jobs(1).await;
    let usage = Usage {
        input_tokens: Some(1_000),
        output_tokens: Some(60),
        total_tokens: Some(1_060),
        cached_input_tokens: None,
        reasoning_tokens: None,
        cost_usd: Some(0.5),
        cost_complete: Some(true),
        reported: true,
    };
    let backend = ScriptedBackend::with_completion(Completion {
        text: reply(&[("evaluate", jobs[0].id, "Fits.")]),
        usage,
    });

    let report = run_triage(&db.pool, &TriageRequest::new(ids(&jobs)), &backend)
        .await
        .expect("triage");
    assert_eq!(report.actual_cost_usd, Some(0.5));

    let rows = llm_usage::recent(&db.pool, None, 10).await.expect("recent");
    assert_eq!(rows[0].source, "reported");
    assert_eq!(rows[0].cost_usd, Some(0.5));
    assert_eq!(rows[0].input_tokens, Some(1_000));

    db.close().await;
}

#[tokio::test]
async fn a_description_refresh_keeps_the_triage_provenance() {
    let (db, jobs) = seeded_jobs(1).await;
    let backend = ScriptedBackend::with_reply(reply(&[(
        "worth_details",
        jobs[0].id,
        "The brief needs the full description.",
    )]));
    run_triage(&db.pool, &TriageRequest::new(ids(&jobs)), &backend)
        .await
        .expect("triage");

    job_repo::set_raw_text(&db.pool, jobs[0].id, "A much longer description.")
        .await
        .expect("refresh the description");

    let row = job_repo::get(&db.pool, jobs[0].id)
        .await
        .expect("get")
        .expect("row");
    assert_eq!(row.raw_text, "A much longer description.");
    assert_eq!(row.queue_state, QueueState::DetailsNeeded.as_str());
    assert_eq!(row.triage_decision.as_deref(), Some("worth_details"));
    assert!(row.triaged_at.is_some(), "the decision has not been erased");

    db.close().await;
}

// ----------------------------------------------------------------------
// Migration 0008
// ----------------------------------------------------------------------

const MIGRATIONS_BEFORE_0008: [&str; 7] = [
    include_str!("../migrations/0001_initial.sql"),
    include_str!("../migrations/0002_portal_fetch_status.sql"),
    include_str!("../migrations/0003_job_identity.sql"),
    include_str!("../migrations/0004_portal_cursor.sql"),
    include_str!("../migrations/0005_job_queue.sql"),
    include_str!("../migrations/0006_evaluation_provenance.sql"),
    include_str!("../migrations/0007_llm_usage.sql"),
];

const MIGRATION_0008: &str = include_str!("../migrations/0008_triage_provenance.sql");

async fn pool_at_0007() -> (tempfile::TempDir, SqlitePool) {
    let dir = tempfile::tempdir().expect("tempdir");
    let options = SqliteConnectOptions::new()
        .filename(dir.path().join("triage-test.db"))
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

    for sql in MIGRATIONS_BEFORE_0008 {
        sqlx::raw_sql(sql).execute(&pool).await.expect("migrate");
    }

    (dir, pool)
}

#[tokio::test]
async fn migration_0008_applies_to_a_0007_database_and_preserves_rows() {
    let (_dir, pool) = pool_at_0007().await;

    sqlx::query(
        "INSERT INTO job_descriptions
         (id, title, company, source_url, raw_text, fetched_at, fetch_tier,
          queue_state, queue_reason)
         VALUES (1, 'Engineer', 'Acme', 'https://example.com/jobs/1', 'body',
                 '2026-01-01T00:00:00Z', 'tier1b:html', 'evaluate', 'the user chose this')",
    )
    .execute(&pool)
    .await
    .expect("insert job");

    sqlx::raw_sql(MIGRATION_0008)
        .execute(&pool)
        .await
        .expect("apply 0008");

    let (title, state, reason, decision, triaged): (
        String,
        String,
        Option<String>,
        Option<String>,
        Option<String>,
    ) = sqlx::query_as(
        "SELECT title, queue_state, queue_reason, triage_decision, triaged_at
         FROM job_descriptions WHERE id = 1",
    )
    .fetch_one(&pool)
    .await
    .expect("the job still exists");

    assert_eq!(title, "Engineer", "the migration must not rewrite rows");
    assert_eq!(state, "evaluate", "the user's queue choice survives");
    assert_eq!(reason.as_deref(), Some("the user chose this"));
    assert_eq!(decision, None, "a pre-0008 row has never been triaged");
    assert_eq!(triaged, None);

    // The new columns accept a write through the queue helper.
    let mut tx = pool.begin().await.expect("transaction");
    let writes = [job_repo::TriageWrite {
        job_id: 1,
        queue_state: QueueState::DetailsNeeded,
        decision: TriageChoice::WorthDetails.as_str(),
        reason: "Needs the full description.",
        model: "deepseek-flash",
        prompt_hash: "abc",
    }];
    let updated = job_repo::apply_triage_tx(&mut tx, &writes)
        .await
        .expect("apply");
    tx.commit().await.expect("commit");
    assert_eq!(updated, 1);

    let (state, decision): (String, Option<String>) =
        sqlx::query_as("SELECT queue_state, triage_decision FROM job_descriptions WHERE id = 1")
            .fetch_one(&pool)
            .await
            .expect("row");
    assert_eq!(state, "details_needed");
    assert_eq!(decision.as_deref(), Some("worth_details"));

    pool.close().await;
}
