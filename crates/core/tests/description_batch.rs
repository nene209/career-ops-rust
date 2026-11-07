//! The bounded description batch (Phase 2.9 WS4).
//!
//! The batch is `refresh_job` in a loop with three guards: a per-click ceiling,
//! a cancel flag checked between postings, and a queue transition only for the
//! postings that actually produced text.

mod common;

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use career_ops_core::acquisition::manual::{self, DESCRIPTION_BATCH_LIMIT};
use career_ops_core::model::{FetchTier, QueueState};
use career_ops_core::repo::job_descriptions as job_repo;
use career_ops_core::repo::job_descriptions::NewJobDescription;
use career_ops_core::repo::portals::{self, NewPortal, RenderMode};
use sqlx::SqlitePool;

use common::MockFetchSource;

/// The detail selector the batch needs. `refresh_job` resolves a portal by host,
/// so the source alone is not enough: without this the extraction has no
/// selector and every posting fails.
const DETAIL_CONFIG: &str = r#"{"list":"article.job","fields":{"title":"h3 a"},"detail":{"fields":{"description":".job-description"},"max_jobs":3}}"#;

fn detail_page(text: &str) -> String {
    format!("<html><body><div class=\"job-description\">{text}</div></body></html>")
}

async fn add_portal(pool: &SqlitePool) {
    portals::create(
        pool,
        NewPortal {
            company_name: "Acme".into(),
            url: "https://example.com/careers".into(),
            render_mode: RenderMode::Static,
            scraper_config: Some(serde_json::from_str(DETAIL_CONFIG).expect("detail config")),
        },
    )
    .await
    .expect("insert portal");
}

/// A stored posting with no description, flagged for details.
async fn add_flagged_job(pool: &SqlitePool, index: usize) -> i64 {
    let job = job_repo::upsert(
        pool,
        NewJobDescription {
            title: format!("Engineer {index}"),
            company: "Acme".into(),
            source_url: format!("https://example.com/jobs/{index}"),
            raw_text: String::new(),
            fetch_tier: FetchTier::Tier1bHtml,
            external_id: None,
        },
    )
    .await
    .expect("store");

    job_repo::set_queue_state(pool, job.id, QueueState::DetailsNeeded, None)
        .await
        .expect("flag for details");

    job.id
}

async fn queue_state(pool: &SqlitePool, id: i64) -> String {
    job_repo::get(pool, id)
        .await
        .expect("get")
        .expect("row")
        .queue_state
}

#[tokio::test]
async fn a_successful_batch_stores_text_and_advances_the_queue() {
    let db = common::TestDb::new_raw().await;
    add_portal(&db.pool).await;
    let first = add_flagged_job(&db.pool, 0).await;
    let second = add_flagged_job(&db.pool, 1).await;

    let source = MockFetchSource::default().pages([
        Ok(detail_page("First body.")),
        Ok(detail_page("Second body.")),
    ]);
    let seen = Mutex::new(Vec::new());
    let report = manual::fetch_descriptions_batch(
        &db.pool,
        &[first, second],
        &source,
        &AtomicBool::new(false),
        |entry| seen.lock().unwrap().push(entry.job_id),
    )
    .await
    .expect("batch");

    assert_eq!(report.succeeded(), 2);
    assert_eq!(report.failed(), 0);
    assert_eq!(report.remaining, 0);
    assert!(!report.cancelled);
    assert!(report.first_error().is_none());
    assert_eq!(*seen.lock().unwrap(), vec![first, second], "one per job");

    let first_row = job_repo::get(&db.pool, first).await.unwrap().unwrap();
    assert_eq!(first_row.raw_text, "First body.");
    assert_eq!(first_row.queue_state, QueueState::Evaluate.as_str());
    assert!(
        first_row.queued_at.is_some(),
        "advancing the queue records when"
    );

    let second_row = job_repo::get(&db.pool, second).await.unwrap().unwrap();
    assert_eq!(second_row.raw_text, "Second body.");
    assert_eq!(second_row.queue_state, QueueState::Evaluate.as_str());

    // The batch must not insert: a refresh never creates a posting.
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM job_descriptions")
        .fetch_one(&db.pool)
        .await
        .expect("count");
    assert_eq!(count, 2);

    db.close().await;
}

