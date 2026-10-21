//! ATS adapter and HTML-extraction tests.
//!
//! Every adapter is tested against its pure `parse_*` function, not
//! against HTTP. The adapter's `fetch_jobs` is a thin wrapper over
//! `parse_*` + a single `reqwest` call; the parsing and normalization
//! logic — where all the format-specific quirks live — is what these
//! tests exercise.
//!
//! Live HTTP tests are environment-gated and run in the
//! `acquisition-fixtures` CI job; they are not part of this file.

use career_ops_core::acquisition::ats::{self, ashby, greenhouse, lever, smartrecruiters};
use career_ops_core::acquisition::html::{
    self, jsonld,
    selectors::{self, SelectorConfig},
};
use url::Url;

// -----------------------------------------------------------------
// Greenhouse
// -----------------------------------------------------------------

const GREENHOUSE_FIXTURE: &str = r#"{
  "jobs": [
    {
      "id": 12345,
      "title": "Senior Backend Engineer",
      "updated_at": "2026-10-15T10:30:00-04:00",
      "absolute_url": "https://boards.greenhouse.io/acme/jobs/12345",
      "location": { "name": "Remote - US" },
      "content": "&lt;p&gt;We are &lt;strong&gt;hiring&lt;/strong&gt; a backend engineer.&lt;/p&gt;",
      "departments": [{ "name": "Engineering" }]
    },
    {
      "id": 12346,
      "title": "Product Designer",
      "updated_at": "2026-10-14T09:00:00-04:00",
      "absolute_url": "https://boards.greenhouse.io/acme/jobs/12346",
      "location": { "name": "New York, NY" },
      "content": "&lt;p&gt;Design things.&lt;/p&gt;",
      "departments": [{ "name": "Design" }]
    }
  ]
}"#;

#[test]
fn greenhouse_parses_full_fixture() {
    let jobs = greenhouse::parse(GREENHOUSE_FIXTURE, "Acme").expect("parse");
    assert_eq!(jobs.len(), 2);

    let first = &jobs[0];
    assert_eq!(first.title, "Senior Backend Engineer");
    assert_eq!(first.company, "Acme");
    assert_eq!(first.external_id, "12345");
    assert_eq!(first.location.as_deref(), Some("Remote - US"));
    assert_eq!(first.department.as_deref(), Some("Engineering"));
    assert_eq!(first.source_url.host_str(), Some("boards.greenhouse.io"));
    assert!(first.posted_at.is_some());
}

#[test]
fn greenhouse_unescapes_html_entities_in_content() {
    // content=true returns double-encoded HTML: entities wrapping tags.
    // The adapter must unescape so downstream code sees real HTML.
    let jobs = greenhouse::parse(GREENHOUSE_FIXTURE, "Acme").expect("parse");
    let raw = &jobs[0].raw_text;

    assert!(
        raw.contains("<p>"),
        "expected literal <p> after unescape, got: {raw}"
    );
    assert!(
        raw.contains("<strong>"),
        "expected literal <strong> after unescape"
    );
    assert!(!raw.contains("&lt;"), "entities were not unescaped: {raw}");
}

