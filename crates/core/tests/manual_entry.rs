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
