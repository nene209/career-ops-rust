# Phase 2.8: Portal Fetch Controls — Detailed Execution Plan

**Parent plan:** Draft Execution Plan: Career-Ops Rust Desktop Application (v0.12), §12
**Status:** Planned — not started
**Last updated:** 2026-11-06
**Duration:** 2–3 days
**Depends on:** Phase 2.5 (`fetch_batch_cancellable`, `refresh_job`, `fetch_batch_cancellable`), Phase 2.7 (none functionally, but the sequencing expects the CV editor to precede this phase)
**Blocks:** Phase 3 benefits but does not require it. Phase 3.1 depends on the per-portal detail toggle.

---

## Change Log

| Version | Change |
| :--- | :--- |
| v0.1 | Initial plan. Splits the current `Fetch` / `Fetch more` into three actions with explicit names, and makes detail enrichment a per-portal opt-in. The intent is to keep the app's fetch semantics legible as the triage agent (Phase 3.2) starts to reason about jobs the user has not looked at. No schema change. |

---

## Purpose

The current fetch UI has two buttons:

- **Fetch** — pages 1..=`pagination.max_pages`, resets to page 1 every time, honors `detail.max_jobs` enrichment.
- **Fetch more** — continues from `portals.last_page_fetched` through `max_pages`, honors `detail.max_jobs`.

Both work, but the labels are doing too much work. A returning user reading "Fetch" expects "get me new postings" (which is page 1), and a user reading "Fetch more" expects "get me more pages" (which is continuation). Once triage (Phase 3.2) starts depending on the pipeline containing fresh and complete postings, the ambiguity becomes a problem for the user rather than a small labeling quirk.

This phase:

1. **Renames the two actions** to their actual behavior and adds a third: **Refresh**, **Continue**, **Fetch all**.
2. **Makes detail enrichment per-portal opt-in.** `detail.max_jobs` continues to bound enrichment when it happens, but the fetch actions no longer request details automatically. The user chooses when, in Phase 3.1's queue and Phase 3's evaluation.
3. **Adds a bounded "Fetch all" loop**, answering Q-18 from Phase 2.5 and the D1 deferral in its retrospective.

The phase is deliberately small. It changes the app's fetch affordance and its enrichment default, but not the acquisition pipeline. Every change is a UI label, a service-level loop, or a config field read.

### Evidence

- `crates/app/src/screens/portals.rs` renders **Fetch** and **Fetch more**; the second is gated on `can_fetch_more`.
- `router::fetch_portal` returns `FetchOutcome { jobs_fetched, tier, first_page, next_page, end_of_list, details_attempted, details_enriched, pages_fetched, stop_reason }` — the fields "Fetch all" needs are already present.
- `core::acquisition::router` enriches details whenever `scraper_config.detail` is present; there is no per-portal toggle, only the JSON block.
- `portals::service::fetch_batch_cancellable` already demonstrates the shape a bounded loop should follow (per-portal progress, cancel between items, one entry per completion).

---

## Scope

| In scope | Out of scope |
| :--- | :--- |
| Rename **Fetch** → **Refresh** (from page 1) | Preference-aware filtering (Phase 3.1) |
| Rename **Fetch more** → **Continue** (from cursor) | Job queue states (Phase 3.1) |
| New **Fetch all** action with a bounded loop | LLM triage (Phase 3.2) |
| Per-portal **Fetch descriptions during fetch** toggle, default off | LLM-assisted portal suggestions (Phase 3.2) |
| Progress and cancel for **Fetch all** | Aggregator or web search (out of scope entirely) |
| Tests, docs, retrospective | A change to list-fetch semantics or pagination caps |

---

## Design decisions

### 1. Three actions, three clear meanings

| Button | What it does | Cursor effect |
| :--- | :--- | :--- |
| **Refresh** | Reads pages 1..=`pagination.max_pages`, updates existing rows, inserts new ones. | **None.** The cursor tracks the deepest page ever fetched, and Refresh does not reset it. A returning user gets new top-of-list postings without losing their crawl position. |
| **Continue** | Reads pages `cursor + 1`..=`cursor + pagination.max_pages`, advances the cursor. Disabled when `end_of_list`. | Advances to the highest page read. |
| **Fetch all** | Repeats Continue in-process until `end_of_list`, the user cancels, or the per-click budget is exhausted. | Cleared to 0 on `end_of_list`; unchanged on cancel or budget hit. |

Two consequences worth stating: Refresh is the "get me new jobs" action a returning user expects, and does not disturb a crawl in progress. Fetch all is the bounded walk Q-18 rejected as a default — here it is an explicit, cancellable click with a visible budget.

### 2. Fetch all has a per-click budget, not a total cap

