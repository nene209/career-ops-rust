//! Router tests.
//!
//! Two tests close the carried-forward Week 2 criteria:
//!
//! - `static_portal_with_ats_url_uses_tier1a`
//! - `js_required_escalates_to_tier2`

mod common;

use std::sync::atomic::AtomicBool;
use std::sync::Arc;

use career_ops_core::acquisition::ats;
use career_ops_core::acquisition::error::{AtsError, FailureCause};
use career_ops_core::acquisition::html::{PageReport, PaginationStop};
use career_ops_core::acquisition::router::{
    decide_initial_route, fetch_portal, fetch_portal_cancellable, preview, InitialRoute,
};
use career_ops_core::backend::{MockBrowserBackend, RenderedPage};
use career_ops_core::model::FetchTier;
use career_ops_core::repo::job_descriptions;
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
async fn both_tiers_failing_keeps_the_tier1_cause() {
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

    // The Tier 2 failure is recorded, but it must not replace the Tier 1 cause:
    // a sidecar that is not running says nothing about why the page failed.
    assert_eq!(err.cause, FailureCause::Blocked);
    assert!(err.detail.contains("Tier 1b"), "{}", err.detail);
    assert!(
        err.detail.contains("browser fallback also failed"),
        "{}",
        err.detail
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

// -----------------------------------------------------------------
// Detail enrichment
// -----------------------------------------------------------------

fn detail_portal(url: &str, config: serde_json::Value) -> NewPortal {
    NewPortal {
        company_name: "Acme".into(),
        url: url.into(),
        render_mode: RenderMode::Static,
        scraper_config: Some(config),
    }
}

fn detail_config(max_jobs: usize) -> serde_json::Value {
    serde_json::json!({
        "list": "article.job",
        "fields": { "title": "h2 a" },
        "detail": {
            "fields": { "description": ".job-description" },
            "max_jobs": max_jobs
        }
    })
}

fn detail_html(text: &str) -> String {
    format!(r#"<html><body><div class="job-description">{text}</div></body></html>"#)
}

async fn stored_texts(pool: &sqlx::SqlitePool) -> Vec<String> {
    job_descriptions::list_recent(pool, 100)
        .await
        .expect("list jobs")
        .into_iter()
        .map(|job| job.raw_text)
        .collect()
}

#[tokio::test]
async fn detail_enrichment_replaces_card_text() {
    let db = common::TestDb::new_raw().await;
    let portal = portals::create(
        &db.pool,
        detail_portal("https://example.com/careers", detail_config(5)),
    )
    .await
    .expect("create portal");

    let source = Arc::new(
        MockFetchSource::with_html(Ok(vec![sample_job("engineer")]))
            .pages([Ok(detail_html("Full description."))]),
    );

    let outcome = fetch_portal(&db.pool, portal.id, source.as_ref(), None)
        .await
        .expect("fetch");

    assert_eq!(outcome.jobs_fetched, 1);
    assert_eq!(outcome.details_attempted, 1);
    assert_eq!(outcome.details_enriched, 1);
    assert_eq!(
        source.page_calls(),
        vec!["https://boards.greenhouse.io/acme/jobs/engineer".to_string()],
        "the detail request must use the job's own URL"
    );
    assert_eq!(stored_texts(&db.pool).await, vec!["Full description."]);

    db.close().await;
}

#[tokio::test]
async fn jsonld_detail_enriches_without_selectors() {
    let db = common::TestDb::new_raw().await;
    let config = serde_json::json!({
        "list": "article.job",
        "fields": { "title": "h2 a" },
        "detail": { "max_jobs": 5 }
    });
    let portal = portals::create(
        &db.pool,
        detail_portal("https://example.com/careers", config),
    )
    .await
    .expect("create portal");

    let detail = r#"
    <html><head><script type="application/ld+json">
    {"@type":"JobPosting","title":"engineer","description":"<p>JSON-LD text.</p>"}
    </script></head></html>
    "#;
    let source = Arc::new(
        MockFetchSource::with_html(Ok(vec![sample_job("engineer")]))
            .pages([Ok(detail.to_string())]),
    );

    let outcome = fetch_portal(&db.pool, portal.id, source.as_ref(), None)
        .await
        .expect("fetch");

    assert_eq!(outcome.details_enriched, 1);
    assert_eq!(stored_texts(&db.pool).await, vec!["<p>JSON-LD text.</p>"]);

    db.close().await;
}

#[tokio::test]
async fn a_failed_detail_fetch_keeps_the_list_row() {
    let db = common::TestDb::new_raw().await;
    let portal = portals::create(
        &db.pool,
        detail_portal("https://example.com/careers", detail_config(5)),
    )
    .await
    .expect("create portal");

    let source = Arc::new(
        MockFetchSource::with_html(Ok(vec![sample_job("engineer")]))
            .pages([Err(AtsError::Timeout("slow".into()))]),
    );

    let outcome = fetch_portal(&db.pool, portal.id, source.as_ref(), None)
        .await
        .expect("the portal fetch still succeeds");

    assert_eq!(outcome.jobs_fetched, 1);
    assert_eq!(outcome.details_attempted, 1);
    assert_eq!(outcome.details_enriched, 0);
    assert_eq!(
        stored_texts(&db.pool).await,
        vec!["test"],
        "sample_job's list-level text is kept"
    );

    db.close().await;
}

#[tokio::test]
async fn max_jobs_caps_detail_requests() {
    let db = common::TestDb::new_raw().await;
    let portal = portals::create(
        &db.pool,
        detail_portal("https://example.com/careers", detail_config(2)),
    )
    .await
    .expect("create portal");

    let source = Arc::new(
        MockFetchSource::with_html(Ok(vec![sample_job("a"), sample_job("b"), sample_job("c")]))
            .pages([
                Ok(detail_html("A")),
                Ok(detail_html("B")),
                Ok(detail_html("C")),
            ]),
    );

    let outcome = fetch_portal(&db.pool, portal.id, source.as_ref(), None)
        .await
        .expect("fetch");

    assert_eq!(outcome.jobs_fetched, 3, "every list row is still stored");
    assert_eq!(outcome.details_attempted, 2);
    assert_eq!(source.page_calls().len(), 2, "the cap bounds requests");

    db.close().await;
}

#[tokio::test]
async fn cancellation_stops_before_the_first_detail_fetch() {
    let db = common::TestDb::new_raw().await;
    let portal = portals::create(
        &db.pool,
        detail_portal("https://example.com/careers", detail_config(5)),
    )
    .await
    .expect("create portal");

    let source = Arc::new(
        MockFetchSource::with_html(Ok(vec![sample_job("engineer")]))
            .pages([Ok(detail_html("Full description."))]),
    );
    let cancel = AtomicBool::new(true);

    let outcome =
        fetch_portal_cancellable(&db.pool, portal.id, source.as_ref(), None, Some(&cancel))
            .await
            .expect("fetch");

    assert!(source.page_calls().is_empty(), "cancelled before enriching");
    assert_eq!(outcome.details_attempted, 0);
    assert_eq!(outcome.jobs_fetched, 1, "the list rows are still stored");

    db.close().await;
}

#[tokio::test]
async fn enrichment_is_idempotent() {
    let db = common::TestDb::new_raw().await;
    let portal = portals::create(
        &db.pool,
        detail_portal("https://example.com/careers", detail_config(5)),
    )
    .await
    .expect("create portal");

    let job = sample_job("engineer");
    let source = Arc::new(
        MockFetchSource::sequence_html(vec![Ok(vec![job.clone()]), Ok(vec![job])]).pages([
            Ok(detail_html("Full description.")),
            Ok(detail_html("Full description.")),
        ]),
    );

    fetch_portal(&db.pool, portal.id, source.as_ref(), None)
        .await
        .expect("first fetch");
    fetch_portal(&db.pool, portal.id, source.as_ref(), None)
        .await
        .expect("second fetch");

    let jobs = job_descriptions::list_recent(&db.pool, 10)
        .await
        .expect("list jobs");
    assert_eq!(jobs.len(), 1, "the second fetch must update, not duplicate");
    assert_eq!(jobs[0].raw_text, "Full description.");

    db.close().await;
}

#[tokio::test]
async fn a_detail_only_config_enriches_an_ats_board() {
    let db = common::TestDb::new_raw().await;
    let config = serde_json::json!({ "detail": { "max_jobs": 1 } });
    let portal = portals::create(
        &db.pool,
        detail_portal("https://boards.greenhouse.io/acme/jobs/1", config),
    )
    .await
    .expect("create portal");

    let source = Arc::new(
        MockFetchSource::with_ats(Ok(vec![sample_job("engineer")])).pages([Ok(
            r#"<html><head><script type="application/ld+json">
        {"@type":"JobPosting","title":"engineer","description":"From the detail page."}
        </script></head></html>"#
                .to_string(),
        )]),
    );

    let outcome = fetch_portal(&db.pool, portal.id, source.as_ref(), None)
        .await
        .expect("fetch");

    assert_eq!(outcome.tier, FetchTier::Tier1aAts);
    assert_eq!(outcome.details_enriched, 1);
    assert_eq!(
        stored_texts(&db.pool).await,
        vec!["From the detail page."],
        "an ATS board can opt into descriptions with a detail-only config"
    );

    db.close().await;
}

#[tokio::test]
async fn a_partial_pagination_is_reported_in_the_outcome() {
    let db = common::TestDb::new_raw().await;
    let portal = portals::create(
        &db.pool,
        detail_portal(
            "https://example.com/careers",
            serde_json::json!({
                "list": "article.job",
                "fields": { "title": "h2 a" },
                "pagination": { "next": "a[rel=next]", "max_pages": 5 }
            }),
        ),
    )
    .await
    .expect("create portal");

    let report = PageReport {
        jobs: vec![sample_job("a"), sample_job("b")],
        pages_fetched: 2,
        stop: PaginationStop::ExtractionFailed,
    };
    let source = Arc::new(MockFetchSource::with_html_report(Ok(report)));

    let outcome = fetch_portal(&db.pool, portal.id, source.as_ref(), None)
        .await
        .expect("fetch");

    assert_eq!(outcome.jobs_fetched, 2);
    assert_eq!(outcome.pages_fetched, Some(2));
    assert_eq!(
        outcome.pagination_stopped,
        Some(PaginationStop::ExtractionFailed),
        "a page that could not be read must not disappear from the outcome"
    );

    db.close().await;
}

#[tokio::test]
async fn no_detail_config_means_no_detail_requests() {
    let db = common::TestDb::new_raw().await;
    let portal = portals::create(&db.pool, static_portal("https://example.com/careers"))
        .await
        .expect("create portal");

    let source = Arc::new(MockFetchSource::with_html(Ok(vec![sample_job("engineer")])));

    let outcome = fetch_portal(&db.pool, portal.id, source.as_ref(), None)
        .await
        .expect("fetch");

    assert_eq!(outcome.details_attempted, 0);
    assert!(source.page_calls().is_empty());

    db.close().await;
}

// -----------------------------------------------------------------
// SmartRecruiters detail endpoint
// -----------------------------------------------------------------

/// A list-level row in the shape `smartrecruiters::normalize` produces: the
/// posting id in the URL and no description.
fn smartrecruiters_job(title: &str, id: &str) -> career_ops_core::acquisition::ats::NormalizedJob {
    let mut job = sample_job(title);
    job.source_url =
        Url::parse(&format!("https://jobs.smartrecruiters.com/Ubisoft2/{id}")).expect("job url");
    job.raw_text = String::new();
    job.department = None;
    job.employment_type = None;
    job
}

const SMART_DETAIL: &str = r#"{
  "id": "744000159231819",
  "department": { "id": 42, "label": "Art/Creative" },
  "typeOfEmployment": { "id": "permanent", "label": "Full-time" },
  "jobAd": {
    "sections": {
      "companyDescription": { "title": "Company Description", "text": "<p>We make games.</p>" },
      "jobDescription": { "title": "Job Description", "text": "<p>Build them.</p>" },
      "qualifications": { "title": "Qualifications", "text": "<p>Ten years.</p>" },
      "additionalInformation": { "title": "Additional Information", "text": "" }
    }
  }
}"#;

#[tokio::test]
async fn a_smartrecruiters_board_enriches_from_the_posting_api() {
    let db = common::TestDb::new_raw().await;
    let portal = portals::create(
        &db.pool,
        detail_portal(
            "https://jobs.smartrecruiters.com/Ubisoft2",
            serde_json::json!({ "detail": { "max_jobs": 2 } }),
        ),
    )
    .await
    .expect("create portal");

    let job = smartrecruiters_job("Technical Art Director", "744000159231819");
    let source =
        Arc::new(MockFetchSource::with_ats(Ok(vec![job])).pages([Ok(SMART_DETAIL.to_string())]));

    let outcome = fetch_portal(&db.pool, portal.id, source.as_ref(), None)
        .await
        .expect("fetch");

    assert_eq!(outcome.tier, FetchTier::Tier1aAts);
    assert_eq!(outcome.details_attempted, 1);
    assert_eq!(outcome.details_enriched, 1);
    assert_eq!(
        source.page_calls(),
        vec![
            "https://api.smartrecruiters.com/v1/companies/Ubisoft2/postings/744000159231819"
                .to_string()
        ],
        "SmartRecruiters reads its own detail endpoint, not the posting page"
    );

    let texts = stored_texts(&db.pool).await;
    assert_eq!(texts.len(), 1);
    assert!(texts[0].starts_with("Company Description"), "{}", texts[0]);
    assert!(texts[0].contains("<p>Build them.</p>"));

    db.close().await;
}

#[tokio::test]
async fn a_failed_smartrecruiters_detail_keeps_the_list_row() {
    let db = common::TestDb::new_raw().await;
    let portal = portals::create(
        &db.pool,
        detail_portal(
            "https://jobs.smartrecruiters.com/Ubisoft2",
            serde_json::json!({ "detail": { "max_jobs": 2 } }),
        ),
    )
    .await
    .expect("create portal");

    let source = Arc::new(
        MockFetchSource::with_ats(Ok(vec![smartrecruiters_job("Engineer", "1")]))
            .pages([Err(AtsError::NotFound("gone".into()))]),
    );

    let outcome = fetch_portal(&db.pool, portal.id, source.as_ref(), None)
        .await
        .expect("a failed detail must not fail the fetch");

    assert_eq!(outcome.jobs_fetched, 1);
    assert_eq!(outcome.details_attempted, 1);
    assert_eq!(outcome.details_enriched, 0);
    assert_eq!(stored_texts(&db.pool).await, vec![String::new()]);

    db.close().await;
}

#[tokio::test]
async fn the_detail_cap_bounds_smartrecruiters_requests() {
    let db = common::TestDb::new_raw().await;
    let portal = portals::create(
        &db.pool,
        detail_portal(
            "https://jobs.smartrecruiters.com/Ubisoft2",
            serde_json::json!({ "detail": { "max_jobs": 2 } }),
        ),
    )
    .await
    .expect("create portal");

    let jobs = vec![
        smartrecruiters_job("a", "1"),
        smartrecruiters_job("b", "2"),
        smartrecruiters_job("c", "3"),
    ];
    let source = Arc::new(
        MockFetchSource::with_ats(Ok(jobs))
            .pages([Ok(SMART_DETAIL.to_string()), Ok(SMART_DETAIL.to_string())]),
    );

    let outcome = fetch_portal(&db.pool, portal.id, source.as_ref(), None)
        .await
        .expect("fetch");

    assert_eq!(outcome.jobs_fetched, 3, "every list row is still stored");
    assert_eq!(outcome.details_attempted, 2);
    assert_eq!(source.page_calls().len(), 2, "the cap bounds requests");

    db.close().await;
}

// -----------------------------------------------------------------
// Preview ("Test config")
// -----------------------------------------------------------------

fn preview_config(max_pages: usize) -> String {
    serde_json::json!({
        "list": "article.job",
        "fields": { "title": "h2 a" },
        "pagination": { "next": "a[rel=next]", "max_pages": max_pages },
        "detail": {
            "fields": { "description": ".job-description" },
            "max_jobs": 40
        }
    })
    .to_string()
}

#[tokio::test]
async fn preview_reports_page_one_counts_and_samples() {
    let url = Url::parse("https://example.com/careers").expect("url");
    let source = MockFetchSource::with_html(Ok(vec![
        sample_job("a"),
        sample_job("b"),
        sample_job("c"),
        sample_job("d"),
    ]))
    .pages([Ok(detail_html("Full description."))]);

    let report = preview(
        &url,
        "Acme",
        "static",
        Some(&preview_config(5)),
        &source,
        None,
    )
    .await
    .expect("preview");

    assert_eq!(report.tier, FetchTier::Tier1bHtml);
    assert_eq!(report.jobs_found, 4);
    assert_eq!(report.pages_planned, 5, "reported, not fetched");
    assert_eq!(report.sample.len(), 3, "three titles at most");
    assert_eq!(report.sample[0].title, "a");
    assert_eq!(
        report.first_description_bytes,
        Some("Full description.".len())
    );
    assert_eq!(
        report.details_attempted, 1,
        "stops at the first description"
    );
    assert_eq!(
        source.html_calls(),
        vec![url.to_string()],
        "one list page regardless of max_pages"
    );
    assert_eq!(source.page_calls().len(), 1);
}

#[tokio::test]
async fn preview_makes_at_most_three_detail_requests() {
    let url = Url::parse("https://example.com/careers").expect("url");
    let source = MockFetchSource::with_html(Ok(vec![
        sample_job("a"),
        sample_job("b"),
        sample_job("c"),
        sample_job("d"),
        sample_job("e"),
    ]))
    .pages([
        Err(AtsError::Timeout("slow".into())),
        Err(AtsError::Timeout("slow".into())),
        Err(AtsError::Timeout("slow".into())),
        Err(AtsError::Timeout("slow".into())),
    ]);

    let report = preview(
        &url,
        "Acme",
        "static",
        Some(&preview_config(1)),
        &source,
        None,
    )
    .await
    .expect("preview");

    assert_eq!(report.jobs_found, 5);
    assert_eq!(report.details_attempted, 3, "the preview cap wins");
    assert_eq!(source.page_calls().len(), 3);
    assert!(report.first_description_bytes.is_none());
}

#[tokio::test]
async fn preview_reports_one_page_when_pagination_has_no_mechanism() {
    let url = Url::parse("https://example.com/careers").expect("url");
    let config = serde_json::json!({
        "list": "article.job",
        "fields": { "title": "h2 a" },
        "pagination": { "max_pages": 10 }
    })
    .to_string();
    let source = MockFetchSource::with_html(Ok(vec![sample_job("a")]));

    let report = preview(&url, "Acme", "static", Some(&config), &source, None)
        .await
        .expect("preview");

    assert_eq!(
        report.pages_planned, 1,
        "no next link and no template means a single page, whatever max_pages says"
    );
}

#[tokio::test]
async fn preview_without_a_detail_config_fetches_no_details() {
    let url = Url::parse("https://example.com/careers").expect("url");
    let config =
        serde_json::json!({ "list": "article.job", "fields": { "title": "h2 a" } }).to_string();
    let source = MockFetchSource::with_html(Ok(vec![sample_job("a")]));

    let report = preview(&url, "Acme", "static", Some(&config), &source, None)
        .await
        .expect("preview");

    assert_eq!(report.details_attempted, 0);
    assert!(source.page_calls().is_empty());
}

#[tokio::test]
async fn preview_rejects_an_invalid_config_before_fetching() {
    let url = Url::parse("https://example.com/careers").expect("url");
    let source = MockFetchSource::with_html(Ok(vec![sample_job("a")]));

    let error = preview(
        &url,
        "Acme",
        "static",
        Some(r#"{"list": 5}"#),
        &source,
        None,
    )
    .await
    .expect_err("invalid config");

    assert_eq!(error.cause, FailureCause::InvalidRequest);
    assert!(error.detail.contains("not valid JSON"), "{}", error.detail);
    assert!(
        source.html_calls().is_empty(),
        "no request for a bad config"
    );
}

#[tokio::test]
async fn preview_an_ats_board_uses_the_adapter_and_its_detail_endpoint() {
    let url = Url::parse("https://jobs.smartrecruiters.com/Ubisoft2").expect("url");
    let config = serde_json::json!({ "detail": { "max_jobs": 40 } }).to_string();
    let source = MockFetchSource::with_ats(Ok(vec![smartrecruiters_job(
        "Technical Art Director",
        "744000159231819",
    )]))
    .pages([Ok(SMART_DETAIL.to_string())]);

    let report = preview(&url, "Ubisoft", "static", Some(&config), &source, None)
        .await
        .expect("preview");

    assert_eq!(report.tier, FetchTier::Tier1aAts);
    assert_eq!(report.jobs_found, 1);
    assert_eq!(report.pages_planned, 0, "the adapter owns pagination");
    assert_eq!(
        source.ats_calls(),
        vec![("smartrecruiters".to_string(), "Ubisoft2".to_string())]
    );
    assert_eq!(
        source.page_calls(),
        vec![
            "https://api.smartrecruiters.com/v1/companies/Ubisoft2/postings/744000159231819"
                .to_string()
        ]
    );
    assert!(report
        .first_description_bytes
        .is_some_and(|bytes| bytes > 0));
}
