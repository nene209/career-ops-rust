//! Portals service-layer tests.
//!
//! `PortalHealth` unit tests live in `health.rs` (they need no database).
//! This file covers the integration tests: `list_with_health`,
//! `fetch_one`, and `fetch_batch`.

mod common;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use career_ops_core::acquisition::error::{AtsError, FailureCause};
use career_ops_core::acquisition::report::{PageReport, PaginationStop};
use career_ops_core::model::FetchTier;
use career_ops_core::portals::health::HealthState;
use career_ops_core::portals::service::{self, BatchEntry, FetchAllStop};
use career_ops_core::repo::portals::{self, NewPortal, RenderMode};

use common::{sample_job, MockFetchSource};

fn static_portal(name: &str, url: &str) -> NewPortal {
    NewPortal {
        company_name: name.into(),
        url: url.into(),
        render_mode: RenderMode::Static,
        scraper_config: None,
    }
}

// -----------------------------------------------------------------
// list_with_health
// -----------------------------------------------------------------

#[tokio::test]
async fn list_with_health_returns_one_entry_per_portal_with_correct_states() {
    let db = common::TestDb::new_raw().await;

    // Four portals, one per health state.
    let never = portals::create(&db.pool, static_portal("Never", "https://example.com/a"))
        .await
        .unwrap();
    let healthy = portals::create(&db.pool, static_portal("Healthy", "https://example.com/b"))
        .await
        .unwrap();
    let degraded = portals::create(&db.pool, static_portal("Degraded", "https://example.com/c"))
        .await
        .unwrap();
    let broken = portals::create(&db.pool, static_portal("Broken", "https://example.com/d"))
        .await
        .unwrap();

    // Healthy: success only.
    portals::record_fetch_attempt(&db.pool, healthy.id, None)
        .await
        .unwrap();

    // Degraded: success then failure.
    portals::record_fetch_attempt(&db.pool, degraded.id, None)
        .await
        .unwrap();
    portals::record_fetch_attempt(&db.pool, degraded.id, Some("blocked"))
        .await
        .unwrap();

    // Broken: failure only.
    portals::record_fetch_attempt(&db.pool, broken.id, Some("timeout"))
        .await
        .unwrap();

    let entries = service::list_with_health(&db.pool).await.expect("list");
    assert_eq!(entries.len(), 4);

    let find = |id: i64| {
        entries
            .iter()
            .find(|e| e.portal.id == id)
            .unwrap_or_else(|| panic!("missing portal {id}"))
    };

    assert_eq!(find(never.id).health.state, HealthState::NeverFetched);
    assert_eq!(find(healthy.id).health.state, HealthState::Healthy);
    assert_eq!(find(degraded.id).health.state, HealthState::Degraded);
    assert_eq!(find(broken.id).health.state, HealthState::Broken);

    // The degraded portal has a suggested action; the healthy one does not.
    assert!(find(degraded.id).health.suggested_action.is_some());
    assert!(find(healthy.id).health.suggested_action.is_none());

    db.close().await;
}

// -----------------------------------------------------------------
// fetch_one
// -----------------------------------------------------------------

#[tokio::test]
async fn fetch_one_returns_updated_portal_on_success() {
    let db = common::TestDb::new_raw().await;
    let portal = portals::create(
        &db.pool,
        static_portal("Acme", "https://boards.greenhouse.io/acme/jobs/1"),
    )
    .await
    .unwrap();

    let source = Arc::new(MockFetchSource::with_ats(Ok(vec![sample_job("engineer")])));

    let report = service::fetch_one(&db.pool, portal.id, source.as_ref(), None)
        .await
        .expect("fetch_one");

    assert!(report.outcome.is_ok());
    assert_eq!(report.outcome.as_ref().unwrap().jobs_fetched, 1);
    assert_eq!(report.outcome.as_ref().unwrap().tier, FetchTier::Tier1aAts);

    // The returned portal reflects the post-fetch state without a second query.
    assert!(report.portal.last_verified_at.is_some());
    assert!(report.portal.last_failure_cause.is_none());

    db.close().await;
}

