// crates/core/tests/scraper_config_roundtrip.rs
mod common;

use career_ops_core::acquisition::html::selectors::SelectorConfig;
use career_ops_core::repo::portals::{self, NewPortal, RenderMode};

#[tokio::test]
async fn scraper_config_round_trips_as_selector_config() {
    let db = common::TestDb::new_raw().await;

    let config = serde_json::json!({
        "list": "article.job-card",
        "fields": { "title": "h2.title", "location": ".location" }
    });

    let portal = portals::create(
        &db.pool,
        NewPortal {
            company_name: "Acme".into(),
            url: "https://example.com/careers".into(),
            render_mode: RenderMode::Static,
            scraper_config: Some(config.clone()),
        },
    )
    .await
    .expect("create");

    let fetched = portals::get(&db.pool, portal.id)
        .await
        .expect("get")
        .expect("exists");

    let stored_json = fetched.scraper_config.expect("config stored");
    let value: serde_json::Value =
        serde_json::from_str(&stored_json).expect("stored config is valid JSON");
    let selector_config: SelectorConfig =
        serde_json::from_value(value).expect("stored config deserializes to SelectorConfig");

    assert_eq!(selector_config.list, "article.job-card");
    assert_eq!(selector_config.fields.title, "h2.title");
    assert_eq!(
        selector_config.fields.location.as_deref(),
        Some(".location")
    );
    // Default field applied even though not present in the JSON
    assert_eq!(selector_config.url_attribute, "href");

    db.close().await;
}
