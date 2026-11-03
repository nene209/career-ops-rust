//! Tier routing and storage.
//!
//! Given a portal row, the router decides which acquisition tier to try,
//! executes the fetch, and stores the results. It owns three decisions
//! that the adapters and the HTML extractor deliberately do not make:
//!
//! 1. **Which tier to try first.** Driven by `portals.render_mode`.
//! 2. **When to escalate.** A Tier 1 failure is escalated to Tier 2
//!    unless the failure is `InvalidRequest`, which a headless browser
//!    cannot fix.
//! 3. **What to record.** The successful tier goes into
//!    `job_descriptions.fetch_tier`; success updates
//!    `portals.last_verified_at`.
//!
//! ## Testability
//!
//! The router depends on `FetchSource` rather than `reqwest::Client`
//! directly. `HttpFetchSource` is the production implementation;
//! `MockFetchSource` (in the tests) lets the routing decisions be
//! exercised without a network. Tier 2 is already abstracted through
//! `BrowserBackend`, so no additional indirection is needed there.

use std::sync::atomic::{AtomicBool, Ordering};

use async_trait::async_trait;
use sqlx::SqlitePool;
use url::Url;

use super::ats::{self, AtsAdapter, JobDetail, NormalizedJob};
use super::config::ScraperConfig;
use super::error::{AtsError, AtsResult, FailureCause};
use super::html;
use super::html::detail::DetailConfig;
use super::html::selectors::SelectorConfig;
use super::html::PageReport;
use super::html::PaginationStop;
use crate::backend::{BrowserBackend, RenderOpts};
use crate::error::Result;
use crate::model::{FetchTier, Portal};
use crate::repo::{job_descriptions, portals};

/// Result type for router operations. Separate from `Result` (which
/// carries `CoreError`) because the router's own failures carry
/// `FetchError`, which distinguishes classification from cause.
type FetchResult<T> = std::result::Result<T, FetchError>;

/// Record the outcome of a fetch attempt against the portal row.
///
/// Deliberately does not return a `Result`. If the fetch itself
/// succeeded, a failure to record it should not turn that success into
/// an error — the caller has already received the outcome. If the
/// fetch failed, the same applies: the caller gets the fetch error,
/// not a recording error. Either way, a recording failure is logged
/// and swallowed.
///
/// If the portal row no longer exists (deleted mid-fetch), the UPDATE
/// affects zero rows and returns `Ok`. That is fine: there is nothing
/// to record against.
async fn record_fetch_outcome(
    pool: &SqlitePool,
    portal_id: i64,
    result: &FetchResult<FetchOutcome>,
) {
    let cause = match result {
        Ok(_) => None,
        Err(e) => Some(e.cause.as_str()),
    };
    if let Err(e) = portals::record_fetch_attempt(pool, portal_id, cause).await {
        tracing::warn!(
            portal = portal_id,
            error = %e,
            "could not record fetch attempt outcome"
        );
    }
}

/// Abstraction over the two Tier 1 fetch paths. The production
/// implementation uses `reqwest`; tests inject a mock.
#[async_trait]
pub trait FetchSource: Send + Sync {
    async fn fetch_ats(
        &self,
        adapter: &dyn AtsAdapter,
        slug: &str,
        company_name: &str,
    ) -> AtsResult<Vec<NormalizedJob>>;

    async fn fetch_html(
        &self,
        url: &Url,
        company_name: &str,
        config: Option<&SelectorConfig>,
    ) -> AtsResult<PageReport>;

    /// Fetch one raw HTML document, such as a job's detail page.
    ///
    /// Separate from `fetch_html` because detail enrichment needs the document
    /// itself: it tries JSON-LD first and selectors second, which is a
    /// different decision from "extract a list of jobs".
    async fn fetch_page(&self, url: &Url) -> AtsResult<String>;
}

/// Production `FetchSource`, backed by a `reqwest::Client`.
pub struct HttpFetchSource {
    client: reqwest::Client,
}

impl HttpFetchSource {
    pub fn new(client: reqwest::Client) -> Self {
        Self { client }
    }
}

#[async_trait]
impl FetchSource for HttpFetchSource {
    async fn fetch_ats(
        &self,
        adapter: &dyn AtsAdapter,
        slug: &str,
        company_name: &str,
    ) -> AtsResult<Vec<NormalizedJob>> {
        adapter.fetch_jobs(&self.client, slug, company_name).await
    }