#[tokio::test]
async fn fetch_one_returns_error_and_updates_portal_on_failure() {
    let db = common::TestDb::new_raw().await;
    let portal = portals::create(
        &db.pool,
        static_portal("Acme", "https://example.com/careers"),
    )
    .await
    .unwrap();

    let source = Arc::new(MockFetchSource::with_html(Err(AtsError::Blocked {
        status: 403,
    })));

    let report = service::fetch_one(&db.pool, portal.id, source.as_ref(), None)
        .await
        .expect("fetch_one should succeed at the service level");

    // The fetch itself failed.
    let err = report.outcome.expect_err("expected fetch failure");
    assert_eq!(err.cause, FailureCause::Blocked);

    // The portal row reflects the failure.
    assert!(report.portal.last_fetch_attempt_at.is_some());
    assert_eq!(report.portal.last_failure_cause.as_deref(), Some("blocked"));
    assert!(report.portal.last_verified_at.is_none());

    db.close().await;
}

// -----------------------------------------------------------------
// fetch_batch
// -----------------------------------------------------------------

#[tokio::test]
async fn fetch_batch_continues_past_failures_and_reports_each_entry() {
    let db = common::TestDb::new_raw().await;

    // Three Greenhouse portals. The batch will call fetch_ats three
    // times; the queue supplies Ok, Err, Ok.
    let first = portals::create(
        &db.pool,
        static_portal("First", "https://boards.greenhouse.io/first/jobs/1"),
    )
    .await
    .unwrap();
    let second = portals::create(
        &db.pool,
        static_portal("Second", "https://boards.greenhouse.io/second/jobs/1"),
    )
    .await
    .unwrap();
    let third = portals::create(
        &db.pool,
        static_portal("Third", "https://boards.greenhouse.io/third/jobs/1"),
    )
    .await
    .unwrap();

    let source = Arc::new(MockFetchSource::sequence_ats(vec![
        Ok(vec![sample_job("first-job")]),
        Err(AtsError::Blocked { status: 403 }),
        Ok(vec![sample_job("third-a"), sample_job("third-b")]),
    ]));

    let progress_log: Arc<Mutex<Vec<BatchEntry>>> = Arc::new(Mutex::new(Vec::new()));
    let progress_log_clone = progress_log.clone();

    let report = service::fetch_batch(
        &db.pool,
        &[first.id, second.id, third.id],
        source.as_ref(),
        None,
        |entry| {
            // The closure receives every entry as it completes. The test
            // records them so it can assert the order and count later.
            progress_log_clone.lock().unwrap().push(BatchEntry {
                portal_id: entry.portal_id,
                portal_name: entry.portal_name.clone(),
                result: entry.result.clone(),
            });
        },
    )
    .await
    .expect("fetch_batch");

    // The batch itself succeeded even though one portal failed.
    assert_eq!(report.results.len(), 3);
    assert_eq!(report.succeeded(), 2);
    assert_eq!(report.failed(), 1);

    // Per-entry results are in the order the portal IDs were supplied.
    assert_eq!(report.results[0].portal_id, first.id);
    assert_eq!(report.results[0].result, Ok(1));
    assert_eq!(report.results[1].portal_id, second.id);
    match &report.results[1].result {
        Err(e) => {
            assert_eq!(e.cause, FailureCause::Blocked);
            assert!(
                !e.detail.is_empty(),
                "FetchError.detail should be populated for the drill-down"
            );
        }
        Ok(n) => panic!("expected Err, got Ok({n})"),
    }
    assert_eq!(report.results[2].portal_id, third.id);
    assert_eq!(report.results[2].result, Ok(2));

    // Progress was called once per portal, in order. Scoped so the
    // `MutexGuard` drops before the awaits below.
    {
        let logged = progress_log.lock().unwrap();
        assert_eq!(logged.len(), 3);
        assert_eq!(logged[0].portal_id, first.id);
        assert_eq!(logged[1].portal_id, second.id);
        assert_eq!(logged[2].portal_id, third.id);
    }

    // The failed portal's row records the cause.
    let refreshed = portals::get(&db.pool, second.id).await.unwrap().unwrap();
    assert_eq!(refreshed.last_failure_cause.as_deref(), Some("blocked"));

    db.close().await;
}

