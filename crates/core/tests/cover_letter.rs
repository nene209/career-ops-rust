//! The cover-letter pipeline (Phase 3, WS4).
//!
//! Offline, like the evaluation tests: a [`FakeBackend`] returns canned text and
//! records the prompt it was given, so these pin the assembly, the tone, the
//! storage, and the regeneration rule without a network.

mod common;

use std::collections::VecDeque;
use std::sync::Mutex;

use async_trait::async_trait;
use career_ops_core::cover_letter::{run_cover_letter, CoverLetterRequest};
use career_ops_core::llm::{CompletionBackend, DeltaSink, LlmError};
use career_ops_core::model::{
    Application, ApplicationStatus, FetchTier, JobDescription, QueueState,
};
use career_ops_core::prompts_seed::seed_or_update_system_prompts;
use career_ops_core::repo::applications;
use career_ops_core::repo::cover_letters::{self, NewCoverLetter};
use career_ops_core::repo::job_descriptions as job_repo;
use career_ops_core::repo::job_descriptions::NewJobDescription;
use career_ops_core::repo::profile::{self, ProfileDetails};

#[tokio::test]
async fn a_cover_letter_is_stored_with_provenance() {
    let db = common::TestDb::new_raw().await;
    seed_profile_and_prompts(&db).await;
    let job = store_job(&db, "Rust Engineer", &job_text()).await;

    let backend = FakeBackend::with_letter(LETTER);
    let result = run_cover_letter(&db.pool, &CoverLetterRequest::new(job.id), &backend)
        .await
        .expect("write");

    assert_eq!(result.content, LETTER.trim());
    assert_eq!(result.model_used, "deepseek-chat");
    assert_eq!(result.prompt_version, 1, "the seeded prompt is version 1");
    assert!(!result.prompt_hash.is_empty());
    assert!(result.input_chars > 0, "the assembled prompt was counted");

    let stored = cover_letters::get(&db.pool, result.cover_letter_id)
        .await
        .expect("get")
        .expect("letter");
    assert_eq!(stored.application_id, result.application_id);
    assert_eq!(stored.content, LETTER.trim());
    assert_eq!(stored.model_used, "deepseek-chat");
    assert_eq!(stored.prompt_version, Some(1));
    assert_eq!(
        stored.prompt_hash.as_deref(),
        Some(result.prompt_hash.as_str())
    );
    assert_eq!(stored.input_chars, Some(result.input_chars as i64));

    // A letter does not change the application's status.
    let application = applications::get(&db.pool, result.application_id)
        .await
        .expect("get")
        .expect("application");
    assert_eq!(application.status, ApplicationStatus::Discovered.as_str());

    db.close().await;
}

#[tokio::test]
async fn regenerating_adds_a_row_and_preserves_the_first() {
    let db = common::TestDb::new_raw().await;
    seed_profile_and_prompts(&db).await;
    let job = store_job(&db, "Rust Engineer", &job_text()).await;

    let first = run_cover_letter(
        &db.pool,
        &CoverLetterRequest::new(job.id),
        &FakeBackend::with_letter("First draft."),
    )
    .await
    .expect("first");
    let second = run_cover_letter(
        &db.pool,
        &CoverLetterRequest::new(job.id),
        &FakeBackend::with_letter("Second draft."),
    )
    .await
    .expect("second");

    assert_ne!(first.cover_letter_id, second.cover_letter_id);
    let all = cover_letters::list_for_application(&db.pool, first.application_id)
        .await
        .expect("list");
    assert_eq!(all.len(), 2, "each generation is kept");
    assert_eq!(all[0].content, "Second draft.", "newest first");
    assert_eq!(all[1].content, "First draft.");

    let latest = cover_letters::latest_for_application(&db.pool, first.application_id)
        .await
        .expect("latest")
        .expect("letter");
    assert_eq!(latest.id, second.cover_letter_id);

    // Deleting the second leaves the first usable.
    cover_letters::delete(&db.pool, second.cover_letter_id)
        .await
        .expect("delete");
    let remaining = cover_letters::list_for_application(&db.pool, first.application_id)
        .await
        .expect("list");
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].id, first.cover_letter_id);

    db.close().await;
}

