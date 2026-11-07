# Phase 2: Acquisition — Detailed Execution Plan

**Parent plan:** Draft Execution Plan: Career-Ops Rust Desktop Application (v0.5), §12
**Status:** Draft — Weeks 1–3 complete; Week 4 restructured around the service/view split
**Last updated:** 2026-10-20
**Duration:** 1 week remaining (Week 4)
**Depends on:** Phase 1 completion (confirmed 2026-10-19)

---

## Change Log

| Version | Change |
| :--- | :--- |
| v0.1 | Initial Phase 2 plan; browser backend feature-gated, `rust-headless-chrome` as default. |
| v0.2 | Browser backend is now a **required** dependency (not feature-gated). Obscura is the chosen backend. `rust-headless-chrome` retained as documented fallback. Tier 1 remains the default path where it works, but is no longer expected to cover the majority of target portals. |
| v0.3 | Revised against Phase 1 outcomes and current ecosystem. Removed the `print_to_pdf` deliverable. Added evaluation of Obscura's embedded Rust API alongside the sidecar model. Added `chromiumoxide` as a second fallback. Made test-support and mock-backend dependencies explicit. Added toolchain requirement to entry conditions. Added a verification-pass exit criterion. |
| v0.4 | **Sidecar integration confirmed.** The embedded model is deferred to a future enhancement. Rationale: crash isolation, GPUI event-loop safety, and portability across browser backend swaps. Reduced duration from 3–5 weeks to 3–4 weeks. |
| v0.5 | **Corrected the PDF claim.** Obscura v0.2.5, built with the `render` feature, supports `Page.printToPDF` (verified: 29305 bytes, `%PDF` magic). Phase 2 still does not implement `print_to_pdf` — PDF rendering is Phase 4. Phase 4's first task is therefore an Obscura-vs-`rust-headless-chrome` fidelity evaluation. |
| v0.6 | Added `portals.last_fetch_attempt_at` and `portals.last_failure_cause` (migration `0002`). The router persists a classified failure cause on every failed fetch, and both columns are cleared/updated on success. `portals::record_fetch_attempt` supersedes `mark_verified`. Workstream 4's "Failure causes are classified and stored" criterion split into classification (unit-testable) and persistence (integration-testable). |
| v0.7 | **Week 4 restructured around the GPUI constraint.** Workstreams 5 and 6 now deliver the **service layer** — CRUD orchestration, health summary, suggested actions, manual entry logic — verified by headless tests. The GPUI view layer is documented as deferred to Phase 5, with the service layer as its intended base. Exit criteria 12 and 13 reworded to reflect the verifiable deliverable. New "View Layer (Deferred)" section records what Phase 5 builds on top. No new workstream added. |
| v0.8 | Workstream 5 complete. Portals service layer delivers `PortalHealth::from_portal` (four health states, cause parsing, suggested actions), `list_with_health`, `fetch_one` with post-fetch refresh, and `fetch_batch` with per-portal results and a plain-closure progress callback. `BatchEntry.result` widened from `Result<usize, FailureCause>` to `Result<usize, FetchError>` so the view layer can render per-portal detail without a second fetch. |

---

## Purpose

Phase 2 makes the application **useful for the first time**. It implements the tiered job acquisition pipeline: Tier 1 (HTTP/JSON) handles portals that return complete content without JavaScript, and Tier 2 (headless browser via Obscura) handles the portals that require JS execution.

**A material change from the v0.1 plan:** `reqwest` + `scraper` are not sufficient for the target portals. JavaScript execution is required. The browser backend is a **required dependency**, not a feature-gated escalation.

**A second material change in v0.3:** Phase 1 established that `BrowserBackend::print_to_pdf` has a default implementation returning `BackendUnsupported`. Phase 2 does not implement `print_to_pdf` on the Obscura backend. PDF rendering is a Phase 4 concern. What changed in v0.5 is the *reason*: Obscura **can** produce PDFs (verified during the Week 1 spike), but PDF rendering is out of scope for Phase 2. Phase 4 will decide whether Obscura or `rust-headless-chrome` is the `PdfBackend`.

**A third material change in v0.4:** **Sidecar is the confirmed integration model.** The rationale is threefold:

1. **Crash isolation.** A V8 panic kills the sidecar process, not the host application.
2. **GPUI event-loop safety.** CDP calls are I/O-bound and can be driven from a tokio task.
3. **Backend portability.** All three candidate backends are CDP-driven subprocesses. The embedded model is unique to Obscura.

**A fourth material change in v0.7:** Week 4 is split into a **service layer** (verifiable, no GPUI dependency) and a **view layer** (deferred to Phase 5). The GPUI constraint is documented at the point it matters: the service layer is what Phase 2 ships, and the view is what Phase 5 builds on top.

---

## Entry Conditions

Phase 2 begins after Phase 1 exit criteria are met. Phase 1 is confirmed complete as of 2026-10-19. Consumed outputs:

