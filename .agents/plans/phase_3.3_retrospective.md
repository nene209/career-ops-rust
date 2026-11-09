# Phase 3.3 — Pipeline Pagination: Retrospective

**Date:** 2026-11-09
**Plan:** `.agents/plans/phase_3.3_pipeline_pagination.md` (v0.6)
**Outcome:** all four workstreams implemented; the offline gates are green and the
manual §6.14 pass is the owner's.

---

## What shipped

- **Core load path (WS1).** `job_descriptions::list_all` returns every row
  newest-first with an `id DESC` tie-break. `list_recent` keeps its limit and
  takes the same order. Migration **0009** adds
  `ix_job_descriptions_fetched_at (fetched_at DESC, id DESC)`. The 500-row
  `PIPELINE_LIMIT` is gone, and the live assertion now requires every stored job
  in the Pipeline. Each load logs `pipeline load` with `jobs`, `bytes`, and
  `elapsed_ms`.
- **View-model pagination (WS2).** `PipelineViewModel` owns `page` and
  `page_size` (`DEFAULT_PAGE_SIZE = 100`, `PAGE_SIZES = [50, 100, 250]`) and the
  pure page math: `filtered_len`, `page_count`, `effective_page`, `page_bounds`,
  `page_rows`, `set_filter`, `set_search`, `set_page`, `set_page_size`, and
  `clamp_page`. Filters, search, and page-size changes reset to page 1; reloads
  keep the page and clamp. `PipelineAction::{SetPage, SetPageSize}` exist.
- **Screen (WS3).** The list renders only `page_rows()`; empty states key on
  `filtered_len()`. A bottom panel holds **First / Previous**,
  `start–end of total`, **Page N of M**, **Next / Last**, and the page-size
  chips. The bar is shown for a single page and hidden when the filtered set is
  empty. Page changes reset the list scroll; **Back to list** keeps the page.
- **Docs and verification (WS4).** Manual script **§6.14a–d**, the
  `docs/LIVE_TESTS.md` note about the stronger live assertion, the parent plan's
  Phase 3.3 row and changelog, and this retrospective.

## What held up

- **Paginate the render, not the query.** Every existing behaviour — the
  description-based `Preferred` match, `Search` over `plain_text`, the queue
  badges, the whole-vault triage and description batches — kept working because
  the rows are still all loaded. Nothing in the filter or triage code changed.
- **Page math in the view model.** `effective_page` clamping means the screen
  never has to reason about a stale page after a delete, a status change, or a
  reload. The screen stayed a renderer; the tests could cover the boundaries
  without a window.
- **The footer owns the window, the header owns the total.** Keeping
  `"{total} jobs"` in the header and `"{start}–{end} of {total}"` in the footer
  avoided the two counts drifting (P3.3-R7).
- **The measure-first migration.** `list_all` with the 0009 index is fast enough
  that Option C is clearly premature (see the measurement below), so the phase
  did not pay for a schema rewrite it might not need.
- **One task, unchanged.** The Phase 2.9 `AsyncTask<PipelineLoad>` still reads
  rows and applications and joins them; only the query helper changed. The
  single-flight tagging and cancellation behaved exactly as before.

## Measurement (Q-53 baseline)

The Q-53 instrument is the `pipeline load` log line, not a test. Two data
points were captured for this retrospective:

| Rows | Bytes | `list_all` | Build | Where |
| ---: | ---: | ---: | :--- | :--- |
| 250 | ~1 MB | part of the app test | debug | `the_pipeline_paginates_a_large_vault` |
| 10,000 | 40,000,000 | **253 ms** | debug | a temporary probe against a migrated temp vault (not committed) |

The 10k-row probe seeds 4 KB of `raw_text` per row, so 40 MB is a deliberately
heavy description vault. The repository read alone is far below the 1.5 s
trigger; the app's join over applications is the only extra work. **Q-53 does
not fire at 10k rows**, so Option A remains the right choice. A real vault with
much larger descriptions and a slow disk is the case the log line will catch.

