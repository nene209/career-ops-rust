# Phase 2.5 Retrospective and Verification Record

**Phase:** Manual-Testing Fixes
**Parent plan:** Draft Execution Plan: Career-Ops Rust Desktop Application (v0.10), §12
**Phase plan:** `.agents/plans/phase_2.5_manual_testing_fixes.md` (v0.7)
**Period:** 2026-11-03 to 2026-11-05
**Estimate:** 4–6 days
**Actual:** three days, five workstreams plus a post-verification polish pass
**Status:** Complete. Manual script §6 verified 2026-11-05.

---

## Summary

Phase 2.5 began with eight observations from the first real use of the app. Only
some were what they first appeared to be:

- The NVIDIA duplicate rows were an identity bug, not a manual-entry bug: the
  dedup rule stopped at the external id, and a manual add had no provider id at
  all, so the same posting arrived under two identities.
- The empty and truncated descriptions were two problems sharing a symptom: a
  detail fetch that produced a card teaser could replace a full description,
  and some postings never got a detail fetch.
- The HTML in the detail panel was a display problem; storage was already doing
  the right thing by keeping what the site served.

The phase fixed the causes in dependency order. Identity and merge policy came
first (WS1), because new controls on top of duplicate rows would only create
more of them. Continuation and repair came next (WS2), then the missing status
(WS3), then delete and search (WS4), then the display normalizer (WS5).

Two decisions shaped the result:

1. **The stored row is the record of what the site said.** Descriptions are
   normalized at read time (`core::text::plain_text`), never stripped at
   acquisition and never rewritten by a weaker fetch.
2. **Anything the user authored is expensive.** A fetched-only row deletes
   without ceremony; a row with a status, report, or cover letter sits behind a
   typed `delete me` gate.

The post-verification pass added three small things the manual run made obvious:
Enter unlocks the app, a search reports how many rows match, and the list comes
back at the same scroll position after a detail view.

---

## Exit criteria status

| # | Criterion | Status | Evidence |
| :--- | :--- | :--- | :--- |
| 1 | Re-adding a portal-fetched posting updates that row (same id), fills its description, and creates no duplicate | ✅ | `a_tracking_parameter_is_not_a_second_posting`, `a_provider_id_matches_across_a_renamed_slug`, `a_second_extraction_of_the_same_posting_updates_it`; `url_fetch_of_a_known_eightfold_posting_uses_the_provider` |
| 2 | A later fetch cannot replace a non-empty description with an empty or strictly shorter teaser; an explicit refresh can | ✅ | `an_empty_refetch_keeps_the_stored_description`, `a_truncated_teaser_never_replaces_a_description`, `a_richer_description_replaces_a_teaser`; `refresh_job_fills_a_missing_eightfold_description_in_place` |
| 3 | **Fetch more** continues after the last page, advances the cursor, and stops at the end of the list; no one-click fetch-all | ✅ | `a_partial_pagination_is_reported_in_the_outcome`, app `fetch_more` tests, and manual §4.4a. There is still no per-portal fetch-all; see "Deferred" below. |
| 4 | **Refresh description** updates a stored posting in place and never inserts | ✅ | `refresh_job_fills_a_missing_eightfold_description_in_place`, app `refresh_description_updates_the_row_in_place` |
| 5 | `Not a match` exists, is settable from the detail view, has a filter, and is excluded from "Not applied" | ✅ | `applications.rs` integration suite, `filters_partition_the_statuses`, `not_a_match_is_not_part_of_not_applied`, kittest `mark_not_a_match_emits_the_action` |
| 6 | A fetched-only job deletes without confirmation; a user-touched job requires typing `delete me` | ✅ | kittest `a_fetched_only_row_deletes_without_confirmation`, `a_touched_row_opens_the_typed_gate`, `a_wrong_phrase_keeps_the_delete_disabled` |
| 7 | The Pipeline has a search field that composes with the filters and survives a reload | ✅ | `search_matches_title_or_company_case_insensitively`, `search_composes_with_the_status_filter`, `replace_jobs_keeps_the_current_filter`, kittest `the_search_field_emits_the_query` |
| 8 | The detail view shows plain text by default and offers the stored markup on demand; stored text unchanged | ✅ | `core::text` unit tests, `the_detail_renders_plain_text_and_offers_the_raw_toggle`, `the_raw_toggle_shows_the_stored_markup` |
| 9 | Waymo's entry enriches every posting on the pages it fetched, and a failed detail keeps the teaser | ✅ | `a_failed_detail_fetch_keeps_the_list_row`; Waymo's config uses `max_jobs: 90` for the nine-card page set; manual §6.8 |
| 10 | Migrations 0003 and 0004 apply to an existing database, the suite and clippy are clean, and a retrospective is recorded | ✅ | `reopening_the_database_backfills_canonical_urls`; verification table below; this document |

