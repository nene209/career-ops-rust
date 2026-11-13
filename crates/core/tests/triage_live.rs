//! Live triage check (Phase 3.2, WS5).
//!
//! Ignored by default and gated on `DEEPSEEK_API_KEY`; see
//! `docs/LIVE_TESTS.md`. It spends tokens, so it runs on demand.
//!
//! The offline suite replays `tests/cassettes/triage.json`; this test is what
//! keeps that recording honest. With `CAREER_OPS_RECORD_CASSETTES=1` a passing
//! run rewrites the cassette — review the diff before committing, because the
//! cassette is a fixture and a changed decision is a changed expectation.

mod common;

use std::sync::Mutex;

use async_trait::async_trait;
use career_ops_core::llm::{
    Completion, CompletionBackend, DeltaSink, LlmClient, LlmConfig, LlmError,
};
use career_ops_core::repo::{job_descriptions as job_repo, llm_usage};
use career_ops_core::triage::{run_triage, TriageChoice, TriageRequest};
use chrono::Utc;
use common::phase3;
use zeroize::Zeroizing;

fn api_key() -> Option<String> {
    std::env::var("DEEPSEEK_API_KEY")
        .ok()
        .filter(|key| !key.trim().is_empty())
}

fn build_client(key: &str) -> LlmClient {
    LlmConfig::default()
        .build_client(&Zeroizing::new(key.to_string()))
        .expect("build the DeepSeek client")
}

fn recording() -> bool {
    std::env::var("CAREER_OPS_RECORD_CASSETTES").is_ok_and(|value| value == "1")
}

/// A real client that keeps the raw reply so a passing run can record it.
struct RecordingBackend {
    inner: LlmClient,
    replies: Mutex<Vec<String>>,
}

impl RecordingBackend {
    fn new(inner: LlmClient) -> Self {
        Self {
            inner,
            replies: Mutex::new(Vec::new()),
        }
    }

    fn last_reply(&self) -> Option<String> {
        self.replies.lock().unwrap().last().cloned()
    }
}

#[async_trait]
impl CompletionBackend for RecordingBackend {
    async fn complete(
        &self,
        prompt: &str,
        model: Option<&str>,
        json_mode: bool,
        deltas: Option<&DeltaSink>,
    ) -> Result<Completion, LlmError> {
        let completion = self
            .inner
            .complete_text(prompt, model, json_mode, deltas)
            .await?;
        self.replies.lock().unwrap().push(completion.text.clone());
        Ok(completion)
    }
}

#[tokio::test]
#[ignore = "live; set CAREER_OPS_LIVE=1, DEEPSEEK_API_KEY, and run with --ignored"]
async fn deepseek_triages_a_small_batch() {
    if !career_ops_test_support::require_live("deepseek_triages_a_small_batch") {
        return;
    }
    let Some(key) = api_key() else {
        eprintln!("skipping deepseek_triages_a_small_batch: DEEPSEEK_API_KEY is not set");
        return;
    };

    let db = common::TestDb::new_raw().await;
    phase3::seed(&db).await;
    let jobs = phase3::store_triage_samples(&db).await;
    let ids: Vec<i64> = jobs.iter().map(|job| job.id).collect();

    let backend = RecordingBackend::new(build_client(&key));
    let report = run_triage(&db.pool, &TriageRequest::new(ids.clone()), &backend)
        .await
        .expect("triage");

    for decision in &report.decisions {
        eprintln!(
            "{} | {} | {}",
            decision.job_id,
            decision.decision.as_str(),
            decision.reason
        );
    }

    assert_eq!(
        report.decisions.len(),
        ids.len(),
        "every posting must have exactly one decision"
    );
    for decision in &report.decisions {
        assert!(ids.contains(&decision.job_id));
        assert!(!decision.reason.trim().is_empty(), "a reason is required");
        assert!(TriageChoice::ALL.contains(&decision.decision));
    }

    // The decisions reached the queue and the provenance columns.
    for decision in &report.decisions {
        let stored = job_repo::get(&db.pool, decision.job_id)
            .await
            .expect("get")
            .expect("row");
        assert_eq!(
            stored.triage_decision.as_deref(),
            Some(decision.decision.as_str())
        );
        assert_eq!(stored.queue_state, decision.decision.queue_state().as_str());
        assert_eq!(
            stored.triage_model.as_deref(),
            Some(report.model_used.as_str())
        );
        assert!(stored.triaged_at.is_some());
    }

    let usage = llm_usage::recent(&db.pool, None, 10).await.expect("recent");
    assert_eq!(usage.len(), 1, "one call, one ledger row");
    assert_eq!(usage[0].task, "triage");

    if recording() {
        let reply_text = backend.last_reply().expect("the model returned a reply");
        let reply: serde_json::Value =
            serde_json::from_str(&reply_text).expect("the triage reply is JSON");
        phase3::save_triage(&phase3::TriageCassette {
            model: report.model_used.clone(),
            synthetic: false,
            recorded_at: Utc::now().format("%Y-%m-%d").to_string(),
            note: "Recorded by triage_live; review the diff before committing.".into(),
            reply,
        });
    }

    db.close().await;
}