## What was harder, and the deviations

- **The log records bytes, not chars.** Counting characters would scan every
  payload and distort the duration it measures. `raw_text.len()` is O(1) and is
  the number that matters for memory pressure.
- **`Panel::bottom`, not `TopBottomPanel`.** egui 0.37 removed the older
  panels; the footer uses `egui::Panel::bottom("pipeline_pagination")`.
- **A page-size chip emits only when it changes the size.** Clicking the active
  size is a no-op rather than a redundant reset to page 1.
- **`clamp_page` has more call sites than the plan named.** The app clamps after
  `replace_jobs` (the plan's case) and also after a delete and after a status
  change (`Mark not a match`, evaluation), because those actions can remove the
  only row on the last page.
- **The item count in the header is unsearched total, not filtered.** With a
  search active the header reads `"{shown} of {total} jobs match"` (the Phase
  2.9 wording); the footer range is always relative to the filtered set. Both
  are unchanged from 2.9, and the manual step pins the distinction.
- **No virtualization.** A page is at most 250 rows in a plain `ScrollArea`; Q-55
  records `ScrollArea::show_rows` / `TableBuilder` as the next step if a page
  ever feels slow.
- **The page size is not persisted.** It is view state; Q-56 declined a settings
  row and a migration for a one-click preference.

## Test strategy as built

| Layer | What it pins |
| :--- | :--- |
| Repository | `list_all` returns > 500 rows; equal timestamps order by `id DESC`; migration 0009 preserves rows and creates the index |
| View model | page count/bounds/rows at every boundary; the reset table; clamp cases; pagination composing with a filter; ten unit tests |
| `egui_kittest` | the bar, the range and page slice, a single page, both ends of the controls, Next/Last, the page-size chip, the empty state, the detail round trip; seven tests |
| App | a 250-row vault through the real load: no cap, a 100-row first page, page 3, the 50-row size, and the filter/search reset |
| Manual | §6.14a–d on a vault with more than one page |

No network. No new dependency. One index-only migration.

## Follow-ups

1. **Manual §6.14** is the owner's pass, on a vault with more than one page.
2. **Q-53:** move to Option C only when the log shows 10,000 rows or a 1.5 s
   load. The probe says 10k rows alone is not enough; the trigger is the
   combination of a large vault and slow storage.
3. **Q-55:** if a 250-row page is slow on a real machine, add
   `ScrollArea::show_rows` inside the page; pagination and virtualization
   compose.
4. **Q-57:** if a 5,000-row backlog makes **Triage (N)** read badly, `Triage
   (50+)` is a one-line change. The count is exact today.
5. **Portals is not paginated** (Q-58) and should not be: a user has tens of
   portals, not thousands.
6. **The scroll position is per-screen, not per-page.** Returning to a page
   starts at the top after a page change (the scroll is reset); the detail round
   trip keeps the position because the page did not change. If users want the
   scroll remembered per page, that is a small view-state addition.

## Exit criteria

All ten are met, pending the owner's manual pass:

1. The Pipeline loads every job row; no 500-row window remains. ✅
2. The list renders exactly one page; the footer shows the range, page count,
   and page-size selector. ✅
3. First/Previous/Next/Last are correct at both ends. ✅
4. Filter, search, explicit Reload, and page-size changes reset to page 1;
   background reloads clamp and keep the page. ✅
5. A detail round trip returns to the page the row was on. ✅
6. The detail pane, preferences, queue actions, triage, and the description
   batch behave exactly as before, over the complete set. ✅
7. The load logs its size and duration; the measured numbers are above. ✅
8. Migration 0009 applies to a 0008 database and preserves rows. ✅
9. Offline suite, fmt, and clippy are clean. ✅
10. Manual §6.14, `patterns.md`, the parent plan, and this retrospective are
    updated. ✅ (the manual run is the owner's)