#[tokio::test]
async fn the_tone_reaches_the_prompt() {
    let db = common::TestDb::new_raw().await;
    seed_profile_and_prompts(&db).await;
    let job = store_job(&db, "Rust Engineer", &job_text()).await;

    let backend = FakeBackend::with_letter(LETTER);
    let mut request = CoverLetterRequest::new(job.id);
    request.tone = Some("  warm  ".into());
    run_cover_letter(&db.pool, &request, &backend)
        .await
        .expect("write");

    let prompts = backend.prompts();
    assert_eq!(prompts.len(), 1);
    assert!(
        prompts[0].contains("**Tone:** warm"),
        "the tone is trimmed and substituted: {}",
        &prompts[0][..prompts[0].len().min(400)]
    );
    assert!(!prompts[0].contains("{{tone}}"));

    // No tone leaves the slot empty rather than literal placeholder text.
    let backend = FakeBackend::with_letter(LETTER);
    run_cover_letter(&db.pool, &CoverLetterRequest::new(job.id), &backend)
        .await
        .expect("write");
    let prompts = backend.prompts();
    assert!(prompts[0].contains("**Tone:**"));
    assert!(!prompts[0].contains("{{tone}}"));

    db.close().await;
}

#[tokio::test]
async fn the_job_description_is_normalized_before_the_prompt() {
    let db = common::TestDb::new_raw().await;
    seed_profile_and_prompts(&db).await;
    let job = store_job(
        &db,
        "Rust Engineer",
        "<p>Build the <strong>network</strong> stack.</p>",
    )
    .await;

    let backend = FakeBackend::with_letter(LETTER);
    run_cover_letter(&db.pool, &CoverLetterRequest::new(job.id), &backend)
        .await
        .expect("write");

    let prompt = &backend.prompts()[0];
    assert!(
        prompt.contains("Build the network stack."),
        "the text survives as prose"
    );
    assert!(!prompt.contains("<p>"), "no markup reaches the model");
    assert!(!prompt.contains("<strong>"));

    db.close().await;
}

#[tokio::test]
async fn a_missing_cv_summary_is_an_actionable_error() {
    let db = common::TestDb::new_raw().await;
    seed_or_update_system_prompts(&db.pool)
        .await
        .expect("seed prompts");
    let job = store_job(&db, "Rust Engineer", &job_text()).await;

    let backend = FakeBackend::with_letter(LETTER);
    let error = run_cover_letter(&db.pool, &CoverLetterRequest::new(job.id), &backend)
        .await
        .expect_err("no CV");

    assert!(
        error.to_string().contains("Add your CV"),
        "the message must say what to do: {error}"
    );
    assert!(backend.prompts().is_empty(), "no call without a CV");
    assert!(cover_letters::list_for_application(&db.pool, 1)
        .await
        .expect("list")
        .is_empty());

    db.close().await;
}

#[tokio::test]
async fn a_missing_prompt_is_an_actionable_error() {
    let db = common::TestDb::new_raw().await;
    seed_profile_and_prompts(&db).await;
    let job = store_job(&db, "Rust Engineer", &job_text()).await;

    let backend = FakeBackend::with_letter(LETTER);
    let mut request = CoverLetterRequest::new(job.id);
    request.prompt_name = "no-such-prompt".into();
    let error = run_cover_letter(&db.pool, &request, &backend)
        .await
        .expect_err("no prompt");

    assert!(
        error
            .to_string()
            .contains("no prompt named `no-such-prompt`"),
        "{error}"
    );
    assert!(backend.prompts().is_empty());

    db.close().await;
}

#[tokio::test]
async fn an_empty_reply_is_an_error_and_stores_nothing() {
    let db = common::TestDb::new_raw().await;
    seed_profile_and_prompts(&db).await;
    let job = store_job(&db, "Rust Engineer", &job_text()).await;

    let backend = FakeBackend::with_letter("   \n\t ");
    let error = run_cover_letter(&db.pool, &CoverLetterRequest::new(job.id), &backend)
        .await
        .expect_err("empty reply");

    assert!(error.to_string().contains("empty cover letter"), "{error}");
    let application = applications::ensure(&db.pool, job.id)
        .await
        .expect("ensure");
    assert!(
        cover_letters::list_for_application(&db.pool, application.id)
            .await
            .expect("list")
            .is_empty(),
        "a blank reply is never stored"
    );

    db.close().await;
}