| Phase 1 Output | How Phase 2 Uses It |
| :--- | :--- |
| `BrowserBackend` trait with `print_to_pdf` default | Implemented by the Obscura sidecar backend; `print_to_pdf` inherits `BackendUnsupported` in Phase 2 |
| `PdfBackend` trait definition | Not used in Phase 2 |
| `MockBrowserBackend` in `core::backend` | Used for routing and escalation tests |
| `career-ops-test-support` crate | Persistent keychain backend for integration tests |
| `portals` table and repository, including `last_fetch_attempt_at` and `last_failure_cause` (migration `0002`) | Source of portal configuration and health state |
| `job_descriptions` table and repository, including `content_hash` and `upsert` | Destination for acquired listings; dedup logic already implemented |
| Keystore and unlock flow | Gates access to portal configuration and acquired data |
| `AppState` state machine | Host for acquisition progress state |
| GPUI shell with navigation | Host for the Portals screen (placeholder) |
| Test/CI infrastructure | Extended with acquisition and browser backend tests |

**Toolchain requirement:** Rust 1.95.0 or newer. The GPUI pin requires `std::hint::cold_path`.

**No Phase 0 output is consumed** because Phase 0 was skipped.

---

## Scope

| In scope | Out of scope |
| :--- | :--- |
| Tier 1a: ATS JSON API adapters (Greenhouse, Lever, Ashby, SmartRecruiters) | AI evaluation of job postings |
| Tier 1b: `reqwest` + `scraper` HTML extraction for static career pages | Cover letter generation |
| **Tier 2: Obscura sidecar-based `BrowserBackend`** | PDF rendering of CV or cover letters |
| Sidecar process lifecycle management | `rig` integration and LLM calls |
| Tier routing with escalation, `fetch_tier` recording, and failure persistence | Prompt seeding and management |
| **Portals service layer:** health summary, suggested actions, batch fetch orchestration | File-based data migration |
| **Manual entry service layer:** URL fetch and paste fallback, with content-hash dedup | Full CV editor |
| Acquisition tests using saved HTML/JSON fixtures | GPUI view layer for Portals or Pipeline screens (deferred to Phase 5) |
| **Verification pass** over Phase 2 exit criteria | Bulk import from external sources |
| Fallback path to `rust-headless-chrome` or `chromiumoxide` if Obscura proves unsuitable | Obscura embedded-crate integration (deferred) |

---

## Sequencing Rationale

```
W1: Obscura sidecar integration                                            ✅ COMPLETE
    └── W2: Tier 1a (ATS JSON) + Tier 1b (HTML extraction)                 ✅ COMPLETE
        └── W3: Tier routing, failure persistence, retry helper            ✅ COMPLETE
            └── W4: Portals service layer, manual entry service layer, verification pass
```

**The service layer is Week 4's deliverable, not the view layer.** This is a deliberate scope boundary driven by two facts:

1. **GPUI is pre-1.0 and unstable.** A view built today would need rewriting when Phase 5 resumes UI work. Paying for the same work twice is a worse outcome than deferring it.
2. **The service layer is what has verifiable behaviour.** A health summary that maps `FailureCause::Blocked` to "the site may be blocking automated requests" is testable. Whether the view renders that message in a card or a table is a polish concern, not a correctness concern.

The view layer is described in the **View Layer (Deferred)** section below, so Phase 5 has a concrete contract to implement against. It is not a Phase 2 deliverable.

---

## Workstream 1 — Obscura Sidecar `BrowserBackend` Integration

**Status:** ✅ COMPLETE (Week 1, 2026-10-20).

### Week 1 Spike Results

| Question | Answer | Evidence |
| :--- | :--- | :--- |
| Sidecar spawns and binds on an allocated port | ✅ | `obscura serve --port <allocated>`; `TcpStream::connect` succeeds within ~100ms |
| `chromiumoxide` connects and drives a page | ✅ | `Browser::connect` succeeded; `new_page` and `goto` worked |
| JS-heavy page returns post-JS DOM | ✅ | `page.content()` returned HTML containing `<html` |
| Sidecar death is detected on next connect | ✅ | After `child.kill()`, `Browser::connect` returned `Err`, not a hang |
| `Page.printToPDF` succeeds | ✅ | 29305 bytes, `%PDF` magic — changes Phase 4's scope |
| End-to-end latency per fetch | ~2.8s | Includes sidecar spawn, CDP connect, navigation, DOM extraction |

**Data points worth preserving:**

- **Obscura version tested:** v0.2.5, built with the `render` feature.
- **Console noise is expected.** Host page console errors are surfaced by Obscura; they do not indicate test failure.
- **`printToPDF` changes Phase 4.** Obscura is not scraping-only. Whether its rendering fidelity is sufficient for ATS-friendly resume templates is a Phase 4 question.

---

## Workstream 2 — Tier 1a: ATS JSON API Adapters