    async fn fetch_html(
        &self,
        url: &Url,
        company_name: &str,
        config: Option<&SelectorConfig>,
    ) -> AtsResult<PageReport> {
        html::fetch_static_jobs(&self.client, url, company_name, config).await
    }

    async fn fetch_page(&self, url: &Url) -> AtsResult<String> {
        let resp = super::http::get_with_retry(&self.client, url.as_str())
            .await
            .map_err(|e| AtsError::Transport(e.to_string()))?;

        if !resp.status().is_success() {
            return Err(AtsError::from_status(resp.status()));
        }

        resp.text()
            .await
            .map_err(|e| AtsError::Transport(e.to_string()))
    }
}

/// The tier that actually produced the returned jobs.
#[derive(Debug, Clone)]
pub struct FetchOutcome {
    pub jobs_fetched: usize,
    pub tier: FetchTier,
    /// List pages fetched by the selector path. `None` when an adapter or the
    /// rendering backend owns pagination.
    pub pages_fetched: Option<usize>,
    /// Why the selector loop stopped. `None` when no selector fetch ran.
    pub pagination_stopped: Option<PaginationStop>,
    /// Detail pages fetched to fill in descriptions. Zero when the portal has
    /// no `detail` config.
    pub details_attempted: usize,
    /// Detail pages that produced usable text.
    pub details_enriched: usize,
}

/// Failure returned when every attempted tier failed. Carries the
/// coarse cause so the caller can show a specific message.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("acquisition failed: {cause:?} — {detail}")]
pub struct FetchError {
    pub cause: FailureCause,
    pub detail: String,
}

impl FetchError {
    fn from_ats(e: AtsError, tier: &'static str) -> Self {
        Self {
            cause: e.cause(),
            detail: format!("{tier}: {e}"),
        }
    }

    fn internal(detail: impl Into<String>) -> Self {
        Self {
            cause: FailureCause::Other,
            detail: detail.into(),
        }
    }

    /// Keep the failure that caused the escalation and record what the fallback
    /// said.
    ///
    /// A sidecar that is not running must not hide the page's own failure: the
    /// user needs "the page was blocked", not "an unrecognized reason".
    fn with_fallback(mut self, fallback: FetchError) -> Self {
        self.detail = format!(
            "{}; the browser fallback also failed: {}",
            self.detail, fallback.detail
        );
        self
    }
}

/// Which initial route to take. Extracted so it can be tested without
/// I/O.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InitialRoute {
    Tier1a {
        adapter_name: &'static str,
        slug: String,
    },
    Tier1b,
    Tier2,
}

/// Decide the initial route from `render_mode` and the portal URL.
///
/// If `render_mode` is `"browser"`, Tier 2 is chosen unconditionally.
/// Otherwise, the first adapter that recognises the URL wins; if no
/// adapter recognises it, Tier 1b (HTML extraction) is the fallback.
pub fn decide_initial_route(
    render_mode: &str,
    url: &Url,
    adapters: &[Box<dyn AtsAdapter>],
) -> InitialRoute {
    if render_mode == "browser" {
        return InitialRoute::Tier2;
    }
    for adapter in adapters {
        if let Some(slug) = adapter.detect(url) {
            return InitialRoute::Tier1a {
                adapter_name: adapter.name(),
                slug,
            };
        }
    }
    InitialRoute::Tier1b
}

/// Fetch a portal end-to-end and store the results.
///
/// Records the outcome on the portal row regardless of success or
/// failure, so the Portals UI health drill-down can show the last
/// attempt time and, on failure, the classified cause.
pub async fn fetch_portal(
    pool: &SqlitePool,
    portal_id: i64,
    source: &dyn FetchSource,
    browser: Option<&dyn BrowserBackend>,
) -> FetchResult<FetchOutcome> {
    fetch_portal_cancellable(pool, portal_id, source, browser, None).await
}