#[test]
fn greenhouse_empty_board_is_not_an_error() {
    let jobs = greenhouse::parse(r#"{"jobs": []}"#, "Acme").expect("parse");
    assert!(jobs.is_empty());
}

#[test]
fn greenhouse_malformed_json_errors() {
    assert!(greenhouse::parse(r#"{"jobs": ["#, "Acme").is_err());
}

// -----------------------------------------------------------------
// Lever
// -----------------------------------------------------------------

const LEVER_FIXTURE: &str = r#"[
  {
    "id": "abc-123",
    "text": "Backend Engineer",
    "hostedUrl": "https://jobs.lever.co/acme/abc-123",
    "createdAt": 1729000000000,
    "descriptionPlain": "We build things.",
    "categories": {
      "location": "San Francisco, CA",
      "department": "Engineering",
      "commitment": "Full-time"
    },
    "workplaceType": "Hybrid"
  }
]"#;

#[test]
fn lever_parses_bare_array() {
    let jobs = lever::parse(LEVER_FIXTURE, "Acme").expect("parse");
    assert_eq!(jobs.len(), 1);

    let job = &jobs[0];
    assert_eq!(job.title, "Backend Engineer");
    assert_eq!(job.company, "Acme");
    assert_eq!(job.external_id, "abc-123");
    assert_eq!(job.location.as_deref(), Some("San Francisco, CA"));
    assert_eq!(job.department.as_deref(), Some("Engineering"));
    assert_eq!(job.employment_type.as_deref(), Some("Full-time"));
    assert_eq!(job.workplace_type.as_deref(), Some("Hybrid"));
    assert_eq!(job.source_url.host_str(), Some("jobs.lever.co"));
}

#[test]
fn lever_converts_epoch_millis_to_datetime() {
    // Lever's createdAt is epoch milliseconds, not ISO-8601. A naive
    // parse as seconds would produce a date in 1970.
    let jobs = lever::parse(LEVER_FIXTURE, "Acme").expect("parse");
    let posted = jobs[0].posted_at.expect("posted_at present");

    // 1729000000000 ms is 2024-10-15T12:26:40Z. Assert the year, which
    // is precise enough to catch a seconds-vs-millis mistake without
    // depending on clock skew.
    assert_eq!(
        posted.format("%Y").to_string(),
        "2024",
        "epoch-millis conversion produced wrong year: {posted}"
    );
}

#[test]
fn lever_empty_board_is_not_an_error() {
    let jobs = lever::parse("[]", "Acme").expect("parse");
    assert!(jobs.is_empty());
}

// -----------------------------------------------------------------
// Ashby
// -----------------------------------------------------------------

const ASHBY_FIXTURE: &str = r#"{
  "jobs": [
    {
      "title": "Staff Engineer",
      "location": "Remote - US",
      "department": "Engineering",
      "team": null,
      "isRemote": true,
      "workplaceType": "Hybrid",
      "descriptionPlain": "Staff role.",
      "publishedAt": "2026-10-10T14:00:00.000Z",
      "employmentType": "FullTime",
      "jobUrl": "https://jobs.ashbyhq.com/acme/xyz-789"
    }
  ]
}"#;

#[test]
fn ashby_parses_full_fixture() {
    let jobs = ashby::parse(ASHBY_FIXTURE, "Acme").expect("parse");
    assert_eq!(jobs.len(), 1);

    let job = &jobs[0];
    assert_eq!(job.title, "Staff Engineer");
    assert_eq!(job.location.as_deref(), Some("Remote - US"));
    assert_eq!(job.department.as_deref(), Some("Engineering"));
    assert_eq!(job.employment_type.as_deref(), Some("FullTime"));
    assert!(job.posted_at.is_some());
}

#[test]
fn ashby_prefers_workplace_type_over_is_remote() {
    // Ashby sets isRemote=true for Hybrid roles. Trusting isRemote
    // would mis-classify hybrid postings as fully remote.
    let jobs = ashby::parse(ASHBY_FIXTURE, "Acme").expect("parse");
    assert_eq!(
        jobs[0].workplace_type.as_deref(),
        Some("Hybrid"),
        "workplaceType must win over isRemote"
    );
}

#[test]
fn ashby_derives_external_id_from_url() {
    // Ashby list responses do not include a stable job ID field. The
    // adapter derives it from the URL's final path segment.
    let jobs = ashby::parse(ASHBY_FIXTURE, "Acme").expect("parse");
    assert_eq!(jobs[0].external_id, "xyz-789");
}

#[test]
fn ashby_empty_board_is_not_an_error() {
    let jobs = ashby::parse(r#"{"jobs": []}"#, "Acme").expect("parse");
    assert!(jobs.is_empty());
}

// -----------------------------------------------------------------
// SmartRecruiters
// -----------------------------------------------------------------

const SMART_PAGE_ENGLISH: &str = r#"{
  "offset": 0,
  "limit": 100,
  "totalFound": 1,
  "content": [
    {
      "id": "sr-001",
      "name": "Principal Engineer",
      "refNumber": "REF-001",
      "language": { "code": "en" },
      "location": { "city": "Berlin", "country": "de" },
      "releasedDate": "2026-10-01T00:00:00Z"
    }
  ]
}"#;

const SMART_PAGE_DUPLICATE_LANG: &str = r#"{
  "offset": 0,
  "limit": 100,
  "totalFound": 2,
  "content": [
    {
      "id": "sr-001-de",
      "name": "Principal Engineer (DE)",
      "refNumber": "REF-001",
      "language": { "code": "de" },
      "location": { "city": "Berlin", "country": "de" },
      "releasedDate": "2026-10-01T00:00:00Z"
    },
    {
      "id": "sr-001-en",
      "name": "Principal Engineer",
      "refNumber": "REF-001",
      "language": { "code": "en" },
      "location": { "city": "Berlin", "country": "de" },
      "releasedDate": "2026-10-01T00:00:00Z"
    }
  ]
}"#;