**Status:** ✅ COMPLETE (Week 2).

Four adapters — Greenhouse, Lever, Ashby, SmartRecruiters — plus URL detection, `NormalizedJob` normalization, and structured `AtsError` classification. 28 fixture tests, 4 live tests (gated by `RUN_LIVE_ATS_TESTS`), 1 offline detection round-trip.

---

## Workstream 3 — Tier 1b: HTML Extraction

**Status:** ✅ COMPLETE (Week 2).

`fetch_static_jobs` (HTTP + JS-required detection) and `extract_from_html` (JSON-LD + CSS-selector chain). `extract_from_html` is reused by Tier 2 on rendered pages.

---

## Workstream 4 — Tier Routing, Normalization, and Storage

**Status:** ✅ COMPLETE (Week 3).

- `router::fetch_portal` with dispatch, escalation, and outcome recording
- `decide_initial_route` as a pure function
- `AtsError` and `FailureCause` classification
- `portals::record_fetch_attempt` persists the outcome to `last_fetch_attempt_at`, `last_verified_at`, and `last_failure_cause`
- Retry helper (`http::get_with_retry`) wired into all four adapters
- Dedup inherited from Phase 1's `job_descriptions::upsert`
- 9 router tests, 4 HTTP helper tests

---

## Workstream 5 — Portals Service Layer

**Priority:** Week 4, first half.
**Estimated duration:** 2–3 days

### Scope

This workstream delivers the **service layer** that a Portals UI will eventually call. It does **not** deliver the UI. Every deliverable here is a pure function or a headless orchestration that a test can exercise without GPUI.

Portal CRUD is already in `repo::portals`. The new work is:

1. **Health summary.** A pure function that turns a `Portal` row into a `PortalHealth` struct the view can render.
2. **Suggested actions.** A mapping from `FailureCause` to a user-facing message.
3. **Batch fetch orchestration.** Fetch all portals (or a filtered subset), continue on individual failures, and report per-portal results.
4. **Fetch-and-refresh.** A single-portal fetch that returns the updated portal so the view does not have to re-read.

### Deliverables

**5.1 `core::portals::health` module**

```rust
// crates/core/src/portals/health.rs

use chrono::{DateTime, Utc};

use crate::acquisition::error::FailureCause;
use crate::model::Portal;

/// Health state for one portal, computed from its stored columns.
/// Pure data — no I/O, no rendering.
#[derive(Debug, Clone)]
pub struct PortalHealth {
    pub state: HealthState,
    pub last_attempt_at: Option<DateTime<Utc>>,
    pub last_success_at: Option<DateTime<Utc>>,
    pub failure_cause: Option<FailureCause>,
    pub suggested_action: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthState {
    /// Never fetched.
    NeverFetched,
    /// Last fetch succeeded.
    Healthy,
    /// Last fetch failed, but `last_verified_at` shows the portal
    /// worked at some point.
    Degraded,
    /// Last fetch failed and the portal has never succeeded.
    Broken,
}

impl PortalHealth {
    pub fn from_portal(portal: &Portal) -> Self {
        let failure_cause = portal
            .last_failure_cause
            .as_deref()
            .and_then(FailureCause::parse);

        let state = match (
            portal.last_fetch_attempt_at,
            portal.last_verified_at,
            failure_cause,
        ) {
            (None, _, _) => HealthState::NeverFetched,
            (Some(_), _, None) => HealthState::Healthy,
            (Some(_), Some(_), Some(_)) => HealthState::Degraded,
            (Some(_), None, Some(_)) => HealthState::Broken,
        };

        let suggested_action = failure_cause.map(suggested_action);

        Self {
            state,
            last_attempt_at: portal.last_fetch_attempt_at,
            last_success_at: portal.last_verified_at,
            failure_cause,
            suggested_action,
        }
    }
}

/// Map a failure cause to a user-facing action. Each cause has exactly
/// one action — the string is the thing the user should try next.
pub fn suggested_action(cause: FailureCause) -> String {
    match cause {
        FailureCause::JsRequired => {
            "Switch this portal to `browser` mode in its settings.".into()
        }
        FailureCause::Blocked => {
            "The site may be blocking automated requests. Try again later, or switch to `browser` mode.".into()
        }
        FailureCause::Timeout => {
            "The request timed out. Check network connectivity, or retry.".into()
        }
        FailureCause::ParseError => {
            "The page structure may have changed. Check the portal URL, or configure scraper selectors.".into()
        }
        FailureCause::NotFound => {
            "The board was not found. Check the company slug in the URL.".into()
        }
        FailureCause::Transport => {
            "Could not reach the site. Check network connectivity.".into()
        }
        FailureCause::InvalidRequest => {
            "The portal configuration is invalid. Check the URL and scraper config.".into()
        }
        FailureCause::Other => {
            "The fetch failed for an unrecognized reason. Try again, or check the portal configuration.".into()
        }
    }
}
```