/// As `fetch_portal`, but stops detail enrichment between jobs when `cancel`
/// is set.
///
/// The batch path passes its shared flag so a 40-posting enrichment can be
/// interrupted; a single fetch has no cancel control in the UI and passes
/// `None`.
pub async fn fetch_portal_cancellable(
    pool: &SqlitePool,
    portal_id: i64,
    source: &dyn FetchSource,
    browser: Option<&dyn BrowserBackend>,
    cancel: Option<&AtomicBool>,
) -> FetchResult<FetchOutcome> {
    let result = fetch_portal_inner(pool, portal_id, source, browser, cancel).await;
    record_fetch_outcome(pool, portal_id, &result).await;
    result
}

/// Fetch a portal end-to-end and store the results.
///
/// `browser` is `None` when no Tier 2 backend is configured. In that
/// case a Tier 1 failure that would otherwise be escalatable is
/// returned as-is; the caller sees the Tier 1 cause.
pub async fn fetch_portal_inner(
    pool: &SqlitePool,
    portal_id: i64,
    source: &dyn FetchSource,
    browser: Option<&dyn BrowserBackend>,
    cancel: Option<&AtomicBool>,
) -> FetchResult<FetchOutcome> {
    let portal = portals::get(pool, portal_id)
        .await
        .map_err(|e| FetchError::internal(format!("could not read portal: {e}")))?
        .ok_or_else(|| FetchError {
            cause: FailureCause::InvalidRequest,
            detail: format!("portal {portal_id} not found"),
        })?;

    let url = Url::parse(&portal.url).map_err(|e| FetchError {
        cause: FailureCause::InvalidRequest,
        detail: format!("portal url is invalid: {e}"),
    })?;

    let adapters = ats::adapters();
    // Parsed once: the tier paths and the enrichment step all use the same
    // config, and a triplicate parse would let them disagree.
    let config = parse_scraper_config(portal.scraper_config.as_deref())?;

    // A provider with its own fetch path bypasses tier routing. WS2 replaces
    // this arm with the Eightfold fetch; returning here keeps the HTML path
    // from running against a site whose content is not in the HTML.
    if let Some(eightfold) = config.as_ref().and_then(ScraperConfig::eightfold) {
        return Err(FetchError {
            cause: FailureCause::InvalidRequest,
            detail: format!(
                "the eightfold provider is not available in this build (domain `{}`)",
                eightfold.domain
            ),
        });
    }

    let selectors_config = config.as_ref().and_then(ScraperConfig::selectors);
    let route = decide_initial_route(&portal.render_mode, &url, &adapters);

    // The adapter that produced the list, when the route was 1a. Enrichment
    // asks it where descriptions live; Tier 1b and Tier 2 have no adapter.
    let mut ats_source: Option<(&dyn AtsAdapter, String)> = None;

    let (jobs, tier, tier1_failure, page_report) = match route {
        InitialRoute::Tier2 => {
            let jobs = fetch_tier2(&portal, &url, browser, selectors_config).await?;
            (jobs, FetchTier::Tier2Obscura, None, None)
        }
        InitialRoute::Tier1a { adapter_name, slug } => {
            let adapter: &dyn AtsAdapter = adapters
                .iter()
                .find(|a| a.name() == adapter_name)
                .map(|a| a.as_ref())
                .expect("adapter registry is stable");
            match source.fetch_ats(adapter, &slug, &portal.company_name).await {
                Ok(jobs) => {
                    ats_source = Some((adapter, slug));
                    (jobs, FetchTier::Tier1aAts, None, None)
                }
                Err(e) if e.escalatable() && browser.is_some() => {
                    let tier1 = FetchError::from_ats(e, "Tier 1a");
                    let failure = tier1.cause;
                    match fetch_tier2(&portal, &url, browser, selectors_config).await {
                        Ok(jobs) => (jobs, FetchTier::Tier2Obscura, Some(failure), None),
                        Err(tier2) => return Err(tier1.with_fallback(tier2)),
                    }
                }
                Err(e) => return Err(FetchError::from_ats(e, "Tier 1a")),
            }
        }
        InitialRoute::Tier1b => {
            match source
                .fetch_html(&url, &portal.company_name, selectors_config)
                .await
            {
                Ok(report) => (
                    report.jobs,
                    FetchTier::Tier1bHtml,
                    None,
                    Some((report.pages_fetched, report.stop)),
                ),
                Err(e) if e.escalatable() && browser.is_some() => {
                    let tier1 = FetchError::from_ats(e, "Tier 1b");
                    let failure = tier1.cause;
                    match fetch_tier2(&portal, &url, browser, selectors_config).await {
                        Ok(jobs) => (jobs, FetchTier::Tier2Obscura, Some(failure), None),
                        Err(tier2) => return Err(tier1.with_fallback(tier2)),
                    }
                }
                Err(e) => return Err(FetchError::from_ats(e, "Tier 1b")),
            }
        }
    };

    let (pages_fetched, pagination_stopped) = match page_report {
        Some((pages, stop)) => (Some(pages), Some(stop)),
        None => (None, None),
    };

    if let Some(cause) = tier1_failure {
        tracing::info!(
            portal = portal_id,
            tier1_cause = cause.as_str(),
            "escalated to Tier 2 after Tier 1 failure"
        );
    }

    let mut jobs = jobs;
    let (details_attempted, details_enriched) = enrich_details(
        &mut jobs,
        source,
        ats_source
            .as_ref()
            .map(|(adapter, slug)| (*adapter, slug.as_str())),
        config.as_ref().and_then(ScraperConfig::detail),
        cancel,
    )
    .await;

    let count = store_jobs(pool, &portal, &jobs, tier)
        .await
        .map_err(|e| FetchError::internal(format!("could not store jobs: {e}")))?;

    if details_attempted > 0 {
        tracing::info!(
            portal = portal_id,
            jobs = count,
            attempted = details_attempted,
            enriched = details_enriched,
            "enriched job descriptions"
        );
    }

    Ok(FetchOutcome {
        jobs_fetched: count,
        tier,
        pages_fetched,
        pagination_stopped,
        details_attempted,
        details_enriched,
    })
}

