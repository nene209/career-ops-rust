//! Live ATS integration tests.
//!
//! **These tests hit the network and are skipped by default.** Set
//! `RUN_LIVE_ATS_TESTS=1` to run them:
//!
//! ```text
//! RUN_LIVE_ATS_TESTS=1 cargo test -p career-ops-core --test ats_live -- --nocapture
//! ```
//!
//! ## Purpose
//!
//! The fixture tests in `ats_adapters.rs` prove the adapters parse the
//! shapes they were written against. They cannot prove the upstream
//! APIs still return those shapes. This file closes that gap.
//!
//! ## Why the assertions are weak
//!
//! The point is detecting an upstream API change, not asserting specific
//! content. `!jobs.is_empty()` and "every title is non-empty" catch the
//! failure mode that matters: a response shape change that makes
//! `serde` produce empty or partially-populated records. Asserting
//! counts or specific job titles would make the tests fail every time a
//! company opens or closes a role, which is noise, not signal.
//!
//! ## Slugs
//!
//! Each test has a default board slug and an environment-variable
//! override. If a company stops using an ATS, override the slug without
//! editing this file:
//!
//! ```text
//! LIVE_GREENHOUSE_SLUG=stripe RUN_LIVE_ATS_TESTS=1 cargo test ...
//! ```

use career_ops_core::acquisition::ats::{
    self, ashby::AshbyAdapter, greenhouse::GreenhouseAdapter, lever::LeverAdapter,
    smartrecruiters::SmartRecruitersAdapter, AtsAdapter,
};
use career_ops_core::acquisition::eightfold;
use career_ops_core::acquisition::http;
use url::Url;

fn live_enabled() -> bool {
    std::env::var("RUN_LIVE_ATS_TESTS").is_ok()
}

fn slug_or(var: &str, default: &str) -> String {
    std::env::var(var).unwrap_or_else(|_| default.to_string())
}

/// Fetch, then assert the weak invariants every adapter must satisfy.
async fn assert_well_formed(
    adapter: &dyn AtsAdapter,
    slug: &str,
    company: &str,
) -> Vec<career_ops_core::acquisition::ats::NormalizedJob> {
    let client = http::default_client().expect("http client");
    let jobs = adapter
        .fetch_jobs(&client, slug, company)
        .await
        .unwrap_or_else(|e| panic!("{} live fetch for `{slug}` failed: {e:?}", adapter.name()));

    assert!(
        !jobs.is_empty(),
        "{} board `{slug}` returned zero jobs — the board may have moved, or the API shape changed",
        adapter.name()
    );

    for job in &jobs {
        assert!(
            !job.title.trim().is_empty(),
            "{} produced a job with an empty title",
            adapter.name()
        );
        assert!(
            job.source_url.host_str().is_some(),
            "{} produced a job with a relative source_url: {}",
            adapter.name(),
            job.source_url
        );
        assert!(
            !job.external_id.trim().is_empty(),
            "{} produced a job with an empty external_id",
            adapter.name()
        );
    }

    eprintln!("{} / {slug}: {} jobs", adapter.name(), jobs.len());
    jobs
}

// -----------------------------------------------------------------
// Detection round-trip (offline, always runs)
// -----------------------------------------------------------------

/// Confirms that the board URL each live test uses is the URL the
/// corresponding adapter claims to detect. This is offline, so it runs
/// in the default suite. If it fails, the live test would have run
/// against a URL the adapter does not recognise — which is a different
/// bug from an upstream API change.
#[test]
fn live_board_urls_match_adapter_detection() {
    let adapters = ats::adapters();

    let boards = [
        ("https://boards.greenhouse.io/figma", "greenhouse", "figma"),
        ("https://jobs.lever.co/spotify", "lever", "spotify"),
        ("https://jobs.ashbyhq.com/linear", "ashby", "linear"),
        (
            "https://jobs.smartrecruiters.com/Ubisoft2",
            "smartrecruiters",
            "Ubisoft2",
        ),
    ];

    for (url_str, expected_name, expected_slug) in boards {
        let url = Url::parse(url_str).expect("url");
        let matched: Vec<(String, String)> = adapters
            .iter()
            .filter_map(|a| a.detect(&url).map(|s| (a.name().to_string(), s)))
            .collect();

        assert!(
            matched
                .iter()
                .any(|(n, s)| n == expected_name && s == expected_slug),
            "expected {expected_name}/{expected_slug} for {url_str}, got {matched:?}"
        );
    }
}

// -----------------------------------------------------------------
// Live tests
// -----------------------------------------------------------------

#[tokio::test]
async fn greenhouse_live() {
    if !live_enabled() {
        eprintln!("skipping greenhouse_live: RUN_LIVE_ATS_TESTS not set");
        return;
    }
    let slug = slug_or("LIVE_GREENHOUSE_SLUG", "figma");
    assert_well_formed(&GreenhouseAdapter, &slug, "Figma").await;
}

