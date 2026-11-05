//! Application status transitions that Phase 2.5 introduced.
//!
//! Phase 2 never created an application row in production, so `mark_not_a_match`
//! is the first writer. These tests cover creating it, the idempotent path, and
//! the guard that keeps a submitted application from being re-labelled.

mod common;

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