#[test]
fn smartrecruiters_parses_page() {
    let page = smartrecruiters::parse_page(SMART_PAGE_ENGLISH).expect("parse");
    assert_eq!(page.total_found, 1);
    assert_eq!(page.content.len(), 1);
    assert_eq!(page.content[0].ref_number.as_deref(), Some("REF-001"));
}

#[test]
fn smartrecruiters_dedupes_language_copies_and_prefers_english() {
    // The same posting appears once per language, all sharing refNumber.
    // A 210-posting board published in two languages would otherwise land
    // as 420 rows.
    let page = smartrecruiters::parse_page(SMART_PAGE_DUPLICATE_LANG).expect("parse");
    let deduped = smartrecruiters::dedupe_by_ref(page.content);

    assert_eq!(deduped.len(), 1, "expected one row per refNumber");
    assert_eq!(deduped[0].id, "sr-001-en", "English copy must be preferred");
}

#[test]
fn smartrecruiters_combines_city_and_country_into_location() {
    let page = smartrecruiters::parse_page(SMART_PAGE_ENGLISH).expect("parse");
    let normalized =
        smartrecruiters::normalize(page.content.into_iter().next().unwrap(), "Acme", "acme")
            .expect("normalize");

    assert_eq!(normalized.location.as_deref(), Some("Berlin, de"));
    assert_eq!(normalized.external_id, "REF-001");
}