/// Fill in descriptions, best effort.
///
/// Bounded by `detail.max_jobs`, sequential, and checked for cancellation
/// between jobs. Boards with a detail endpoint use it (one request per
/// posting); every other portal fetches the posting page and extracts
/// JSON-LD or a selector. A failed detail request leaves the list-level text
/// in place: enrichment never fails the portal fetch.
async fn enrich_details(
    jobs: &mut [NormalizedJob],
    source: &dyn FetchSource,
    ats: Option<(&dyn AtsAdapter, &str)>,
    detail: Option<&DetailConfig>,
    cancel: Option<&AtomicBool>,
) -> (usize, usize) {
    let Some(detail) = detail else {
        return (0, 0);
    };

    let mut attempted = 0;
    let mut enriched = 0;

    for job in jobs.iter_mut().take(detail.max_jobs()) {
        if cancel.is_some_and(|cancel| cancel.load(Ordering::Relaxed)) {
            tracing::info!(attempted, enriched, "detail enrichment cancelled");
            break;
        }

        attempted += 1;
        let Some(found) = job_detail(source, ats, job, detail).await else {
            continue;
        };

        if !found.text.trim().is_empty() {
            job.raw_text = found.text;
            enriched += 1;
        }
        // List metadata wins when present; the detail fills the gaps.
        if job.department.is_none() {
            job.department = found.department;
        }
        if job.employment_type.is_none() {
            job.employment_type = found.employment_type;
        }
    }

    (attempted, enriched)
}

/// One posting's description: the board's detail endpoint when it has one,
/// otherwise the job page itself.
async fn job_detail(
    source: &dyn FetchSource,
    ats: Option<(&dyn AtsAdapter, &str)>,
    job: &NormalizedJob,
    detail: &DetailConfig,
) -> Option<JobDetail> {
    if let Some((adapter, slug)) = ats {
        if let Some(url) = adapter.detail_url(slug, job) {
            let body = match source.fetch_page(&url).await {
                Ok(body) => body,
                Err(error) => {
                    tracing::warn!(%url, %error, "board detail request failed");
                    return None;
                }
            };
            return adapter.detail_from_body(&body);
        }
    }

    let text = html_detail_text(source, job, detail).await?;
    Some(JobDetail {
        text,
        ..JobDetail::default()
    })
}

async fn html_detail_text(
    source: &dyn FetchSource,
    job: &NormalizedJob,
    detail: &DetailConfig,
) -> Option<String> {
    let body = match source.fetch_page(&job.source_url).await {
        Ok(body) => body,
        Err(error) => {
            tracing::warn!(url = %job.source_url, %error, "detail page fetch failed");
            return None;
        }
    };

    html::detail::extract_detail(&body, &job.source_url, &job.title, detail)
}