`FailureCause::parse` is a new method on the existing enum:

```rust
impl FailureCause {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "js_required" => Self::JsRequired,
            "blocked" => Self::Blocked,
            "timeout" => Self::Timeout,
            "parse_error" => Self::ParseError,
            "not_found" => Self::NotFound,
            "transport" => Self::Transport,
            "invalid_request" => Self::InvalidRequest,
            "other" => Self::Other,
            _ => return None,
        })
    }
}
```

**5.2 `core::portals::service` module**

```rust
// crates/core/src/portals/service.rs

use sqlx::SqlitePool;

use super::health::PortalHealth;
use crate::acquisition::error::FailureCause;
use crate::acquisition::router::{self, FetchOutcome, FetchSource};
use crate::backend::BrowserBackend;
use crate::error::Result;
use crate::model::Portal;
use crate::repo::portals;

/// A portal with its health summary, for list views.
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

/// Fetch one portal and return the updated portal row.
///
/// The router records the outcome to the portal row; this function
/// re-reads it so the caller has the post-fetch state without a second
/// query.
pub async fn fetch_one(
    pool: &SqlitePool,
    portal_id: i64,
    source: &dyn FetchSource,
    browser: Option<&dyn BrowserBackend>,
) -> Result<FetchReport> {
    let outcome = router::fetch_portal(pool, portal_id, source, browser).await;
    let portal = portals::get(pool, portal_id)
        .await?
        .ok_or_else(|| crate::error::CoreError::Internal("portal vanished during fetch"))?;
    Ok(FetchReport { portal, outcome })
}

/// Outcome of a single-portal fetch, plus the post-fetch portal state.
pub struct FetchReport {
    pub portal: Portal,
    pub outcome: std::result::Result<FetchOutcome, router::FetchError>,
}

/// Result of a batch fetch, one entry per portal attempted.
pub struct BatchReport {
    pub results: Vec<BatchEntry>,
}

pub struct BatchEntry {
    pub portal_id: i64,
    pub portal_name: String,
    pub result: std::result::Result<usize, FailureCause>,
}

/// Fetch a batch of portals, continuing past individual failures.
///
/// `progress` is called after each portal completes. It is a plain
/// closure so the caller (a view, a CLI, a test) can record progress
/// without this module knowing about any UI framework.
pub async fn fetch_batch<F>(
    pool: &SqlitePool,
    portal_ids: &[i64],
    source: &dyn FetchSource,
    browser: Option<&dyn BrowserBackend>,
    mut progress: F,
) -> Result<BatchReport>
where
    F: FnMut(&BatchEntry),
{
    let mut results = Vec::with_capacity(portal_ids.len());

    for &portal_id in portal_ids {
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
                result: Err(e.cause),
            },
        };

        progress(&entry);
        results.push(entry);
    }

    Ok(BatchReport { results })
}
```

**5.3 `crates/core/src/portals/mod.rs`**

```rust
//! Portals service layer.
//!
//! Pure functions and headless orchestration that a Portals view will
//! eventually call. No GPUI dependency — the view is Phase 5.

pub mod health;
pub mod service;
```

**5.4 `crates/core/src/lib.rs`** — one line

```rust
pub mod portals;
```

The `acquisition` and `repo` modules are unchanged; this is a new sibling for the service-layer logic that composes them.

### Critical Implementation Notes

1. **No `browser_status` method.** The sidecar status indicator was in the earlier plan, but `BrowserBackend` has no status query, and adding one would mean changing the trait, every mock, and the concrete implementation for a feature no UI is there to consume. It is deferred to Phase 5, when the view exists and the trait can be extended with a real consumer.
2. **`PortalHealth::from_portal` is pure.** No I/O, no time source beyond the fields in the row. Every state transition is testable by constructing a `Portal` with different column values.
3. **`suggested_action` returns `String`, not `&'static str`.** Two of the messages are composed from dynamic values today (none, actually — all are static). `String` gives room to include portal-specific detail later without a signature change.
4. **Batch fetch is sequential.** Fetching portals in parallel would be faster but complicates progress reporting and error attribution. A desktop app fetching 10–20 portals sees no meaningful latency difference, and sequential keeps the code straightforward. Parallelism is a Phase 5 or Phase 7 concern.
5. **The progress callback is a plain `FnMut(&BatchEntry)`.** It does not know about GPUI, tokio tasks, or channels. The view layer will wrap it in whatever the framework needs.

### Acceptance Criteria

