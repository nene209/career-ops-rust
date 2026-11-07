//! Preferences round-trip through `profile.preferences`.
//!
//! The column is the storage contract between the Settings screen and the
//! Pipeline, so these tests go through the repository rather than through the
//! view model: JSON shape, `NULL`, and a malformed value are all here.

mod common;

use career_ops_core::preferences::{PreferenceFacts, Preferences};
use career_ops_core::repo::profile;

fn preferences() -> Preferences {
    Preferences {
        preferred_companies: vec!["Anthropic".into(), "Figma".into()],
        preferred_locations: vec!["Remote".into()],
        keywords: vec!["compiler".into(), "Rust".into()],
        excluded_companies: vec!["RecruiterCo".into()],
        seniority: Some("staff".into()),
    }
}

#[tokio::test]
async fn preferences_round_trip_through_the_profile_row() {
    let db = common::TestDb::new_raw().await;

    // A vault that has never opened Settings has no preferences.
    assert_eq!(
        profile::preferences(&db.pool).await.unwrap(),
        Preferences::default()
    );

    let saved = preferences();
    let row = profile::update_preferences(&db.pool, &saved).await.unwrap();
    assert!(
        row.preferences.is_some(),
        "the column holds the serialized object"
    );

    assert_eq!(profile::preferences(&db.pool).await.unwrap(), saved);
    // Preferences are acquisition settings, not candidate data: they must not
    // leak into the summary Phase 3 sends as `{{cv_summary}}`.
    assert!(profile::cv_summary(&db.pool)
        .await
        .unwrap()
        .is_none_or(|summary| !summary.contains("Rust")));

    let hit = saved.matches(PreferenceFacts::new(
        "Anthropic",
        Some("Remote"),
        "Compiler Engineer",
    ));
    assert!(hit.preferred);
    assert!(!hit.excluded);

    db.close().await;
}

#[tokio::test]
async fn saving_an_empty_object_clears_every_list() {
    let db = common::TestDb::new_raw().await;
    let saved = Preferences {
        keywords: vec!["rust".into()],
        ..Default::default()
    };
    profile::update_preferences(&db.pool, &saved).await.unwrap();

    profile::update_preferences(&db.pool, &Preferences::default())
        .await
        .unwrap();

    let stored = profile::preferences(&db.pool).await.unwrap();
    assert!(stored.is_empty());
    assert_eq!(stored.keywords, Vec::<String>::new());
    db.close().await;
}

#[tokio::test]
async fn a_malformed_stored_value_reads_as_empty_and_is_left_alone() {
    let db = common::TestDb::new_raw().await;
    profile::ensure_singleton(&db.pool).await.unwrap();
    sqlx::query("UPDATE profile SET preferences = ? WHERE id = 1")
        .bind("{not json")
        .execute(&db.pool)
        .await
        .unwrap();

    assert_eq!(
        profile::preferences(&db.pool).await.unwrap(),
        Preferences::default(),
        "a broken value must not make the Pipeline unopenable"
    );

    // Reading is not a repair: the user's text stays until they save.
    let row = profile::get(&db.pool).await.unwrap().unwrap();
    assert_eq!(row.preferences.as_deref(), Some("{not json"));
    db.close().await;
}