**Manual verification (2026-11-05).** The owner ran manual script §6 and marked
it good: filters, search and its count, the detail view's formatted text and raw
toggle, mark-not-a-match, refresh, and both delete paths. Network steps from §4
and §5 were verified in Phase 2.3/2.4 and are indexed in `docs/LIVE_TESTS.md`.

**Verification commands and results (2026-11-05):**

| Command | Result |
| :--- | :--- |
| `cargo fmt --all --check` | clean |
| `cargo clippy --workspace --all-targets --offline -- -D warnings` | clean |
| `cargo test --workspace --features career-ops-app/keychain --offline -- --skip obscura` | all passed; 27 pipeline-screen tests, 6 lock-screen tests, 13 `core::text` tests |
| Manual script §6 | passed by the owner |

No network was needed for the suite. The live paths remain gated and indexed in
`docs/LIVE_TESTS.md`.

---

## What the implementation found

### F1 — Identity was a string comparison, and the fallback was conditional (real bug) — fixed

The old lookup used the external id when present and skipped the title/company
fallback in that case; manual entry had no provider, so it wrote a synthetic
identity and created a second row. WS1 added a stored `canonical_url`, made the
lookup order explicit (provider-scoped id → canonical URL + id → hash →
canonical URL + title + company), backfilled existing rows in migration 0003,
and routed manual URL entry through the provider. The duplicate is now a
regression test, not a cleanup task.

**Lesson.** A dedup rule is part of the write contract, not a search helper.
Every path that inserts a posting has to produce the same identity.

### F2 — A merge policy needs a notion of quality (real bug) — fixed

A fetch that returned empty text, or a card teaser that was a strict prefix of
the stored description, overwrote a good description. `merge_raw_text` now
keeps the better text; only an explicit per-job refresh bypasses it (and that
path is documented as the repair tool).

**Lesson.** "Last write wins" is wrong for a description that is the input to an
evaluation. The writer needs to know which text is better.

### F3 — A cursor is not a crawl (design note)

`portals.last_page_fetched` plus `FetchStart::{Beginning, Resume}` gives a
resumable **Fetch more** without making the app a crawler. It also answers the
"fetch all" request in a bounded way: the user advances in configured batches
and can raise `max_pages` knowingly.

**Why it matters.** A one-click walk of NVIDIA is roughly 269 requests against
an unofficial API. Making that a button turns a UI action into an operational
commitment (rate limits, WAF, minutes of unattended work). The cursor gets to
the same place in steps the user chose.

### F4 — A status with no writer is decoration (design note)

Phase 2 had never created an `applications` row in production. WS3 made
`mark_not_a_match` its first writer (`ensure` then `transition`, idempotent)
before adding the badge and filter, so the status could be exercised by a real
action rather than a test fixture.

### F5 — The delete gate belongs to the view (design note)

The gate is `PipelineView` state. `screens::pipeline::show` opens it and only
sends `DeleteJob` after the phrase matches; the app deletes and reports the
outcome, clearing the gate on success. This is the same shape as Phase 2.4's
chooser: a dialog's decision is applied after the borrow of its own state ends,
and local view state does not travel through the app just to come back.

**Lesson.** If the app does not need to know until the user confirms, keep the
intermediate state in the view.

### F6 — The fragment parser lifts `title` out of `head` (small, real) — fixed

`Html::parse_fragment` parses with a body context, so `<head>` is ignored and
`<title>` survives as an element. The normalizer drops `head` by name but had to
drop `title` too. A unit test caught it; the fix is one name.

**Lesson.** "Drop `head`" is not the same as "drop everything the document considers
metadata" once a fragment parser is involved. Test the intent, not the tag.

### F7 — Enter did not unlock because focus had already moved (real bug) — fixed

