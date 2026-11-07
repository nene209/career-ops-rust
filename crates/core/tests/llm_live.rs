//! Live DeepSeek smoke test (Phase 3 workstream 1).
//!
//! Ignored by default: see `docs/LIVE_TESTS.md`. It proves the key is accepted
//! and the endpoint is reachable. `verify()` asks DeepSeek for the account
//! balance, so a pass spends no tokens and produces no completion.

use career_ops_core::llm::LlmConfig;
use zeroize::Zeroizing;

#[tokio::test]
#[ignore = "hits the network; set DEEPSEEK_API_KEY and run with --ignored"]
async fn deepseek_accepts_the_configured_key() {
    let key = match std::env::var("DEEPSEEK_API_KEY") {
        Ok(key) if !key.trim().is_empty() => key,
        _ => {
            // A skipped run must not look like a failure: the gate is the
            // environment, and `--ignored` may be used for every live test.
            eprintln!("skipping deepseek_accepts_the_configured_key: DEEPSEEK_API_KEY is not set");
            return;
        }
    };

    let client = LlmConfig::default()
        .build_client(&Zeroizing::new(key))
        .expect("build the DeepSeek client");

    client
        .verify()
        .await
        .expect("DeepSeek did not accept the key");
}
