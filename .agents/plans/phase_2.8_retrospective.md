# Phase 2.8 Retrospective and Verification Record

**Phase:** Portal Fetch Controls
**Parent plan:** Draft Execution Plan: Career-Ops Rust Desktop Application (v0.15), §12
**Phase plan:** `.agents/plans/phase_2.8_portal_fetch_controls.md` (v0.8)
**Period:** 2026-11-06
**Estimate:** 2–3 days
**Actual:** one session across three workstreams
**Status:** Complete. Manual testing found three problems (F8, F9, F10), all fixed and re-tested: §4.3e and §4.4a–c are verified by the owner. §4.4d (**Skip page N**) is test-covered but was not exercised manually, because F10 removed the stall it handles.

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
| 2 | Fetch all pages stops at end-of-list, cancel, no progress, or 50 pages, and reports which | ✅ | `fetch_all_pages_stops_at_the_end_of_the_list`, `…_when_the_user_cancels`, `…_at_the_page_budget`, `…_when_the_provider_cannot_continue`, `…_reports_an_unreadable_page` |
| 3 | Fetch all pages is cancellable and shows cumulative per-chunk progress | ✅ | `fetch_all_pages_stops_at_the_end_of_the_list` (progress `[2, 4, 5]`), `a_running_walk_shows_cumulative_progress_and_stop`, `fetch_all_pages_runs_through_the_app` |
| 4 | Refresh never lowers `last_page_fetched`; Continue is the only run that clears it | ✅ | `refresh_does_not_rewind_a_deep_cursor`, `a_refresh_that_reaches_the_end_leaves_the_cursor_alone`, `fetch_more_resumes_after_the_last_page` |
| 5 | The detail toggle exists, defaults off for new configs, disables after a hand edit, and is honored by every fetch path | ✅ | `the_detail_toggle_is_off_and_usable_for_a_fresh_portal`, `a_hand_edited_config_disables_the_detail_toggle`, `a_disabled_detail_block_skips_enrichment` |
| 6 | A stalled walk records the pages it read and offers **Skip page N and continue** | ✅ | `fetch_all_pages_reports_an_unreadable_page` (cursor moves to the last good page, retry starts at the failed one), `skipping_a_page_resumes_the_walk_after_it`, `a_stalled_walk_offers_skip_for_the_unread_page` |
| 7 | Existing `detail` blocks continue to enrich without changes | ✅ | `a_detail_block_without_enabled_still_enriches`, `an_old_detail_block_shows_the_toggle_on`; the registry entries were not touched |
| 8 | No schema change and no new dependency | ✅ | `migrations/` and `Cargo.toml` unchanged |
| 9 | Offline suite, fmt, and clippy are clean | ✅ | See the command table below |
| 10 | Manual script §4.4, `docs/known-sites.md`, parent plan, and retrospective updated | ✅ | §4.3e and §4.4/§4.4a–d written and marked; `known-sites.md` gains both the enrichment and the skip rules; `main.md` v0.15; this document. §4.3e and §4.4a–c were verified by the owner; §4.4d is covered by the three skip tests and left unexercised. |

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

### F8 — A refused page looked like "no progress" (found in manual testing)

The first live walk returned `Fetched 40 jobs; this board has no page to continue
from.` after one chunk, while Continue on the same portal had just read pages
26–29 and reported that page 30 could not be read.

The cause is a field with two meanings. `FetchOutcome::next_page` is `None` both
when a provider has no page accounting at all (an ATS adapter) and when a chunk
stopped at a page it could not read — the router deliberately leaves the cursor
alone in the second case so a retry re-reads the range. The walk only tested
`next_page.is_none()`, so 4 pages and 40 jobs of real progress were reported as
the provider having no page to continue from, and the walk gave up after one
chunk.

The fix distinguishes the cases: `NoProgress` now requires
`pages_fetched.is_none()` (the provider owns pagination), and a chunk that read
pages but cannot continue reports `Stopped { unread_page }`, where the page is
`first_page + pages_fetched` for `FetchFailed`/`ExtractionFailed` and unknown for
the "board stopped advancing" cases. A regression test
(`fetch_all_pages_reports_an_unreadable_page`) pins the reason, the stored count,
and that the walk does not retry the refused page in a loop.

### F9 — Retrying a page that keeps refusing needed a way out (found in manual testing)

The first fix reported the page and left the cursor where it was, so Continue
asked for the same page again — and on NVIDIA that page kept failing, which made
"Continue retries it" a promise the app could not keep. Two changes:

- **The walk records the pages it read.** On a partial chunk it raises the
  cursor to the last page that produced jobs, so the next attempt starts at the
  unread page instead of re-reading four good ones. The router still leaves the
  cursor alone on a partial failure, which is right for a single Continue; the
  walk knows more, so it writes.
- **The screen offers `Skip page N and continue`.** It is only shown when a walk
  stalled with a page number, it moves the cursor onto N and resumes at N+1, and
  it is never automatic: the page's postings stay unread. That is the difference
  between "the site refused one page" and "the board is done", and the app
  cannot tell which — the user can.

The skip is still the right escape hatch, but the page that motivated it was
not a refusal at all: F10 found a parse failure behind it.

**What the probe showed.** `https://jobs.nvidia.com/api/pcsx/search?domain=nvidia.com&start=290`
answers `200` with positions, both with `curl`'s user agent and with
`career-ops/0.1.0`. That ruled out an offset cap and the end of the list, but it
did not explain the failure — see F10, which found the actual cause in the body.

**Lesson.** A nullable field that means two things will be read as one of them.
The stop reason needed to come from `pagination_stopped`, not from the absence of
a cursor.

### F10 — One float timestamp failed a whole page (found by reading F9's error)

With the failure finally visible in the message (F9 made the walk report the
page; the app now also surfaces the classified error), the cause was plain:

```
Eightfold: response could not be parsed: eightfold json: invalid type:
floating point `0.0`, expected i64 at line 1 column 4165
```

One posting on page 30 of the NVIDIA board carries `"postedTs": 0.0` while
`Position.posted_ts` was `Option<i64>`. Because positions were deserialized as
one `Vec<Position>`, that single value failed the entire page — every time, at
the same offset, which is exactly the "stuck page" behaviour F9 tried to work
around. `postedTs` is optional metadata: a posting with no date is not an error.

Two changes, both narrow:

- `posted_ts` is read defensively — integer, float, or numeric string — and zero
  or an unreadable value becomes `None` rather than 1970 or a hard failure.
- `parse_page` deserializes positions one at a time, so a posting that still
  cannot be read is skipped with a warning instead of costing the page.

`test config` succeeded all along because it only reads page 1, where every
`postedTs` is an integer. That is worth remembering as a debugging signal: a
preview passing and a deep page failing points at the payload, not the transport.

**Lesson.** Parsing an untrusted array as one struct makes the smallest field a
single point of failure. Deserialize items individually and keep the page.

---

## Estimate accuracy

| Workstream | Plan | Actual |
| :--- | :--- | :--- |
| WS1 backend: cursor rule, `fetch_all_pages`, detail gate | 1 day | Part of 2026-11-06 |
| WS2 UI: labels, progress/Stop, form toggle, app wiring | 1 day | Part of 2026-11-06 |
| WS3 validation, manual script, docs, retrospective | 0.5 day | Part of 2026-11-06 |
| **Total** | **2–3 days** | **One session plus one manual-testing cycle (F8, F9, F10)** |

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