#[tokio::test]
async fn a_backend_failure_stores_nothing() {
    let db = common::TestDb::new_raw().await;
    seed_profile_and_prompts(&db).await;
    let job = store_job(&db, "Rust Engineer", &job_text()).await;

    let error = run_cover_letter(
        &db.pool,
        &CoverLetterRequest::new(job.id),
        &FakeBackend::failing(),
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
    assert!(
        cover_letters::list_for_application(&db.pool, application.id)
            .await
            .expect("list")
            .is_empty()
    );

    db.close().await;
}

#[tokio::test]
async fn a_cover_letter_leaves_a_submitted_application_alone() {
    let db = common::TestDb::new_raw().await;
    seed_profile_and_prompts(&db).await;
    let application = store_applied(&db, "Applied role").await;

    let backend = FakeBackend::with_letter(LETTER);
    let result = run_cover_letter(
        &db.pool,
        &CoverLetterRequest::new(application.job_description_id),
        &backend,
    )
    .await
    .expect("write");

    let after = applications::get(&db.pool, application.id)
        .await
        .expect("get")
        .expect("row");
    assert_eq!(after.status, ApplicationStatus::Applied.as_str());
    assert_eq!(result.application_id, application.id);

    // A letter does not touch the queue either.
    let job = job_repo::get(&db.pool, application.job_description_id)
        .await
        .expect("get")
        .expect("job");
    assert_eq!(job.queue_state, QueueState::New.as_str());

    db.close().await;
}

#[tokio::test]
async fn the_repository_rejects_deleting_a_missing_row() {
    let db = common::TestDb::new_raw().await;
    assert!(cover_letters::delete(&db.pool, 404).await.is_err());
    db.close().await;
}

#[tokio::test]
async fn a_letter_can_be_inserted_without_provenance() {
    // The repository is also the seam for a future import, where the prompt is
    // unknown; the columns are nullable for exactly that case.
    let db = common::TestDb::new_raw().await;
    let job = store_job(&db, "Engineer", &job_text()).await;
    let application = applications::ensure(&db.pool, job.id)
        .await
        .expect("ensure");

    let id = cover_letters::insert(
        &db.pool,
        NewCoverLetter {
            application_id: application.id,
            content: "Imported letter.",
            model_used: "unknown",
            prompt_version: None,
            prompt_hash: None,
            input_chars: None,
        },
    )
    .await
    .expect("insert");

    let stored = cover_letters::get(&db.pool, id)
        .await
        .expect("get")
        .expect("letter");
    assert_eq!(stored.prompt_version, None);
    assert_eq!(stored.input_chars, None);

    db.close().await;
}

// ----------------------------------------------------------------------
// Helpers
// ----------------------------------------------------------------------

const LETTER: &str =
    "Dear Hiring Manager,\n\nI build offline-first desktop software.\n\nSincerely,\nAda";

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
    fn with_letter(letter: &str) -> Self {
        Self {
            replies: Mutex::new(VecDeque::from([Reply::Text(letter.to_string())])),
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
        json_mode: bool,
        _deltas: Option<&DeltaSink>,
    ) -> Result<String, LlmError> {
        assert!(!json_mode, "a cover letter is free text, not JSON");
        self.prompts.lock().unwrap().push(prompt.to_string());
        match self.replies.lock().unwrap().pop_front() {
            Some(Reply::Text(text)) => Ok(text),
            Some(Reply::Fail) => Err(LlmError::Transport("fake transport failure".into())),
            None => Err(LlmError::Transport("the fake ran out of replies".into())),
        }
    }
}

fn job_text() -> String {
    let mut text = String::from("<p>We are hiring a Rust engineer.</p>");
    while text.chars().count() < 600 {
        text.push_str("<p>You will build offline-first desktop software with SQLite.</p>");
    }
    text
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

async fn store_applied(db: &common::TestDb, title: &str) -> Application {
    let job = store_job(db, title, &job_text()).await;
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
