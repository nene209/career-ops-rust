//! Portals service layer.
//!
//! Headless orchestration that a Portals view will call. Every function
//! takes explicit dependencies (a pool, a `FetchSource`, an optional
//! `BrowserBackend`) so tests can inject mocks without any framework.

use std::sync::atomic::{AtomicBool, Ordering};

use sqlx::SqlitePool;
use url::Url;

use super::health::PortalHealth;
use crate::acquisition::error::FailureCause;
use crate::acquisition::report::PaginationStop;
use crate::acquisition::router::{self, FetchError, FetchOutcome, FetchSource};
use crate::backend::BrowserBackend;
use crate::error::{CoreError, Result};
use crate::model::Portal;
use crate::repo::portals;

/// A portal plus its computed health, ready for a list view.
#[derive(Debug)]
pub struct PortalWithHealth {
    pub portal: Portal,
    pub health: PortalHealth,
}

/// List all portals with health summaries computed.
pub async fn list_with_health(pool: &SqlitePool) -> Result<Vec<PortalWithHealth>> {
    let rows = portals::list(pool).await?;
    Ok(rows
        .into_iter()
        .map(|portal| PortalWithHealth {
            health: PortalHealth::from_portal(&portal),
            portal,
        })
        .collect())
}

/// Result of a single-portal fetch, plus the post-fetch portal state.
#[derive(Debug)]
pub struct FetchReport {
    pub portal: Portal,
    pub outcome: std::result::Result<FetchOutcome, FetchError>,
}

/// Fetch one portal and return the updated portal row.
///
/// The router writes the outcome (success timestamps and failure cause)
/// to the portal row. This function re-reads it so the caller has the
/// post-fetch state without a second query of its own.
pub async fn fetch_one(
    pool: &SqlitePool,
    portal_id: i64,
    source: &dyn FetchSource,
    browser: Option<&dyn BrowserBackend>,
) -> Result<FetchReport> {
    let outcome = router::fetch_portal(pool, portal_id, source, browser).await;
    report(pool, portal_id, outcome).await
}

/// Continue a portal from its stored cursor. Behaves like `fetch_one` for a
/// portal whose provider has no explicit page.
pub async fn fetch_more(
    pool: &SqlitePool,
    portal_id: i64,
    source: &dyn FetchSource,
    browser: Option<&dyn BrowserBackend>,
) -> Result<FetchReport> {
    let outcome = router::fetch_portal_more(pool, portal_id, source, browser).await;
    report(pool, portal_id, outcome).await
}

async fn report(
    pool: &SqlitePool,
    portal_id: i64,
    outcome: std::result::Result<FetchOutcome, FetchError>,
) -> Result<FetchReport> {
    let portal = portals::get(pool, portal_id)
        .await?
        .ok_or_else(|| CoreError::Internal("portal vanished during fetch"))?;
    Ok(FetchReport { portal, outcome })
}

/// Hard page budget for one **Fetch all pages** click.
///
/// The budget is a per-click policy, not a per-portal cap: `pagination.max_pages`
/// stays the depth of a single Continue. Fifty pages on a ten-per-page board is
/// five hundred postings, and a board that needs more is five clicks rather than
/// one accidental crawl.
pub const FETCH_ALL_PAGE_BUDGET: usize = 50;

/// Why a Fetch-all-pages run stopped.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FetchAllStop {
    /// The portal reported the end of its list.
    EndOfList,
    /// The user pressed Stop.
    Cancelled,
    /// The per-click page budget ran out; more pages remain.
    BudgetExhausted,
    /// The provider cannot continue from a cursor (an adapter that pages
    /// internally): repeating would re-read the same page.
    NoProgress,
    /// A chunk read pages but stopped before the end of the list without
    /// leaving a cursor to continue from — usually a page the site refused.
    /// `unread_page` names it when the report does. The pages that were read are
    /// recorded in the cursor, so Continue retries only the unread page, and the
    /// user can skip it through the Portals screen.
    Stopped { unread_page: Option<usize> },
    /// A chunk failed. The pages already read are stored.
    Failed(FetchError),
}

impl FetchAllStop {
    /// A short noun phrase for the result message: "stopped at …".
    pub fn label(&self) -> &'static str {
        match self {
            Self::EndOfList => "the end of the list",
            Self::Cancelled => "Stop",
            Self::BudgetExhausted => "the page budget",
            Self::NoProgress => "a board that cannot continue",
            Self::Stopped { .. } => "a page that could not be read",
            Self::Failed(_) => "an error",
        }
    }

    pub fn is_failure(&self) -> bool {
        matches!(self, Self::Failed(_))
    }
}

