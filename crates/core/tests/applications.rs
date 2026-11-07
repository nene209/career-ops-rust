//! Application status transitions that Phase 2.5 introduced.
//!
//! Phase 2 never created an application row in production, so `mark_not_a_match`
//! is the first writer. These tests cover creating it, the idempotent path, and
//! the guard that keeps a submitted application from being re-labelled.

mod common;

use std::collections::HashSet;

use career_ops_core::model::{ApplicationStatus, FetchTier};
use career_ops_core::repo::applications;
use career_ops_core::repo::job_descriptions::{self, NewJobDescription};
use common::TestDb;

fn job(url: &str) -> NewJobDescription {
    NewJobDescription {
        title: "Engineer".into(),
        company: "Acme".into(),
        source_url: url.into(),
        raw_text: "body".into(),
        fetch_tier: FetchTier::Tier1bHtml,
        external_id: None,
    }
}

#[tokio::test]
async fn mark_not_a_match_creates_the_application_when_absent() {
    let db = TestDb::new_raw().await;
    let stored = job_descriptions::upsert(&db.pool, job("https://example.com/jobs/1"))
        .await
        .expect("job");

    assert!(applications::list(&db.pool).await.expect("list").is_empty());

    let application = applications::mark_not_a_match(&db.pool, stored.id)
        .await
        .expect("mark");

    assert_eq!(application.status, "not_a_match");
    assert_eq!(application.job_description_id, stored.id);
    assert_eq!(applications::list(&db.pool).await.expect("list").len(), 1);

    db.close().await;
}

#[tokio::test]
async fn mark_not_a_match_is_idempotent() {
    let db = TestDb::new_raw().await;
    let stored = job_descriptions::upsert(&db.pool, job("https://example.com/jobs/2"))
        .await
        .expect("job");

    let first = applications::mark_not_a_match(&db.pool, stored.id)
        .await
        .expect("first");
    let second = applications::mark_not_a_match(&db.pool, stored.id)
        .await
        .expect("second");

    assert_eq!(first.id, second.id);
    assert_eq!(second.status, ApplicationStatus::NotAMatch.as_str());
    assert_eq!(applications::list(&db.pool).await.expect("list").len(), 1);

    db.close().await;
}

#[tokio::test]
async fn a_submitted_application_cannot_be_marked_not_a_match() {
    let db = TestDb::new_raw().await;
    let stored = job_descriptions::upsert(&db.pool, job("https://example.com/jobs/3"))
        .await
        .expect("job");

    let application = applications::create(&db.pool, stored.id)
        .await
        .expect("create");
    applications::transition(&db.pool, application.id, ApplicationStatus::Ready)
        .await
        .expect("ready");
    applications::mark_applied(&db.pool, application.id)
        .await
        .expect("applied");

    let error = applications::mark_not_a_match(&db.pool, stored.id)
        .await
        .expect_err("a submitted application is not a mis-filed posting");

    assert!(error.to_string().contains("invalid transition"), "{error}");

    db.close().await;
}

#[tokio::test]
async fn deleting_a_job_removes_its_application() {
    let db = TestDb::new_raw().await;
    let stored = job_descriptions::upsert(&db.pool, job("https://example.com/jobs/4"))
        .await
        .expect("job");
    applications::mark_not_a_match(&db.pool, stored.id)
        .await
        .expect("mark");

    job_descriptions::delete(&db.pool, stored.id)
        .await
        .expect("delete");

    assert!(
        job_descriptions::get(&db.pool, stored.id)
            .await
            .expect("get")
            .is_none(),
        "the posting is gone"
    );
    assert!(
        applications::list(&db.pool).await.expect("list").is_empty(),
        "the application cascades"
    );

    db.close().await;
}

/// `user_touched_ids` is what decides whether the Pipeline asks for the typed
/// confirmation. A row that only exists because it was fetched must not count.
#[tokio::test]
async fn user_touched_ids_lists_only_rows_the_user_worked_on() {
    let db = TestDb::new_raw().await;
    let fetched = job_descriptions::upsert(&db.pool, job("https://example.com/jobs/5"))
        .await
        .expect("fetched job");
    let marked = job_descriptions::upsert(&db.pool, job("https://example.com/jobs/6"))
        .await
        .expect("marked job");

    // A discovered application on its own is not user state.
    applications::create(&db.pool, fetched.id)
        .await
        .expect("create");
    applications::mark_not_a_match(&db.pool, marked.id)
        .await
        .expect("mark");

    let touched = job_descriptions::user_touched_ids(&db.pool)
        .await
        .expect("user touched ids");

    assert_eq!(touched, HashSet::from([marked.id]));
    assert!(!touched.contains(&fetched.id));

    db.close().await;
}
