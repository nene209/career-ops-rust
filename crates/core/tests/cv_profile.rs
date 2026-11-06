//! Profile and CV-entry repository integration tests.
//!
//! Phase 2.7 adds the editor and the canonical `cv_summary`. These tests cover
//! the paths the editor uses — the singleton, the full profile update, the
//! existing CV CRUD/reorder functions, and the rendered summary — against a
//! real encrypted database.

mod common;

use career_ops_core::repo::cv_entries::{self, CvEntryKind, NewCvEntry};
use career_ops_core::repo::profile::{self, ProfileDetails};
use common::TestDb;

fn details() -> ProfileDetails {
    ProfileDetails {
        name: "Ada Lovelace".into(),
        email: "ada@example.com".into(),
        phone: Some("+44 20 0000 0000".into()),
        location: Some("London".into()),
        summary: Some("Mathematician and first programmer.".into()),
    }
}

fn entry(kind: CvEntryKind, title: &str, sort_order: Option<i64>) -> NewCvEntry {
    NewCvEntry {
        profile_id: 1,
        kind,
        title: title.into(),
        organization: None,
        start_date: None,
        end_date: None,
        description: None,
        metadata: None,
        sort_order,
    }
}

#[tokio::test]
async fn ensure_singleton_is_idempotent() {
    let db = TestDb::new_raw().await;

    assert!(profile::get(&db.pool).await.expect("get").is_none());

    let first = profile::ensure_singleton(&db.pool)
        .await
        .expect("first ensure");
    let second = profile::ensure_singleton(&db.pool)
        .await
        .expect("second ensure");

    assert_eq!(first.id, 1);
    assert_eq!(second.id, 1);
    assert!(second.name.is_empty());
    assert!(second.summary.is_none());

    db.close().await;
}

#[tokio::test]
async fn profile_details_round_trip_and_render() {
    let db = TestDb::new_raw().await;

    let updated = profile::update_details(&db.pool, &details())
        .await
        .expect("update");

    assert_eq!(updated.name, "Ada Lovelace");
    assert_eq!(updated.email, "ada@example.com");
    assert_eq!(updated.phone.as_deref(), Some("+44 20 0000 0000"));
    assert_eq!(updated.location.as_deref(), Some("London"));
    assert_eq!(
        updated.summary.as_deref(),
        Some("Mathematician and first programmer.")
    );

    let mut experience = entry(CvEntryKind::Experience, "Engineer", None);
    experience.organization = Some("Acme".into());
    experience.start_date = Some("2020".into());
    experience.end_date = Some("Present".into());
    experience.description = Some("Built things.".into());
    cv_entries::create(&db.pool, experience)
        .await
        .expect("entry");

    let summary = profile::cv_summary(&db.pool)
        .await
        .expect("summary")
        .expect("a populated profile has a summary");

    assert!(
        summary.starts_with("Ada Lovelace\nLondon | ada@example.com"),
        "{summary}"
    );
    assert!(summary.contains("Summary:\nMathematician and first programmer."));
    assert!(summary.contains("- Engineer — Acme (2020 - Present)\n  Built things."));

    db.close().await;
}

#[tokio::test]
async fn blank_optional_fields_are_stored_as_null() {
    let db = TestDb::new_raw().await;

    let updated = profile::update_details(
        &db.pool,
        &ProfileDetails {
            name: "  Ada  ".into(),
            email: "  ada@example.com  ".into(),
            phone: Some("   ".into()),
            location: Some(String::new()),
            summary: Some("  A summary.  ".into()),
        },
    )
    .await
    .expect("update");

    assert_eq!(updated.name, "Ada");
    assert_eq!(updated.email, "ada@example.com");
    assert!(updated.phone.is_none());
    assert!(updated.location.is_none());
    assert_eq!(updated.summary.as_deref(), Some("A summary."));

    db.close().await;
}

#[tokio::test]
async fn cv_entries_create_update_reorder_and_delete() {
    let db = TestDb::new_raw().await;
    profile::ensure_singleton(&db.pool).await.expect("profile");

    let a = cv_entries::create(&db.pool, entry(CvEntryKind::Experience, "A", None))
        .await
        .expect("a");
    let b = cv_entries::create(&db.pool, entry(CvEntryKind::Experience, "B", None))
        .await
        .expect("b");
    let c = cv_entries::create(&db.pool, entry(CvEntryKind::Experience, "C", None))
        .await
        .expect("c");

    let titles = |entries: Vec<career_ops_core::model::CvEntry>| {
        entries
            .into_iter()
            .map(|entry| entry.title)
            .collect::<Vec<_>>()
    };

    assert_eq!(
        titles(
            cv_entries::list_for_profile(&db.pool, 1)
                .await
                .expect("list")
        ),
        vec!["A", "B", "C"]
    );

    let mut renamed = entry(CvEntryKind::Experience, "B2", None);
    renamed.organization = Some("Acme".into());
    let updated = cv_entries::update(&db.pool, b.id, renamed)
        .await
        .expect("update");
    assert_eq!(updated.title, "B2");
    assert_eq!(updated.organization.as_deref(), Some("Acme"));

    cv_entries::reorder(&db.pool, 1, &[c.id, a.id, b.id])
        .await
        .expect("reorder");
    assert_eq!(
        titles(
            cv_entries::list_for_profile(&db.pool, 1)
                .await
                .expect("list")
        ),
        vec!["C", "A", "B2"]
    );

    cv_entries::delete(&db.pool, a.id).await.expect("delete");
    assert_eq!(
        titles(
            cv_entries::list_for_profile(&db.pool, 1)
                .await
                .expect("list")
        ),
        vec!["C", "B2"]
    );

    db.close().await;
}

#[tokio::test]
async fn cv_summary_groups_by_kind_in_a_fixed_order() {
    let db = TestDb::new_raw().await;
    profile::ensure_singleton(&db.pool).await.expect("profile");

    // Creation order deliberately differs from the section order.
    cv_entries::create(&db.pool, entry(CvEntryKind::Skill, "Rust", Some(0)))
        .await
        .expect("skill");
    cv_entries::create(&db.pool, entry(CvEntryKind::Education, "BSc", Some(1)))
        .await
        .expect("education");
    cv_entries::create(
        &db.pool,
        entry(CvEntryKind::Experience, "Engineer", Some(2)),
    )
    .await
    .expect("experience");

    let summary = profile::cv_summary(&db.pool)
        .await
        .expect("summary")
        .expect("entries count as candidate data");

    let headings: Vec<&str> = summary.lines().filter(|line| line.ends_with(':')).collect();
    assert_eq!(headings, vec!["Experience:", "Education:", "Skills:"]);

    db.close().await;
}

#[tokio::test]
async fn cv_summary_is_none_for_an_empty_profile() {
    let db = TestDb::new_raw().await;

    assert_eq!(
        profile::cv_summary(&db.pool).await.expect("summary"),
        None,
        "an empty profile must not produce an empty prompt section"
    );

    // The call also creates the singleton as a side effect.
    assert!(profile::get(&db.pool).await.expect("get").is_some());

    db.close().await;
}
