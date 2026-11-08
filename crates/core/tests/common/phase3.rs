//! Phase 3 fixture set and cassette replay.
//!
//! The fixtures are three small, curated postings with known expectations: a
//! strong match, a clear non-match, and a borderline role. Each has a recorded
//! provider reply in `tests/cassettes/`, so the default suite replays the
//! interaction offline.
//!
//! The shipped cassettes are marked `synthetic`. They are hand-authored
//! placeholders that exercise the whole pipeline (prompt assembly, validation,
//! storage, status) with a realistic reply. Re-record them against the real
//! provider with `CAREER_OPS_RECORD_CASSETTES=1` and a key; see
//! `docs/LIVE_TESTS.md`. A recording replaces the file and sets
//! `synthetic: false`.

#![allow(dead_code)]

use std::path::PathBuf;
use std::sync::Mutex;

use async_trait::async_trait;
use career_ops_core::llm::{CompletionBackend, DeltaSink, LlmError};
use career_ops_core::model::{FetchTier, JobDescription};
use career_ops_core::prompts_seed::seed_or_update_system_prompts;
use career_ops_core::repo::job_descriptions as job_repo;
use career_ops_core::repo::profile::{self, ProfileDetails};
use serde::{Deserialize, Serialize};

/// The candidate summary every fixture is evaluated against.
pub const CV_SUMMARY: &str = include_str!("../fixtures/phase3/cv.txt");

#[derive(Debug, Clone, Copy)]
pub struct Fixture {
    pub name: &'static str,
    pub title: &'static str,
    pub job_html: &'static str,
    /// A phrase the assembled prompt must contain, proving the right posting
    /// reached the model.
    pub distinctive: &'static str,
    /// The expected `match_score` range.
    pub band: (f32, f32),
    /// The status the report's recommendation should produce.
    pub expect_not_a_match: bool,
}

pub const FIXTURES: &[Fixture] = &[
    Fixture {
        name: "strong",
        title: "Senior Rust Engineer, Desktop",
        job_html: include_str!("../fixtures/phase3/strong_job.html"),
        distinctive: "offline-first desktop application",
        band: (0.8, 1.0),
        expect_not_a_match: false,
    },
    Fixture {
        name: "weak",
        title: "Brand Marketing Manager",
        job_html: include_str!("../fixtures/phase3/weak_job.html"),
        distinctive: "brand campaign",
        band: (0.0, 0.3),
        expect_not_a_match: true,
    },
    Fixture {
        name: "borderline",
        title: "Rust Backend Engineer",
        job_html: include_str!("../fixtures/phase3/borderline_job.html"),
        distinctive: "Kubernetes",
        band: (0.4, 0.6),
        expect_not_a_match: true,
    },
];

impl Fixture {
    pub fn by_name(name: &str) -> &'static Fixture {
        FIXTURES
            .iter()
            .find(|fixture| fixture.name == name)
            .unwrap_or_else(|| panic!("unknown fixture `{name}`"))
    }
}

/// The profile and prompts every fixture test needs.
pub async fn seed(db: &super::TestDb) {
    profile::update_details(
        &db.pool,
        &ProfileDetails {
            name: "Ada Lovelace".into(),
            email: "ada@example.com".into(),
            phone: None,
            location: Some("London".into()),
            summary: Some(CV_SUMMARY.to_string()),
        },
    )
    .await
    .expect("profile");
    seed_or_update_system_prompts(&db.pool)
        .await
        .expect("seed prompts");
}

