//! Phase 3.1: every provider call leaves a ledger row.
//!
//! Offline: a scripted backend returns `Completion`s (with or without reported
//! usage, or an error), and the real evaluation and cover-letter pipelines
//! write through the real repository.

mod common;

use std::collections::VecDeque;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use career_ops_core::cover_letter::{run_cover_letter, CoverLetterRequest};
use career_ops_core::evaluation::{run_evaluation, EvaluationRequest};
use career_ops_core::llm::{
    CancelToken, Completion, CompletionBackend, DeltaSink, LlmError, Usage,
};
use career_ops_core::repo::llm_usage;

use common::phase3;

type BackendResult = std::result::Result<Completion, LlmError>;

struct ScriptedBackend {
    replies: Mutex<VecDeque<BackendResult>>,
}

impl ScriptedBackend {
    fn new(replies: impl IntoIterator<Item = BackendResult>) -> Self {
        Self {
            replies: Mutex::new(replies.into_iter().collect()),
        }
    }

    fn ok(completion: Completion) -> Self {
        Self::new([Ok(completion)])
    }
}

#[async_trait]
impl CompletionBackend for ScriptedBackend {
    async fn complete(
        &self,
        _prompt: &str,
        _model: Option<&str>,
        _json_mode: bool,
        _deltas: Option<&DeltaSink>,
    ) -> BackendResult {
        self.replies
            .lock()
            .unwrap()
            .pop_front()
            .expect("the script ran out of replies")
    }
}

fn reported_usage(input: u64, output: u64) -> Usage {
    Usage {
        input_tokens: Some(input),
        output_tokens: Some(output),
        total_tokens: Some(input + output),
        cached_input_tokens: None,
        reasoning_tokens: None,
        cost_usd: None,
        cost_complete: None,
        reported: true,
    }
}

fn json_reply(score: f32) -> String {
    format!(
        r###"{{"match_score": {score},
            "recommendation": "strong_match",
            "strengths": ["a", "b", "c"],
            "gaps": ["x"],
            "rationale": "fits",
            "full_report_markdown": "A strong fit."}}"###
    )
}

async fn seeded() -> (common::TestDb, career_ops_core::model::JobDescription) {
    let db = common::TestDb::new_raw().await;
    phase3::seed(&db).await;
    let job = phase3::store_job(&db, phase3::Fixture::by_name("strong")).await;
    (db, job)
}

#[tokio::test]
async fn an_evaluation_records_the_provider_reported_counters() {
    let (db, job) = seeded().await;
    let backend = ScriptedBackend::ok(Completion {
        text: json_reply(0.9),
        usage: reported_usage(100, 20),
    });

    let result = run_evaluation(
        &db.pool,
        &EvaluationRequest::new(job.id),
        &backend,
        None,
        None,
    )
    .await
    .expect("evaluate");

    let rows = llm_usage::recent(&db.pool, None, 10).await.expect("recent");
    assert_eq!(rows.len(), 1, "one call, one row");

    let row = &rows[0];
    assert_eq!(row.provider, "deepseek");
    assert_eq!(row.model, result.model_used);
    assert_eq!(row.task, "evaluation");
    assert_eq!(row.source, "reported");
    assert_eq!(row.outcome, "ok");
    assert_eq!(row.error_kind, None);
    assert_eq!(row.input_tokens, Some(100));
    assert_eq!(row.output_tokens, Some(20));
    assert_eq!(row.total_tokens, Some(120));
    assert_eq!(row.cost_usd, None, "the fake reported no cost");
    assert_eq!(row.job_title.as_deref(), Some(job.title.as_str()));
    assert_eq!(row.company.as_deref(), Some("Acme"));
    assert!(row.duration_ms >= 0);

    db.close().await;
}

#[tokio::test]
async fn an_unreported_usage_is_estimated_from_the_prompt_and_reply() {
    let (db, job) = seeded().await;
    let backend = ScriptedBackend::ok(Completion::text_only(json_reply(0.9)));

    run_evaluation(
        &db.pool,
        &EvaluationRequest::new(job.id),
        &backend,
        None,
        None,
    )
    .await
    .expect("evaluate");

    let rows = llm_usage::recent(&db.pool, None, 10).await.expect("recent");
    let row = &rows[0];
    assert_eq!(row.source, "estimated");
    assert!(
        row.input_tokens.is_some_and(|tokens| tokens > 0),
        "the prompt was estimated"
    );
    assert!(
        row.output_tokens.is_some_and(|tokens| tokens > 0),
        "the reply was estimated"
    );
    assert_eq!(row.cost_usd, None, "an estimate is not priced");

    db.close().await;
}

#[tokio::test]
async fn a_repair_writes_a_second_row() {
    let (db, job) = seeded().await;
    let backend = ScriptedBackend::new([
        Ok(Completion::text_only("this is not JSON")),
        Ok(Completion::text_only(json_reply(0.8))),
    ]);

    let result = run_evaluation(
        &db.pool,
        &EvaluationRequest::new(job.id),
        &backend,
        None,
        None,
    )
    .await
    .expect("the repair produced a report");
    assert_eq!(result.match_score, 0.8);

    let rows = llm_usage::recent(&db.pool, None, 10).await.expect("recent");
    assert_eq!(rows.len(), 2, "the first attempt and the repair");
    assert_eq!(rows[0].task, "evaluation_repair", "newest first");
    assert_eq!(rows[1].task, "evaluation");
    assert!(rows.iter().all(|row| row.outcome == "ok"));

    db.close().await;
}

