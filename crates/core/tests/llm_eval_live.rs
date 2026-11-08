//! Live prompt-quality checks (Phase 3 WS7).
//!
//! Ignored by default and gated on `DEEPSEEK_API_KEY`; see `docs/LIVE_TESTS.md`.
//! These are the checks that only a real model can make: that the curated
//! fixtures land in their expected score bands and that a generated letter is
//! close to a reference. They spend tokens, so they run on demand.
//!
//! With `CAREER_OPS_RECORD_CASSETTES=1`, a passing run rewrites
//! `crates/core/tests/cassettes/`, which is how the offline fixtures are
//! refreshed after a prompt or model change. Review the diff before committing:
//! the cassettes are fixtures, and a changed reply is a changed expectation.

mod common;

use std::sync::Mutex;

use async_trait::async_trait;
use career_ops_core::cover_letter::{run_cover_letter, CoverLetterRequest};
use career_ops_core::evaluation::{run_evaluation, EvaluationRequest};
use career_ops_core::llm::{CompletionBackend, DeltaSink, LlmClient, LlmConfig, LlmError};
use career_ops_core::repo::job_descriptions as job_repo;
use chrono::Utc;
use zeroize::Zeroizing;

use common::phase3::{self, FIXTURES};

/// A hand-written reference for the strong fixture. The letter judge compares
/// against this, not against a previous model output, so re-recording a cassette
/// does not move the goalposts.
const REFERENCE_LETTER: &str = "Dear Hiring Manager,\n\nYour posting describes an offline-first desktop application with SQLite as the source of truth, which is the product I have spent six years building. At Analytical Engines I designed the local-first storage layer: SQLite with envelope encryption, a migration runner, and crash-safe writes. I led the egui front end and cut cold start from 4.2 seconds to 0.9.\n\nBefore that I wrote a headless browser sidecar and the fixture-driven parsers behind it, which cut scraping failures by 40 percent. I write tests before shipping and prefer small, reversible changes.\n\nSincerely,\nAda Lovelace";

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

/// A real client that keeps the raw replies so a passing run can record them.
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
    ) -> Result<String, LlmError> {
        let reply = self
            .inner
            .complete_text(prompt, model, json_mode, deltas)
            .await?;
        self.replies.lock().unwrap().push(reply.clone());
        Ok(reply)
    }
}

#[tokio::test]
#[ignore = "hits the network and spends tokens; set DEEPSEEK_API_KEY and run with --ignored"]
async fn deepseek_scores_the_fixtures_in_their_bands() {
    let Some(key) = api_key() else {
        eprintln!(
            "skipping deepseek_scores_the_fixtures_in_their_bands: DEEPSEEK_API_KEY is not set"
        );
        return;
    };

    for fixture in FIXTURES {
        let db = common::TestDb::new_raw().await;
        phase3::seed(&db).await;
        let job = phase3::store_job(&db, fixture).await;

        let backend = RecordingBackend::new(build_client(&key));
        let result = run_evaluation(
            &db.pool,
            &EvaluationRequest::new(job.id),
            &backend,
            None,
            None,
        )
        .await
        .unwrap_or_else(|error| panic!("{}: {error}", fixture.name));

        eprintln!(
            "fixture {}: {:.2} ({})",
            fixture.name,
            result.match_score,
            result.recommendation.label()
        );

        assert!(
            (fixture.band.0..=fixture.band.1).contains(&result.match_score),
            "{} scored {:.2}, expected {:.2}..={:.2}. If the band is wrong after \
             a prompt or model change, update the fixture deliberately.",
            fixture.name,
            result.match_score,
            fixture.band.0,
            fixture.band.1
        );

        if recording() {
            let reply_text = backend.last_reply().expect("the model returned a reply");
            let reply: serde_json::Value =
                serde_json::from_str(&reply_text).expect("the evaluation reply is JSON");
            phase3::save_evaluation(&phase3::EvaluationCassette {
                fixture: fixture.name.to_string(),
                model: result.model_used.clone(),
                synthetic: false,
                recorded_at: Utc::now().format("%Y-%m-%d").to_string(),
                note: "Recorded by llm_eval_live; review the diff before committing.".into(),
                reply,
            });
        }

        // The stored provenance must round-trip for every live run too.
        let stored = job_repo::get(&db.pool, job.id)
            .await
            .expect("get")
            .expect("job");
        assert_eq!(stored.queue_state, "evaluated");
        db.close().await;
    }
}

#[tokio::test]
#[ignore = "hits the network and spends tokens; set DEEPSEEK_API_KEY and run with --ignored"]
async fn deepseek_writes_a_usable_cover_letter() {
    let Some(key) = api_key() else {
        eprintln!("skipping deepseek_writes_a_usable_cover_letter: DEEPSEEK_API_KEY is not set");
        return;
    };

    let db = common::TestDb::new_raw().await;
    phase3::seed(&db).await;
    let job = phase3::store_job(&db, phase3::Fixture::by_name("strong")).await;

    let backend = RecordingBackend::new(build_client(&key));
    let letter = run_cover_letter(&db.pool, &CoverLetterRequest::new(job.id), &backend)
        .await
        .expect("write the letter");

    let lower = letter.content.to_lowercase();
    for banned in [
        "i am writing to",
        "i am excited",
        "perfect fit",
        "passionate",
        "team player",
        "hit the ground running",
        "proven track record",
    ] {
        assert!(!lower.contains(banned), "the letter uses `{banned}`");
    }
    assert!(!letter.content.contains('—'), "no em dashes");

    // A model judge for the one property a lexical check cannot see: whether
    // the letter actually says what the reference says. One sample, a low bar,
    // and the score is printed so a drift is visible in the log.
    let judge = build_client(&key);
    let prompt = format!(
        "Compare a generated cover letter with a reference for the same job. \
         Score the semantic similarity from 0 to 1: 1 means the same concrete \
         claims and emphasis, 0 means unrelated. Return only the number.\n\n\
         Reference:\n{REFERENCE_LETTER}\n\nGenerated:\n{}\n",
        letter.content
    );
    let reply = judge
        .complete_text(&prompt, None, false, None)
        .await
        .expect("the judge answered");
    let score = first_float(&reply).unwrap_or_else(|| panic!("no number in `{reply}`"));
    eprintln!("cover letter similarity: {score:.2}");
    assert!(
        score >= 0.7,
        "the letter is too far from the reference ({score:.2})"
    );

    if recording() {
        phase3::save_cover_letter(&phase3::CoverLetterCassette {
            fixture: "strong".into(),
            model: letter.model_used.clone(),
            synthetic: false,
            recorded_at: Utc::now().format("%Y-%m-%d").to_string(),
            note: "Recorded by llm_eval_live; review the diff before committing.".into(),
            reply: letter.content.clone(),
        });
    }

    db.close().await;
}

/// The first decimal number in a model's reply, tolerating prose around it.
fn first_float(text: &str) -> Option<f64> {
    text.split(|c: char| c.is_whitespace() || c == ',' || c == ';')
        .find_map(|token| {
            let cleaned = token.trim_matches(|c: char| !c.is_ascii_digit() && c != '.');
            cleaned.parse::<f64>().ok()
        })
}
