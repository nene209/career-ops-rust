//! The portal-suggestion service (Phase 3.2, WS4).
//!
//! Offline: a scripted backend returns one canned reply, and the real service,
//! prompt assembly, known-site lookup, and usage ledger do the work.

mod common;

use std::collections::VecDeque;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use career_ops_core::acquisition::known_sites;
use career_ops_core::llm::{CancelToken, Completion, CompletionBackend, DeltaSink, LlmError};
use career_ops_core::preferences::Preferences;
use career_ops_core::prompts_seed::seed_or_update_system_prompts;
use career_ops_core::repo::llm_usage;
use career_ops_core::repo::profile;
use career_ops_core::suggest::{suggest_portals, SuggestError, MAX_SUGGESTIONS};

use common::phase3;

type BackendResult = std::result::Result<Completion, LlmError>;

struct ScriptedBackend {
    replies: Mutex<VecDeque<BackendResult>>,
    prompts: Mutex<Vec<String>>,
}

impl ScriptedBackend {
    fn with_reply(reply: impl Into<String>) -> Self {
        Self {
            replies: Mutex::new(VecDeque::from([Ok(Completion::text_only(reply))])),
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
        assert!(json_mode, "suggestions ask for JSON mode");
        self.prompts.lock().unwrap().push(prompt.to_string());
        self.replies
            .lock()
            .unwrap()
            .pop_front()
            .expect("the script ran out of replies")
    }
}

fn reply(entries: &[(&str, &str, &str, &str)]) -> String {
    let rows: Vec<String> = entries
        .iter()
        .map(|(company, engine, slug, rationale)| {
            format!(
                r#"{{"company":"{company}","engine":"{engine}","slug":"{slug}","rationale":"{rationale}"}}"#
            )
        })
        .collect();
    format!(r#"{{"suggestions":[{}]}}"#, rows.join(", "))
}

async fn seeded() -> common::TestDb {
    let db = common::TestDb::new_raw().await;
    phase3::seed(&db).await;
    profile::update_preferences(
        &db.pool,
        &Preferences {
            keywords: vec!["Rust".into(), "compiler".into()],
            ..Default::default()
        },
    )
    .await
    .expect("preferences");
    db
}

#[tokio::test]
async fn a_reply_becomes_boards_with_derived_urls_and_known_configs() {
    let db = seeded().await;
    let backend = ScriptedBackend::with_reply(reply(&[
        (
            "Figma",
            "greenhouse",
            "figma",
            "Design tooling matches the CV.",
        ),
        ("Spotify", "lever", "spotify", "Audio platform work."),
        ("NVIDIA", "eightfold", "nvidia.com", "GPU tooling."),
        ("Acme", "greenhouse", "acme", "A small board."),
    ]));

    let report = suggest_portals(&db.pool, &CancelToken::default(), &backend)
        .await
        .expect("suggestions");

    assert_eq!(report.suggestions.len(), 4);
    assert_eq!(report.model_used, "deepseek-flash");
    assert_eq!(report.prompt_version, 1);
    assert!(!report.prompt_hash.is_empty());
    assert!(report.input_chars > 0);
    assert!(
        report.estimated_cost_usd.is_some_and(|cost| cost > 0.0),
        "the pre-call estimate is priced"
    );
    assert_eq!(report.actual_cost_usd, None);

    let figma = &report.suggestions[0];
    assert_eq!(figma.company, "Figma");
    assert_eq!(figma.engine_label, "Greenhouse");
    assert_eq!(figma.host, "job-boards.greenhouse.io");
    assert_eq!(figma.url, "https://job-boards.greenhouse.io/figma");
    assert_eq!(
        figma.config.as_deref(),
        Some(known_sites::FIGMA.config),
        "a known board keeps its hand-written config"
    );

    let nvidia = &report.suggestions[2];
    assert_eq!(nvidia.url, "https://jobs.nvidia.com/careers");
    assert_eq!(nvidia.config.as_deref(), Some(known_sites::NVIDIA.config));

    let acme = &report.suggestions[3];
    assert_eq!(acme.url, "https://job-boards.greenhouse.io/acme");
    assert_eq!(
        acme.config, None,
        "an ATS board is selected by its URL and needs no config"
    );

    // The prompt carried the known-site block and the preferences.
    let prompts = backend.prompts();
    assert_eq!(prompts.len(), 1);
    assert!(prompts[0].contains("- Figma | greenhouse | slug: figma"));
    assert!(prompts[0].contains("- NVIDIA | eightfold | slug: nvidia.com"));
    assert!(prompts[0].contains("Keywords: Rust, compiler"));
    assert!(
        !prompts[0].contains("- Waymo"),
        "a selector site is not suggested"
    );

    let usage = llm_usage::recent(&db.pool, None, 10).await.expect("recent");
    assert_eq!(usage.len(), 1);
    assert_eq!(usage[0].task, "suggest_portals");
    assert_eq!(usage[0].outcome, "ok");
    assert_eq!(usage[0].job_title, None, "a suggestion covers no posting");

    db.close().await;
}

#[tokio::test]
async fn an_eightfold_board_with_no_registry_entry_stores_the_domain_config() {
    let db = seeded().await;
    let backend = ScriptedBackend::with_reply(reply(&[(
        "Example Corp",
        "eightfold",
        "example.com",
        "Runs an Eightfold board.",
    )]));

    let report = suggest_portals(&db.pool, &CancelToken::default(), &backend)
        .await
        .expect("suggestions");

    let suggestion = &report.suggestions[0];
    assert_eq!(suggestion.url, "https://jobs.example.com/careers");
    assert_eq!(
        suggestion.config.as_deref(),
        Some(r#"{"provider":"eightfold","domain":"example.com"}"#)
    );

    db.close().await;
}

#[tokio::test]
async fn an_unknown_engine_is_rejected() {
    let db = seeded().await;
    let backend = ScriptedBackend::with_reply(reply(&[("Acme", "workday", "acme", "Fits.")]));

    let error = suggest_portals(&db.pool, &CancelToken::default(), &backend)
        .await
        .expect_err("unknown engine");
    assert!(matches!(error, SuggestError::InvalidResponse(_)), "{error}");

    // The call itself succeeded, so its tokens are still recorded.
    let usage = llm_usage::recent(&db.pool, None, 10).await.expect("recent");
    assert_eq!(usage.len(), 1);
    assert_eq!(usage[0].task, "suggest_portals");

    db.close().await;
}

#[tokio::test]
async fn an_unsafe_slug_is_rejected() {
    let db = seeded().await;
    let backend =
        ScriptedBackend::with_reply(reply(&[("Acme", "greenhouse", "../acme/jobs", "Fits.")]));

    let error = suggest_portals(&db.pool, &CancelToken::default(), &backend)
        .await
        .expect_err("unsafe slug");
    assert!(matches!(error, SuggestError::InvalidResponse(_)), "{error}");

    db.close().await;
}

#[tokio::test]
async fn more_than_the_ceiling_is_rejected() {
    let db = seeded().await;
    let rows: Vec<String> = (0..=MAX_SUGGESTIONS)
        .map(|index| {
            format!(
                r#"{{"company":"Acme {index}","engine":"greenhouse","slug":"acme{index}","rationale":"Fits."}}"#
            )
        })
        .collect();
    let backend =
        ScriptedBackend::with_reply(format!(r#"{{"suggestions":[{}]}}"#, rows.join(", ")));

    let error = suggest_portals(&db.pool, &CancelToken::default(), &backend)
        .await
        .expect_err("too many");
    assert!(matches!(error, SuggestError::InvalidResponse(_)), "{error}");

    db.close().await;
}

#[tokio::test]
async fn a_cancelled_call_is_recorded_and_returns_nothing() {
    let db = seeded().await;
    let backend = ScriptedBackend::with_reply(reply(&[("Figma", "greenhouse", "figma", "Fits.")]));

    let flag = Arc::new(AtomicBool::new(true));
    let cancel = CancelToken::new(flag);

    let error = suggest_portals(&db.pool, &cancel, &backend)
        .await
        .expect_err("cancelled");
    assert!(
        matches!(error, SuggestError::Llm(LlmError::Cancelled)),
        "{error}"
    );
    assert!(
        backend.prompts().is_empty(),
        "the call never reached the model"
    );

    let usage = llm_usage::recent(&db.pool, None, 10).await.expect("recent");
    assert_eq!(usage.len(), 1, "a cancelled call still spent its input");
    assert_eq!(usage[0].outcome, "cancelled");

    db.close().await;
}

#[tokio::test]
async fn without_a_cv_the_call_is_not_made() {
    let db = common::TestDb::new_raw().await;
    // Prompts are installed, but the profile has no summary.
    seed_or_update_system_prompts(&db.pool)
        .await
        .expect("seed prompts");

    let backend = ScriptedBackend::with_reply("{}");
    let error = suggest_portals(&db.pool, &CancelToken::default(), &backend)
        .await
        .expect_err("no CV");
    assert!(error.to_string().contains("CV"), "{error}");
    assert!(
        backend.prompts().is_empty(),
        "the call never reached the model"
    );

    let usage = llm_usage::recent(&db.pool, None, 10).await.expect("recent");
    assert!(usage.is_empty(), "a local failure spends nothing");

    db.close().await;
}
