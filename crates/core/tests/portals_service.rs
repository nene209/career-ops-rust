//! Portals service-layer tests.
//!
//! `PortalHealth` unit tests live in `health.rs` (they need no database).
//! This file covers the integration tests: `list_with_health`,
//! `fetch_one`, and `fetch_batch`.

mod common;

use std::sync::{Arc, Mutex};

use career_ops_core::acquisition::error::{AtsError, FailureCause};
use career_ops_core::model::FetchTier;
use career_ops_core::portals::health::HealthState;
use career_ops_core::portals::service::{self, BatchEntry};
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
