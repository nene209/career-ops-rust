//! LLM configuration, keychain storage, and readiness.
//!
//! No network: building a client is offline, `verify()` is the only network
//! call and is exercised by the environment-gated test in `llm_live.rs`
//! (`docs/LIVE_TESTS.md`).

mod common;

use career_ops_core::llm::{self, LlmConfig, LlmProvider, StructuredOutput};

#[tokio::test]
async fn the_llm_config_round_trips_through_settings() {
    let db = common::TestDb::new_raw().await;

    // A vault that has never opened the AI section uses the documented
    // defaults: DeepSeek, `deepseek-chat`.
    assert_eq!(
        LlmConfig::load(&db.pool).await.unwrap(),
        LlmConfig::default()
    );

    let saved = LlmConfig {
        provider: LlmProvider::DeepSeek,
        model: "deepseek-reasoner".into(),
        temperature: 0.7,
        max_tokens: Some(2_048),
        max_job_chars: 9_000,
    };
    let stored = saved.save(&db.pool).await.unwrap();
    assert_eq!(stored, saved);

    assert_eq!(LlmConfig::load(&db.pool).await.unwrap(), saved);

    db.close().await;
}

#[tokio::test]
async fn each_setting_falls_back_on_its_own() {
    let db = common::TestDb::new_raw().await;

    // A provider this build does not know, and a temperature that is not even
    // a number. Both must read as defaults without discarding the model.
    sqlx::query("INSERT INTO settings (key, value) VALUES ('llm.provider', '\"bedrock\"')")
        .execute(&db.pool)
        .await
        .expect("insert provider");
    sqlx::query("INSERT INTO settings (key, value) VALUES ('llm.temperature', '\"hot\"')")
        .execute(&db.pool)
        .await
        .expect("insert temperature");
    sqlx::query("INSERT INTO settings (key, value) VALUES ('llm.model', '\"deepseek-v4-pro\"')")
        .execute(&db.pool)
        .await
        .expect("insert model");

    let config = LlmConfig::load(&db.pool).await.unwrap();
    assert_eq!(config.provider, LlmProvider::DeepSeek, "unknown provider");
    assert_eq!(config.temperature, LlmConfig::default().temperature);
    assert_eq!(config.model, "deepseek-v4-pro", "a good key survives");

    db.close().await;
}

#[tokio::test]
async fn saving_normalizes_an_empty_model() {
    let db = common::TestDb::new_raw().await;

    let saved = LlmConfig {
        model: "   ".into(),
        temperature: 5.0,
        ..LlmConfig::default()
    }
    .save(&db.pool)
    .await
    .unwrap();

    assert_eq!(saved.model, "deepseek-chat");
    assert_eq!(saved.temperature, 2.0);
    assert_eq!(LlmConfig::load(&db.pool).await.unwrap(), saved);

    db.close().await;
}

#[cfg(feature = "keychain")]
#[tokio::test]
async fn an_api_key_round_trips_through_the_keychain() {
    common::setup_keyring_mock();
    let provider = LlmProvider::DeepSeek;

    llm::keys::delete_api_key(provider).expect("clean slate");
    assert!(!llm::keys::has_api_key(provider).expect("has"));

    llm::keys::set_api_key(provider, "  sk-test-deepseek  ").expect("set");
    assert!(llm::keys::has_api_key(provider).expect("has"));

    let key = llm::keys::get_api_key(provider)
        .expect("get")
        .expect("a key is stored");
    assert_eq!(key.as_str(), "sk-test-deepseek", "the key is trimmed");

    llm::keys::delete_api_key(provider).expect("delete");
    assert!(!llm::keys::has_api_key(provider).expect("has"));
}

#[cfg(feature = "keychain")]
#[tokio::test]
async fn an_empty_key_is_rejected() {
    common::setup_keyring_mock();
    let result = llm::keys::set_api_key(LlmProvider::OpenAI, "   ");
    assert!(result.is_err(), "a blank key must not be stored");
}

#[cfg(feature = "keychain")]
#[tokio::test]
async fn status_reports_the_first_unmet_condition() {
    common::setup_keyring_mock();
    let db = common::TestDb::new_raw().await;
    let provider = LlmProvider::DeepSeek;
    llm::keys::delete_api_key(provider).expect("clean slate");

    let blocked = llm::status(&db.pool).await.expect("status");
    assert!(!blocked.ready());
    assert_eq!(
        blocked.blocked_reason().as_deref(),
        Some("Add a DeepSeek API key in Settings.")
    );
    assert_eq!(
        blocked.capabilities.structured_output,
        StructuredOutput::JsonObject
    );

    llm::keys::set_api_key(provider, "sk-test-deepseek").expect("set");
    let ready = llm::status(&db.pool).await.expect("status");
    assert!(ready.ready());
    assert!(ready.blocked_reason().is_none());

    // A reasoning model is stored but cannot return structured output.
    LlmConfig {
        model: "deepseek-reasoner".into(),
        ..LlmConfig::default()
    }
    .save(&db.pool)
    .await
    .expect("save");
    let blocked = llm::status(&db.pool).await.expect("status");
    assert!(!blocked.ready());
    assert!(blocked
        .blocked_reason()
        .is_some_and(|reason| reason.contains("structured output")));

    llm::keys::delete_api_key(provider).expect("cleanup");
    db.close().await;
}