| Criterion | Verification |
| :--- | :--- |
| `PortalHealth::from_portal` returns `NeverFetched` for a portal with no attempt | Unit test |
| `PortalHealth::from_portal` returns `Healthy` for a portal whose last attempt succeeded | Unit test |
| `PortalHealth::from_portal` returns `Degraded` when the last attempt failed but a prior success exists | Unit test |
| `PortalHealth::from_portal` returns `Broken` when every attempt has failed | Unit test |
| Each `FailureCause` maps to a distinct `suggested_action` | Unit test (exhaustive match) |
| `FailureCause::parse` round-trips every `as_str` value | Unit test |
| `list_with_health` returns one entry per portal | Integration test |
| `fetch_one` returns the updated portal after a successful fetch | Integration test |
| `fetch_batch` continues when one portal fails | Integration test with mock backend |
| `fetch_batch` reports per-portal results | Integration test (assert the shape of `BatchReport`) |

---

## Workstream 6 — Manual Job Entry Service Layer

**Priority:** Week 4, second half.
**Estimated duration:** 1–2 days

### Scope

Manual entry has two entry points:

- **URL fetch** — user pastes a careers URL; the service classifies it (ATS or not), fetches, and stores.
- **Paste fallback** — user pastes raw text; the service stores it with `fetch_tier = 'manual'`.

Both paths go through `job_descriptions::upsert`, which already handles content-hash deduplication. The service layer's job is to normalize the input into the shape `upsert` expects.

### Deliverables

**6.1 `core::acquisition::manual` module**

```rust
// crates/core/src/acquisition/manual.rs

//! Manual job entry.
//!
//! Two paths, both ending in `job_descriptions::upsert`:
//!
//! - **From a URL** — the URL is classified and fetched via the same
//!   router logic a portal uses, then stored with `fetch_tier` set to
//!   the tier that succeeded.
//! - **From pasted text** — the user supplies title, company, and body
//!   directly. Stored with `fetch_tier = 'manual'` and a synthetic
//!   external ID derived from the content hash.
//!
//! ## Deduplication
//!
//! `job_descriptions::upsert` dedups by `(source_url, external_id)`,
//! then by `(source_url, content_hash)`, then by
//! `(source_url, title, company)`. For manual entries the source URL is
//! whatever the user supplied (or a synthetic `manual://` URL if they
//! pasted text with no URL), and the content hash is computed from
//! title + company + raw_text. This means a manually-entered job that
//! later appears via a portal fetch will be merged by content hash, as
//! long as both entries have the same source URL. They usually will,
//! because users paste the URL they found the job at.

use chrono::Utc;
use sqlx::SqlitePool;
use url::Url;

use super::ats::{self, AtsAdapter, NormalizedJob};
use super::error::AtsError;
use super::html;
use super::router::{self, FetchSource};
use crate::backend::BrowserBackend;
use crate::error::{CoreError, Result};
use crate::model::{FetchTier, JobDescription};
use crate::repo::job_descriptions;

/// Result of a manual URL fetch.
pub struct ManualFetchResult {
    pub stored: JobDescription,
    pub tier: FetchTier,
    pub warnings: Vec<String>,
}