#[tokio::test]
async fn lever_live() {
    if !live_enabled() {
        eprintln!("skipping lever_live: RUN_LIVE_ATS_TESTS not set");
        return;
    }
    let slug = slug_or("LIVE_LEVER_SLUG", "spotify");
    assert_well_formed(&LeverAdapter, &slug, "Spotify").await;
}

#[tokio::test]
async fn ashby_live() {
    if !live_enabled() {
        eprintln!("skipping ashby_live: RUN_LIVE_ATS_TESTS not set");
        return;
    }
    let slug = slug_or("LIVE_ASHBY_SLUG", "linear");
    assert_well_formed(&AshbyAdapter, &slug, "Linear").await;
}

#[tokio::test]
async fn smartrecruiters_live() {
    if !live_enabled() {
        eprintln!("skipping smartrecruiters_live: RUN_LIVE_ATS_TESTS not set");
        return;
    }
    // Ubisoft is a large, long-lived SmartRecruiters board. The company's
    // own board (`smartrecruiters`) also works but is smaller. The board slug
    // is `Ubisoft2`
    let slug = slug_or("LIVE_SMARTRECRUITERS_SLUG", "Ubisoft2");
    assert_well_formed(&SmartRecruitersAdapter, &slug, "Ubisoft").await;
}

/// The list endpoint omits descriptions, so the adapter's value depends on the
/// detail endpoint still returning `jobAd.sections`. Assert that one real
/// posting yields text: a shape change there would leave every row empty while
/// the list test above stayed green.
#[tokio::test]
async fn smartrecruiters_detail_live() {
    if !live_enabled() {
        eprintln!("skipping smartrecruiters_detail_live: RUN_LIVE_ATS_TESTS not set");
        return;
    }

    let slug = slug_or("LIVE_SMARTRECRUITERS_SLUG", "Ubisoft2");
    let adapter = SmartRecruitersAdapter;
    let client = http::default_client().expect("http client");
    let jobs = adapter
        .fetch_jobs(&client, &slug, "Ubisoft")
        .await
        .expect("live fetch");
    let job = jobs.first().expect("a non-empty board");

    let url = adapter.detail_url(&slug, job).expect("detail url");
    let resp = http::get_with_retry(&client, url.as_str())
        .await
        .expect("detail request");
    assert!(
        resp.status().is_success(),
        "detail returned {}",
        resp.status()
    );
    let body = resp.text().await.expect("detail body");

    let detail = adapter
        .detail_from_body(&body)
        .unwrap_or_else(|| panic!("detail for `{}` did not parse", job.title));
    assert!(
        !detail.text.trim().is_empty(),
        "detail for `{}` produced no description text; jobAd.sections may have changed",
        job.title
    );
    eprintln!(
        "smartrecruiters detail: {} bytes for `{}`",
        detail.text.len(),
        job.title
    );
}

/// The Eightfold engine end to end: list pages, then the position detail that
/// supplies the description. NVIDIA is the reference site.
#[tokio::test]
async fn eightfold_live() {
    if !live_enabled() {
        eprintln!("skipping eightfold_live: RUN_LIVE_ATS_TESTS not set");
        return;
    }

    let host = slug_or("LIVE_EIGHTFOLD_HOST", "jobs.nvidia.com");
    let domain = slug_or("LIVE_EIGHTFOLD_DOMAIN", "nvidia.com");
    let client = http::default_client().expect("http client");

    let report = eightfold::fetch_jobs(&client, &host, &domain, "NVIDIA", 2)
        .await
        .expect("live eightfold list");
    assert!(
        !report.jobs.is_empty(),
        "{host}/{domain} returned no positions — the board may have moved or the API shape changed"
    );

    for job in &report.jobs {
        assert!(!job.title.trim().is_empty());
        assert!(
            job.external_id.starts_with(&format!("eightfold:{domain}:")),
            "unstable external id: {}",
            job.external_id
        );
        assert_eq!(job.source_url.host_str(), Some(host.as_str()));
    }

    let job = &report.jobs[0];
    let url = eightfold::detail_url(&host, &domain, job).expect("detail url");
    let response = http::get_with_retry(&client, url.as_str())
        .await
        .expect("detail request");
    assert!(
        response.status().is_success(),
        "detail returned {}",
        response.status()
    );
    let body = response.text().await.expect("detail body");
    let detail = eightfold::detail_from_body(&body)
        .unwrap_or_else(|| panic!("detail for `{}` did not parse", job.title));
    assert!(
        !detail.text.trim().is_empty(),
        "detail for `{}` produced no description text; jobDescription may have changed",
        job.title
    );

    eprintln!(
        "eightfold {host}/{domain}: {} jobs in {} pages; detail {} bytes for `{}`",
        report.jobs.len(),
        report.pages_fetched,
        detail.text.len(),
        job.title
    );
}
