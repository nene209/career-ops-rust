//! Manual job entry tests.
//!
//! Covers the nine acceptance criteria from Workstream 6:
//!
//! - URL fetch stores an ATS job
//! - URL fetch stores an HTML-extracted job
//! - URL fetch escalates to Tier 2 when Tier 1b yields nothing
//! - URL fetch warns when a board has multiple jobs
//! - Paste stores with `fetch_tier = 'manual'`
//! - Paste dedups identical pastes
//! - Paste rejects empty title or body
//! - URL fetch dedups against a prior portal fetch
//! - Paste does not merge into a portal-fetched row

mod common;

use std::sync::Arc;

use career_ops_core::acquisition::error::AtsError;
use career_ops_core::acquisition::manual;
use career_ops_core::acquisition::router;
use career_ops_core::backend::{MockBrowserBackend, RenderedPage};
use career_ops_core::model::FetchTier;
use career_ops_core::repo::job_descriptions;
use career_ops_core::repo::portals::{self, NewPortal, RenderMode};
use url::Url;

use common::{sample_job, MockFetchSource};

async fn count_jobs(pool: &sqlx::SqlitePool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM job_descriptions")
        .fetch_one(pool)
        .await
        .expect("count")
}

// -----------------------------------------------------------------
// URL fetch
// -----------------------------------------------------------------

#[tokio::test]
async fn url_fetch_stores_ats_job() {
    let db = common::TestDb::new_raw().await;
    let source = Arc::new(MockFetchSource::with_ats(Ok(vec![sample_job("engineer")])));

    let url = "https://boards.greenhouse.io/acme/jobs/engineer";
    let result = manual::add_job_from_url(&db.pool, url, source.as_ref(), None)
        .await
        .expect("add_job_from_url");

    assert_eq!(result.tier, FetchTier::Tier1aAts);
    assert!(result.warnings.is_empty());
    assert_eq!(count_jobs(&db.pool).await, 1);

    let stored = job_descriptions::get(&db.pool, result.stored.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.fetch_tier, "tier1a:ats");
    assert_eq!(stored.source_url, url);

    db.close().await;
}

#[tokio::test]
async fn url_fetch_stores_html_job_from_non_ats_url() {
    let db = common::TestDb::new_raw().await;
    let source = Arc::new(MockFetchSource::with_html(Ok(vec![sample_job("only")])));

    let url = "https://example.com/careers/senior-engineer";
    let result = manual::add_job_from_url(&db.pool, url, source.as_ref(), None)
        .await
        .expect("add_job_from_url");

    assert_eq!(result.tier, FetchTier::Tier1bHtml);
    assert_eq!(count_jobs(&db.pool).await, 1);

    let stored = job_descriptions::get(&db.pool, result.stored.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.fetch_tier, "tier1b:html");

    db.close().await;
}

#[tokio::test]
async fn url_fetch_escalates_to_tier2_when_tier1b_yields_nothing() {
    let db = common::TestDb::new_raw().await;

    // Tier 1b signals js_required.
    let source = Arc::new(MockFetchSource::with_html(Err(AtsError::Unsupported(
        "js_required",
    ))));

    // Tier 2 renders a page with JSON-LD.
    let browser = MockBrowserBackend {
        canned: RenderedPage {
            final_url: Url::parse("https://example.com/careers").unwrap(),
            html: r#"<script type="application/ld+json">
                {"@type":"JobPosting","title":"Rendered","description":"x"}
            </script>"#
                .into(),
            status: 200,
        },
        pdf_unsupported: true,
    };

    let result = manual::add_job_from_url(
        &db.pool,
        "https://example.com/careers",
        source.as_ref(),
        Some(&browser),
    )
    .await
    .expect("add_job_from_url");

    assert_eq!(result.tier, FetchTier::Tier2Obscura);
    assert_eq!(count_jobs(&db.pool).await, 1);

    db.close().await;
}

#[tokio::test]
async fn url_fetch_warns_when_board_has_multiple_jobs() {
    let db = common::TestDb::new_raw().await;

    // Two jobs on the board; the pasted URL matches the first.
    let source = Arc::new(MockFetchSource::with_ats(Ok(vec![
        sample_job("engineer"),
        sample_job("designer"),
    ])));

    let url = "https://boards.greenhouse.io/acme/jobs/engineer";
    let result = manual::add_job_from_url(&db.pool, url, source.as_ref(), None)
        .await
        .expect("add_job_from_url");

    assert_eq!(result.warnings.len(), 1);
    assert!(
        result.warnings[0].contains("2 jobs"),
        "warning should mention the board size: {:?}",
        result.warnings
    );
    assert_eq!(count_jobs(&db.pool).await, 1);

    db.close().await;
}