/// Fetch a single job from a URL and store it.
///
/// Unlike a portal fetch, which expects a board with many jobs, this
/// fetches one page and extracts the first job it finds. If the page
/// is an ATS board URL with many jobs, the first one is stored and a
/// warning is returned noting the count.
pub async fn add_job_from_url(
    pool: &SqlitePool,
    url_str: &str,
    source: &dyn FetchSource,
    browser: Option<&dyn BrowserBackend>,
) -> Result<ManualFetchResult> {
    let url = Url::parse(url_str)
        .map_err(|e| CoreError::Internal(format!("invalid url: {e}")))?;

    let adapters = ats::adapters();
    let mut warnings = Vec::new();

    // Try each adapter's detection first. If an ATS URL is recognized,
    // fetch the whole board and take the first job whose source_url
    // matches — that is the one the user meant.
    for adapter in &adapters {
        if let Some(slug) = adapter.detect(&url) {
            let jobs = source
                .fetch_ats(adapter.as_ref(), &slug, "Manual")
                .await
                .map_err(|e| CoreError::Internal(format!("fetch failed: {e}")))?;

            let target = pick_job_for_url(&jobs, &url).ok_or_else(|| {
                CoreError::Internal(format!(
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

    // Not an ATS URL. Try Tier 1b.
    match source.fetch_html(&url, "Manual", None).await {
        Ok(jobs) if !jobs.is_empty() => {
            let target = pick_job_for_url(&jobs, &url).unwrap_or_else(|| jobs[0].clone());
            return store_one(pool, target, FetchTier::Tier1bHtml, warnings).await;
        }
        _ if browser.is_some() => {
            // Tier 1b failed or returned nothing; escalate to Tier 2
            // using the same extraction path.
            let backend = browser.unwrap();
            let page = backend
                .fetch_rendered(&url, crate::backend::RenderOpts::default())
                .await
                .map_err(|e| CoreError::Internal(format!("tier 2 fetch failed: {e}")))?;
            match html::extract_from_html(&page.html, &url, "Manual", None) {
                Ok(jobs) if !jobs.is_empty() => {
                    let target = pick_job_for_url(&jobs, &url).unwrap_or_else(|| jobs[0].clone());
                    return store_one(pool, target, FetchTier::Tier2Obscura, warnings).await;
                }
                _ => {}
            }
            return Err(CoreError::Internal(
                "fetched the page but no jobs were found".into(),
            ));
        }
        Err(e) => {
            return Err(CoreError::Internal(format!("fetch failed: {e}")));
        }
        Ok(_) => {
            return Err(CoreError::Internal(
                "fetched the page but no jobs were found".into(),
            ));
        }
    }
}

/// Store a manually-pasted job.
///
/// The source URL is synthesized from the title and company so that
/// dedup by `(source_url, content_hash)` can work later. A user who
/// pastes the same job twice gets one row; a user who pastes it once
/// and later fetches it via a portal gets two rows only if the portal
/// URL differs, which is expected — the portal URL is the canonical
/// source, and the pasted version is a manual copy the user can delete.
pub async fn add_job_from_text(
    pool: &SqlitePool,
    title: &str,
    company: &str,
    raw_text: &str,
) -> Result<JobDescription> {
    if title.trim().is_empty() {
        return Err(CoreError::Internal("title cannot be empty".into()));
    }
    if raw_text.trim().is_empty() {
        return Err(CoreError::Internal("job description cannot be empty".into()));
    }

    let synthetic_url = format!(
        "manual://{}/{}",
        slugify(company),
        slugify(title)
    );

    let external_id = format!(
        "manual:{}",
        crate::repo::content_hash(&format!("{title}\n{company}\n{raw_text}"))
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
        .map_err(|e| CoreError::Internal(format!("store failed: {e}")))
}

fn pick_job_for_url(jobs: &[NormalizedJob], target: &Url) -> Option<NormalizedJob> {
    // Exact match first.
    if let Some(j) = jobs.iter().find(|j| j.source_url == *target) {
        return Some(j.clone());
    }
    // Path match second — the user may have pasted a URL with a query
    // string or fragment.
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
        .map_err(|e| CoreError::Internal(format!("store failed: {e}")))?;
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
```

**6.2 `crates/core/src/acquisition/mod.rs`** — one line

```rust
pub mod manual;
```

### Critical Implementation Notes

1. **Manual paste cannot dedup against portal fetches.** A pasted job has a synthetic `manual://` URL, while a portal-fetched copy has the real job URL. Dedup is by `(source_url, content_hash)`, and the URLs differ, so both rows survive. This is deliberate: the user who pasted the job did so for a reason, and silently merging it into a portal-fetched row would delete their manual copy. The UI should offer "duplicate of portal-fetched job" as an action, but the service layer does not merge.
2. **Manual paste does dedup against itself.** Pasting the same title, company, and body twice produces the same content hash and the same synthetic URL, so `upsert` returns one row.
3. **URL fetch does dedup against portal fetches.** The stored `source_url` is the real job URL, so if the portal fetch already stored that job, the manual fetch updates the existing row rather than creating a duplicate.
4. **`pick_job_for_url` tries exact then path match.** A user pasting a URL with tracking parameters (`?utm_source=...`) will not match the job's canonical URL exactly, but the path will match.
5. **`add_job_from_url` requires a `FetchSource`.** Same abstraction as the router. Tests inject a mock. The `browser` argument is optional, as it is for the router.

### Acceptance Criteria

| Criterion | Verification |
| :--- | :--- |
| `add_job_from_url` stores an ATS job fetched from a board URL | Integration test with mock source |
| `add_job_from_url` stores an HTML-extracted job from a non-ATS URL | Integration test |
| `add_job_from_url` escalates to Tier 2 when Tier 1b yields nothing and a browser is available | Integration test |
| `add_job_from_url` returns a warning when a board has multiple jobs | Unit test |
| `add_job_from_text` stores a pasted job with `fetch_tier = 'manual'` | Integration test |
| `add_job_from_text` dedups identical pastes | Integration test (paste twice, assert one row) |
| `add_job_from_text` rejects empty title or body | Unit test |
| Manual URL fetch dedups against a prior portal fetch of the same URL | Integration test |
| Manual paste does **not** merge into a portal-fetched row | Integration test (assert two rows) |

---

## View Layer (Deferred)

**Status:** Not a Phase 2 deliverable. Documented here so Phase 5 has a contract.

### What Phase 5 will build

The Portals screen. The service layer above provides everything the view needs:

| View element | Service source |
| :--- | :--- |
| Portal list | `portals::service::list_with_health` |
| Health badge (Healthy / Degraded / Broken / Never fetched) | `PortalWithHealth.health.state` |
| Last attempt / last success timestamps | `PortalHealth.last_attempt_at`, `.last_success_at` |
| Failure cause and suggested action | `PortalHealth.failure_cause`, `.suggested_action` |
| "Fetch now" button | `portals::service::fetch_one` |
| "Fetch all" button with per-portal progress | `portals::service::fetch_batch` with a progress callback |
| Add / edit / delete portal | `portals::create`, `portals::update`, `portals::delete` (from Phase 1) |
| Manual URL entry | `acquisition::manual::add_job_from_url` |
| Manual paste entry | `acquisition::manual::add_job_from_text` |

The view constructs these calls, renders the returned structs, and threads user actions into them. It does no business logic.

### Why it is deferred

- **GPUI is pre-1.0.** Every view file in `crates/app/src/screens.rs` is still a placeholder. Building views against an unstable API means paying the cost twice when it stabilises.
- **The service layer is what has behaviour.** A view that renders a `PortalHealth` struct is trivial once the struct exists and is correct. The correctness lives in the service layer, and that is what Phase 2 verifies.
- **Phase 5 owns the UI.** §12 of the main plan lists Settings, onboarding, and visual polish under Phase 5. Adding a Phase 2 view layer would mean Phase 5 has less to do and Phase 2 has a deliverable that cannot be verified.

### What "acceptable" means for Phase 5

When Phase 5 builds the view, the acceptance criteria should be:

- Every service-layer struct field that carries user-meaningful information is rendered somewhere on the screen.
- Every `HealthState` variant has a visually distinct presentation.
- Every `suggested_action` is displayed alongside its `failure_cause`.
- Every action button calls the corresponding service function and refreshes the list on completion.
- Progress during `fetch_batch` is visible and cancellable.

Those criteria are testable in a way that "the view exists" is not.

---

## Test Strategy

### Fixture-Based Testing (Weeks 2–3, complete)

| Fixture type | Source | Used by |
| :--- | :--- | :--- |
| JS-heavy SPA shell HTML | Curated | Obscura integration, routing |
| ATS JSON responses | Curated | Tier 1a adapters |
| Static career page HTML | Curated | Tier 1b extraction |
| Malformed JSON / HTML | Hand-crafted | Error handling |

### Mock-Based Testing

- Routing logic tests use `MockBrowserBackend` from `core::backend` and `MockFetchSource` in `tests/router.rs`.
- Keychain-dependent integration tests use `career-ops-test-support::install()`.
- Portals service-layer tests use `MockFetchSource` for fetch orchestration and construct `Portal` rows directly for `PortalHealth` unit tests.

### Integration Tests

End-to-end flows:

1. Add a Greenhouse portal → fetch → verify `fetch_tier = 'tier1a:ats'`. ✅ (Week 3)
2. Add a static career page → fetch → verify `fetch_tier = 'tier1b:html'`. ✅ (Week 3)
3. Add an SPA portal → fetch → verify escalation to Tier 2 → `fetch_tier = 'tier2:obscura'`. ✅ (Week 3)
4. Fetch the same portal twice → verify no duplicates via `upsert`. ✅ (Phase 1 test)
5. Failed fetch records a classified cause in `last_failure_cause`. ✅ (Week 3)
6. `list_with_health` returns a `PortalHealth` for each portal with correct state. (Week 4)
7. `fetch_batch` continues past failures and reports per-portal results. (Week 4)
8. `add_job_from_url` stores a job from an ATS board URL. (Week 4)
9. `add_job_from_text` stores a pasted job and dedups on repeat. (Week 4)
10. Manual URL fetch dedups against a prior portal fetch. (Week 4)

### CI Jobs

| Job | Purpose |
| :--- | :--- |
| `acquisition-fixtures` | Tier 1a and 1b tests against saved fixtures |
| `browser-obscura` | Obscura sidecar integration tests (environment-gated) |
| `browser-fallback` | Same scraping tests with `rust-headless-chrome` or `chromiumoxide`, verifying the trait abstraction |
| `routing` | Tier routing and escalation tests with mock backends |
| `portals-service` | Portal health, batch fetch, and manual entry tests |
| `live-ats` | Nightly live ATS adapter tests (separate workflow) |

---

## Phase 2 Risks

| ID | Risk | Impact | Mitigation |
| :--- | :--- | :--- | :--- |
| P2-R1 | **Obscura integration proves harder than expected.** | Resolved | Week 1 spike confirmed CDP compatibility. |
| P2-R2 | **Obscura cannot render PDFs.** | Resolved | Week 1 spike confirmed `Page.printToPDF` works. |
| P2-R3 | **Sidecar process management introduces complexity.** | Mitigated | Port allocation, stale-sidecar detection, SIGTERM-then-SIGKILL shutdown, `kill_on_drop(true)`. Tested in Week 1. |
| P2-R4 | **Tier 1 is less valuable than expected.** | Low | Accepted. Tier 1 is an optimization, not the primary path. |
| P2-R5 | **Portals service-layer scope creep.** | Medium | Service layer is pure functions and headless orchestration. No rendering, no GPUI. Resist additions. |
| P2-R6 | **Test passes for the wrong reason.** | Medium | Week 4 includes a verification pass. For each exit criterion, confirm the test exercises the property, not a proxy. |
| P2-R7 | **A future backend swap invalidates the sidecar assumptions.** | Low | All three candidate backends are CDP-driven subprocesses. |
| P2-R8 | **Console noise from target pages obscures real errors.** | Low | Filter console output by level in production. |
| P2-R9 | **Service layer and view layer diverge during Phase 5.** The view may need fields the service layer does not provide. | Medium | The "View Layer (Deferred)" section is the contract. If the view needs a new field, add it to the service layer via a test-first change; do not compute it in the view. |

---

## Weekly Budget

| Week | Focus | Deliverable |
| :--- | :--- | :--- |
| **1** | Obscura sidecar spike | ✅ Complete |
| **2** | Tier 1a and 1b | ✅ Complete |
| **3** | Tier routing, failure persistence, retry helper | ✅ Complete |
| **4** | Portals service layer, manual entry service layer, verification pass, retrospective | Health summary, batch fetch, manual entry, verification memo |

**Total:** Week 4 is 3–4 days of implementation plus 1–2 days of verification and retrospective.

---

## Exit Criteria

Phase 2 is complete when **all** of the following are true:

1. ✅ **Complete (Week 1).** The Obscura sidecar fetches JS-heavy pages and returns post-JS DOM.
2. ✅ **Complete (Week 1).** The sidecar starts lazily, uses an allocated port, and recovers after a crash.
3. ✅ **Complete (Week 1).** Stale sidecars are detected and cleaned up on startup.
4. ✅ **Complete (Week 1).** The sidecar is shut down cleanly on application exit.
5. ✅ **Complete (Week 1).** `ObscuraBrowserBackend::print_to_pdf` returns `BackendUnsupported` in Phase 2. (Obscura can render PDFs; the default is a Phase 2 scoping decision. Phase 4 revisits.)
6. ✅ **Complete (Week 2).** At least four ATS adapters fetch and normalize real job listings.
7. ✅ **Complete (Week 2).** HTML extraction handles JSON-LD and CSS-selector-based career pages.
8. ✅ **Complete (Week 3).** JS-required pages are routed to Tier 2, not surfaced as errors.
9. ✅ **Complete (Week 3).** Tier routing follows `render_mode` with unconditional escalation.
10. ✅ **Complete (Week 3).** `fetch_tier` is recorded per listing; `last_verified_at` is updated on success; `last_failure_cause` is recorded on failure.
11. ✅ **Complete (Phase 1 + Week 3).** Duplicate listings are deduplicated via `job_descriptions::upsert`.
12.  ✅ **Complete.** Portals **service layer** supports listing with health
   summaries, per-portal fetch with post-fetch refresh, batch fetch with
   per-portal results, and suggested actions mapped from every `FailureCause`.
   Verified by headless tests. (The GPUI view is Phase 5.)
13. **Week 4.** Manual job entry **service layer** supports URL fetch and paste fallback, and deduplicates repeated pastes and URL-fetched duplicates against prior portal fetches.
14. **Week 4.** A fallback build using `rust-headless-chrome` or `chromiumoxide` passes the same scraping tests, proving the trait abstraction and sidecar-model portability.
15. **Week 4.** CI is green on all platforms, including the new `portals-service` job.
16. **Week 4.** Verification pass complete. Each exit criterion has been checked against the code, with a note on how it is verified and whether the verification actually exercises the property.
17. **Week 4.** A retrospective memo records the service/view split decision, whether the fallback was needed, and what the verification pass found.

---

## What Phase 2 Is Not

Phase 2 acquires job postings and provides the service layer that a Portals view will call. It does not evaluate them, score them, generate cover letters, or produce PDFs. Specifically:

- There is no AI evaluation, no `rig` integration, no match scoring.
- There is no cover letter generation.
- There is no PDF rendering. `print_to_pdf` returns `BackendUnsupported` for the Obscura backend; PDF rendering is Phase 4.
- There is no prompt seeding or management.
- There is no Obscura embedded-crate integration. The sidecar model is confirmed; the embedded model is a deferred future enhancement.
- **There is no GPUI view for the Portals or Pipeline screens.** Phase 2 ships the service layer; Phase 5 builds the view on top. The "View Layer (Deferred)" section is the contract between them.

The browser backend being required does not change the phase boundary. It only changes the acquisition pipeline's internal dependency graph.

---

The single most important thing to take from this plan: **the Week 1 spike is complete and the pipeline works end-to-end.** The Obscura sidecar drives cleanly over CDP, returns post-JS DOM, and — unexpectedly — produces valid PDFs. That last finding changes Phase 4's scope but not Phase 2's.

The second most important thing: **Week 4 delivers a service layer, not a view.** Every deliverable is a pure function or a headless orchestration that a test can exercise without GPUI. When Phase 5 builds the view, it will have a stable base and a contract to implement against. Building the view now would mean paying for the work twice and verifying none of it.