The loop is bounded by a **per-click budget of 50 pages** (not per-portal `pagination.max_pages`, which stays as the depth of a normal click). Fifty pages on a 10-per-page board is 500 jobs; the actual NVIDIA board would be five clicks. This is deliberate:

- The user asked for a bounded action, not a crawl. Each click is a decision.
- The budget is visible in the button tooltip ("Fetch all (up to 50 pages)") and in the result message ("Fetched 500 jobs across 50 pages. There is more; Continue or Fetch all again.").
- A hard-coded ceiling means a board with 269 pages cannot be accidentally walked to completion in a single click by a user who did not read the tooltip.

The tooltip and the result message are the whole disclosure. No second confirmation dialog.

### 3. Detail enrichment is a per-portal toggle

`scraper_config.detail` continues to mean "how to extract a description"; a new `scraper_config.detail.enabled` (Boolean, default `false`) means "do it during fetch." The toggle appears in the portal form's advanced section, labeled **Fetch descriptions during fetch (slower, ~25 requests per page)**.

Backward compatibility: existing portals that already have a `detail` block (from Phase 2.2, 2.3, or the registry entries like Waymo and Ubisoft) behave as they do today because a missing `enabled` is treated as `true` for configs that already have `detail`. Only brand-new configs default to off. That is the least surprising behavior for existing users and the correct default for new ones.

**Consequence for the pipeline.** Most new postings arrive with only a card teaser. That is fine for Phase 3.2's triage (which wants titles and briefs anyway), but Phase 3's evaluation needs full text. The Phase 3 plan update at the end of the multi-document response handles this with a combined "fetch description then evaluate" action, so the user is not stuck with "no description" on a job they want to evaluate.

### 4. Cancellation and progress reuse the Phase 2.5 pattern

`fetch_all` is a service function in `portals::service` with the same shape as `fetch_batch_cancellable`:

- A shared `AtomicBool` cancel flag, checked between pages.
- A per-page progress callback (`FnMut(&PageProgress)`), so the UI can show "page 12 of 50, 240 jobs" rather than a spinner.
- One `FetchAllReport` at the end: `pages_fetched`, `jobs_stored`, `stopped_reason` (`EndOfList` | `Cancelled` | `BudgetExhausted` | `Error { .. }`).

The existing batch cancellation (`fetch_batch_cancellable`) is unchanged. `fetch_all` is a new per-portal operation, not a modification of the batch.

### 5. The button UI is a single row per portal

Three buttons is right at the boundary where a toolbar becomes clutter. They render as a compact row in each portal entry, with Fetch all slightly de-emphasized (outline style) to reflect that it is the more consequential action. The detail toggle is inside the advanced form, not on the row.

### 6. No migration

`portals.last_page_fetched` already exists. `scraper_config` is JSON. Nothing changes at the schema level.

---

## Workstream 1 — Backend

**Estimated duration:** 1 day.

### Deliverables

1. `portals::service::fetch_all(pool, portal_id, source, browser, cancel, progress) -> Result<FetchAllReport>`. Loops `router::fetch_portal_more` until `end_of_list`, the cancel flag is set, or 50 pages have been read. Each iteration reuses the existing cursor logic; `fetch_all` does not touch the cursor directly.
2. `DetailConfig` gains `enabled: bool`. `fetch_static_jobs`, `fetch_pages`, and `eightfold::fetch` honor it: `enabled == true` runs the existing enrichment pass; `enabled == false` (or `detail` absent, or `detail.enabled == false`) skips it entirely. Existing `detail` blocks without `enabled` continue to enrich (backward compatibility).
3. `fetch_portal` (Refresh) and `fetch_portal_more` (Continue) keep their current semantics. `fetch_all` is new.
4. Unit tests:
   - `fetch_all` stops at `end_of_list`.
   - `fetch_all` stops when the cancel flag is set.
   - `fetch_all` stops at the per-click budget with `BudgetExhausted`.
   - `fetch_all` reports `pages_fetched` and `jobs_stored` accurately.
   - Refresh does not change `portals.last_page_fetched`.
   - A `detail` block without `enabled` still enriches (regression test for existing configs).
   - A `detail` block with `enabled: false` skips enrichment.
5. An integration test: seed a portal and a `MockFetchSource` producing 5 pages, run Refresh then Continue then Fetch all, and assert the cursor, row count, and enrichment calls follow the table in design decision 1.

### Acceptance criteria

| Criterion | Verification |
| :--- | :--- |
| `fetch_all` reaches the end of the list and clears the cursor | Core test with a scripted source |
| Cancel stops between pages | Core test with a pre-set flag |
| Budget stops at 50 pages | Core test |
| Refresh leaves `last_page_fetched` unchanged | Integration test |
| `detail.enabled` gates enrichment without breaking old configs | Unit + integration tests |

---

## Workstream 2 — UI

**Estimated duration:** 1 day.

### Deliverables

