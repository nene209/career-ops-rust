// crates/core/src/acquisition/manual.rs

//! Manual job entry.
//!
//! Two paths, both ending in `job_descriptions::upsert`:
//!
//! - **From a URL** — the URL is classified and fetched via the same
//!   adapter/HTML/Tier 2 chain a portal uses, then stored with
//!   `fetch_tier` set to the tier that succeeded.
//! - **From pasted text** — the user supplies title, company, and body
//!   directly. Stored with `fetch_tier = 'manual'` and a synthetic
//!   external ID derived from the content hash.
//!
//! ## Deduplication
//!
//! `job_descriptions::upsert` dedups by `(source_url, external_id)`,
//! then by `(source_url, content_hash)`, then by
//! `(source_url, title, company)`.
//!
//! - A **URL fetch** uses the real job URL as `source_url`. If the same
//!   job was previously fetched via a portal, the existing row is
//!   updated — one row, not two.
//! - A **paste** uses a synthetic `manual://` URL. If the same job was
//!   previously fetched via a portal, two rows exist, because the URLs
//!   differ. That is deliberate: the user pasted it for a reason, and
//!   silently merging it into a portal-fetched row would delete their
//!   manual copy. The UI should offer "duplicate of portal-fetched job"
//!   as an action, but the service layer does not merge.
//! - A paste **does** dedup against itself: identical title, company,
//!   and body produce the same synthetic URL and the same external ID,
//!   so pasting twice yields one row.

use sqlx::SqlitePool;
use url::Url;

use super::ats::{self, NormalizedJob};
use super::error::AtsError;
use super::html;
use super::router::FetchSource;
use crate::backend::BrowserBackend;
use crate::error::{CoreError, Result};
use crate::model::{FetchTier, JobDescription};
use crate::repo::{content_hash, job_descriptions};

/// Result of a manual URL fetch.
#[derive(Debug)]
pub struct ManualFetchResult {
    pub stored: JobDescription,
    pub tier: FetchTier,
    pub warnings: Vec<String>,
}

/// Fetch a single job from a URL and store it.
///
/// The URL is classified in the same order a portal fetch uses:
///
/// 1. **ATS adapter** — if a known adapter recognises the host, fetch
///    the whole board and pick the job whose URL matches the pasted one.
/// 2. **Tier 1b HTML** — if no ATS matches, fetch the page directly.
/// 3. **Tier 2** — if Tier 1b yields nothing (or fails) and a browser
///    backend is available, render the page and extract from the DOM.
///
/// Storing goes through `upsert`, so a manual fetch of a job that was
/// already fetched via a portal updates the existing row rather than
/// creating a duplicate.
pub async fn add_job_from_url(
    pool: &SqlitePool,
    url_str: &str,
    source: &dyn FetchSource,
    browser: Option<&dyn BrowserBackend>,
) -> Result<ManualFetchResult> {
    let url = Url::parse(url_str).map_err(|e| CoreError::Message(format!("invalid url: {e}")))?;

    let adapters = ats::adapters();
    let mut warnings = Vec::new();

    // 1. ATS path.
    for adapter in &adapters {
        if let Some(slug) = adapter.detect(&url) {
            let jobs = source
                .fetch_ats(adapter.as_ref(), &slug, "Manual")
                .await
                .map_err(|e| CoreError::Message(format!("fetch failed: {e}")))?;

            let target = pick_job_for_url(&jobs, &url).ok_or_else(|| {
                CoreError::Message(format!(
                    "ATS board returned {} jobs but none matched the URL",
                    jobs.len()
                ))
            })?;

            if jobs.len() > 1 {
                warnings.push(format!(
                    "Board has {} jobs; stored the one matching the URL.",
                    jobs.len()
                ));
            }

            return store_one(pool, target, FetchTier::Tier1aAts, warnings).await;
        }
    }

    // 2. Tier 1b, with 3. Tier 2 fallback.
    let tier1b = source.fetch_html(&url, "Manual", None).await;

    // Tier 1b succeeded and found at least one job. Store the one
    // whose URL matches the pasted URL.
    if let Ok(jobs) = &tier1b {
        if !jobs.is_empty() {
            let target = pick_job_for_url(jobs, &url)
                .unwrap_or_else(|| jobs.first().cloned().expect("non-empty checked"));
            return store_one(pool, target, FetchTier::Tier1bHtml, warnings).await;
        }
    }

    // Tier 1b failed and there is no browser to escalate to. Surface
    // the fetch error as-is.
    if let Err(e) = &tier1b {
        if browser.is_none() {
            return Err(CoreError::Message(format!("fetch failed: {e}")));
        }
    }

    // At this point Tier 1b either returned no jobs or failed. If a
    // browser is available, try Tier 2. If not, the page produced
    // nothing we can extract from — report that as an error.
    let Some(backend) = browser else {
        return Err(CoreError::Message(
            "fetched the page but no jobs were found, and no browser backend is available to render it".into(),
        ));
    };

    let page = backend
        .fetch_rendered(&url, crate::backend::RenderOpts::default())
        .await
        .map_err(|e| CoreError::Message(format!("tier 2 fetch failed: {e}")))?;

    match html::extract_from_html(&page.html, &url, "Manual", None) {
        Ok(jobs) if !jobs.is_empty() => {
            let target = pick_job_for_url(&jobs, &url)
                .unwrap_or_else(|| jobs.first().cloned().expect("non-empty checked"));
            store_one(pool, target, FetchTier::Tier2Obscura, warnings).await
        }
        _ => Err(CoreError::Message(
            "rendered the page but no jobs were found".into(),
        )),
    }
}

