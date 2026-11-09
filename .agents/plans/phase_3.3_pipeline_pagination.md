# Phase 3.3: Pipeline Pagination — Detailed Execution Plan

**Parent plan:** Draft Execution Plan: Career-Ops Rust Desktop Application (v0.45), §12
**Status:** In progress — WS1–WS3 complete; WS4 not started
**Last updated:** 2026-11-08
**Duration:** 2–3 days
**Depends on:** Phase 2.9 (filters, search, preferences, queue), Phase 3.2 (`Triaged` filter; triage and description batches iterate the loaded rows)
**Blocks:** nothing. It is a UI/read-path change; Phase 4 (rendering) does not depend on it.

---

## Change Log

| Version | Change |
| :--- | :--- |
| v0.1 | Initial plan. Lifts the 500-row Pipeline load window, paginates the rendered list in the view model, keeps every existing filter/search/preference behaviour by loading all rows, and moves the page position into `PipelineViewModel`. One index-only migration. No new dependency, no AI work. |
| v0.2 | **Decision note added.** Database-level pagination and SQL-level filtering were evaluated. Option A (in-memory pagination) is chosen now; Option C (SQL pagination with precomputed preference flags) is recorded as the future scale path; Option B (naive SQL `LIKE` predicates) is rejected because it silently changes what `Preferred` means. The decision, the trigger, and Option C's work are in the new section “Database-level pagination (decision note)”. |
| v0.3 | **WS1 implemented.** `job_descriptions::list_all` returns every row newest-first with `id DESC` as the tie-break; `list_recent` keeps its limit and takes the same order. Migration **0009** adds `ix_job_descriptions_fetched_at (fetched_at DESC, id DESC)`. `start_pipeline_load` uses `list_all`, `PIPELINE_LIMIT` is removed, and the live assertion becomes `rows.len() == stored.len()`. The load logs `pipeline load` with `jobs`, `bytes` (the sum of `raw_text` lengths, O(1) per row), and `elapsed_ms` — the Q-53 measurement. **Deviations:** the log records bytes rather than chars, because counting every character would scan the payload and distort the duration it measures; `crates/app/docs/patterns.md` is updated now rather than in WS4 because WS1 removes the constant it referenced. Three tests in `crates/core/tests/pipeline_load.rs`: more than 500 rows, equal-timestamp ordering, and the 0009 upgrade with the index present. WS2 (view-model pagination) is next. |
| v0.4 | **WS2 implemented.** `PipelineViewModel` gains `page`, `page_size` (`DEFAULT_PAGE_SIZE = 100`, `PAGE_SIZES = [50, 100, 250]`) and the pure page math: `filtered_len`, `page_count`, `effective_page`, `page_bounds`, `page_rows`, `set_filter`, `set_search`, `set_page`, `set_page_size`, and `clamp_page`. Filters and search reset to page 1; a page-size change resets; a reload keeps the page and clamps. `set_preferences`, `set_queue`, and `set_triage` also clamp, because each can shrink the filtered set (an exclusion, a dismissal, a skip). `PipelineAction::{SetPage, SetPageSize}` and their app handlers exist; the screen resets the list scroll on a page change and still renders the full list until WS3. Ten unit tests cover the boundaries, the reset table, the clamp cases, and pagination composing with a filter. **Deviation:** the app also calls `clamp_page` after a delete and after a status change (`Mark not a match`, evaluation), not only after `replace_jobs`, because those actions can remove a row from a status filter. WS3 (the footer controls) is next. |
| v0.5 | **WS3 implemented.** The list renders `page_rows()` and the empty states key on `filtered_len()`; a bottom `egui::Panel` (egui 0.37's `TopBottomPanel`) holds First / Previous / `start–end of total` / `Page N of M` / Next / Last and the 50/100/250 page-size chips. The bar is shown for a single page (the range and size are still useful) and hidden when the filtered set is empty. Page changes reset the list scroll; Back to list keeps the page. **Deviations:** the footer uses `Panel::bottom` rather than the deprecated `TopBottomPanel`, and a page-size chip emits only when it changes the size. Seven `egui_kittest` tests cover the range and page slice, a single page, both ends of the controls, Next/Last actions, the page-size chip, the empty state, and the detail round trip; one app test seeds 250 jobs and proves the full load, 100/50-row pages, page 3, the 50-row size, and the filter/search reset. WS4 (manual §6.14, docs, retrospective) is next. |

---

## Purpose

The Pipeline loads the 500 most recently fetched jobs:

```rust
// crates/app/src/app.rs
const PIPELINE_LIMIT: i64 = 500;
...
let jobs = job_repo::list_recent(&pool, PIPELINE_LIMIT).await?;
```

Everything below that line then assumes the window is the whole world:

- The header's **N jobs** is the loaded page, not the vault.
- The filter chips, the search, the preference badges, **Triage (N)**, and **Fetch descriptions (N)** only see the newest 500 rows. A `new` posting older than that is invisible to the agent features that are supposed to sort it.
- A user who scrapes several boards for a few months cannot reach an older posting at all. There is no "load more", no page control, and no scroll that reveals it.

The cap existed for one reason: the list renders every loaded row in a plain `ScrollArea`, so 500 was the point where per-frame cost became noticeable. That is a **rendering** problem, and pagination solves it directly: render one page, not the whole set. The load itself never needed a cap.

This phase:

1. **Loads every job row** (no `LIMIT`), so filters, search, preferences, triage, and the description batch operate on the complete vault.
2. **Paginates the rendered list** with explicit page controls and a page-size selector.
3. Keeps the detail view, the row actions, and the text/preference semantics exactly as they are.

### Evidence

- `PIPELINE_LIMIT = 500` and `job_repo::list_recent` (`ORDER BY fetched_at DESC LIMIT ?`) in `app.rs` / `repo/job_descriptions.rs`.
- `rows_from_jobs` carries `raw_text` and `plain_text` per row; `JobRow::preference_facts` matches keywords and locations against `plain_text`. A summary-only query would silently lose every description-based **Preferred** badge, so this phase keeps the full rows.
- `PipelineViewModel` already owns `filter` and `search`; pagination belongs beside them.
- `triage_candidates()` and `start_descriptions_batch()` iterate `self.pipeline.rows`; lifting the load cap makes both cover the whole vault (their per-click caps of 50 and 20 stay).
- `crates/app/docs/patterns.md` ("List pattern") says: raise the 500 bound only together with a virtualization swap. Pagination is that swap by another means — the rendered row count is bounded by the page size, so `TableBuilder` is not required for the goal.

---

## Scope

| In scope | Out of scope |
| :--- | :--- |
| Load every job row (no limit), deterministic order | Server-side filtering/search (Q-54) |
| Page state and page math in `PipelineViewModel` | A summary-only row that drops `raw_text` (part of Option C) |
| **First / Previous / Next / Last**, range, page count | Virtualized `TableBuilder` rendering |
| Page-size selector: 50 / 100 / 250 (default 100) | Infinite scroll / "load more" |
| Reset rules for filter, search, reload, page size | Persisting the page size across restarts |
| Index migration 0009, load timing log | Any change to the detail pane or the queue |
| Manual script, patterns doc, tests | Any AI/LLM behaviour |

---

## Design decisions

### 1. Paginate the render, not the query

The load returns **all** rows. Pagination is applied in memory, after the filter and the search, in `PipelineViewModel`.

Why not `LIMIT/OFFSET` in SQL:

- **The filters are view-model logic.** `Preferred` and `Excluded` are computed from `Preferences` and, for keywords and locations, from the posting's plain text. `Not applied` / `Applied` / `Rejected` / `Not a match` come from a second map (`applications`), and the search is a case-insensitive substring over title and company. Re-implementing that set in SQL would duplicate matching semantics between Rust and SQLite, which is exactly the drift the phase aims to avoid.
- **A page of a filtered set must be a page of the whole filtered set.** Filtering only the loaded page would silently change what `Preferred`, `Triaged`, and the search mean. That is a worse regression than the 500 cap.
- **The expensive part is rendering, not reading.** One query of a few thousand rows is fast on local SQLite; 500 `ScrollArea` rows per frame are not. Bounding the rendered set fixes the actual cost.

The database-level alternative was evaluated separately and is recorded as an explicit decision (see “Database-level pagination (decision note)” below): Option A is chosen now, and Option C is the successor if the vault outgrows it.

The memory trade-off is real and stated: every row keeps `raw_text` and `plain_text`. WS1 measures a synthetic large vault and logs `jobs`, `bytes`, and `elapsed_ms` on every load. The trigger for Option C (Q-53) is either **more than 10,000 rows** or **more than 1.5 s** to load; until then, this is the smallest change that preserves every existing behaviour.

### 2. Deterministic order

`ORDER BY fetched_at DESC` alone leaves ties (a fetch inserts many rows in the same second) in an arbitrary order, so two loads can page differently. The query becomes:

```sql
SELECT * FROM job_descriptions
ORDER BY fetched_at DESC, id DESC
```

`id DESC` is the tie-breaker; ids are monotonic, so equal timestamps have a stable order. `fetched_at` is updated on a re-fetch, which is the intended "move it to the top" behaviour and is unchanged.

### 3. Page state lives in `PipelineViewModel`

```rust
pub struct PipelineViewModel {
    pub rows: Vec<JobRow>,
    pub filter: JobFilter,
    pub search: String,
    pub page: usize,        // 0-based
    pub page_size: usize,   // default 100
    pub preferences: Preferences,
}
```

`filter` and `search` already live here; the page follows the same rule. The screen receives the view model immutably and emits actions, so page changes are `PipelineAction::SetPage(usize)` and `PipelineAction::SetPageSize(usize)`; the app calls the view-model setters. The math stays pure and unit-testable:

| Method | Meaning |
| :--- | :--- |
| `filtered_len()` | Rows after the filter and search. |
| `page_count()` | `max(1, ceil(filtered_len / page_size))`. |
| `effective_page()` | `page.min(page_count() - 1)`, the page actually rendered. |
| `page_bounds()` | 1-based inclusive `(start, end)`, or `(0, 0)` when empty. |
| `page_rows()` | The `effective_page` slice of the filtered iterator. |
| `set_filter` / `set_search` | Set and reset `page` to 0. |
| `set_page` | Clamp to the last page. |
| `set_page_size` | Set and reset `page` to 0. |
| `clamp_page` | Called after a reload or a row change that shrinks the set. |

### 4. Reset rules

| Event | Page effect |
| :--- | :--- |
| **SetFilter** | Page 1. A different question starts at the top. |
| **SetSearch** (including Clear) | Page 1. |
| **Reload list** | Page 1. This is the explicit "show me the current list" action. |
| Background reload (evaluation finished, description fetched, job deleted) | Keep the page and clamp; the user's place is not a side effect of a row update. |
| Page-size change | Page 1. Keeps the intent simple and the range legible. |
| **Back to list** from a detail | Keep the page and the list scroll offset. |
| A row action hides the last row on the last page | Clamp to the new last page (the footer updates; the list is not blank). |

The screen computes with `effective_page()` even if the stored page is stale, so a clamp is a display correction, not a data mutation.

### 5. The page is a list, not a scroll

Page controls sit in a footer row under the list:

```
First  Previous   [ 1–100 of 1,184 ]  Page 1 of 12  Next  Last     Rows per page: 50 [100] 250
```

- **First / Previous** are disabled on page 1; **Next / Last** on the last page.
- The range is `start–end of filtered total`; the page count follows.
- The page-size chips emit `SetPageSize`.
- The footer is hidden when the filtered set is empty; the existing `EmptyState` explains why.
- The header keeps its current wording: **N jobs** with no search, **X of Y jobs match** with one. The footer's range is the authoritative "where am I".
- Changing the page resets the list's scroll offset to the top (the existing `list_scroll` field); within one page the offset is preserved.

### 6. The detail pane is untouched

Rows still carry `raw_text` and `plain_text`, so the Description tab, the Raw toggle, the thin-row **Fetch description and evaluate** decision, the preference badges, and `set_description` after a refresh all behave exactly as they do today. This is the main reason to load full rows rather than summaries.

### 7. One index-only migration (0009)

```sql
CREATE INDEX ix_job_descriptions_fetched_at
    ON job_descriptions(fetched_at DESC, id DESC);
```

The order is now a two-column sort; the index makes the scan ordered instead of a sort on every load. No table change, no backfill, no data loss.

### 8. The 500 bound is removed, not raised

`PIPELINE_LIMIT` disappears. `list_recent(limit)` stays for tests and any future paged caller, but the app uses a new `list_all(pool)`. The live tests that assert `rows.len() == stored.len().min(PIPELINE_LIMIT)` become `== stored.len()`.

### 9. Side effect: the batches see the whole vault

`triage_candidates()` and the `DetailsNeeded` batch iterate `self.pipeline.rows`, so after this phase they cover rows older than the newest 500. The per-click bounds (50 triage, 20 descriptions) are unchanged; the **Triage (N)** count may now be very large, which is honest. Q-50 (triage over the loaded page) closes as part of this phase.

---

## Database-level pagination (decision note)

**Decision (2026-11-08): implement Option A now. Option C is the recorded successor if the vault outgrows it. Option B is rejected.**

### The options

| Option | What it is | Effort | Verdict |
| :--- | :--- | :--- | :--- |
| **A** | Load every full row; filter/search in memory; paginate the render | 2–3 days | **Chosen for this phase** |
| **B** | `LIMIT/OFFSET` plus dynamic SQL `WHERE` with `LIKE`/`lower` over `raw_text` | 6–9 days | **Rejected** |
| **C** | SQL pagination with precomputed `preferred`/`excluded` flags (and optionally lowercased search columns), so every predicate is exact and indexable | 9–13 days, 1–2 migrations | **Recorded as the future scale path** |

### Why A now

- The 500 cap is a **rendering** cost. A removes it with no semantic change: every filter, search, preference badge, and the detail pane behave exactly as they do today.
- The page controls, page-size selector, reset rules, and `PipelineViewModel` page math are **identical in A and C**. Choosing A now does not throw away the screen work; C replaces the data source later.
- B is the worst of the three: it looks like the scalable answer, but matching `Preferred` against `raw_text` changes the meaning of the filter (markup and attribute matches; locations in HTML that plain text strips) and SQLite's ASCII-only `lower()` diverges from Rust's Unicode `to_lowercase()`. It still needs a migration to become exact.
- Expected growth is new postings, not history: a board re-fetch dedups, and a few thousand rows is the realistic 6–12 month shape. A is cheap enough to be replaced later.

### Why B is rejected

`Preferences::matches` checks preferred companies, then keywords against the title **and** the description, then locations against the location **and** the description. The description used today is `text::plain_text(raw_text)`. Naive SQL would:

- match class names, URLs, or attributes in the HTML that the plain view strips;
- miss a location that only exists in the text without also matching markup;
- compare case-sensitively for non-ASCII text, because SQLite's `lower()` is ASCII-only;
- still need a migration to fix either problem.

It also needs a `COUNT(*)` with the same `%LIKE%` predicates, which cannot use a B-tree index and scans on every page.

### What Option C would add

1. **Schema + backfill (1–2 migrations):** `preferred INTEGER` / `excluded INTEGER` on `job_descriptions` (and optionally `title_lower` / `company_lower`), plus indexes for the filter columns.
2. **Flag maintenance:** compute the flags with the existing Rust matcher on insert/update (`upsert`, `set_raw_text`, manual add) and on **preferences save** (an O(n) async pass, with the UI showing its progress). Flags make the "re-badge without a reload" test become "recompute, then reload".
3. **Query layer:** `PipelineQuery { filter, search, page, page_size, snapshot }` and `pipeline_page(pool, query) -> (Vec<JobRow>, total)`. The same predicate feeds the count. A snapshot boundary (`fetched_at`/`id`) keeps offset pages stable while a fetch runs.
4. **Side queries:** triage candidates (50 new, non-excluded), the description batch (20 `details_needed`), the header summary (`preferred of shown`, distinct excluded companies).
5. **Async search in the app:** debounce (200–300 ms), loading/error states, cancellation, and stale-request tagging — the same single-flight discipline Prompts, Pipeline detail, and Usage already needed.
6. **Tests:** SQL-vs-Rust equivalence (markup in descriptions, accented text, duplicate `applications` rows), page boundaries, snapshot stability, and stale-search races.

### What stays the same

The screen's contract and the page state do not change: `SetPage` / `SetPageSize`, First/Previous/Next/Last, the range, and the page-size selector are reused. C moves the filtering and the count into the query and turns filter/search handlers into small async loads; the screen keeps reading a page and a total.

### The trigger

WS1 logs `jobs`, `bytes`, and `elapsed_ms` on every load. Move to Option C when **10,000 rows** or **1.5 s** is reached (Q-53), or earlier if the vault is expected to exceed ~20,000 rows in the next year. Until then, A is the deliberate choice, not a stopgap.

---

## Workstream 1 — Core load path

**Estimated duration:** 0.5 day.

### Deliverables

1. `repo::job_descriptions::list_all(pool) -> Result<Vec<JobDescription>>` with `ORDER BY fetched_at DESC, id DESC` and no `LIMIT`.
2. `list_recent` keeps its signature but uses the same tie-broken order, so tests and any future page agree.
3. Migration `0009_job_fetched_index.sql` with the index above.
4. `start_pipeline_load` calls `list_all`; `PIPELINE_LIMIT` is removed and the stale comment is updated to point at this plan.
5. A `tracing::info!` line after the load: `pipeline load` with `jobs`, `bytes` (the sum of `raw_text` lengths, O(1) per row), and `elapsed_ms`. This is the measurement that decides Q-53.
6. Tests:
   - `list_all` returns every row (seed > 500 in a repository test).
   - Equal `fetched_at` values come back `id DESC`.
   - Migration 0009 applies to a 0008 database and preserves rows; the index is present.
   - The live tests' `rows.len()` assertions drop the `min(PIPELINE_LIMIT)`.

### Acceptance criteria

| Criterion | Verification |
| :--- | :--- |
| The app loads every job row | Repository test with 501 rows + app load of a 250-row vault |
| The order is deterministic under timestamp ties | Repository test with equal `fetched_at` |
| Migration 0009 upgrades a 0008 database | Migration test |
| A load logs its size and duration | Code review + Logs line |

---

## Workstream 2 — View-model pagination

**Estimated duration:** 0.5 day.

### Deliverables

1. `PipelineViewModel` fields `page` and `page_size` with `DEFAULT_PAGE_SIZE = 100` and `PAGE_SIZES = [50, 100, 250]`.
2. The methods in design decision 3.
3. `replace_jobs` calls `clamp_page`; `set_filter`, `set_search`, `set_page`, and `set_page_size` follow the reset table.
4. `PipelineAction::{SetPage, SetPageSize}` and their app handlers.
5. Unit tests:
   - `page_count` for 0, 1, exactly one page, and a remainder.
   - `page_bounds` for the first, middle, and last pages, and `(0, 0)` when empty.
   - `page_rows` returns the right ids and no more than `page_size`.
   - `set_page` clamps above the last page; `set_page_size` resets to page 0.
   - `set_filter` / `set_search` reset to page 0.
   - A reload that shrinks the filtered set clamps the page.

### Acceptance criteria

| Criterion | Verification |
| :--- | :--- |
| Page math is exact at every boundary | Unit tests |
| Filters and search reset the page | Unit tests |
| A shrinking set never leaves the page out of range | Unit test + `clamp_page` in `replace_jobs` |

---

## Workstream 3 — Screen

**Estimated duration:** 1 day.

### Deliverables

1. `list()` renders `view_model.page_rows()` and the existing empty states keyed on `filtered_len()`.
2. A `pagination_bar` under the list with First / Previous / range / page count / Next / Last and the page-size chips.
3. `show()` treats `SetPage` and `SetPageSize` like `SetFilter`: reset `list_scroll` to 0.
4. The header's counts use `filtered_len()`; no copy change beyond what already exists.
5. `egui_kittest`:
   - With more than one page of rows, the footer shows the range and page count.
   - Page 1 disables First/Previous; the last page disables Next/Last.
   - **Next** emits `SetPage(1)`; **Last** emits the final index.
   - A page-size chip emits `SetPageSize(50)`.
   - Changing a filter or the search resets the page (assert the action and the rendered page).
   - A selected row on page 2 returns to page 2 via **Back to list**.
   - An empty filtered set hides the footer and keeps the right `EmptyState`.
6. An app test: seed 250 jobs, load, assert the first page holds 100, page 3 holds 50, and `SetPageSize(50)` yields 5 pages; assert `pipeline.rows.len() == 250` (proving no 500 cap and no truncation).

### Acceptance criteria

| Criterion | Verification |
| :--- | :--- |
| Every job is reachable through the pages | App test |
| The controls are correct at both ends | `egui_kittest` |
| Filters/search restart at page 1 | `egui_kittest` + app test |
| A round trip to a detail keeps the page | `egui_kittest` |
| An empty result hides the bar | `egui_kittest` |

---

## Workstream 4 — Docs, verification, retrospective

**Estimated duration:** 0.5 day.

1. Manual script: add **§6.14** (with a page-size selector and at least two pages of jobs): the range and page count, Next/Last, filter resets to page 1, a page-size change, and a detail round trip returning to the same page. Update §6.1 if its "N jobs" wording changes.
2. `crates/app/docs/patterns.md`: replace the "raise the bound only with `TableBuilder`" paragraph with the decision this phase makes — the Pipeline loads all rows and bounds the rendered set by the page; `TableBuilder` remains the alternative for a single long list.
3. Full validation; parent plan v0.42 changelog and the Phase 3.3 row.
4. `docs/LIVE_TESTS.md` touch-up only if a live assertion changed.
5. Short retrospective in `.agents/plans/phase_3.3_retrospective.md`, including the measured load size/time.

---

## Test Strategy

| Layer | Covers |
| :--- | :--- |
| Repository | `list_all` with > 500 rows; the tie-broken order; migration 0009 |
| View model | page count/bounds/rows at every boundary; reset and clamp rules |
| `egui_kittest` | the bar, the four controls, the page-size chips, filter reset, detail round trip, empty state |
| App | a 250-row vault through the real load and page transitions |
| Manual | §6.14 on a vault with more than one page |

No network. No new dependency. One index-only migration.

---

## Risks

| ID | Risk | Impact | Mitigation |
| :--- | :--- | :--- | :--- |
| P3.3-R1 | **Loading every full row is heavy on a large vault.** | Medium | WS1 logs `jobs`, `bytes`, and `elapsed_ms`. Q-53 defines the trigger for Option C (10k rows or 1.5 s). Pagination bounds rendering, which was the original cap's reason. |
| P3.3-R2 | **Option C must not lose the description-based preference match.** Keywords and locations match the description today. | High if built naively | The decision note rejects naive SQL `LIKE` (Option B) and specifies precomputed flags (Option C) computed by the existing Rust matcher. |
| P3.3-R3 | **A background reload shifts page contents.** | Low | Keep the page and clamp; only explicit Reload/filter/search/page-size reset to 1. |
| P3.3-R4 | **A row action empties the last page.** | Low | `effective_page` clamps on render and `replace_jobs` clamps on reload. |
| P3.3-R5 | **Timestamp ties page differently between loads.** | Low | `id DESC` tie-breaker + the index. |
| P3.3-R6 | **A 250-row page is slower than expected.** | Low | Measure in WS3; if it is, 100 stays the default and `TableBuilder` is the next step (Q-55). |
| P3.3-R7 | **The header count and the footer range disagree.** | Low | The header says the total/match wording; the footer is the only place that states the window. |

---

## Exit Criteria

1. The Pipeline loads every job row; no 500-row window remains.
2. The list renders exactly one page; the footer shows the range, page count, and page-size selector.
3. First/Previous/Next/Last are correct at both ends.
4. Filter, search, explicit Reload, and page-size changes reset to page 1; background reloads clamp and keep the page.
5. A detail round trip returns to the page the row was on.
6. The detail pane, preferences, queue actions, triage, and the description batch behave exactly as before, over the complete set.
7. The load logs its size and duration; the measured numbers are in the retrospective.
8. Migration 0009 applies to a 0008 database and preserves rows.
9. Offline suite, fmt, and clippy are clean.
10. Manual §6.14, `patterns.md`, the parent plan, and the retrospective are updated.

---

## Open Questions

| # | Question | Position |
| :--- | :--- | :--- |
| Q-53 | **What triggers the move to Option C (precomputed flags + SQL pagination)?** | 10,000 jobs or a 1.5 s load, whichever comes first, measured from the new log line. Option C is the recorded successor, not a separate summary-only refactor; it must preserve the description-based preference match by precomputing the flags. |
| Q-54 | **Should filters and search move into SQL?** | Decided: not now (Option A). Option C is the successor when Q-53 fires; Option B is rejected. See “Database-level pagination (decision note)”. |
| Q-55 | **Should the list virtualize instead of paginate?** | Not now. Pagination bounds the rendered set without `egui_extras`. If a 250-row page is slow, `ScrollArea::show_rows` or `TableBuilder` is the next step, and the two can coexist (a page that virtualizes). |
| Q-56 | **Should the page size persist across restarts?** | No. It is view state; a settings row would be a migration for a preference the user can set in one click. |
| Q-57 | **Should the Triage count cap its display once the whole vault is loaded?** | Not now. An exact count is honest; the confirmation states the batch size, and one click covers 50. If a 5,000-row backlog makes the button read badly, `Triage (50+)` is a one-line change. |
| Q-58 | **Should pagination apply to the Portals list too?** | No. A user has tens of portals, not thousands. |

---

## What Phase 3.3 Is Not

- Not an AI phase. No prompt, model, or ledger change.
- Not database-level pagination or SQL-level filtering. That is Option C in the decision note, triggered by Q-53; Option B is rejected.
- Not a database rewrite: no new columns, no summary row, no backfill in this phase.
- Not a filter/search redesign: the same matcher runs over more rows.
- Not a detail-pane change: rows still carry their text.
- Not a virtualization phase: one page of at most 250 rows stays in a plain `ScrollArea`.
- Not a change to triage/description caps or to the per-click semantics of any batch.