1. Replace the two buttons with three: **Refresh**, **Continue** (disabled when `end_of_list`), **Fetch all**.
2. Per-portal progress on Fetch all: a one-line status under the row (`Page 12 of 50 · 240 jobs · Stop`) with a Stop button.
3. The **detail toggle** in the portal form, labeled as in design decision 3.
4. `PortalsAction` gains `FetchAll(id)`, `CancelFetchAll`. The existing `Fetch` becomes `Refresh` in the UI only (the `PortalsAction::Fetch` variant is unchanged; a comment records the label).
5. `egui_kittest`:
   - Three buttons render.
   - Continue is disabled for a portal that has reached the end of its list.
   - Stop cancels a running Fetch all.
   - The detail toggle is present and default off for a fresh portal.
   - The detail toggle is checked for an old portal whose config has a `detail` block (backward compatibility).

### Acceptance criteria

| Criterion | Verification |
| :--- | :--- |
| Three actions are present per portal with distinct labels | `egui_kittest` |
| Continue is disabled at end-of-list | `egui_kittest` |
| Stop cancels a running Fetch all | App test |
| The detail toggle defaults off for a new portal, stays on for an old config | `egui_kittest` |

---

## Workstream 3 — Docs, verification, retrospective

**Estimated duration:** 0.5 day.

1. Manual script §4.4: replace the Fetch/Fetch more rows with Refresh/Continue/Fetch all and a detail-toggle step.
2. `docs/known-sites.md`: note that Waymo and Ubisoft keep their existing enrichment because their configs already have a `detail` block; new entries default to off.
3. Full validation: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --offline -- -D warnings`, `cargo test --workspace --features career-ops-app/keychain --offline -- --skip obscura`.
4. Parent plan: v0.13 changelog and a Phase 2.8 row.
5. Short retrospective in `.agents/plans/phase_2.8_retrospective.md`.

---

## Test Strategy

| Layer | Covers |
| :--- | :--- |
| Core unit | `fetch_all` stop conditions; `DetailConfig.enabled` gating; Refresh cursor invariance |
| Core integration | Three actions in sequence against a scripted source |
| `egui_kittest` | Three buttons, Continue gating, Stop, detail toggle, backward-compatible detail default |
| App | Cancel and progress flow through `career_ops_app` |
| Manual | §4.4 on a real portal |

No network for any new test. No new dependency. No migration.

---

## Risks

| ID | Risk | Impact | Mitigation |
| :--- | :--- | :--- | :--- |
| P2.8-R1 | **Detail opt-in reduces evaluation quality.** The pipeline carries thin rows by default. | Medium | Phase 3's combined fetch-then-evaluate action (see Phase 3 update) and Phase 3.2's triage (which wants titles anyway). |
| P2.8-R2 | **Fetch all runs into a WAF.** A 50-page walk is many requests. | Medium | Bounded per click, cancellable, honest result message. The WAF already skips and reports; no new evasion. |
| P2.8-R3 | **Refresh unexpectedly skips new jobs.** Some boards reorder on each fetch. | Low | Refresh reads from page 1 every time; the new-postings case is exactly what it is for. |
| P2.8-R4 | **Old `detail` blocks silently start enriching.** | Low | Backward-compatible default is `enabled: true` when `detail` exists; a regression test pins it. |

---

## Exit Criteria

1. **Refresh**, **Continue**, and **Fetch all** are present, distinct, and behave per design decision 1.
2. **Fetch all** stops at end-of-list, cancel, or 50 pages, and reports which.
3. **Fetch all** is cancellable and shows per-page progress.
4. The per-portal detail toggle exists, defaults off for new configs, and is honored by all fetch paths.
5. Existing `detail` blocks (Waymo, Ubisoft) continue to enrich without changes.
6. No schema change and no new dependency.
7. Offline suite, fmt, and clippy are clean.
8. Manual script §4.4, `docs/known-sites.md`, parent plan, and a short retrospective are updated.

---

## Open Questions

| # | Question | Position |
| :--- | :--- | :--- |
| Q-33 | **Should the 50-page Fetch-all budget be user-configurable?** | Not in this phase. A visible constant is a policy the user can reason about; a per-portal setting is a knob the user would have to discover. Revisit if the fixed budget turns out to be wrong for a real board. |
| Q-34 | **Should Refresh also enrich details for new postings?** | No. Refresh is the "cheap check for new work" action. Enrichment is Phase 3.1's queue action or Phase 3's combined fetch-then-evaluate. |

---

## What Phase 2.8 Is Not

- Not a triage phase (Phase 3.2).
- Not a preference layer (Phase 3.1).
- Not a change to acquisition semantics: the router, the tiers, and the failure classification are untouched.
- Not a change to the list caps or the pipeline limit.
- Not a UI redesign of the Portals screen beyond the fetch row.