pub async fn store_job(db: &super::TestDb, fixture: &Fixture) -> JobDescription {
    job_repo::upsert(
        &db.pool,
        job_repo::NewJobDescription {
            title: fixture.title.into(),
            company: "Acme".into(),
            source_url: format!("https://example.com/jobs/{}", fixture.name),
            raw_text: fixture.job_html.into(),
            fetch_tier: FetchTier::Tier1bHtml,
            external_id: None,
        },
    )
    .await
    .expect("store job")
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvaluationCassette {
    pub fixture: String,
    pub model: String,
    pub synthetic: bool,
    pub recorded_at: String,
    #[serde(default)]
    pub note: String,
    /// The model's JSON object, stored structured so a human can read a diff.
    pub reply: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverLetterCassette {
    pub fixture: String,
    pub model: String,
    pub synthetic: bool,
    pub recorded_at: String,
    #[serde(default)]
    pub note: String,
    pub reply: String,
}

fn cassette_path(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("cassettes")
        .join(format!("{name}.json"))
}

pub fn load_evaluation(name: &str) -> EvaluationCassette {
    let path = cassette_path(name);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|error| panic!("parse {}: {error}", path.display()))
}

pub fn save_evaluation(cassette: &EvaluationCassette) {
    let path = cassette_path(&cassette.fixture);
    let text = serde_json::to_string_pretty(cassette).expect("serialize the cassette");
    std::fs::write(&path, format!("{text}\n"))
        .unwrap_or_else(|error| panic!("write {}: {error}", path.display()));
}

pub fn load_cover_letter(name: &str) -> CoverLetterCassette {
    let path = cassette_path(name);
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|error| panic!("read {}: {error}", path.display()));
    serde_json::from_str(&text).unwrap_or_else(|error| panic!("parse {}: {error}", path.display()))
}

pub fn save_cover_letter(cassette: &CoverLetterCassette) {
    let path = cassette_path(&format!("cover_letter_{}", cassette.fixture));
    let text = serde_json::to_string_pretty(cassette).expect("serialize the cassette");
    std::fs::write(&path, format!("{text}\n"))
        .unwrap_or_else(|error| panic!("write {}: {error}", path.display()));
}

/// Replays an evaluation cassette and records the prompts it was given.
pub struct CassetteBackend {
    cassette: EvaluationCassette,
    prompts: Mutex<Vec<String>>,
}

impl CassetteBackend {
    pub fn new(name: &str) -> Self {
        Self {
            cassette: load_evaluation(name),
            prompts: Mutex::new(Vec::new()),
        }
    }

    pub fn prompts(&self) -> Vec<String> {
        self.prompts.lock().unwrap().clone()
    }

    pub fn cassette(&self) -> &EvaluationCassette {
        &self.cassette
    }
}

#[async_trait]
impl CompletionBackend for CassetteBackend {
    async fn complete(
        &self,
        prompt: &str,
        model: Option<&str>,
        json_mode: bool,
        _deltas: Option<&DeltaSink>,
    ) -> Result<String, LlmError> {
        assert!(
            model.is_none_or(|model| model == self.cassette.model),
            "the cassette was recorded with `{}`, not `{}`",
            self.cassette.model,
            model.unwrap_or("")
        );
        if json_mode {
            // The evaluation asks for JSON; the cover letter does not.
            assert!(
                self.cassette.reply.is_object(),
                "an evaluation cassette must hold a JSON object"
            );
        }
        self.prompts.lock().unwrap().push(prompt.to_string());
        Ok(serde_json::to_string(&self.cassette.reply).expect("serialize the recorded reply"))
    }
}

/// Replays a cover-letter cassette.
pub struct LetterCassetteBackend {
    cassette: CoverLetterCassette,
    prompts: Mutex<Vec<String>>,
}

impl LetterCassetteBackend {
    pub fn new(name: &str) -> Self {
        Self {
            cassette: load_cover_letter(name),
            prompts: Mutex::new(Vec::new()),
        }
    }

    pub fn prompts(&self) -> Vec<String> {
        self.prompts.lock().unwrap().clone()
    }
}

#[async_trait]
impl CompletionBackend for LetterCassetteBackend {
    async fn complete(
        &self,
        prompt: &str,
        _model: Option<&str>,
        _json_mode: bool,
        _deltas: Option<&DeltaSink>,
    ) -> Result<String, LlmError> {
        self.prompts.lock().unwrap().push(prompt.to_string());
        Ok(self.cassette.reply.clone())
    }
}