/// Cumulative progress after each chunk of a Fetch-all-pages run.
///
/// The router reports pages per call, not per page, so the totals update once
/// per chunk. They are cumulative rather than per-chunk, which is what the row's
/// status line shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FetchAllProgress {
    pub pages_fetched: usize,
    pub jobs_stored: usize,
    /// The budget for this click, so the UI can say "of 50".
    pub budget: usize,
}

/// Result of a Fetch-all-pages run, plus the post-run portal row.
#[derive(Debug, Clone)]
pub struct FetchAllPagesReport {
    pub portal: Portal,
    pub pages_fetched: usize,
    pub jobs_stored: usize,
    pub stopped_reason: FetchAllStop,
}

/// Walk a portal's list from its cursor until the end, the budget, or Stop.
///
/// This is a bounded, cancellable loop over `fetch_portal_more`, not a new
/// acquisition path: every chunk goes through the router, which owns the cursor
/// and the storage. `cancel` is checked between chunks, so a Stop takes effect
/// within one chunk rather than mid-request.
///
/// The loop also stops when a *provider* cannot advance — an ATS adapter pages
/// internally and reports no page count, so repeating it would re-read the same
/// board until the budget. A chunk that read pages but stopped early (a page the
/// site refused) is a different case: the walk stops with `Stopped` and leaves
/// the cursor alone, so Continue retries the range instead of skipping it.
pub async fn fetch_all_pages<F>(
    pool: &SqlitePool,
    portal_id: i64,
    source: &dyn FetchSource,
    browser: Option<&dyn BrowserBackend>,
    cancel: &AtomicBool,
    mut progress: F,
) -> Result<FetchAllPagesReport>
where
    F: FnMut(&FetchAllProgress),
{
    let mut pages_fetched = 0;
    let mut jobs_stored = 0;
    let mut last_start = 0;

    let stopped_reason = loop {
        if cancel.load(Ordering::Relaxed) {
            break FetchAllStop::Cancelled;
        }

        let outcome = match router::fetch_portal_more_cancellable(
            pool,
            portal_id,
            source,
            browser,
            Some(cancel),
        )
        .await
        {
            Ok(outcome) => outcome,
            Err(error) => break FetchAllStop::Failed(error),
        };

        // A run that starts where the last one started cannot make progress,
        // which also covers a cursor write that failed silently.
        if outcome.first_page <= last_start {
            break FetchAllStop::NoProgress;
        }
        last_start = outcome.first_page;

        let chunk_pages = outcome.pages_fetched.unwrap_or(0);
        pages_fetched += chunk_pages;
        jobs_stored += outcome.jobs_fetched;
        progress(&FetchAllProgress {
            pages_fetched,
            jobs_stored,
            budget: FETCH_ALL_PAGE_BUDGET,
        });

        // One line per chunk: the walk's own account of what it read. The range
        // starts at `first_page` because that is how the user sees it, and
        // `deepest` is the page the cursor now covers.
        tracing::info!(
            portal = portal_id,
            first_page = outcome.first_page,
            pages = chunk_pages,
            deepest = outcome.first_page + chunk_pages.saturating_sub(1),
            jobs = outcome.jobs_fetched,
            stop = outcome
                .pagination_stopped
                .map(PaginationStop::as_str)
                .unwrap_or("provider"),
            "fetch-all chunk"
        );

        if outcome.end_of_list {
            break FetchAllStop::EndOfList;
        }
        if cancel.load(Ordering::Relaxed) {
            break FetchAllStop::Cancelled;
        }
        // No page accounting at all: an adapter that pages internally. There is
        // nothing to continue from, and repeating would re-read the board.
        if outcome.pages_fetched.is_none() {
            break FetchAllStop::NoProgress;
        }
        if outcome.next_page.is_none() {
            // Pages were read, but the chunk ended without leaving a cursor.
            // The usual cause is a page the site refused (`FetchFailed`) or a
            // page that yielded nothing (`ExtractionFailed`). The router leaves
            // the cursor alone so a retry re-reads the whole range; the walk
            // records the pages that did work, so the retry starts at the page
            // that failed instead of re-reading four good ones.
            let last_read = outcome.first_page + chunk_pages - 1;
            if chunk_pages > 0 {
                portals::set_last_page_fetched_at_least(pool, portal_id, last_read as i64).await?;
            }
            let unread_page = match outcome.pagination_stopped {
                Some(PaginationStop::FetchFailed | PaginationStop::ExtractionFailed) => {
                    Some(last_read + 1)
                }
                _ => None,
            };
            break FetchAllStop::Stopped { unread_page };
        }
        // Checked between chunks, so a chunk that crosses the budget finishes:
        // the total can exceed the budget by at most `max_pages - 1`.
        if pages_fetched >= FETCH_ALL_PAGE_BUDGET {
            break FetchAllStop::BudgetExhausted;
        }
    };

    let portal = portals::get(pool, portal_id)
        .await?
        .ok_or_else(|| CoreError::Internal("portal vanished during fetch"))?;

    Ok(FetchAllPagesReport {
        portal,
        pages_fetched,
        jobs_stored,
        stopped_reason,
    })
}

