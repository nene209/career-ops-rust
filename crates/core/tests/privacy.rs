//! Redaction at the storage boundary: the toggle and the stored profile.
//!
//! The matching rules are unit-tested in `core::privacy`. These tests prove the
//! two things a unit test cannot: the setting's defaults and round trip against
//! a real `settings` table, and `redact_cv` reading the identity the app
//! actually stores.

mod common;

use career_ops_core::privacy::{self, PrivacyConfig, RedactionReport};
use career_ops_core::repo::profile::{self, ProfileDetails};

fn ada() -> ProfileDetails {
    ProfileDetails {
        name: "Ada Lovelace".into(),
        email: "ada@example.com".into(),
        phone: Some("+44 20 0000 0000".into()),
        location: Some("London".into()),
        summary: Some("Mathematician and first programmer.".into()),
    }
}

#[tokio::test]
async fn the_setting_defaults_to_on_and_round_trips() {
    let db = common::TestDb::new_raw().await;

    assert_eq!(
        PrivacyConfig::load(&db.pool).await.expect("load"),
        PrivacyConfig::default(),
        "a vault that never opened Settings redacts"
    );
    assert!(PrivacyConfig::default().redact_cv);

    PrivacyConfig { redact_cv: false }
        .save(&db.pool)
        .await
        .expect("save");
    assert_eq!(
        PrivacyConfig::load(&db.pool).await.expect("load"),
        PrivacyConfig { redact_cv: false }
    );

    db.close().await;
}

#[tokio::test]
async fn a_malformed_setting_still_redacts() {
    let db = common::TestDb::new_raw().await;

    sqlx::query("INSERT INTO settings (key, value) VALUES ('privacy.redact_cv', '\"yes\"')")
        .execute(&db.pool)
        .await
        .expect("insert");

    assert!(
        PrivacyConfig::load(&db.pool).await.expect("load").redact_cv,
        "a hand-edited value must not turn privacy off by accident"
    );

    db.close().await;
}

#[tokio::test]
async fn redact_cv_uses_the_stored_profile_and_the_toggle() {
    let db = common::TestDb::new_raw().await;
    profile::update_details(&db.pool, &ada())
        .await
        .expect("profile");

    let summary = profile::cv_summary(&db.pool)
        .await
        .expect("summary")
        .expect("a configured profile renders");

    let redacted = privacy::redact_cv(&db.pool, &summary)
        .await
        .expect("redact");
    assert!(!redacted.text.contains("Ada Lovelace"), "{}", redacted.text);
    assert!(!redacted.text.contains("ada@example.com"));
    assert!(
        redacted.text.contains("London"),
        "a city passes through: {}",
        redacted.text
    );
    assert_eq!(redacted.report.emails, 1);
    assert_eq!(redacted.report.phones, 1);
    assert_eq!(redacted.restore(&redacted.text), summary);

    // The toggle is honored on the same path, not only in the pure function.
    PrivacyConfig { redact_cv: false }
        .save(&db.pool)
        .await
        .expect("save");
    let passthrough = privacy::redact_cv(&db.pool, &summary)
        .await
        .expect("redact");
    assert_eq!(passthrough.text, summary);
    assert_eq!(passthrough.report, RedactionReport::default());

    db.close().await;
}

#[tokio::test]
async fn an_address_like_location_is_redacted_from_a_real_summary() {
    let db = common::TestDb::new_raw().await;
    profile::update_details(
        &db.pool,
        &ProfileDetails {
            location: Some("123 Main St, Springfield".into()),
            ..ada()
        },
    )
    .await
    .expect("profile");

    let summary = profile::cv_summary(&db.pool)
        .await
        .expect("summary")
        .expect("renders");

    let redacted = privacy::redact_cv(&db.pool, &summary)
        .await
        .expect("redact");
    assert!(
        !redacted.text.contains("123 Main St"),
        "a street address is personal data: {}",
        redacted.text
    );
    assert!(redacted.text.contains("⟦LOCATION⟧"));

    db.close().await;
}
