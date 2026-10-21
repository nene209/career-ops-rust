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

use async_trait::async_trait;
use sqlx::SqlitePool;
use url::Url;

use super::ats::{self, AtsAdapter, NormalizedJob};
use super::error::{AtsError, AtsResult, FailureCause};
use super::html;
use super::html::selectors::SelectorConfig;
use crate::backend::{BrowserBackend, RenderOpts};
use crate::error::Result;
use crate::model::{FetchTier, Portal};
use crate::repo::{job_descriptions, portals};

/// Result type for router operations. Separate from `Result` (which
/// carries `CoreError`) because the router's own failures carry
/// `FetchError`, which distinguishes classification from cause.
type FetchResult<T> = std::result::Result<T, FetchError>;

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
    ) -> AtsResult<Vec<NormalizedJob>>;
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
    ) -> AtsResult<Vec<NormalizedJob>> {
        html::fetch_static_jobs(&self.client, url, company_name, config).await
    }
}

/// The tier that actually produced the returned jobs.
#[derive(Debug, Clone)]
pub struct FetchOutcome {
    pub jobs_fetched: usize,
    pub tier: FetchTier,
}

/// Failure returned when every attempted tier failed. Carries the
/// coarse cause so the caller can show a specific message.
#[derive(Debug, thiserror::Error)]
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
/// `browser` is `None` when no Tier 2 backend is configured. In that
/// case a Tier 1 failure that would otherwise be escalatable is
/// returned as-is; the caller sees the Tier 1 cause.
pub async fn fetch_portal(
    pool: &SqlitePool,
    portal_id: i64,
    source: &dyn FetchSource,
    browser: Option<&dyn BrowserBackend>,
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
    let route = decide_initial_route(&portal.render_mode, &url, &adapters);

    let (jobs, tier, tier1_failure) = match route {
        InitialRoute::Tier2 => {
            let jobs = fetch_tier2(&portal, &url, browser).await?;
            (jobs, FetchTier::Tier2Obscura, None)
        }
        InitialRoute::Tier1a { adapter_name, slug } => {
            let adapter = adapters
                .iter()
                .find(|a| a.name() == adapter_name)
                .expect("adapter registry is stable");
            match source
                .fetch_ats(adapter.as_ref(), &slug, &portal.company_name)
                .await
            {
                Ok(jobs) => (jobs, FetchTier::Tier1aAts, None),
                Err(e) if e.escalatable() && browser.is_some() => {
                    let failure = e.cause();
                    let jobs = fetch_tier2(&portal, &url, browser).await?;
                    (jobs, FetchTier::Tier2Obscura, Some(failure))
                }
                Err(e) => return Err(FetchError::from_ats(e, "Tier 1a")),
            }
        }
        InitialRoute::Tier1b => {
            let config = parse_scraper_config(portal.scraper_config.as_deref());
            match source
                .fetch_html(&url, &portal.company_name, config.as_ref())
                .await
            {
                Ok(jobs) => (jobs, FetchTier::Tier1bHtml, None),
                Err(e) if e.escalatable() && browser.is_some() => {
                    let failure = e.cause();
                    let jobs = fetch_tier2(&portal, &url, browser).await?;
                    (jobs, FetchTier::Tier2Obscura, Some(failure))
                }
                Err(e) => return Err(FetchError::from_ats(e, "Tier 1b")),
            }
        }
    };

    if let Some(cause) = tier1_failure {
        tracing::info!(
            portal = portal_id,
            tier1_cause = cause.as_str(),
            "escalated to Tier 2 after Tier 1 failure"
        );
    }

    let count = store_jobs(pool, &portal, &jobs, tier)
        .await
        .map_err(|e| FetchError::internal(format!("could not store jobs: {e}")))?;

    portals::mark_verified(pool, portal_id)
        .await
        .map_err(|e| FetchError::internal(format!("could not update last_verified_at: {e}")))?;

    Ok(FetchOutcome {
        jobs_fetched: count,
        tier,
    })
}

async fn fetch_tier2(
    portal: &Portal,
    url: &Url,
    browser: Option<&dyn BrowserBackend>,
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
    let config = parse_scraper_config(portal.scraper_config.as_deref());
    html::extract_from_html(&page.html, url, &portal.company_name, config.as_ref())
        .map_err(|e| FetchError::from_ats(e, "Tier 2"))
}

fn parse_scraper_config(raw: Option<&str>) -> Option<SelectorConfig> {
    let raw = raw?;
    match serde_json::from_str(raw) {
        Ok(config) => Some(config),
        Err(e) => {
            tracing::warn!(error = %e, "portal scraper_config is invalid; ignoring");
            None
        }
    }
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
