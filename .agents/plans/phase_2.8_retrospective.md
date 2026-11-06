# Phase 2.8 Retrospective and Verification Record

**Phase:** Portal Fetch Controls
**Parent plan:** Draft Execution Plan: Career-Ops Rust Desktop Application (v0.15), §12
**Phase plan:** `.agents/plans/phase_2.8_portal_fetch_controls.md` (v0.4)
**Period:** 2026-11-06
**Estimate:** 2–3 days
**Actual:** one session across three workstreams
**Status:** Complete against the automated criteria. Manual script §4.3e and §4.4a–c are written and await the owner's pass; §4.4 has not been run yet.

---

## Summary

The Portals screen had two row buttons and two header buttons, and three of the
four were misnamed in ways that only mattered once the app started fetching a
lot. **Fetch** meant "read page one", not "get new postings"; **Fetch more**
meant "continue from the cursor"; the header's **Refresh** reloaded the list and
its **Fetch all** fetched every portal. Phase 2.8 made the vocabulary match the
behavior and added the one action users actually asked for: a bounded walk to the
end of a board.

Three things changed, and only the first is cosmetic:

1. **Three per-portal actions.** Refresh (page one), Continue (from the cursor),
   Fetch all pages (repeat Continue). The header became **Reload list** and
   **Fetch all portals** so no two buttons share a label — the old "Fetch all"
   was already ambiguous at a glance and would have been a compile error at the
   `PortalsAction` level, since `FetchAll` is the cross-portal batch.
2. **Detail enrichment became opt-in.** `scraper_config.detail.enabled` decides
   whether a fetch spends ~25 requests per page on descriptions. It is off for
   new selector portals, on for existing `detail` blocks (Waymo, Ubisoft) and for
   Eightfold boards, whose list carries no text at all.
3. **The continuation cursor stopped going backwards.** A page-one fetch used to
   write its own depth back to `portals.last_page_fetched`: a portal crawled to
   page 40 dropped to 5, and the next Continue re-read pages 6–10. Refresh now
   raises the cursor to the deepest page it read and never lowers it, and only
   Continue clears it at the end of the list.

---

## Exit criteria status

| # | Criterion | Status | Evidence |
| :--- | :--- | :--- | :--- |
| 1 | Refresh, Continue, and Fetch all pages are present and distinct; the header reads Reload list / Fetch all portals | ✅ | `a_paginated_row_offers_three_distinct_actions`, `header_offers_add_and_fetch_all_when_portals_exist` |
| 2 | Fetch all pages stops at end-of-list, cancel, no progress, or 50 pages, and reports which | ✅ | `fetch_all_pages_stops_at_the_end_of_the_list`, `…_when_the_user_cancels`, `…_at_the_page_budget`, `…_when_the_provider_cannot_continue` |
| 3 | Fetch all pages is cancellable and shows cumulative per-chunk progress | ✅ | `fetch_all_pages_stops_at_the_end_of_the_list` (progress `[2, 4, 5]`), `a_running_walk_shows_cumulative_progress_and_stop`, `fetch_all_pages_runs_through_the_app` |
| 4 | Refresh never lowers `last_page_fetched`; Continue is the only run that clears it | ✅ | `refresh_does_not_rewind_a_deep_cursor`, `a_refresh_that_reaches_the_end_leaves_the_cursor_alone`, `fetch_more_resumes_after_the_last_page` |
| 5 | The detail toggle exists, defaults off for new configs, disables after a hand edit, and is honored by every fetch path | ✅ | `the_detail_toggle_is_off_and_usable_for_a_fresh_portal`, `a_hand_edited_config_disables_the_detail_toggle`, `a_disabled_detail_block_skips_enrichment` |
| 6 | Existing `detail` blocks continue to enrich without changes | ✅ | `a_detail_block_without_enabled_still_enriches`, `an_old_detail_block_shows_the_toggle_on`; the registry entries were not touched |
| 7 | No schema change and no new dependency | ✅ | `migrations/` and `Cargo.toml` unchanged |
| 8 | Offline suite, fmt, and clippy are clean | ✅ | See the command table below |
| 9 | Manual script §4.4, `docs/known-sites.md`, parent plan, and retrospective updated | ✅ | §4.3e and §4.4/§4.4a–c rewritten; `known-sites.md` gains "Enrichment is opt-in"; `main.md` v0.15; this document. The manual pass itself is pending. |

**Verification commands and results (2026-11-06):**

| Command | Result |
| :--- | :--- |
| `cargo fmt --all --check` | clean |
| `cargo clippy --workspace --all-targets --offline -- -D warnings` | clean |
| `cargo test --workspace --features career-ops-app/keychain --offline -- --skip obscura` | all passed; 44 router tests, 12 portals-service tests, 40 portals-screen tests |

No live tests were added. The phase changes which requests are made, not how
they are made, and every new test runs against a scripted `FetchSource`.

