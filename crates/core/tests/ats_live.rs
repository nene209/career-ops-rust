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