#[test]
fn smartrecruiters_empty_page_is_not_an_error() {
    let page =
        smartrecruiters::parse_page(r#"{"offset":0,"limit":100,"totalFound":0,"content":[]}"#)
            .expect("parse");
    assert!(page.content.is_empty());
}

// -----------------------------------------------------------------
// URL detection
// -----------------------------------------------------------------

#[test]
fn detection_identifies_each_supported_ats() {
    let adapters = ats::adapters();

    let cases = [
        (
            "https://boards.greenhouse.io/acme/jobs/123",
            "greenhouse",
            "acme",
        ),
        ("https://jobs.lever.co/acme/abc-123", "lever", "acme"),
        ("https://jobs.ashbyhq.com/acme/xyz-789", "ashby", "acme"),
        (
            "https://careers.smartrecruiters.com/acme/listing",
            "smartrecruiters",
            "acme",
        ),
    ];

    for (url_str, expected_name, expected_slug) in cases {
        let url = Url::parse(url_str).unwrap();
        let matched: Vec<(String, String)> = adapters
            .iter()
            .filter_map(|a| a.detect(&url).map(|slug| (a.name().to_string(), slug)))
            .collect();

        assert!(
            matched
                .iter()
                .any(|(n, s)| n == expected_name && s == expected_slug),
            "expected {expected_name}/{expected_slug} for {url_str}, got {matched:?}"
        );
    }
}

#[test]
fn detection_returns_none_for_unknown_hosts() {
    let adapters = ats::adapters();
    let url = Url::parse("https://example.com/careers").unwrap();

    for adapter in &adapters {
        assert!(
            adapter.detect(&url).is_none(),
            "adapter {} should not claim example.com",
            adapter.name()
        );
    }
}

// -----------------------------------------------------------------
// JSON-LD extraction
// -----------------------------------------------------------------

const JSONLD_SINGLE: &str = r#"
<html><head>
<script type="application/ld+json">
{
  "@context": "https://schema.org",
  "@type": "JobPosting",
  "title": "Data Engineer",
  "description": "<p>Build pipelines.</p>",
  "datePosted": "2026-10-12",
  "employmentType": "FULL_TIME",
  "jobLocation": {
    "@type": "Place",
    "address": { "addressLocality": "London" }
  },
  "identifier": { "@type": "PropertyValue", "value": "job-42" }
}
</script>
</head><body></body></html>
"#;

#[test]
fn jsonld_extracts_single_job_posting() {
    let url = Url::parse("https://example.com/careers").unwrap();
    let jobs = jsonld::extract(JSONLD_SINGLE, &url, "Acme").expect("extract");

    assert_eq!(jobs.len(), 1);
    let job = &jobs[0];
    assert_eq!(job.title, "Data Engineer");
    assert_eq!(job.external_id, "job-42");
    assert_eq!(job.location.as_deref(), Some("London"));
    assert_eq!(job.employment_type.as_deref(), Some("FULL_TIME"));
    assert_eq!(job.raw_text, "<p>Build pipelines.</p>");
}

#[test]
fn jsonld_handles_array_of_postings() {
    let html = r#"
<html><head><script type="application/ld+json">
[
  {"@type": "JobPosting", "title": "A", "description": "x"},
  {"@type": "JobPosting", "title": "B", "description": "y"}
]
</script></head></html>
"#;
    let url = Url::parse("https://example.com/careers").unwrap();
    let jobs = jsonld::extract(html, &url, "Acme").expect("extract");
    assert_eq!(jobs.len(), 2);
    assert_eq!(jobs[0].title, "A");
    assert_eq!(jobs[1].title, "B");
}

#[test]
fn jsonld_ignores_non_jobposting_objects() {
    let html = r#"
<html><head><script type="application/ld+json">
{"@type": "Organization", "name": "Acme"}
</script></head></html>
"#;
    let url = Url::parse("https://example.com/careers").unwrap();
    assert!(jsonld::extract(html, &url, "Acme").is_none());
}

// -----------------------------------------------------------------
// JS-required detection
// -----------------------------------------------------------------

#[test]
fn js_required_detects_spa_shell_without_job_links() {
    // A bare SPA root with no job-like anchors in the static HTML is
    // the routing signal that escalates to Tier 2.
    let html = r#"<html><body><div id="root"></div></body></html>"#;
    assert!(html::is_js_required(html));
}

#[test]
fn js_required_false_when_jsonld_is_present() {
    // JSON-LD in the initial HTML is positive proof the page is not
    // JS-required, even if an SPA root also appears.
    let html = r#"
<html><body>
<div id="root"></div>
<script type="application/ld+json">
{"@type": "JobPosting", "title": "Test"}
</script>
</body></html>
"#;
    assert!(!html::is_js_required(html));
}

#[test]
fn js_required_false_when_job_links_are_present() {
    // An SPA root plus job anchors means the static HTML carries enough
    // to work with — no escalation needed.
    let html = r#"
<html><body>
<div id="root"></div>
<a href="/jobs/123">Senior Engineer</a>
<a href="/jobs/124">Product Designer</a>
</body></html>
"#;
    assert!(!html::is_js_required(html));
}

// -----------------------------------------------------------------
// CSS selector extraction (Tier 1b fallback)
// -----------------------------------------------------------------

const SELECTOR_FIXTURE: &str = r#"
<html><body>
<article class="job-card">
  <h2 class="title"><a href="/jobs/one">Senior Engineer</a></h2>
  <span class="location">Remote - US</span>
  <span class="department">Engineering</span>
</article>
<article class="job-card">
  <h2 class="title"><a href="/jobs/two">Designer</a></h2>
  <span class="location">Berlin</span>
</article>
</body></html>
"#;

fn selector_config() -> SelectorConfig {
    serde_json::from_value(serde_json::json!({
        "list": "article.job-card",
        "fields": {
            "title": "h2.title",
            "location": ".location",
            "department": ".department"
        }
    }))
    .expect("valid config")
}

#[test]
fn selectors_extract_happy_path() {
    let url = Url::parse("https://example.com/careers").unwrap();
    let jobs =
        selectors::extract(SELECTOR_FIXTURE, &url, "Acme", &selector_config()).expect("extract");

    assert_eq!(jobs.len(), 2);
    assert_eq!(jobs[0].title, "Senior Engineer");
    assert_eq!(jobs[0].location.as_deref(), Some("Remote - US"));
    assert_eq!(jobs[0].department.as_deref(), Some("Engineering"));

    // Second card omits the department span; the field is None, not empty.
    assert_eq!(jobs[1].title, "Designer");
    assert!(jobs[1].department.is_none());
}

#[test]
fn selectors_resolve_relative_urls_against_source() {
    let url = Url::parse("https://example.com/careers").unwrap();
    let jobs =
        selectors::extract(SELECTOR_FIXTURE, &url, "Acme", &selector_config()).expect("extract");

    assert_eq!(
        jobs[0].source_url.as_str(),
        "https://example.com/jobs/one",
        "relative href must be resolved against the source URL"
    );
    assert_eq!(jobs[1].source_url.as_str(), "https://example.com/jobs/two");
}

#[test]
fn selectors_external_id_uses_resolved_url() {
    let url = Url::parse("https://example.com/careers").unwrap();
    let jobs =
        selectors::extract(SELECTOR_FIXTURE, &url, "Acme", &selector_config()).expect("extract");

    assert_eq!(jobs[0].external_id, "tier1b:https://example.com/jobs/one");
}

#[test]
fn selectors_return_none_on_empty_list() {
    let html = r#"<html><body><div>No jobs here</div></body></html>"#;
    let url = Url::parse("https://example.com/careers").unwrap();
    assert!(selectors::extract(html, &url, "Acme", &selector_config()).is_none());
}

#[test]
fn selectors_explicit_url_selector_wins_over_title() {
    let html = r#"
    <div class="job">
      <h3 class="heading">Engineer</h3>
      <a class="apply" href="/jobs/abc">Apply</a>
    </div>
    "#;
    let config: SelectorConfig = serde_json::from_value(serde_json::json!({
        "list": ".job",
        "fields": {
            "title": ".heading",
            "url": "a.apply"
        }
    }))
    .unwrap();

    let url = Url::parse("https://example.com/careers").unwrap();
    let jobs = selectors::extract(html, &url, "Acme", &config).expect("extract");
    assert_eq!(jobs[0].source_url.as_str(), "https://example.com/jobs/abc");
}

#[test]
fn malformed_html_does_not_panic() {
    let url = Url::parse("https://example.com/careers").unwrap();

    let cases = [
        // Truncated mid-tag
        r#"<html><body><article class="job-card"><h2 class="title"><a href="/jobs/1">Title"#,
        // Broken JSON-LD
        r#"<html><head><script type="application/ld+json">{invalid json}</script></head></html>"#,
        // Unclosed attribute
        r#"<html><body><article class="job-card"</body></html>"#,
        // Empty document
        "",
    ];

    for case in cases {
        // None of these should panic.
        let _ = html::is_js_required(case);
        let _ = jsonld::extract(case, &url, "Acme");
        let _ = selectors::extract(case, &url, "Acme", &selector_config());
    }
}