#[tokio::test]
async fn fetch_batch_cancellable_stops_between_portals() {
    let db = common::TestDb::new_raw().await;

    let first = portals::create(
        &db.pool,
        static_portal("First", "https://boards.greenhouse.io/first/jobs/1"),
    )
    .await
    .unwrap();
    let second = portals::create(
        &db.pool,
        static_portal("Second", "https://boards.greenhouse.io/second/jobs/1"),
    )
    .await
    .unwrap();
    let third = portals::create(
        &db.pool,
        static_portal("Third", "https://boards.greenhouse.io/third/jobs/1"),
    )
    .await
    .unwrap();

    let source = Arc::new(MockFetchSource::sequence_ats(vec![
        Ok(vec![sample_job("first-job")]),
        Ok(vec![sample_job("second-job")]),
        Ok(vec![sample_job("third-job")]),
    ]));

    let cancel = AtomicBool::new(false);
    let report = service::fetch_batch_cancellable(
        &db.pool,
        &[first.id, second.id, third.id],
        source.as_ref(),
        None,
        &cancel,
        |_entry| {
            // Cancel after the first completed portal. The fetch already
            // in progress is allowed to finish; the next one must not start.
            cancel.store(true, Ordering::Relaxed);
        },
    )
    .await
    .expect("fetch_batch_cancellable");

    assert_eq!(
        report.results.len(),
        1,
        "cancellation should stop the batch between portals"
    );
    assert_eq!(report.results[0].portal_id, first.id);
    assert_eq!(
        source.ats_calls().len(),
        1,
        "no fetch should start after the cancellation flag is set"
    );

    // A batch cancelled before it starts does no work.
    let already_cancelled = AtomicBool::new(true);
    let report_after = service::fetch_batch_cancellable(
        &db.pool,
        &[second.id, third.id],
        source.as_ref(),
        None,
        &already_cancelled,
        |_| {},
    )
    .await
    .expect("cancelled batch");
    assert!(report_after.results.is_empty());

    db.close().await;
}

#[tokio::test]
async fn fetch_batch_with_empty_list_returns_empty_report() {
    let db = common::TestDb::new_raw().await;
    let source = Arc::new(MockFetchSource::default());

    let report = service::fetch_batch(&db.pool, &[], source.as_ref(), None, |_| {})
        .await
        .expect("fetch_batch");

    assert!(report.results.is_empty());
    assert_eq!(report.succeeded(), 0);
    assert_eq!(report.failed(), 0);

    db.close().await;
}

/// Full lifecycle: never fetched → broken → healthy.
///
/// Closes the loop between the writer (`record_fetch_attempt`, called by
/// `fetch_one` via the router) and the reader (`PortalHealth::from_portal`,
/// called by `list_with_health`). The two halves are tested individually
/// elsewhere; this proves they agree on the on-disk representation.
#[tokio::test]
async fn portal_lifecycle_never_fetched_to_broken_to_healthy() {
    let db = common::TestDb::new_raw().await;
    let portal = portals::create(
        &db.pool,
        static_portal("Acme", "https://example.com/careers"),
    )
    .await
    .unwrap();

    // Stage 1: never fetched.
    let entries = service::list_with_health(&db.pool).await.unwrap();
    assert_eq!(entries[0].health.state, HealthState::NeverFetched);

    // Stage 2: fetch fails with `blocked`.
    let source = Arc::new(MockFetchSource::with_html(Err(AtsError::Blocked {
        status: 403,
    })));
    let report = service::fetch_one(&db.pool, portal.id, source.as_ref(), None)
        .await
        .expect("fetch_one");
    assert!(report.outcome.is_err());

    let entries = service::list_with_health(&db.pool).await.unwrap();
    assert_eq!(entries[0].health.state, HealthState::Broken);
    assert_eq!(entries[0].health.failure_cause, Some(FailureCause::Blocked));
    assert!(entries[0].health.suggested_action.is_some());
    assert!(entries[0].health.last_attempt_at.is_some());
    assert!(entries[0].health.last_success_at.is_none());

    // Stage 3: fetch succeeds.
    let source = Arc::new(MockFetchSource::with_html(Ok(vec![sample_job("only")])));
    let report = service::fetch_one(&db.pool, portal.id, source.as_ref(), None)
        .await
        .expect("fetch_one");
    assert!(report.outcome.is_ok());

    let entries = service::list_with_health(&db.pool).await.unwrap();
    assert_eq!(entries[0].health.state, HealthState::Healthy);
    assert!(entries[0].health.failure_cause.is_none());
    assert!(entries[0].health.suggested_action.is_none());
    assert!(entries[0].health.last_success_at.is_some());

    db.close().await;
}

// -----------------------------------------------------------------
// fetch_all_pages
// -----------------------------------------------------------------