#[tokio::test]
async fn a_failed_call_is_recorded_with_its_kind() {
    let (db, job) = seeded().await;
    let backend = ScriptedBackend::new([Err(LlmError::Transport("offline".into()))]);

    run_evaluation(
        &db.pool,
        &EvaluationRequest::new(job.id),
        &backend,
        None,
        None,
    )
    .await
    .expect_err("the provider failed");

    let rows = llm_usage::recent(&db.pool, None, 10).await.expect("recent");
    assert_eq!(rows.len(), 1, "a failure still spent a request");
    let row = &rows[0];
    assert_eq!(row.outcome, "error");
    assert_eq!(row.error_kind.as_deref(), Some("transport"));
    assert_eq!(row.source, "estimated");
    assert!(
        row.input_tokens.is_some_and(|tokens| tokens > 0),
        "the prompt is estimated even when the call fails"
    );
    assert_eq!(row.output_tokens, None, "there was no reply to estimate");

    db.close().await;
}

#[tokio::test]
async fn a_cancelled_call_is_recorded_as_such() {
    let (db, job) = seeded().await;
    let backend = ScriptedBackend::new([]);
    let mut request = EvaluationRequest::new(job.id);
    request.cancel = CancelToken::new(Arc::new(AtomicBool::new(true)));

    run_evaluation(&db.pool, &request, &backend, None, None)
        .await
        .expect_err("cancelled before the call");

    let rows = llm_usage::recent(&db.pool, None, 10).await.expect("recent");
    assert_eq!(rows.len(), 1, "a cancelled call is recorded too");
    assert_eq!(rows[0].outcome, "cancelled");
    assert_eq!(rows[0].error_kind.as_deref(), Some("cancelled"));

    db.close().await;
}

#[tokio::test]
async fn a_cover_letter_and_its_usage_commit_together() {
    let (db, job) = seeded().await;
    let backend = ScriptedBackend::ok(Completion {
        text: "Dear Hiring Manager,\n\nSincerely,\nAda".into(),
        usage: reported_usage(500, 100),
    });

    let result = run_cover_letter(&db.pool, &CoverLetterRequest::new(job.id), &backend)
        .await
        .expect("write the letter");

    let rows = llm_usage::recent(&db.pool, None, 10).await.expect("recent");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].task, "cover_letter");
    assert_eq!(rows[0].source, "reported");
    assert_eq!(rows[0].input_tokens, Some(500));
    assert_eq!(rows[0].output_tokens, Some(100));
    assert_eq!(rows[0].company.as_deref(), Some("Acme"));

    // The letter is one row and the usage another, written in one transaction.
    let letter = career_ops_core::repo::cover_letters::get(&db.pool, result.cover_letter_id)
        .await
        .expect("get")
        .expect("letter");
    assert_eq!(letter.application_id, result.application_id);

    db.close().await;
}

#[tokio::test]
async fn a_failed_cover_letter_is_recorded_and_stores_no_letter() {
    let (db, job) = seeded().await;
    let backend = ScriptedBackend::new([Err(LlmError::RateLimited {
        provider: career_ops_core::llm::LlmProvider::DeepSeek,
        retry_after: None,
        detail: "slow down".into(),
    })]);

    run_cover_letter(&db.pool, &CoverLetterRequest::new(job.id), &backend)
        .await
        .expect_err("the provider refused");

    let rows = llm_usage::recent(&db.pool, None, 10).await.expect("recent");
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].task, "cover_letter");
    assert_eq!(rows[0].outcome, "error");
    assert_eq!(rows[0].error_kind.as_deref(), Some("rate_limited"));

    let letters = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM cover_letters")
        .fetch_one(&db.pool)
        .await
        .expect("count");
    assert_eq!(letters, 0, "a refused call stores no letter");

    db.close().await;
}

#[tokio::test]
async fn deleting_a_job_keeps_the_record_of_what_it_cost() {
    let (db, job) = seeded().await;
    let backend = ScriptedBackend::ok(Completion {
        text: json_reply(0.9),
        usage: reported_usage(300, 60),
    });
    run_evaluation(
        &db.pool,
        &EvaluationRequest::new(job.id),
        &backend,
        None,
        None,
    )
    .await
    .expect("evaluate");

    // Deleting the job cascades its application, its report, and its letter.
    career_ops_core::repo::job_descriptions::delete(&db.pool, job.id)
        .await
        .expect("delete the job");

    let rows = llm_usage::recent(&db.pool, None, 10).await.expect("recent");
    assert_eq!(rows.len(), 1, "the ledger outlives the posting");
    assert_eq!(rows[0].total_tokens, Some(360));
    assert_eq!(
        rows[0].job_title, None,
        "the join finds no row, but the usage row remains"
    );

    let count = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM llm_usage")
        .fetch_one(&db.pool)
        .await
        .expect("count");
    assert_eq!(count, 1);

    db.close().await;
}