---

## What the implementation found

### F1 — Refresh had to be monotonic, not inert (design correction)

The plan first said Refresh would leave the cursor alone entirely. Implementation
showed why that is wrong: after a first Refresh of pages 1–5, Continue would
start again at page 1, so the common "look at the new postings, then crawl
deeper" flow would re-read everything. The actual bug is the *lowering* half.

`record_fetch_outcome` now takes the run's `FetchStart`: a `Beginning` run raises
the cursor through `portals::set_last_page_fetched_at_least` (SQL
`WHERE last_page_fetched < ?`), and only a `Resume` run can clear it. The plan's
v0.3 changelog records the correction rather than leaving the text disagreeing
with the code.

### F2 — "Fetch all" already meant something (naming)

`PortalsAction::FetchAll` is the cross-portal batch, and the header already had a
**Fetch all** button. The new per-portal action is `FetchAllPages(i64)`, and the
header labels became **Fetch all portals** and **Reload list**. The collision
would have been a compile error for the variant and an ambiguous screen for both
sets of labels, which is why the naming decision was settled before WS1.

### F3 — A portal that cannot paginate never reports `end_of_list` (correctness)

`end_of_list` comes from `PaginationStop`, which only the selector and Eightfold
paths produce. An ATS adapter pages internally and reports neither pages nor a
stop reason, so a loop watching only `end_of_list` would re-fetch the same board
fifty times. Two guards: the button is gated on the existing `can_fetch_more`,
and the service stops with `NoProgress` when a chunk returns no cursor or no
pages. A third guard catches a cursor write that failed silently: if a chunk
starts where the previous one started, the walk stops instead of repeating.

### F4 — Eightfold needed a provider-specific default (backward compatibility)

Making `detail` absent mean "off" would have silently stopped every Eightfold
board from storing descriptions — including NVIDIA, whose registry config has no
`detail` block at all. Rather than rewrite the registry entry (which would flip
every existing NVIDIA portal to "Modified from the known config"), the rule is:
no `detail` block means off **except** for Eightfold, whose list has no text to
begin with. The form reads the same rule, so the toggle shows on for NVIDIA
instead of lying.

This also required a hand-written `Default for DetailConfig`: the router
constructs one by hand for those boards, and the derived bool default (`false`)
would have disabled the whole path. `the_default_detail_config_enriches` pins it.

### F5 — The toggle derives from the JSON (data-flow correction)

The first implementation stored `detail_enabled: bool` on the form and synced it
on load. That drifts the moment a form is built without going through
`from_row`/`apply_known_site` — which is exactly what the screen tests do, and
what any future caller would do. The toggle now reads
`detail_enabled_in(config_text, provider)` every frame and writes back through
`apply_detail_enabled`, so the JSON is the single source of truth and the
checkbox cannot disagree with what Save stores. `detail_edited` is the only
state, and it is cleared by **Restore known config**.

### F6 — A running walk never lets `Harness::run()` settle (testing)

The row shows a spinner while the walk runs, and egui's spinner requests a
repaint forever, so `Harness::run` panics with "exceeded max_steps". The screen
test uses `run_steps(2)`, the established pattern for spinner frames.

### F7 — The budget is per chunk, so it can overshoot (documented)

The 50-page budget is checked between chunks, not inside one, because the router
owns the page loop. A chunk that crosses the budget finishes, so the total can
exceed 50 by at most `pagination.max_pages - 1`. That is intentional: aborting
mid-chunk would need a second page-level callback in the router for a bound that
is already a policy, not a limit.

---

## Estimate accuracy

| Workstream | Plan | Actual |
| :--- | :--- | :--- |
| WS1 backend: cursor rule, `fetch_all_pages`, detail gate | 1 day | Part of 2026-11-06 |
| WS2 UI: labels, progress/Stop, form toggle, app wiring | 1 day | Part of 2026-11-06 |
| WS3 validation, manual script, docs, retrospective | 0.5 day | Part of 2026-11-06 |
| **Total** | **2–3 days** | **One session** |

The two plan corrections (monotonic cursor, Eightfold default) cost nothing extra
because they were found by tests during WS1 rather than in a manual pass.

---

## Deferred, unchanged

- **Q-33 configurable Fetch-all budget.** Still a visible constant. Revisit if a
  real board makes 50 pages feel wrong.
- **Q-43 the batch's own vocabulary.** The header batch keeps "Fetch all
  portals" and no budget; if it grows one, it should adopt this phase's naming.
- **Sidecar state after a walk.** `apply_portal` refreshes health but not the
  sidebar sidecar badge; a walk is a Tier 1 operation and says nothing about the
  browser backend.
- **`docs/LIVE_TESTS.md`** is unchanged: no new network path.

Phase 2.9 (preferences and the job queue) is next and depends on this phase's
per-portal detail toggle.