async fn fetch_tier2(
    portal: &Portal,
    url: &Url,
    browser: Option<&dyn BrowserBackend>,
    config: Option<&SelectorConfig>,
) -> FetchResult<Vec<NormalizedJob>> {
    let Some(backend) = browser else {
        return Err(FetchError {
            cause: FailureCause::JsRequired,
            detail: "Tier 2 required but no browser backend is configured".into(),
        });
    };

    let page = backend
        .fetch_rendered(url, RenderOpts::default())
        .await
        .map_err(|e| FetchError::internal(format!("tier 2 fetch failed: {e}")))?;

    // Tier 2 returns a rendered HTML string. Extract jobs from it using
    // the same JSON-LD and selector paths as Tier 1b, since the
    // structure of a rendered page is the same as a static one.
    html::extract_from_html(&page.html, url, &portal.company_name, config)
        .map_err(|e| FetchError::from_ats(e, "Tier 2"))
}

fn parse_scraper_config(
    raw: Option<&str>,
) -> std::result::Result<Option<ScraperConfig>, FetchError> {
    let Some(raw) = raw.map(str::trim).filter(|raw| !raw.is_empty()) else {
        return Ok(None);
    };

    ScraperConfig::parse(raw)
        .map(Some)
        .map_err(|error| FetchError {
            cause: FailureCause::InvalidRequest,
            detail: error.to_string(),
        })
}

async fn store_jobs(
    pool: &SqlitePool,
    portal: &Portal,
    jobs: &[NormalizedJob],
    tier: FetchTier,
) -> Result<usize> {
    let mut count = 0;
    for job in jobs {
        let input = job_descriptions::NewJobDescription {
            title: job.title.clone(),
            company: job.company.clone(),
            source_url: job.source_url.to_string(),
            raw_text: job.raw_text.clone(),
            fetch_tier: tier,
            external_id: Some(job.external_id.clone()),
        };
        job_descriptions::upsert(pool, input).await?;
        count += 1;
    }
    tracing::debug!(
        portal = portal.id,
        tier = tier.as_str(),
        count,
        "stored jobs"
    );
    Ok(count)
}

// ---------------------------------------------------------------------
// Preview ("Test config")
// ---------------------------------------------------------------------

/// Titles a preview reports.
pub const PREVIEW_SAMPLE_SIZE: usize = 3;

/// Detail requests a preview may make, independent of `detail.max_jobs`.
/// The config may ask for 40; a preview stops at three.
pub const PREVIEW_MAX_DETAILS: usize = 3;

/// One job as the preview shows it.
#[derive(Debug, Clone)]
pub struct PreviewJob {
    pub title: String,
    pub source_url: Url,
}

/// What a "Test config" run found.
#[derive(Debug, Clone)]
pub struct PreviewReport {
    pub tier: FetchTier,
    pub jobs_found: usize,
    /// Pages the saved config would follow, including the first. `0` for an
    /// ATS board, whose adapter paginates internally and is not previewed
    /// page by page.
    pub pages_planned: usize,
    pub sample: Vec<PreviewJob>,
    /// Length of the first description the preview could extract.
    pub first_description_bytes: Option<usize>,
    /// Detail requests actually made (at most `PREVIEW_MAX_DETAILS`).
    pub details_attempted: usize,
}