// -----------------------------------------------------------------
// Paste
// -----------------------------------------------------------------

#[tokio::test]
async fn paste_stores_with_manual_tier() {
    let db = common::TestDb::new_raw().await;

    let stored = manual::add_job_from_text(
        &db.pool,
        "Senior Engineer",
        "Acme Corp",
        "Build things. Break things.",
    )
    .await
    .expect("add_job_from_text");

    assert_eq!(stored.fetch_tier, "manual");
    assert!(stored.source_url.starts_with("manual://"));
    assert!(stored.source_url.contains("acme-corp"));
    assert!(stored.source_url.contains("senior-engineer"));
    assert_eq!(count_jobs(&db.pool).await, 1);

    db.close().await;
}

#[tokio::test]
async fn paste_dedups_identical_pastes() {
    let db = common::TestDb::new_raw().await;

    let first = manual::add_job_from_text(
        &db.pool,
        "Senior Engineer",
        "Acme Corp",
        "Build things. Break things.",
    )
    .await
    .expect("first paste");

    let second = manual::add_job_from_text(
        &db.pool,
        "Senior Engineer",
        "Acme Corp",
        "Build things. Break things.",
    )
    .await
    .expect("second paste");

    assert_eq!(
        first.id, second.id,
        "identical pastes should return the same row"
    );
    assert_eq!(count_jobs(&db.pool).await, 1);

    db.close().await;
}

#[tokio::test]
async fn paste_rejects_empty_title_or_body() {
    let db = common::TestDb::new_raw().await;

    let empty_title = manual::add_job_from_text(&db.pool, "   ", "Acme", "body").await;
    assert!(empty_title.is_err());

    let empty_body = manual::add_job_from_text(&db.pool, "Title", "Acme", "   ").await;
    assert!(empty_body.is_err());

    assert_eq!(count_jobs(&db.pool).await, 0);

    db.close().await;
}

// -----------------------------------------------------------------
// Cross-path dedup
// -----------------------------------------------------------------

#[tokio::test]
async fn url_fetch_dedups_against_prior_portal_fetch() {
    let db = common::TestDb::new_raw().await;

    // A portal fetch of the same job.
    let portal = portals::create(
        &db.pool,
        NewPortal {
            company_name: "Acme".into(),
            url: "https://boards.greenhouse.io/acme/jobs/1".into(),
            render_mode: RenderMode::Static,
            scraper_config: None,
        },
    )
    .await
    .expect("create portal");

    let job = sample_job("engineer");

    // Two identical results: one for the portal fetch, one for the
    // manual fetch.
    let source = Arc::new(MockFetchSource::sequence_ats(vec![
        Ok(vec![job.clone()]),
        Ok(vec![job.clone()]),
    ]));

    router::fetch_portal(&db.pool, portal.id, source.as_ref(), None)
        .await
        .expect("portal fetch");
    assert_eq!(count_jobs(&db.pool).await, 1);

    // Manual fetch of the same URL. The mock's second result has the
    // same URL and external_id, so upsert finds the existing row.
    let result = manual::add_job_from_url(&db.pool, job.source_url.as_ref(), source.as_ref(), None)
        .await
        .expect("manual fetch");

    assert_eq!(
        count_jobs(&db.pool).await,
        1,
        "should update, not duplicate"
    );
    assert_eq!(result.tier, FetchTier::Tier1aAts);

    db.close().await;
}

#[tokio::test]
async fn paste_does_not_merge_into_portal_fetched_row() {
    let db = common::TestDb::new_raw().await;

    // Portal fetch stores a job.
    let portal = portals::create(
        &db.pool,
        NewPortal {
            company_name: "Acme".into(),
            url: "https://boards.greenhouse.io/acme/jobs/engineer".into(),
            render_mode: RenderMode::Static,
            scraper_config: None,
        },
    )
    .await
    .expect("create portal");

    let job = sample_job("engineer");
    let source = Arc::new(MockFetchSource::with_ats(Ok(vec![job.clone()])));

    router::fetch_portal(&db.pool, portal.id, source.as_ref(), None)
        .await
        .expect("portal fetch");
    assert_eq!(count_jobs(&db.pool).await, 1);

    // Paste the same content. The synthetic URL differs from the
    // portal-fetched URL, so this is a new row.
    manual::add_job_from_text(&db.pool, &job.title, &job.company, &job.raw_text)
        .await
        .expect("paste");

    assert_eq!(
        count_jobs(&db.pool).await,
        2,
        "paste should be a separate row from the portal fetch"
    );

    db.close().await;
}