fn eightfold_portal(name: &str, max_pages: usize) -> NewPortal {
    NewPortal {
        company_name: name.into(),
        url: "https://jobs.nvidia.com/careers".into(),
        render_mode: RenderMode::Static,
        scraper_config: Some(serde_json::json!({
            "provider": "eightfold",
            "domain": "nvidia.com",
            "pagination": { "max_pages": max_pages }
        })),
    }
}

fn chunk(title: &str, pages: usize, stop: PaginationStop) -> Result<PageReport, AtsError> {
    Ok(PageReport {
        jobs: vec![sample_job(title)],
        pages_fetched: pages,
        stop,
    })
}

async fn cursor_of(pool: &sqlx::SqlitePool, portal_id: i64) -> i64 {
    portals::get(pool, portal_id)
        .await
        .expect("portal")
        .expect("row")
        .last_page_fetched
}

/// The walk reads chunk after chunk until the board ends, reports it, and
/// leaves the cursor cleared so the next Continue starts over.
#[tokio::test]
async fn fetch_all_pages_stops_at_the_end_of_the_list() {
    let db = common::TestDb::new_raw().await;
    let portal = portals::create(&db.pool, eightfold_portal("Acme", 2))
        .await
        .unwrap();

    let source = Arc::new(MockFetchSource::sequence_eightfold([
        chunk("a", 2, PaginationStop::LimitReached),
        chunk("b", 2, PaginationStop::LimitReached),
        chunk("c", 1, PaginationStop::EndOfList),
    ]));

    let cancel = AtomicBool::new(false);
    let mut progress = Vec::new();
    let report =
        service::fetch_all_pages(&db.pool, portal.id, source.as_ref(), None, &cancel, |p| {
            progress.push(*p)
        })
        .await
        .expect("fetch_all_pages");

    assert_eq!(report.stopped_reason, FetchAllStop::EndOfList);
    assert_eq!(report.pages_fetched, 5);
    assert_eq!(report.jobs_stored, 3);
    assert_eq!(source.eightfold_calls().len(), 3);
    assert_eq!(
        cursor_of(&db.pool, portal.id).await,
        0,
        "the end of the list clears the cursor"
    );
    assert!(report.portal.last_verified_at.is_some());

    // Progress is cumulative and reported once per chunk.
    let pages: Vec<usize> = progress.iter().map(|p| p.pages_fetched).collect();
    assert_eq!(pages, vec![2, 4, 5]);
    assert_eq!(progress.len(), 3);
    assert!(progress.iter().all(|p| p.budget == 50));

    db.close().await;
}

#[tokio::test]
async fn fetch_all_pages_stops_when_the_user_cancels() {
    let db = common::TestDb::new_raw().await;
    let portal = portals::create(&db.pool, eightfold_portal("Acme", 2))
        .await
        .unwrap();
    let source = Arc::new(MockFetchSource::sequence_eightfold([chunk(
        "a",
        2,
        PaginationStop::LimitReached,
    )]));

    // Set before the first chunk: nothing is requested at all.
    let cancel = AtomicBool::new(true);
    let report =
        service::fetch_all_pages(&db.pool, portal.id, source.as_ref(), None, &cancel, |_| {})
            .await
            .expect("fetch_all_pages");

    assert_eq!(report.stopped_reason, FetchAllStop::Cancelled);
    assert_eq!(report.pages_fetched, 0);
    assert!(source.eightfold_calls().is_empty());

    db.close().await;
}

#[tokio::test]
async fn fetch_all_pages_stops_at_the_page_budget() {
    let db = common::TestDb::new_raw().await;
    let portal = portals::create(&db.pool, eightfold_portal("Acme", 5))
        .await
        .unwrap();

    // Ten chunks of five pages reach the fifty-page budget exactly.
    let source =
        Arc::new(MockFetchSource::sequence_eightfold((0..10).map(|index| {
            chunk(&format!("job-{index}"), 5, PaginationStop::LimitReached)
        })));

    let cancel = AtomicBool::new(false);
    let report =
        service::fetch_all_pages(&db.pool, portal.id, source.as_ref(), None, &cancel, |_| {})
            .await
            .expect("fetch_all_pages");

    assert_eq!(report.stopped_reason, FetchAllStop::BudgetExhausted);
    assert_eq!(report.pages_fetched, service::FETCH_ALL_PAGE_BUDGET);
    assert_eq!(source.eightfold_calls().len(), 10);

    db.close().await;
}