/// Preview a draft portal configuration without writing anything.
///
/// This is the "Test config" button's core path. It takes no pool: a preview
/// cannot store rows, and the URL is validated here so a typo is classified as
/// `InvalidRequest` instead of surfacing as a transport failure.
pub async fn preview_config(
    url: &str,
    company_name: &str,
    render_mode: &str,
    scraper_config: Option<&str>,
    source: &dyn FetchSource,
    browser: Option<&dyn BrowserBackend>,
) -> std::result::Result<router::PreviewReport, FetchError> {
    let parsed = Url::parse(url.trim()).map_err(|error| FetchError {
        cause: FailureCause::InvalidRequest,
        detail: format!("portal url is invalid: {error}"),
    })?;

    router::preview(
        &parsed,
        company_name,
        render_mode,
        scraper_config,
        source,
        browser,
    )
    .await
}

/// One portal's result within a batch.
#[derive(Debug, Clone)]
pub struct BatchEntry {
    pub portal_id: i64,
    pub portal_name: String,
    /// Number of jobs stored on success; full fetch error on failure.
    /// The error carries both the classified `cause` (for the badge)
    /// and the human-readable `detail` (for the drill-down).
    pub result: std::result::Result<usize, FetchError>,
}

/// Result of a batch fetch.
#[derive(Debug)]
pub struct BatchReport {
    pub results: Vec<BatchEntry>,
}

impl BatchReport {
    pub fn succeeded(&self) -> usize {
        self.results.iter().filter(|e| e.result.is_ok()).count()
    }

    pub fn failed(&self) -> usize {
        self.results.iter().filter(|e| e.result.is_err()).count()
    }
}

/// Fetch a batch of portals, continuing past individual failures.
///
/// `progress` is called after each portal completes, so a view can
/// display per-portal status without waiting for the whole batch. It is
/// a plain closure — this module does not know about any UI framework.
///
/// The batch itself only fails on an unexpected DB error (reading a
/// portal row). Individual fetch failures are recorded as `Err` in the
/// corresponding `BatchEntry`; the error carries both the classified
/// cause and the human-readable detail.
pub async fn fetch_batch<F>(
    pool: &SqlitePool,
    portal_ids: &[i64],
    source: &dyn FetchSource,
    browser: Option<&dyn BrowserBackend>,
    progress: F,
) -> Result<BatchReport>
where
    F: FnMut(&BatchEntry),
{
    fetch_batch_inner(pool, portal_ids, source, browser, None, progress).await
}

/// Fetch a batch of portals, stopping between portals when `cancel` is set.
///
/// A fetch that is already in progress finishes first; cancellation is checked
/// before the next portal starts. This is the same coarse guarantee the UI
/// offers: the user does not have to wait for the whole batch, but no portal is
/// left half-written.
///
/// The callback still receives every completed entry, so the caller can keep
/// per-portal progress without waiting for the batch to return.
pub async fn fetch_batch_cancellable<F>(
    pool: &SqlitePool,
    portal_ids: &[i64],
    source: &dyn FetchSource,
    browser: Option<&dyn BrowserBackend>,
    cancel: &AtomicBool,
    progress: F,
) -> Result<BatchReport>
where
    F: FnMut(&BatchEntry),
{
    fetch_batch_inner(pool, portal_ids, source, browser, Some(cancel), progress).await
}

async fn fetch_batch_inner<F>(
    pool: &SqlitePool,
    portal_ids: &[i64],
    source: &dyn FetchSource,
    browser: Option<&dyn BrowserBackend>,
    cancel: Option<&AtomicBool>,
    mut progress: F,
) -> Result<BatchReport>
where
    F: FnMut(&BatchEntry),
{
    let mut results = Vec::with_capacity(portal_ids.len());

    for &portal_id in portal_ids {
        if cancel.is_some_and(|cancel| cancel.load(Ordering::Relaxed)) {
            break;
        }

        let name = portals::get(pool, portal_id)
            .await?
            .map(|p| p.company_name)
            .unwrap_or_else(|| format!("portal {portal_id}"));

        let entry = match router::fetch_portal_cancellable(pool, portal_id, source, browser, cancel)
            .await
        {
            Ok(outcome) => BatchEntry {
                portal_id,
                portal_name: name,
                result: Ok(outcome.jobs_fetched),
            },
            Err(e) => BatchEntry {
                portal_id,
                portal_name: name,
                result: Err(e),
            },
        };

        progress(&entry);
        results.push(entry);
    }

    Ok(BatchReport { results })
}
