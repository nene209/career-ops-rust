//! Router tests.
//!
//! Two tests close the carried-forward Week 2 criteria:
//!
//! - `static_portal_with_ats_url_uses_tier1a`
//! - `js_required_escalates_to_tier2`

mod common;

use std::sync::Arc;

use career_ops_core::acquisition::ats;
use career_ops_core::acquisition::error::AtsError;
use career_ops_core::acquisition::router::{decide_initial_route, fetch_portal, InitialRoute};
use career_ops_core::backend::{MockBrowserBackend, RenderedPage};
use career_ops_core::model::FetchTier;
use career_ops_core::repo::portals::{self, NewPortal, RenderMode};
use url::Url;

use common::{sample_job, MockFetchSource};

// -----------------------------------------------------------------
// decide_initial_route — pure logic
// -----------------------------------------------------------------

#[test]
fn static_portal_with_ats_url_uses_tier1a() {
    let adapters = ats::adapters();
    let url = Url::parse("https://boards.greenhouse.io/acme/jobs/1").unwrap();

    match decide_initial_route("static", &url, &adapters) {
        InitialRoute::Tier1a { adapter_name, slug } => {
            assert_eq!(adapter_name, "greenhouse");
            assert_eq!(slug, "acme");
        }
        other => panic!("expected Tier1a, got {other:?}"),
    }
}

#[test]
fn static_portal_with_non_ats_url_uses_tier1b() {
    let adapters = ats::adapters();
    let url = Url::parse("https://example.com/careers").unwrap();
    assert_eq!(
        decide_initial_route("static", &url, &adapters),
        InitialRoute::Tier1b
    );
}

#[test]
fn browser_render_mode_short_circuits_to_tier2() {
    let adapters = ats::adapters();
    let url = Url::parse("https://boards.greenhouse.io/acme/jobs/1").unwrap();
    assert_eq!(
        decide_initial_route("browser", &url, &adapters),
        InitialRoute::Tier2
    );
}

// -----------------------------------------------------------------
// Router integration
// -----------------------------------------------------------------

fn static_portal(url: &str) -> NewPortal {
    NewPortal {
        company_name: "Acme".into(),
        url: url.into(),
        render_mode: RenderMode::Static,
        scraper_config: None,
    }
}

fn browser_portal(url: &str) -> NewPortal {
    NewPortal {
        company_name: "Acme".into(),
        url: url.into(),
        render_mode: RenderMode::Browser,
        scraper_config: None,
    }
}

#[tokio::test]
async fn static_portal_with_ats_url_records_tier1a() {
    let db = common::TestDb::new_raw().await;
    let portal = portals::create(
        &db.pool,
        static_portal("https://boards.greenhouse.io/acme/jobs/1"),
    )
    .await
    .expect("create portal");

    let source = Arc::new(MockFetchSource::with_ats(Ok(vec![
        sample_job("engineer"),
        sample_job("designer"),
    ])));

    let outcome = fetch_portal(&db.pool, portal.id, source.as_ref(), None)
        .await
        .expect("fetch");

    assert_eq!(outcome.jobs_fetched, 2);
    assert_eq!(outcome.tier, FetchTier::Tier1aAts);

    let rows: Vec<(String,)> = sqlx::query_as("SELECT fetch_tier FROM job_descriptions")
        .fetch_all(&db.pool)
        .await
        .unwrap();
    for (tier,) in &rows {
        assert_eq!(tier, "tier1a:ats");
    }

    let refreshed = portals::get(&db.pool, portal.id).await.unwrap().unwrap();
    assert!(refreshed.last_verified_at.is_some());
    assert!(refreshed.last_fetch_attempt_at.is_some());
    assert!(refreshed.last_failure_cause.is_none());

    db.close().await;
}

#[tokio::test]
async fn static_portal_with_non_ats_url_records_tier1b() {
    let db = common::TestDb::new_raw().await;
    let portal = portals::create(&db.pool, static_portal("https://example.com/careers"))
        .await
        .expect("create portal");

    let source = Arc::new(MockFetchSource::with_html(Ok(vec![sample_job("only")])));

    let outcome = fetch_portal(&db.pool, portal.id, source.as_ref(), None)
        .await
        .expect("fetch");

    assert_eq!(outcome.tier, FetchTier::Tier1bHtml);
    assert_eq!(outcome.jobs_fetched, 1);
    assert_eq!(source.html_calls().len(), 1);

    db.close().await;
}