/// Fetch a draft portal configuration without storing anything.
///
/// Deliberately shares the router's real decisions: the same tier routing, the
/// same extraction, and the same detail helpers. The differences are bounds,
/// not behaviour — one list page, three details, and no database write — which
/// is what makes the preview trustworthy. It takes no pool, so it cannot write
/// even by accident.
pub async fn preview(
    url: &Url,
    company_name: &str,
    render_mode: &str,
    scraper_config: Option<&str>,
    source: &dyn FetchSource,
    browser: Option<&dyn BrowserBackend>,
) -> std::result::Result<PreviewReport, FetchError> {
    // Unlike a real fetch, an unparsable config is an error here: the whole
    // point of the button is to tell the user that before they save it.
    let config = match scraper_config.map(str::trim).filter(|raw| !raw.is_empty()) {
        None => None,
        Some(raw) => Some(ScraperConfig::parse(raw).map_err(|error| FetchError {
            cause: FailureCause::InvalidRequest,
            detail: error.to_string(),
        })?),
    };

    if let Some(eightfold) = config.as_ref().and_then(ScraperConfig::eightfold) {
        return Err(FetchError {
            cause: FailureCause::InvalidRequest,
            detail: format!(
                "the eightfold provider is not available in this build (domain `{}`)",
                eightfold.domain
            ),
        });
    }

    let selectors_config = config.as_ref().and_then(ScraperConfig::selectors);
    let adapters = ats::adapters();
    let route = decide_initial_route(render_mode, url, &adapters);
    let mut ats_source: Option<(&dyn AtsAdapter, String)> = None;

    let (jobs, tier, pages_planned) = match route {
        InitialRoute::Tier2 => {
            let Some(backend) = browser else {
                return Err(FetchError {
                    cause: FailureCause::JsRequired,
                    detail: "browser mode needs the rendering backend, which is not available"
                        .into(),
                });
            };
            let page = backend
                .fetch_rendered(url, RenderOpts::default())
                .await
                .map_err(|error| FetchError::internal(format!("tier 2 fetch failed: {error}")))?;
            let jobs = html::extract_from_html(&page.html, url, company_name, selectors_config)
                .map_err(|error| FetchError::from_ats(error, "Tier 2"))?;
            (jobs, FetchTier::Tier2Obscura, 1)
        }
        InitialRoute::Tier1a { adapter_name, slug } => {
            let adapter: &dyn AtsAdapter = adapters
                .iter()
                .find(|candidate| candidate.name() == adapter_name)
                .map(|candidate| candidate.as_ref())
                .expect("adapter registry is stable");
            let jobs = source
                .fetch_ats(adapter, &slug, company_name)
                .await
                .map_err(|error| FetchError::from_ats(error, "Tier 1a"))?;
            ats_source = Some((adapter, slug));
            (jobs, FetchTier::Tier1aAts, 0)
        }
        InitialRoute::Tier1b => {
            // One page, whatever `pagination.max_pages` says. The real fetch
            // still follows the config; the preview reports how many pages it
            // would follow without fetching them.
            let single_page = selectors_config.map(|config| SelectorConfig {
                pagination: None,
                ..config.clone()
            });
            let jobs = source
                .fetch_html(url, company_name, single_page.as_ref())
                .await
                .map_err(|error| FetchError::from_ats(error, "Tier 1b"))?
                .jobs;
            let pages = selectors_config
                .and_then(|config| config.pagination.as_ref())
                .map(|pagination| {
                    // A block with no mechanism fetches one page, exactly as
                    // `fetch_pages` does; reporting its default limit would lie.
                    if pagination.next.is_none() && pagination.url_template.is_none() {
                        1
                    } else {
                        pagination.page_limit()
                    }
                })
                .unwrap_or(1);
            (jobs, FetchTier::Tier1bHtml, pages)
        }
    };

    let sample = jobs
        .iter()
        .take(PREVIEW_SAMPLE_SIZE)
        .map(|job| PreviewJob {
            title: job.title.clone(),
            source_url: job.source_url.clone(),
        })
        .collect();

    let enrichment = ats_source.is_some()
        || config
            .as_ref()
            .is_some_and(|config| config.detail().is_some());
    let mut first_description_bytes = None;
    let mut details_attempted = 0;

    if enrichment {
        let preview_detail = DetailConfig {
            fields: config
                .as_ref()
                .and_then(ScraperConfig::detail)
                .and_then(|detail| detail.fields.clone()),
            max_jobs: Some(PREVIEW_MAX_DETAILS),
        };
        let ats = ats_source
            .as_ref()
            .map(|(adapter, slug)| (*adapter, slug.as_str()));

        // Stop at the first description: the report needs one length, and each
        // request is real network traffic.
        for job in jobs.iter().take(PREVIEW_MAX_DETAILS) {
            details_attempted += 1;
            if let Some(found) = job_detail(source, ats, job, &preview_detail).await {
                if !found.text.trim().is_empty() {
                    first_description_bytes = Some(found.text.len());
                    break;
                }
            }
        }
    }

    Ok(PreviewReport {
        tier,
        jobs_found: jobs.len(),
        pages_planned,
        sample,
        first_description_bytes,
        details_attempted,
    })
}