/// Store a manually-pasted job.
///
/// The source URL is synthesized from the title and company so that
/// dedup by `(source_url, content_hash)` can work if the same paste is
/// submitted twice. A user who pastes the same job twice gets one row;
/// a user who pastes it once and later fetches it via a portal gets two
/// rows (different source URLs), which is expected — see the module
/// docstring for why.
pub async fn add_job_from_text(
    pool: &SqlitePool,
    title: &str,
    company: &str,
    raw_text: &str,
) -> Result<JobDescription> {
    if title.trim().is_empty() {
        return Err(CoreError::Message("title cannot be empty".into()));
    }
    if raw_text.trim().is_empty() {
        return Err(CoreError::Message("job description cannot be empty".into()));
    }

    // Fall back to "unknown" if the slug would otherwise be empty
    // (e.g. a company name that is all punctuation). The external ID
    // still disambiguates via the content hash, so collisions here are
    // not a correctness problem — the URL is just a synthetic marker.
    let company_slug = non_empty_or(slugify(company), "unknown");
    let title_slug = non_empty_or(slugify(title), "untitled");
    let synthetic_url = format!("manual://{company_slug}/{title_slug}");

    // The hash includes title, company, and body in the same order as
    // `job_descriptions::hash_job`, so the external ID and the stored
    // content_hash column agree. This matters for dedup: upsert looks
    // up `(source_url, external_id)` first, and if two pastes produce
    // the same external_id they collapse to one row.
    let external_id = format!(
        "manual:{}",
        content_hash(&format!("{title}\n{company}\n{raw_text}"))
    );

    let input = job_descriptions::NewJobDescription {
        title: title.to_string(),
        company: company.to_string(),
        source_url: synthetic_url,
        raw_text: raw_text.to_string(),
        fetch_tier: FetchTier::Manual,
        external_id: Some(external_id),
    };

    job_descriptions::upsert(pool, input)
        .await
        .map_err(|e| CoreError::Message(format!("store failed: {e}")))
}

// ----------------------------------------------------------------------
// Helpers
// ----------------------------------------------------------------------

/// Pick the job whose URL best matches `target`.
///
/// Exact match first, then path match. The path fallback handles URLs
/// with tracking parameters (`?utm_source=...`) that differ from the
/// canonical job URL but share the path.
fn pick_job_for_url(jobs: &[NormalizedJob], target: &Url) -> Option<NormalizedJob> {
    if let Some(j) = jobs.iter().find(|j| j.source_url == *target) {
        return Some(j.clone());
    }
    let target_path = target.path();
    jobs.iter()
        .find(|j| j.source_url.path() == target_path)
        .cloned()
}

async fn store_one(
    pool: &SqlitePool,
    job: NormalizedJob,
    tier: FetchTier,
    warnings: Vec<String>,
) -> Result<ManualFetchResult> {
    let input = job_descriptions::NewJobDescription {
        title: job.title,
        company: job.company,
        source_url: job.source_url.to_string(),
        raw_text: job.raw_text,
        fetch_tier: tier,
        external_id: Some(job.external_id),
    };
    let stored = job_descriptions::upsert(pool, input)
        .await
        .map_err(|e| CoreError::Message(format!("store failed: {e}")))?;
    Ok(ManualFetchResult {
        stored,
        tier,
        warnings,
    })
}

fn slugify(s: &str) -> String {
    s.chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() {
                c.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_string()
}

fn non_empty_or(s: String, fallback: &str) -> String {
    if s.is_empty() {
        fallback.to_string()
    } else {
        s
    }
}

/// Re-export `AtsError` so tests can construct escalation signals
/// without importing from the parent module. Unused in the module
/// body itself; the `use` above is retained for the `map_err` calls.
#[allow(unused_imports)]
use AtsError as _AtsError;