#[tokio::test]
async fn js_required_escalates_to_tier2() {
    let db = common::TestDb::new_raw().await;
    let portal = portals::create(&db.pool, static_portal("https://example.com/careers"))
        .await
        .expect("create portal");

    let source = Arc::new(MockFetchSource::with_html(Err(AtsError::Unsupported(
        "js_required",
    ))));

    let browser = MockBrowserBackend {
        canned: RenderedPage {
            final_url: Url::parse("https://example.com/careers").unwrap(),
            html: r#"<script type="application/ld+json">
                {"@type":"JobPosting","title":"Rendered Job","description":"x"}
            </script>"#
                .into(),
            status: 200,
        },
        pdf_unsupported: true,
    };

    let outcome = fetch_portal(&db.pool, portal.id, source.as_ref(), Some(&browser))
        .await
        .expect("fetch");

    assert_eq!(outcome.tier, FetchTier::Tier2Obscura);
    assert_eq!(outcome.jobs_fetched, 1);

    let titles: Vec<(String,)> = sqlx::query_as("SELECT title FROM job_descriptions")
        .fetch_all(&db.pool)
        .await
        .unwrap();
    assert_eq!(titles.len(), 1);
    assert_eq!(titles[0].0, "Rendered Job");

    db.close().await;
}

#[tokio::test]
async fn browser_render_mode_goes_directly_to_tier2() {
    let db = common::TestDb::new_raw().await;
    let portal = portals::create(
        &db.pool,
        browser_portal("https://boards.greenhouse.io/acme/jobs/1"),
    )
    .await
    .expect("create portal");

    let source = Arc::new(MockFetchSource::default());
    let browser = MockBrowserBackend {
        canned: RenderedPage {
            final_url: Url::parse("https://boards.greenhouse.io/acme/jobs/1").unwrap(),
            html: r#"<a href="/jobs/1">Rendered</a>"#.into(),
            status: 200,
        },
        pdf_unsupported: true,
    };

    let result = fetch_portal(&db.pool, portal.id, source.as_ref(), Some(&browser)).await;

    assert!(source.ats_calls().is_empty());
    assert!(source.html_calls().is_empty());
    assert!(result.is_err(), "extraction should fail with no job markup");

    db.close().await;
}

#[tokio::test]
async fn both_tiers_failing_returns_classified_error() {
    let db = common::TestDb::new_raw().await;
    let portal = portals::create(&db.pool, static_portal("https://example.com/careers"))
        .await
        .expect("create portal");

    let source = Arc::new(MockFetchSource::with_html(Err(AtsError::Blocked {
        status: 403,
    })));
    let browser = MockBrowserBackend {
        canned: RenderedPage {
            final_url: Url::parse("https://example.com/careers").unwrap(),
            html: "<html><body>no jobs</body></html>".into(),
            status: 200,
        },
        pdf_unsupported: true,
    };

    let err = fetch_portal(&db.pool, portal.id, source.as_ref(), Some(&browser))
        .await
        .expect_err("both tiers should fail");

    assert_eq!(
        err.cause,
        career_ops_core::acquisition::error::FailureCause::ParseError
    );

    db.close().await;
}

#[tokio::test]
async fn failed_fetch_records_classified_cause() {
    let db = common::TestDb::new_raw().await;
    let portal = portals::create(&db.pool, static_portal("https://example.com/careers"))
        .await
        .expect("create portal");

    let source = Arc::new(MockFetchSource::with_html(Err(AtsError::Unsupported(
        "js_required",
    ))));

    let result = fetch_portal(&db.pool, portal.id, source.as_ref(), None).await;
    assert!(result.is_err());

    let refreshed = portals::get(&db.pool, portal.id).await.unwrap().unwrap();
    assert!(refreshed.last_fetch_attempt_at.is_some());
    assert_eq!(refreshed.last_failure_cause.as_deref(), Some("js_required"));
    assert!(refreshed.last_verified_at.is_none());

    db.close().await;
}

#[tokio::test]
async fn fetching_same_portal_twice_dedups() {
    let db = common::TestDb::new_raw().await;
    let portal = portals::create(
        &db.pool,
        static_portal("https://boards.greenhouse.io/acme/jobs/1"),
    )
    .await
    .expect("create portal");

    // Two identical fetch results.
    let source = Arc::new(MockFetchSource::sequence_ats(vec![
        Ok(vec![sample_job("engineer")]),
        Ok(vec![sample_job("engineer")]),
    ]));

    let first = fetch_portal(&db.pool, portal.id, source.as_ref(), None)
        .await
        .expect("first fetch");
    let second = fetch_portal(&db.pool, portal.id, source.as_ref(), None)
        .await
        .expect("second fetch");

    assert_eq!(first.jobs_fetched, 1);
    assert_eq!(second.jobs_fetched, 1);

    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM job_descriptions")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(count, 1, "second fetch should update, not duplicate");

    // The row was updated in place, not replaced: same id.
    let ids: Vec<(i64,)> = sqlx::query_as("SELECT id FROM job_descriptions")
        .fetch_all(&db.pool)
        .await
        .unwrap();
    assert_eq!(ids.len(), 1);

    db.close().await;
}