The unlock screen checked `response.has_focus()` for the passphrase field. A
single-line `TextEdit` surrenders focus when Enter is pressed, so in the frame
that matters `has_focus()` is already false and the key press was ignored.
`lost_focus()` plus `key_pressed(Enter)` is the correct signal. Found during the
post-verification pass; `enter_in_the_passphrase_field_requests_an_unlock`
locks it in.

**Lesson.** In immediate-mode UI, check the transition (`lost_focus`) rather than
the resting state (`has_focus`) when the input itself changes the state.

### F8 — Search and scroll are view preferences, not data (polish) — added

The search count follows `apply_filter()` (so it composes with the status
filter), and the list scroll offset is stored on `PipelineView` so a detail round
trip returns to the same place. A filter or search change resets the offset to
the top, because the result set changed underneath it.

---

## Deferred to a future phase

### D1 — A bounded "fetch to end" action (not this phase)

The request that surfaced during verification was "add a fetch all for the
portals". A **Fetch all** button already exists: it runs one configured fetch for
every portal, in sequence, with per-portal progress and cancel (manual §4.6). It
does not walk every page of every portal.

Q-18 in the phase plan records the decision against an unbounded walk. If a
future phase wants a stronger action, the shape should be:

1. **Bounded by configuration, not by hope.** Reuse `pagination.max_pages` as
   the hard stop, and surface it in the portal form (today it is JSON-only).
2. **Resumable and cancellable.** Loop `router::fetch_portal_more` until
   `FetchOutcome.end_of_list`, the cap, or cancel; report progress per portal
   the way the existing batch bar does.
3. **One portal at a time.** The same coarse cancellation guarantee as
   `fetch_batch_cancellable`: finish the in-flight request, then stop.
4. **Tested with `MockFetchSource`.** The loop is arithmetic and cursor state,
   so it belongs in offline tests; a live run is a separate, ignored check.

Estimate: 1–2 days as a small workstream (service loop + view action + tests).
It is not required for Phase 3, which evaluates stored rows.

### D2 — Live-test policy (done, in `docs/LIVE_TESTS.md`)

All network-dependent tests are now indexed in one place with their gates, what
they prove, and a standing mock review. The only genuine mock candidate is the
Obscura spike's GitHub navigation (a local page with inline JavaScript would
prove the same thing). The ATS and registry tests should stay live: a fixture
cannot detect an upstream payload change.

One follow-up is recorded there: `obscura_print_to_pdf_if_supported` does not
check whether the binary is available, which is why validation skips the whole
file with `--skip obscura`. Add the gate when that spike is next touched.

### D3 — Existing duplicates are not merged (unchanged)

Q-19 stands: the delete action is the tool. An automatic merge would have to
choose which text, status, and evaluations survive, which is a product decision.

---

## Estimate accuracy

| Workstream | Plan | Actual |
| :--- | :--- | :--- |
| WS1 identity, dedup, manual add | 1–1.5 days | Part of 2026-11-03 |
| WS2 cursor and refresh | 0.5–1 day | Part of 2026-11-03/04 |
| WS3 `Not a match` | 0.5–1 day | Part of 2026-11-04 |
| WS4 delete and search | 0.5–1 day | Part of 2026-11-04 |
| WS5 plain text and raw toggle | 0.5 day | Part of 2026-11-04/05 |
| Post-verification polish (Enter, count, scroll) | not estimated | Part of 2026-11-05 |
| **Total** | **4–6 days** | **Three days** |

The phase came in shorter than the estimate because every workstream had a
test seam written down in the plan before implementation, and because the last
three items were small view-layer changes rather than data work.

---

## Lessons to carry into Phase 3

1. **Fix identity before adding features.** New controls on duplicate rows make
   more duplicates, not fewer.
2. **Normalize at the boundary where it is consumed.** `plain_text` exists once
   and both the UI and prompts will call it; stating the rule in a prompt would
   be unverifiable.
3. **A destructive action needs a cost model.** Typing a phrase is proportionate
   to an evaluation that cost tokens, not to a row the portal can fetch again.
4. **Manual testing finds transitions that unit tests miss.** Enter, scroll, and
   the search count were invisible to the existing tests because they are about
   what happens between two stable states.
5. **Keep the live set small and weak.** The offline suite proves our logic; the
   live tests prove the world still looks the way the logic expects. Do not blur
   the two.
