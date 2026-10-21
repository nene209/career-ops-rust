// crates/core/src/acquisition/html/mod.rs

//! Static HTML extraction for career pages that are not on a recognized ATS.

use reqwest::Client;
use scraper::{Html, Selector};
use url::Url;

use self::selectors::SelectorConfig;
use super::ats::NormalizedJob;
use super::error::{AtsError, AtsResult};
use super::http;

/// Fetch a URL and extract jobs from its static HTML.
///
/// The extraction chain is:
///
/// 1. Fetch via `http::get_with_retry`. Transport failures become
///    `AtsError::Transport`.
/// 2. Reject non-2xx status, classified via `AtsError::from_status`
///    so a 403 becomes `Blocked` and a 5xx becomes `Blocked`.
/// 3. Detect JS-required. A page with an SPA shell and no job-like
///    links becomes `AtsError::Unsupported("js_required")`, which the
///    router escalates.
/// 4. Try JSON-LD `JobPosting`, then CSS selectors from the portal
///    config.
///
/// The `scraper_config` argument is already-parsed. The router parses
/// `portals.scraper_config` once and passes the result through, so this
/// function does not see raw JSON.
pub async fn fetch_static_jobs(
    client: &Client,
    url: &Url,
    company_name: &str,
    scraper_config: Option<&SelectorConfig>,
) -> AtsResult<Vec<NormalizedJob>> {
    let resp = http::get_with_retry(client, url.as_str())
        .await
        .map_err(|e| AtsError::Transport(e.to_string()))?;

    // Reject non-2xx before the JS-required check. A 500 with an SPA
    // shell is a server problem, not a JS-required page, and escalating
    // it to Tier 2 would just fail again.
    if !resp.status().is_success() {
        return Err(AtsError::from_status(resp.status()));
    }

    let body = resp
        .text()
        .await
        .map_err(|e| AtsError::Transport(e.to_string()))?;

    if is_js_required(&body) {
        return Err(AtsError::Unsupported("js_required"));
    }

    extract_from_html(&body, url, company_name, scraper_config)
}

/// Run the extraction chain on an HTML string.
///
/// This is the same logic the static-fetch path uses, minus the HTTP and
/// JS-required check. It exists as a separate public function so Tier 2
/// (which has already rendered the page and therefore cannot be
/// JS-required) can reuse the extraction without a fetch wrapper.
pub fn extract_from_html(
    html: &str,
    url: &Url,
    company_name: &str,
    scraper_config: Option<&SelectorConfig>,
) -> AtsResult<Vec<NormalizedJob>> {
    if let Some(jobs) = jsonld::extract(html, url, company_name) {
        if !jobs.is_empty() {
            return Ok(jobs);
        }
    }

    if let Some(config) = scraper_config {
        if let Some(jobs) = selectors::extract(html, url, company_name, config) {
            if !jobs.is_empty() {
                return Ok(jobs);
            }
        }
    }

    Err(AtsError::Parse(
        "no jobs found in HTML (neither JSON-LD nor selectors matched)".into(),
    ))
}

/// Heuristic: a page that returns a shell with a known SPA root and no
/// job-like links is likely to need JavaScript. This is deliberately
/// conservative — a false positive escalates to Tier 2, which works; a
/// false negative returns an empty board, which looks like a bug.
pub fn is_js_required(html: &str) -> bool {
    let document = Html::parse_document(html);

    // If JSON-LD JobPosting is present, the page is not JS-required —
    // the data is already in the initial HTML.
    if document
        .select(&Selector::parse("script[type='application/ld+json']").unwrap())
        .next()
        .is_some()
    {
        return false;
    }

    // Common SPA roots.
    let spa_roots = ["#root", "#app", "#__next", "[data-reactroot]"];
    for sel in spa_roots {
        if let Ok(s) = Selector::parse(sel) {
            if document.select(&s).next().is_some() {
                // An SPA root alone is not proof; require that there are
                // also no job-like links in the static HTML.
                let job_links =
                    Selector::parse("a[href*='job'], a[href*='career'], a[href*='position']")
                        .unwrap();
                if document.select(&job_links).next().is_none() {
                    return true;
                }
            }
        }
    }
    false
}

pub mod jsonld;
pub mod selectors;