// -----------------------------------------------------------------
// Known sites
// -----------------------------------------------------------------

/// The detail response NVIDIA's `pcsx` endpoint returns for one position.
const NVIDIA_DETAIL: &str = r#"{
  "status": 200,
  "data": {
    "id": 893394864453,
    "name": "Senior Network Product Engineer",
    "publicUrl": "https://jobs.nvidia.com/careers/job/893394864453",
    "department": "Engineer, Product Development",
    "locations": ["Remote - Vietnam"],
    "efcustomTextTimeType": ["Full time"],
    "jobDescription": "<p>Build the network stack.</p>"
  }
}"#;

/// A known Eightfold posting is fetched through the provider, not through the
/// generic HTML path. That is what lets the row carry the same id the portal
/// fetch produced.
#[tokio::test]
async fn url_fetch_of_a_known_eightfold_posting_uses_the_provider() {
    let db = common::TestDb::new_raw().await;
    let source = Arc::new(
        MockFetchSource::default()
            .pages([Ok(NVIDIA_DETAIL.to_string()), Ok(NVIDIA_DETAIL.to_string())]),
    );

    let url = "https://jobs.nvidia.com/careers/job/893394864453";
    let first = manual::add_job_from_url(&db.pool, url, source.as_ref(), None)
        .await
        .expect("add_job_from_url");

    assert_eq!(first.tier, FetchTier::Tier1aAts);
    assert_eq!(
        first.stored.external_id.as_deref(),
        Some("eightfold:nvidia.com:893394864453")
    );
    assert_eq!(first.stored.company, "NVIDIA");
    assert!(first.stored.raw_text.contains("network stack"));
    assert_eq!(source.page_calls().len(), 1, "one detail request");
    assert!(
        source.html_calls().is_empty(),
        "the provider path must not fetch HTML"
    );

    // Adding the same posting again updates the row instead of inserting.
    let second = manual::add_job_from_url(&db.pool, url, source.as_ref(), None)
        .await
        .expect("second add");
    assert_eq!(second.stored.id, first.stored.id);
    assert_eq!(count_jobs(&db.pool).await, 1);

    db.close().await;
}

/// A known selectors site is fetched with its embedded config.
#[tokio::test]
async fn url_fetch_of_a_known_site_uses_its_selector_config() {
    let db = common::TestDb::new_raw().await;
    let source = Arc::new(MockFetchSource::with_html(Ok(vec![sample_job("waymo")])));

    let url = "https://careers.withwaymo.com/jobs/search";
    manual::add_job_from_url(&db.pool, url, source.as_ref(), None)
        .await
        .expect("add_job_from_url");

    assert_eq!(source.html_calls(), vec![url.to_string()]);
    assert!(
        source.ats_calls().is_empty(),
        "a known site is not an ATS board"
    );
    assert_eq!(count_jobs(&db.pool).await, 1);

    db.close().await;
}

/// A URL on a host the user configured as a portal uses that portal's
/// provider, even though the registry does not know the site.
#[tokio::test]
async fn url_fetch_uses_a_configured_portal_for_the_host() {
    let db = common::TestDb::new_raw().await;

    portals::create(
        &db.pool,
        NewPortal {
            company_name: "Acme".into(),
            url: "https://careers.acme.example/careers".into(),
            render_mode: RenderMode::Static,
            scraper_config: Some(serde_json::json!({
                "provider": "eightfold",
                "domain": "acme.com"
            })),
        },
    )
    .await
    .expect("portal");

    let detail = r#"{
      "status": 200,
      "data": {
        "id": 7,
        "name": "Platform Engineer",
        "department": "Engineering",
        "locations": ["Remote"],
        "jobDescription": "<p>Run the platform.</p>"
      }
    }"#;
    let source = Arc::new(MockFetchSource::default().pages([Ok(detail.to_string())]));

    let result = manual::add_job_from_url(
        &db.pool,
        "https://careers.acme.example/careers/job/7",
        source.as_ref(),
        None,
    )
    .await
    .expect("add_job_from_url");

    assert_eq!(result.stored.company, "Acme");
    assert_eq!(
        result.stored.external_id.as_deref(),
        Some("eightfold:acme.com:7")
    );
    assert_eq!(
        result.stored.source_url, "https://careers.acme.example/careers/job/7",
        "the pasted URL is the fallback when the detail has no public URL"
    );
    assert_eq!(source.page_calls().len(), 1);
    assert!(source.html_calls().is_empty());

    db.close().await;
}

