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
//! `job_descriptions::upsert` identifies a posting by its canonical URL and
//! the provider's id: first the provider-scoped `external_id`, then
//! `(canonical_url, external_id)`, then `(canonical_url, content_hash)`, then
//! `(canonical_url, title, company)`. Tracking parameters and a trailing slash
//! are not significant.
//!
//! - A **URL fetch** uses the real job URL as `source_url`. If the same
//!   job was previously fetched via a portal, the existing row is
//!   updated — one row, not two.
//! - The URL fetch resolves the host through the user's portals and the
//!   embedded registry first, so a known provider produces the same identity
//!   (and a description) the portal path would.
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
use std::sync::atomic::{AtomicBool, Ordering};
use url::Url;

use super::ats::{self, NormalizedJob};
use super::config::{EightfoldConfig, ScraperConfig};
use super::eightfold;
use super::error::AtsError;
use super::html;
use super::known_sites;
use super::router::FetchSource;
use crate::backend::BrowserBackend;
use crate::error::{CoreError, Result};
use crate::model::{FetchTier, JobDescription, QueueState};
use crate::repo::{content_hash, job_descriptions, portals as portal_repo};

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

    // The app may already know this host: a portal the user configured, or an
    // embedded known site. Using that provider keeps the identity and the
    // description the portal path would produce, so adding a posting that a
    // portal already fetched updates that row instead of creating a duplicate.
    if let Some(target) = known_target(pool, &url).await {
        match target.config {
            ScraperConfig::Selectors(config) => {
                // Use the known config when it produces a posting. If it does
                // not, fall through rather than returning: the generic path can
                // still try JSON-LD and Tier 2, which is what it did before
                // this branch existed.
                if let Ok(report) = source
                    .fetch_html(&url, &target.company, Some(&config), 1)
                    .await
                {
                    if let Some(job) = pick_job_for_url(&report.jobs, &url)
                        .or_else(|| report.jobs.first().cloned())
                    {
                        return store_one(pool, job, FetchTier::Tier1bHtml, Vec::new()).await;
                    }
                }
            }
            ScraperConfig::Eightfold(config) => {
                return add_eightfold_job(pool, &url, &target.company, &config, source).await;
            }
        }
    }

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
    let tier1b = source.fetch_html(&url, "Manual", None, 1).await;

    // Tier 1b succeeded and found at least one job. Store the one
    // whose URL matches the pasted URL.
    if let Ok(report) = &tier1b {
        let jobs = &report.jobs;
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
/// The company name and provider config for a URL the app already knows.
struct KnownTarget {
    company: String,
    config: ScraperConfig,
}

/// Resolve a URL against the user's portals first, then the embedded registry.
///
/// A portal is the user's own configuration and wins over the shipped entry.
/// A selectors config with no list selector has nothing to contribute to a
/// single-posting fetch, so it is ignored and the ATS/generic paths handle the
/// board as before.
async fn known_target(pool: &SqlitePool, url: &Url) -> Option<KnownTarget> {
    let mut best: Option<(usize, KnownTarget)> = None;

    if let Ok(portals) = portal_repo::list(pool).await {
        for portal in portals {
            let Ok(portal_url) = Url::parse(&portal.url) else {
                continue;
            };
            if portal_url.host_str() != url.host_str() {
                continue;
            }

            let Some(config) = portal
                .scraper_config
                .as_deref()
                .and_then(|raw| ScraperConfig::parse(raw).ok())
            else {
                continue;
            };
            if matches!(&config, ScraperConfig::Selectors(selectors) if selectors.list.is_none()) {
                continue;
            }

            // A list config describes the board page, not a posting page. Using
            // it on a pasted posting URL would extract (and store) the wrong
            // job, so a selectors portal only applies to its own page.
            let same_page =
                url.path().trim_end_matches('/') == portal_url.path().trim_end_matches('/');
            if matches!(&config, ScraperConfig::Selectors(_)) && !same_page {
                continue;
            }

            // One host can serve several boards. The portal whose path is the
            // longest prefix of the target wins, so `/careers` and `/jobs`
            // keep their own configs.
            let score = if url.path().starts_with(portal_url.path()) {
                portal_url.path().len()
            } else {
                0
            };
            let best_score = best.as_ref().map_or(0, |(score, _)| *score);
            if best.is_none() || score > best_score {
                best = Some((
                    score,
                    KnownTarget {
                        company: portal.company_name,
                        config,
                    },
                ));
            }
        }
    }

    if let Some((_, target)) = best {
        return Some(target);
    }

    let site = known_sites::lookup(url)?;
    let config = ScraperConfig::parse(site.config).ok()?;
    Some(KnownTarget {
        company: site.name.to_string(),
        config,
    })
}

/// Fetch one Eightfold posting through the provider's detail endpoint.
///
/// The position id is in the URL, so there is no need to page through the
/// board. The result carries the same `external_id` the list path assigns,
/// which is what lets the store step update an existing row.
async fn add_eightfold_job(
    pool: &SqlitePool,
    url: &Url,
    company: &str,
    config: &EightfoldConfig,
    source: &dyn FetchSource,
) -> Result<ManualFetchResult> {
    let host = url
        .host_str()
        .ok_or_else(|| CoreError::Message("the URL has no host".into()))?;

    let position_id = eightfold::position_id_from_url(url).ok_or_else(|| {
        CoreError::Message(format!(
            "could not find a posting id in {url}; open the posting from its board first"
        ))
    })?;

    let detail_url = eightfold::detail_url_for(host, &config.domain, &position_id)
        .ok_or_else(|| CoreError::Message("could not build the Eightfold detail URL".into()))?;

    let body = source
        .fetch_page(&detail_url)
        .await
        .map_err(|e| CoreError::Message(format!("fetch failed: {e}")))?;

    let job =
        eightfold::job_from_detail_body(&body, company, &config.domain, url).ok_or_else(|| {
            CoreError::Message("the Eightfold detail response did not contain a posting".into())
        })?;

    store_one(pool, job, FetchTier::Tier1aAts, Vec::new()).await
}

/// Re-fetch one stored posting's description, in place.
///
/// This is the repair tool for a row whose description is missing or is a card
/// teaser. The provider is inferred from `external_id` first (an Eightfold id
/// carries the domain and position), then the posting page is fetched and its
/// JSON-LD or the portal's `detail` selector is used. It never inserts a row.
pub async fn refresh_job(
    pool: &SqlitePool,
    job_id: i64,
    source: &dyn FetchSource,
) -> Result<JobDescription> {
    let job = job_descriptions::get(pool, job_id)
        .await?
        .ok_or_else(|| CoreError::Message(format!("job {job_id} was not found")))?;
    let url = Url::parse(&job.source_url)
        .map_err(|e| CoreError::Message(format!("job url is invalid: {e}")))?;

    let text = fetch_description(pool, &job, &url, source).await?;
    if text.trim().is_empty() {
        return Err(CoreError::Message(
            "the posting page has no description to extract".into(),
        ));
    }

    job_descriptions::set_raw_text(pool, job_id, &text).await?;
    job_descriptions::get(pool, job_id)
        .await?
        .ok_or_else(|| CoreError::Message(format!("job {job_id} vanished")))
}

/// The per-click ceiling on a description batch.
///
/// Twenty is a click's worth of work, not a crawl: every entry is at least one
/// real HTTP request, and the flagged set can be the whole page. A user who
/// wants the rest clicks again.
pub const DESCRIPTION_BATCH_LIMIT: usize = 20;

/// One posting's outcome within a description batch.
#[derive(Debug, Clone)]
pub struct DescriptionEntry {
    pub job_id: i64,
    /// The text that is now stored, or the cause on failure.
    ///
    /// The text travels with the entry so the Pipeline can render the new
    /// description without a reload; a byte count would leave the detail view
    /// showing the teaser it just replaced.
    pub result: std::result::Result<String, String>,
}

/// Result of a description batch.
#[derive(Debug, Clone, Default)]
pub struct DescriptionBatchReport {
    pub entries: Vec<DescriptionEntry>,
    /// The caller's flag stopped the batch before it ran out of postings.
    pub cancelled: bool,
    /// Postings this call did not attempt, because the ceiling was reached or
    /// the batch was stopped. They stay in the queue for the next click.
    pub remaining: usize,
}

impl DescriptionBatchReport {
    pub fn succeeded(&self) -> usize {
        self.entries.iter().filter(|e| e.result.is_ok()).count()
    }

    pub fn failed(&self) -> usize {
        self.entries.iter().filter(|e| e.result.is_err()).count()
    }

    /// The first failure's cause, for the header's one-line explanation.
    pub fn first_error(&self) -> Option<&str> {
        self.entries
            .iter()
            .find_map(|entry| entry.result.as_ref().err().map(String::as_str))
    }
}

/// Fetch the full description for each flagged posting, in order.
///
/// This runs `refresh_job` for every id — the same repair path the detail view
/// exposes one posting at a time — and advances each succeeded row from
/// `details_needed` to `evaluate`, in one transaction, because the user flagged
/// it for exactly this. A failure leaves the row where it is: the state is the
/// record of what is still wanted.
///
/// `cancel` is checked between postings, so a stop never leaves one half
/// written. `progress` sees every completed attempt, which is what lets the
/// header count as the batch runs instead of waiting for the whole thing.
pub async fn fetch_descriptions_batch<F>(
    pool: &SqlitePool,
    job_ids: &[i64],
    source: &dyn FetchSource,
    cancel: &AtomicBool,
    mut progress: F,
) -> Result<DescriptionBatchReport>
where
    F: FnMut(&DescriptionEntry),
{
    let mut entries = Vec::with_capacity(job_ids.len().min(DESCRIPTION_BATCH_LIMIT));
    let mut cancelled = false;

    for &job_id in job_ids.iter().take(DESCRIPTION_BATCH_LIMIT) {
        if cancel.load(Ordering::Relaxed) {
            cancelled = true;
            break;
        }

        let result = match refresh_job(pool, job_id, source).await {
            Ok(job) => {
                tracing::info!(
                    job = job_id,
                    bytes = job.raw_text.len(),
                    "description fetched"
                );
                Ok(job.raw_text)
            }
            Err(error) => {
                tracing::warn!(job = job_id, detail = %error, "description fetch failed");
                Err(error.to_string())
            }
        };

        let entry = DescriptionEntry { job_id, result };
        progress(&entry);
        entries.push(entry);
    }

    let advanced: Vec<i64> = entries
        .iter()
        .filter(|entry| entry.result.is_ok())
        .map(|entry| entry.job_id)
        .collect();
    job_descriptions::set_queue_states(pool, &advanced, QueueState::Evaluate, None).await?;

    Ok(DescriptionBatchReport {
        remaining: job_ids.len().saturating_sub(entries.len()),
        entries,
        cancelled,
    })
}

async fn fetch_description(
    pool: &SqlitePool,
    job: &JobDescription,
    url: &Url,
    source: &dyn FetchSource,
) -> Result<String> {
    // A provider id is the cheapest route: it names the domain and position.
    if let Some(rest) = job
        .external_id
        .as_deref()
        .and_then(|external_id| external_id.strip_prefix("eightfold:"))
    {
        if let Some((domain, position_id)) = rest.split_once(':') {
            let host = url.host_str().unwrap_or_default();
            if let Some(detail_url) = eightfold::detail_url_for(host, domain, position_id) {
                let body = source
                    .fetch_page(&detail_url)
                    .await
                    .map_err(|e| CoreError::Message(format!("fetch failed: {e}")))?;
                if let Some(detail) = eightfold::detail_from_body(&body) {
                    return Ok(detail.text);
                }
            }
        }
    }

    // Otherwise fetch the posting page: JSON-LD first, then the portal's
    // description selector when one is configured.
    let body = source
        .fetch_page(url)
        .await
        .map_err(|e| CoreError::Message(format!("fetch failed: {e}")))?;
    let detail_config = portal_detail_config(pool, url).await;
    if let Some(config) = detail_config.as_ref() {
        if let Some(text) = html::detail::extract_detail(&body, url, &job.title, config) {
            return Ok(text);
        }
    }
    if let Some(text) = html::detail::extract_detail(&body, url, &job.title, &Default::default()) {
        return Ok(text);
    }

    Err(CoreError::Message(
        "no description was found on the posting page".into(),
    ))
}

/// The `detail` block of a portal whose host matches the posting, if any.
async fn portal_detail_config(
    pool: &SqlitePool,
    url: &Url,
) -> Option<super::html::detail::DetailConfig> {
    let portals = portal_repo::list(pool).await.ok()?;
    for portal in portals {
        let Ok(portal_url) = Url::parse(&portal.url) else {
            continue;
        };
        if portal_url.host_str() != url.host_str() {
            continue;
        }
        let Some(config) = portal
            .scraper_config
            .as_deref()
            .and_then(|raw| ScraperConfig::parse(raw).ok())
        else {
            continue;
        };
        if let Some(detail) = config.detail() {
            return Some(detail.clone());
        }
    }
    None
}

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
