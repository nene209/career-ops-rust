// crates/core/src/acquisition/html/mod.rs

use reqwest::Client;
use scraper::{Html, Selector};
use url::Url;

use self::selectors::SelectorConfig;
use super::ats::NormalizedJob;
use crate::error::{CoreError, Result};

pub async fn fetch_static_jobs(
    client: &Client,
    url: &Url,
    company_name: &str,
    scraper_config: Option<&serde_json::Value>,
) -> Result<Vec<NormalizedJob>> {
    let resp = client
        .get(url.clone())
        .send()
        .await
        .map_err(|_| CoreError::Internal("html request failed"))?;

    let status = resp.status();
    let body = resp
        .text()
        .await
        .map_err(|_| CoreError::Internal("html body read failed"))?;

    if !status.is_success() {
        return Err(CoreError::Internal("html non-2xx"));
    }

    // Detect JS-required only on a successful response. A 500 with an
    // SPA shell is a server problem, not a JS-required page, and
    // escalating it to Tier 2 would just fail again.
    if is_js_required(&body) {
        return Err(CoreError::BackendUnsupported("js_required"));
    }

    // Primary path: JSON-LD JobPosting.
    if let Some(jobs) = jsonld::extract(&body, url, company_name) {
        if !jobs.is_empty() {
            return Ok(jobs);
        }
    }

    // Fallback: CSS selectors from portal config.
    if let Some(config_value) = scraper_config {
        let config: SelectorConfig = serde_json::from_value(config_value.clone())
            .map_err(|_| CoreError::Internal("scraper_config is not a valid SelectorConfig"))?;
        if let Some(jobs) = selectors::extract(&body, url, company_name, &config) {
            if !jobs.is_empty() {
                return Ok(jobs);
            }
        }
    }

    Ok(Vec::new())
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