/// A URL that is not a known site still goes through the old chain.
#[tokio::test]
async fn an_unknown_host_still_uses_the_generic_path() {
    let db = common::TestDb::new_raw().await;
    let source = Arc::new(MockFetchSource::with_html(Ok(vec![sample_job("only")])));

    manual::add_job_from_url(
        &db.pool,
        "https://example.com/careers/senior-engineer",
        source.as_ref(),
        None,
    )
    .await
    .expect("add_job_from_url");

    assert_eq!(source.html_calls().len(), 1);
    assert!(source.page_calls().is_empty(), "no provider detail request");

    db.close().await;
}

// -----------------------------------------------------------------
// Per-job refresh
// -----------------------------------------------------------------

#[tokio::test]
async fn refresh_job_fills_a_missing_eightfold_description_in_place() {
    let db = common::TestDb::new_raw().await;
    let stored = job_descriptions::upsert(
        &db.pool,
        job_descriptions::NewJobDescription {
            title: "Senior Network Product Engineer".into(),
            company: "NVIDIA".into(),
            source_url: "https://jobs.nvidia.com/careers/job/893394864453".into(),
            raw_text: String::new(),
            fetch_tier: FetchTier::Tier1aAts,
            external_id: Some("eightfold:nvidia.com:893394864453".into()),
        },
    )
    .await
    .expect("store");

    let source = Arc::new(MockFetchSource::default().pages([Ok(NVIDIA_DETAIL.to_string())]));
    let refreshed = manual::refresh_job(&db.pool, stored.id, source.as_ref())
        .await
        .expect("refresh");

    assert_eq!(refreshed.id, stored.id);
    assert!(refreshed.raw_text.contains("network stack"));
    assert_eq!(count_jobs(&db.pool).await, 1, "a refresh never inserts");
    assert_eq!(source.page_calls().len(), 1);
    assert!(
        source.html_calls().is_empty(),
        "the provider id routes straight to the detail endpoint"
    );

    db.close().await;
}

const JSONLD_POSTING: &str = r#"<html><head>
<script type="application/ld+json">
{"@type":"JobPosting","title":"Engineer","description":"<p>Full description from JSON-LD.</p>"}
</script></head><body></body></html>"#;

#[tokio::test]
async fn refresh_job_replaces_a_teaser_from_the_posting_page() {
    let db = common::TestDb::new_raw().await;
    let stored = job_descriptions::upsert(
        &db.pool,
        job_descriptions::NewJobDescription {
            title: "Engineer".into(),
            company: "Acme".into(),
            source_url: "https://example.com/jobs/1".into(),
            raw_text: "teaser".into(),
            fetch_tier: FetchTier::Tier1bHtml,
            external_id: None,
        },
    )
    .await
    .expect("store");

    let source = Arc::new(MockFetchSource::default().pages([Ok(JSONLD_POSTING.to_string())]));
    let refreshed = manual::refresh_job(&db.pool, stored.id, source.as_ref())
        .await
        .expect("refresh");

    assert_eq!(refreshed.id, stored.id);
    assert!(refreshed.raw_text.contains("Full description from JSON-LD"));
    assert_eq!(count_jobs(&db.pool).await, 1);

    db.close().await;
}

#[tokio::test]
async fn a_failed_refresh_leaves_the_stored_text_alone() {
    let db = common::TestDb::new_raw().await;
    let stored = job_descriptions::upsert(
        &db.pool,
        job_descriptions::NewJobDescription {
            title: "Engineer".into(),
            company: "Acme".into(),
            source_url: "https://example.com/jobs/2".into(),
            raw_text: "teaser".into(),
            fetch_tier: FetchTier::Tier1bHtml,
            external_id: None,
        },
    )
    .await
    .expect("store");

    let source = Arc::new(
        MockFetchSource::default().pages([Ok("<html><body>No posting here.</body></html>".into())]),
    );
    let error = manual::refresh_job(&db.pool, stored.id, source.as_ref())
        .await
        .expect_err("no description");

    assert!(error.to_string().contains("no description"));
    let after = job_descriptions::get(&db.pool, stored.id)
        .await
        .expect("get")
        .expect("row");
    assert_eq!(after.raw_text, "teaser");
    assert_eq!(count_jobs(&db.pool).await, 1);

    db.close().await;
}
