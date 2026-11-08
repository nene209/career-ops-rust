//! Live portal-suggestion check (Phase 3.2, WS4).
//!
//! Ignored by default and gated on `DEEPSEEK_API_KEY`; see
//! `docs/LIVE_TESTS.md`. It spends tokens, so it runs on demand.
//!
//! What the live run proves that the offline tests cannot: the prompt still
//! elicits the JSON contract from the real model, and every proposal's URL is
//! derived from the engine's own host — a reply can never point the app at an
//! arbitrary site.

mod common;

use career_ops_core::llm::{CancelToken, LlmClient, LlmConfig};
use career_ops_core::suggest::{suggest_portals, MAX_SUGGESTIONS};
use common::phase3;
use url::Url;
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

#[tokio::test]
#[ignore = "hits the network and spends tokens; set DEEPSEEK_API_KEY and run with --ignored"]
async fn deepseek_proposes_real_boards() {
    let Some(key) = api_key() else {
        eprintln!("skipping deepseek_proposes_real_boards: DEEPSEEK_API_KEY is not set");
        return;
    };

    let db = common::TestDb::new_raw().await;
    phase3::seed(&db).await;
    let client = build_client(&key);

    let report = suggest_portals(&db.pool, &CancelToken::default(), &client)
        .await
        .expect("suggest portals");

    eprintln!(
        "{} suggestions, about {:?} (model {})",
        report.suggestions.len(),
        report.actual_cost_usd,
        report.model_used
    );
    for suggestion in &report.suggestions {
        eprintln!(
            "{} | {} | {} | {}",
            suggestion.company, suggestion.engine_label, suggestion.url, suggestion.rationale
        );
    }

    assert!(!report.suggestions.is_empty());
    assert!(report.suggestions.len() <= MAX_SUGGESTIONS);

    for suggestion in &report.suggestions {
        assert!(!suggestion.company.trim().is_empty());
        assert!(!suggestion.rationale.trim().is_empty());

        // The URL is ours, not the model's: it must sit on the engine's host.
        let url = Url::parse(&suggestion.url).expect("the derived URL parses");
        assert!(matches!(url.scheme(), "http" | "https"));
        let host = url.host_str().expect("the derived URL has a host");
        let expected = match suggestion.engine_label.as_str() {
            "Greenhouse" => host == "job-boards.greenhouse.io",
            "Lever" => host == "jobs.lever.co",
            "Ashby" => host == "jobs.ashbyhq.com",
            "SmartRecruiters" => host == "jobs.smartrecruiters.com",
            "Eightfold" => host.starts_with("jobs."),
            other => panic!("unexpected engine {other}"),
        };
        assert!(
            expected,
            "{} was derived on an unexpected host: {}",
            suggestion.company, suggestion.url
        );
    }

    db.close().await;
}