#[tokio::test]
async fn a_failure_leaves_the_row_marked_for_details() {
    let db = common::TestDb::new_raw().await;
    add_portal(&db.pool).await;
    let good = add_flagged_job(&db.pool, 0).await;
    let bad = add_flagged_job(&db.pool, 1).await;

    // An empty page is what a site that blocks the request or moved the posting
    // looks like, and it is a failure rather than an empty description.
    let source =
        MockFetchSource::default().pages([Ok(detail_page("Good body.")), Ok(String::new())]);
    let report = manual::fetch_descriptions_batch(
        &db.pool,
        &[good, bad],
        &source,
        &AtomicBool::new(false),
        |_| {},
    )
    .await
    .expect("batch");

    assert_eq!(report.succeeded(), 1);
    assert_eq!(report.failed(), 1);
    assert!(
        report
            .first_error()
            .is_some_and(|cause| cause.contains("no description")),
        "report: {:?}",
        report.first_error()
    );

    assert_eq!(queue_state(&db.pool, good).await, "evaluate");
    assert_eq!(
        queue_state(&db.pool, bad).await,
        QueueState::DetailsNeeded.as_str(),
        "a failed fetch keeps the work in the queue"
    );
    assert_eq!(
        job_repo::get(&db.pool, bad)
            .await
            .unwrap()
            .unwrap()
            .raw_text,
        "",
        "nothing is written on failure"
    );

    db.close().await;
}

#[tokio::test]
async fn the_cancel_flag_stops_between_postings() {
    let db = common::TestDb::new_raw().await;
    add_portal(&db.pool).await;
    let ids = [
        add_flagged_job(&db.pool, 0).await,
        add_flagged_job(&db.pool, 1).await,
        add_flagged_job(&db.pool, 2).await,
    ];
    let source = MockFetchSource::default().pages([
        Ok(detail_page("One.")),
        Ok(detail_page("Two.")),
        Ok(detail_page("Three.")),
    ]);

    let cancel = Arc::new(AtomicBool::new(false));
    let cancel_in_callback = Arc::clone(&cancel);
    let calls = AtomicUsize::new(0);
    let report = manual::fetch_descriptions_batch(&db.pool, &ids, &source, &cancel, |_entry| {
        // A user pressing Stop while the first posting is being stored.
        calls.fetch_add(1, Ordering::Relaxed);
        cancel_in_callback.store(true, Ordering::Relaxed);
    })
    .await
    .expect("batch");

    assert!(report.cancelled, "the report says why it stopped");
    assert_eq!(report.entries.len(), 1, "the flag is read between jobs");
    assert_eq!(report.remaining, 2);
    assert_eq!(source.page_calls().len(), 1, "no request for the rest");

    assert_eq!(queue_state(&db.pool, ids[0]).await, "evaluate");
    assert_eq!(queue_state(&db.pool, ids[1]).await, "details_needed");
    assert_eq!(queue_state(&db.pool, ids[2]).await, "details_needed");

    db.close().await;
}

#[tokio::test]
async fn the_batch_is_bounded_per_click() {
    let db = common::TestDb::new_raw().await;
    add_portal(&db.pool).await;
    let total = DESCRIPTION_BATCH_LIMIT + 5;
    let mut ids = Vec::with_capacity(total);
    for index in 0..total {
        ids.push(add_flagged_job(&db.pool, index).await);
    }

    let source = MockFetchSource::default()
        .pages((0..DESCRIPTION_BATCH_LIMIT).map(|_| Ok(detail_page("Body."))));
    let report =
        manual::fetch_descriptions_batch(&db.pool, &ids, &source, &AtomicBool::new(false), |_| {})
            .await
            .expect("batch");

    assert_eq!(report.entries.len(), DESCRIPTION_BATCH_LIMIT);
    assert_eq!(report.remaining, 5);
    assert_eq!(
        source.page_calls().len(),
        DESCRIPTION_BATCH_LIMIT,
        "an unattempted posting costs no request"
    );

    let still_flagged: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM job_descriptions WHERE queue_state = 'details_needed'",
    )
    .fetch_one(&db.pool)
    .await
    .expect("count");
    assert_eq!(still_flagged, 5);

    db.close().await;
}

#[tokio::test]
async fn an_empty_batch_does_nothing() {
    let db = common::TestDb::new_raw().await;
    let source = MockFetchSource::default();

    let report =
        manual::fetch_descriptions_batch(&db.pool, &[], &source, &AtomicBool::new(false), |_| {
            panic!("no entry for an empty batch")
        })
        .await
        .expect("batch");

    assert!(report.entries.is_empty());
    assert_eq!(report.remaining, 0);
    assert!(source.page_calls().is_empty());

    db.close().await;
}
