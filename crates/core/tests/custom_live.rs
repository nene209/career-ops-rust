//! Live local-endpoint smoke test (Phase 3.5).
//!
//! Ignored by default: see `docs/LIVE_TESTS.md`. The offline test in
//! `custom_endpoint.rs` proves the request shape against a fake server; this
//! one proves a real Ollama, `llama-server`, vLLM, or gateway answers it.
//!
//! ```text
//! CAREER_OPS_LIVE=1 \
//!   CAREER_OPS_LOCAL_LLM_URL=http://localhost:11434/v1 \
//!   CAREER_OPS_LOCAL_LLM_MODEL=llama3.2 \
//!   cargo test -p career-ops-core --test custom_live -- --ignored --nocapture
//! ```
//!
//! The model is required rather than defaulted: model names are
//! engine-specific, so a guessed one fails with a 404 that looks like a bug.
//! `CAREER_OPS_LOCAL_LLM_KEY` is optional, matching the provider.

use career_ops_core::llm::{LlmConfig, LlmProvider};
use zeroize::Zeroizing;

fn required(name: &str) -> Option<String> {
    match std::env::var(name) {
        Ok(value) if !value.trim().is_empty() => Some(value.trim().to_string()),
        _ => None,
    }
}

#[tokio::test]
#[ignore = "live; set CAREER_OPS_LIVE=1, CAREER_OPS_LOCAL_LLM_URL and _MODEL, and run with --ignored"]
async fn custom_endpoint_answers_a_prompt() {
    if !career_ops_test_support::require_live("custom_endpoint_answers_a_prompt") {
        return;
    }
    let (Some(base_url), Some(model)) = (
        required("CAREER_OPS_LOCAL_LLM_URL"),
        required("CAREER_OPS_LOCAL_LLM_MODEL"),
    ) else {
        eprintln!(
            "skipping custom_endpoint_answers_a_prompt: CAREER_OPS_LOCAL_LLM_URL and \
             CAREER_OPS_LOCAL_LLM_MODEL are not both set"
        );
        return;
    };
    let key = required("CAREER_OPS_LOCAL_LLM_KEY").unwrap_or_default();

    let config = LlmConfig {
        provider: LlmProvider::Custom,
        model,
        base_url,
        // A plain chat completion is what this test is for; JSON mode would
        // conflate "the server answers" with "the server takes
        // `response_format`".
        json_mode: false,
        ..LlmConfig::default()
    };
    let client = config
        .build_client(&Zeroizing::new(key))
        .expect("build the local client");

    // `/models` is optional on a local server, so a miss is reported and the
    // run continues — exactly what the Settings button tells the user.
    match client.verify().await {
        Ok(()) => eprintln!("local endpoint: /models answered"),
        Err(error) => eprintln!("local endpoint: /models did not answer ({error})"),
    }

    let completion = client
        .complete_text("Reply with the single word: pong", None, false, None)
        .await
        .expect("the local endpoint did not answer the prompt");

    eprintln!("local endpoint replied: {:?}", completion.text);
    assert!(
        !completion.text.trim().is_empty(),
        "an empty reply is not a pass"
    );

    eprintln!(
        "usage: input {:?}, output {:?}, total {:?}",
        completion.usage.input_tokens,
        completion.usage.output_tokens,
        completion.usage.total_tokens
    );
    if completion.usage.input_tokens.is_none() {
        eprintln!("note: no counters were reported; the ledger will estimate them");
    }
    assert_eq!(
        completion.usage.cost_usd, None,
        "a self-hosted call is never priced by the catalog"
    );
}
