//! Portals service layer.
//!
//! Headless orchestration that a Portals view will call. Every function
//! takes explicit dependencies (a pool, a `FetchSource`, an optional
//! `BrowserBackend`) so tests can inject mocks without any framework.

use std::sync::atomic::{AtomicBool, Ordering};

use sqlx::SqlitePool;

use super::health::PortalHealth;
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
    let portal = portals::get(pool, portal_id)
        .await?
        .ok_or_else(|| CoreError::Internal("portal vanished during fetch"))?;
    Ok(FetchReport { portal, outcome })
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

        let entry = match router::fetch_portal(pool, portal_id, source, browser).await {
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
