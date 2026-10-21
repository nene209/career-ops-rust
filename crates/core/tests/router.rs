//! Router tests.
//!
//! Two of these tests close the carried-forward Week 2 criteria:
//!
//! - `static_portal_with_ats_url_uses_tier1a`
//! - `js_required_escalates_to_tier2`
//!
//! The rest verify storage, `fetch_tier` recording, `last_verified_at`,
//! and the `browser` render-mode short-circuit.

mod common;

use std::sync::{Arc, Mutex};

use async_trait::async_trait;
use career_ops_core::acquisition::ats::{AtsAdapter, NormalizedJob};
use career_ops_core::acquisition::error::AtsError;
use career_ops_core::acquisition::html::selectors::SelectorConfig;
use career_ops_core::acquisition::router::{
    decide_initial_route, fetch_portal, FetchSource, InitialRoute,
};
use career_ops_core::backend::{MockBrowserBackend, RenderedPage};
use career_ops_core::model::FetchTier;
use career_ops_core::repo::portals::{self, NewPortal, RenderMode};
use url::Url;

// -----------------------------------------------------------------
// Mock FetchSource
// -----------------------------------------------------------------

#[derive(Default)]
struct MockFetchSource {
    ats_calls: Mutex<Vec<(String, String)>>,
    html_calls: Mutex<Vec<String>>,
    ats_result: Mutex<Option<Result<Vec<NormalizedJob>, AtsError>>>,
    html_result: Mutex<Option<Result<Vec<NormalizedJob>, AtsError>>>,
}

impl MockFetchSource {
    fn with_ats(result: Result<Vec<NormalizedJob>, AtsError>) -> Self {
        Self {
            ats_result: Mutex::new(Some(result)),
            ..Default::default()
        }
    }

    fn with_html(result: Result<Vec<NormalizedJob>, AtsError>) -> Self {
        Self {
            html_result: Mutex::new(Some(result)),
            ..Default::default()
        }
    }
}

#[async_trait]
impl FetchSource for MockFetchSource {
    async fn fetch_ats(
        &self,
        adapter: &dyn AtsAdapter,
        slug: &str,
        _company_name: &str,
    ) -> Result<Vec<NormalizedJob>, AtsError> {
        self.ats_calls
            .lock()
            .unwrap()
            .push((adapter.name().to_string(), slug.to_string()));
        self.ats_result
            .lock()
            .unwrap()
            .take()
            .unwrap_or_else(|| Ok(vec![]))
    }

    async fn fetch_html(
        &self,
        url: &Url,
        _company_name: &str,
        _config: Option<&SelectorConfig>,
    ) -> Result<Vec<NormalizedJob>, AtsError> {
        self.html_calls.lock().unwrap().push(url.to_string());
        self.html_result
            .lock()
            .unwrap()
            .take()
            .unwrap_or_else(|| Ok(vec![]))
    }
}

fn sample_job(title: &str) -> NormalizedJob {
    NormalizedJob {
        external_id: format!("id-{title}"),
        title: title.into(),
        company: "Acme".into(),
        location: Some("Remote".into()),
        department: Some("Engineering".into()),
        employment_type: None,
        workplace_type: None,
        source_url: Url::parse(&format!("https://boards.greenhouse.io/acme/jobs/{title}")).unwrap(),
        raw_text: "test".into(),
        posted_at: None,
    }
}

// -----------------------------------------------------------------
// decide_initial_route — pure logic
// -----------------------------------------------------------------

#[test]
fn static_portal_with_ats_url_uses_tier1a() {
    let adapters = career_ops_core::acquisition::ats::adapters();
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
    let adapters = career_ops_core::acquisition::ats::adapters();
    let url = Url::parse("https://example.com/careers").unwrap();
    assert_eq!(
        decide_initial_route("static", &url, &adapters),
        InitialRoute::Tier1b
    );
}

#[test]
fn browser_render_mode_short_circuits_to_tier2() {
    let adapters = career_ops_core::acquisition::ats::adapters();
    // Even a URL an adapter would recognise must route to Tier 2 when
    // render_mode is explicitly "browser".
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

    // fetch_tier recorded on each job_descriptions row
    let rows: Vec<(String,)> = sqlx::query_as("SELECT fetch_tier FROM job_descriptions")
        .fetch_all(&db.pool)
        .await
        .unwrap();
    for (tier,) in &rows {
        assert_eq!(tier, "tier1a:ats");
    }

    // last_verified_at set
    let refreshed = portals::get(&db.pool, portal.id).await.unwrap().unwrap();
    assert!(refreshed.last_verified_at.is_some());

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
    assert_eq!(source.html_calls.lock().unwrap().len(), 1);

    db.close().await;
}

#[tokio::test]
async fn js_required_escalates_to_tier2() {
    // The carried-forward Week 2 criterion: a Tier 1b result of
    // `Unsupported("js_required")` must be routed to Tier 2 rather than
    // surfaced as an error.
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

    // The stored job came from the rendered HTML
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
    // Deliberately an ATS URL: a portal explicitly configured as
    // `browser` must not use the ATS adapter.
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

    // The mock returns no jobs from JSON-LD or selectors, so extraction
    // fails — but the point of the test is that Tier 1 was never called.
    let result = fetch_portal(&db.pool, portal.id, source.as_ref(), Some(&browser)).await;

    assert!(source.ats_calls.lock().unwrap().is_empty());
    assert!(source.html_calls.lock().unwrap().is_empty());
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

    // Tier 2 extraction failed, so the cause is parse_error, not blocked.
    // The blocked cause from Tier 1 was consumed by escalation.
    assert_eq!(
        err.cause,
        career_ops_core::acquisition::error::FailureCause::ParseError
    );

    db.close().await;
}