/// An ATS adapter pages internally and reports no cursor. Without the
/// no-progress guard the loop would repeat the same request fifty times.
#[tokio::test]
async fn fetch_all_pages_stops_when_the_provider_cannot_continue() {
    let db = common::TestDb::new_raw().await;
    let portal = portals::create(
        &db.pool,
        static_portal("Acme", "https://boards.greenhouse.io/acme/jobs/1"),
    )
    .await
    .unwrap();
    let source = Arc::new(MockFetchSource::with_ats(Ok(vec![sample_job("engineer")])));

    let cancel = AtomicBool::new(false);
    let report =
        service::fetch_all_pages(&db.pool, portal.id, source.as_ref(), None, &cancel, |_| {})
            .await
            .expect("fetch_all_pages");

    assert_eq!(report.stopped_reason, FetchAllStop::NoProgress);
    assert_eq!(report.pages_fetched, 0);
    assert_eq!(report.jobs_stored, 1);
    assert_eq!(
        source.ats_calls().len(),
        1,
        "the walk must not repeat a request that cannot advance"
    );

    db.close().await;
}

/// A failing chunk stops the walk and keeps the pages already stored.
#[tokio::test]
async fn fetch_all_pages_reports_a_failed_chunk() {
    let db = common::TestDb::new_raw().await;
    let portal = portals::create(&db.pool, eightfold_portal("Acme", 2))
        .await
        .unwrap();
    let source = Arc::new(MockFetchSource::sequence_eightfold([
        chunk("a", 2, PaginationStop::LimitReached),
        Err(AtsError::Transport("boom".into())),
    ]));

    let cancel = AtomicBool::new(false);
    let report =
        service::fetch_all_pages(&db.pool, portal.id, source.as_ref(), None, &cancel, |_| {})
            .await
            .expect("fetch_all_pages");

    assert!(report.stopped_reason.is_failure());
    assert_eq!(report.pages_fetched, 2);
    assert_eq!(report.jobs_stored, 1);
    match report.stopped_reason {
        FetchAllStop::Failed(error) => assert_eq!(error.cause, FailureCause::Transport),
        other => panic!("expected a failed chunk, got {other:?}"),
    }

    db.close().await;
}

/// A chunk that read pages but stopped at a page the site refused is not "no
/// progress": it must report the page, keep what it stored, and record the
/// pages that worked so the next attempt starts at the page that failed.
#[tokio::test]
async fn fetch_all_pages_reports_an_unreadable_page() {
    let db = common::TestDb::new_raw().await;
    let portal = portals::create(&db.pool, eightfold_portal("Acme", 4))
        .await
        .unwrap();
    let source = Arc::new(MockFetchSource::sequence_eightfold([Ok(PageReport {
        jobs: vec![sample_job("a")],
        pages_fetched: 4,
        stop: PaginationStop::FetchFailed,
    })]));

    let cancel = AtomicBool::new(false);
    let report =
        service::fetch_all_pages(&db.pool, portal.id, source.as_ref(), None, &cancel, |_| {})
            .await
            .expect("fetch_all_pages");

    assert_eq!(
        report.stopped_reason,
        FetchAllStop::Stopped {
            unread_page: Some(5)
        },
        "pages 1-4 were read, so page 5 is the one that failed"
    );
    assert_eq!(report.pages_fetched, 4);
    assert_eq!(report.jobs_stored, 1);
    assert!(!report.stopped_reason.is_failure());
    assert_eq!(
        cursor_of(&db.pool, portal.id).await,
        4,
        "the pages that worked are recorded, so a retry starts at the one that failed"
    );
    assert_eq!(
        source.eightfold_calls().len(),
        1,
        "it must not retry a refused page in a loop"
    );

    // The next attempt asks for the failed page, not pages 1-4 again.
    let retry = Arc::new(MockFetchSource::sequence_eightfold([Ok(PageReport {
        jobs: vec![sample_job("b")],
        pages_fetched: 1,
        stop: PaginationStop::EndOfList,
    })]));
    service::fetch_all_pages(&db.pool, portal.id, retry.as_ref(), None, &cancel, |_| {})
        .await
        .expect("retry");
    assert_eq!(
        retry.eightfold_calls()[0].3,
        5,
        "it resumes at the failed page"
    );

    db.close().await;
}
