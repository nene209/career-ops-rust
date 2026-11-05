# Phase 2.5: Manual-Testing Fixes — Detailed Execution Plan

**Parent plan:** Draft Execution Plan: Career-Ops Rust Desktop Application (v0.10), §12
**Status:** In progress — WS1–WS5 complete; manual verification of WS4–WS5 pending
**Last updated:** 2026-11-05
**Duration:** 4–6 days
**Depends on:** Phase 2.2 (pagination, detail enrichment), Phase 2.3 (providers, registry), Phase 2.4 (catalog UI)
**Blocks:** Phase 3 benefits from WS5 (`plain_text`) but does not depend on the rest.

---

## Change Log

| Version | Change |
| :--- | :--- |
| v0.1 | Initial plan, written from the first manual test session against NVIDIA and Waymo. Eight findings, five workstreams. Two are data-integrity bugs (duplicate postings, description clobbering), three are missing controls, one is a missing status, one is a missing search, and two are display questions the data already answers. |
| v0.2 | Decisions after review. Deletion needs no confirmation for a fetched-only row, but an evaluated (or otherwise user-touched) row is behind a typed `delete me` gate, because evaluations cost real tokens and are not re-fetchable. Waymo's production entry enriches **every posting on the pages it fetched**, not `max_jobs: 2`; the small caps stay in test configs only. WS1 starts. |
| v0.3 | Workstream 1 implemented: canonical URL identity, migration 0003 with a guarded backfill, dedup rules that survive different extractions, never-clobber on update, provider-aware manual add, and the JSON-LD company fallback. The reported duplicate is reproduced by a test and fixed. |
| v0.4 | Workstream 2 implemented: migration 0004 and `portals.last_page_fetched`, an explicit start page in both fetch paths, `FetchStart::{Beginning, Resume}` with **Fetch more**, a per-job **Refresh description**, and Waymo's production entry enriching every posting on its fetched pages. |
| v0.5 | Workstream 3 implemented: `Not a match` as an application status, its transition rules, the first production writer of `applications` (create-if-absent, idempotent), a filter that excludes it from **Not applied**, and the detail action. |
| v0.6 | Workstream 4 implemented: `job_descriptions::delete` with cascading application/report/letter removal, a typed `delete me` gate for rows the user touched, a case-insensitive Pipeline search over title and company with its own empty state and **Clear**, and a search that survives a reload. The gate is view state, so only a confirmed delete reaches the app. |
| v0.7 | Workstream 5 implemented: `core::text::plain_text` (idempotent, entity-safe, block-aware, char-boundary truncation, parsed with `scraper`), the Pipeline detail rendering the normalized text with paragraph breaks, and a **Raw text** toggle with the stored markup and its source note. Phase 3's plan points at the function instead of re-specifying it. |

---

## Purpose

Phase 2.5 exists because the app has now been used the way a person uses it, not
the way a test does. Eight observations came back, and only some of them are what
they first appear to be. This phase fixes the causes, in the order the data
depends on them: identity and dedup first (otherwise new features add rows), then
continuation and repair, then the pipeline controls built on top.

### The findings, and what they actually are

| # | Report | What is actually happening |
| :--- | :--- | :--- |
| 1 | A fetch only ever reads the page limit from the start; no "continue" or "fetch all". | Both true and deliberate. `pagination.max_pages` (default 5) bounds every fetch, and every fetch starts at page 1 / offset 0. Nothing stores a cursor, so there is no way to continue. "Fetch all" was excluded on purpose in 2.3 (P2.3-R2: NVIDIA reports ~2,750 postings, about 269 requests). |
| 2 | Some NVIDIA descriptions were empty; re-adding the same URL created a duplicate instead of updating the row. | Three separate causes. (a) `manual::add_job_from_url` knows ATS adapters and generic HTML/Tier 2, but **not** the provider engines, so it cannot derive the Eightfold `external_id` the portal row has. (b) `job_descriptions::find_existing` skips the title/company fallback whenever an `external_id` is present, and every rule is scoped to the exact `source_url`, so a second extraction of the same posting inserts. (c) `manual` passes `"Manual"` as the company name for non-ATS pages, so even the fallback cannot match. Separately, `update` overwrites `raw_text` unconditionally, so a later fetch can replace a description with a teaser. |
| 3 | The pipeline needs an "invalid / not a match" status. | `ApplicationStatus` has seven variants and no "not a match". Also: no production code calls `applications::create` today (only a test does), so no pipeline status is currently set by the app. The new status needs a writer, not just an enum variant. |
| 4 | A job cannot be deleted from the UI. | `job_descriptions` has no delete path. The schema already cascades (`applications` → reports and cover letters) and `foreign_keys = ON` is set, so the mechanism exists but is unused. |
| 5 | No way to refetch one job. | No service for it. The identity needed already exists: Eightfold rows store `eightfold:{domain}:{id}` in `external_id`, so the engine and its arguments are derivable. |
| 6 | The Pipeline needs search. | `apply_filter` matches status only. Title and company are already in memory on every `JobRow`. |
| 7 | Some NVIDIA descriptions contain HTML and others do not. Is the UI supposed to render it that way? | **No.** The detail panel renders `raw_text` verbatim in monospace. HTML is what is stored for JSON-LD, Eightfold, and SmartRecruiters; plain text comes from card selectors and pastes. Storage is right; display is not. |
| 8 | Some Waymo descriptions are cut off with `...`. | Not our truncation. Only 2 of 90 rows are enriched (`detail.max_jobs: 2`); the other 88 store `p.job-search-results-summary`, which is the site's own truncated card teaser. The fix is coverage, not an ellipsis bug. |

---

## Scope

| In scope | Out of scope |
| :--- | :--- |
| Provider-aware manual add and one identity per posting | A crawl mode or scheduled fetches |
| Never replacing a good description with a worse one | Automatic merging of existing duplicates (delete the worse copy) |
| A resumable page cursor and **Fetch more** | An unbounded "fetch all" button |
| Per-job **Refresh description** | Re-fetching every row on a schedule |
| `Not a match` status with a filter and a writer | A full status workflow editor |
| Delete a job with an explicit confirmation | Undo/trash for deletions |
| Pipeline search composed with the filters | Full-text search over descriptions |
| Plain-text display with a raw toggle | Rendering HTML safely in the detail panel |
| A higher Waymo detail cap | Scraping the teaser's full text from the listing page |

---

## Design decisions

### 1. One posting, one identity

A posting is identified by its canonical URL plus the provider's stable id. The
canonical form lowercases the host, drops the fragment, the default port, the
trailing slash, and tracking parameters, and is computed in one helper
(`core::acquisition::canonical_url`) used by both the portal and manual paths.

Dedup then works on the canonical URL:

1. `(canonical_url, external_id)` — the provider's own identity.
2. `(canonical_url, content_hash)` — a repost under a new id.
3. `(canonical_url, title, company)` — **regardless of whether an external_id
   exists.** The same URL with the same title and company is the same posting;
   the old rule's exclusion of that case is what allowed finding 2.

New rows store the canonical URL. A one-time backfill rewrites existing rows in
the same migration, because otherwise a pre-2.5 row and a post-2.5 fetch of the
same posting still would not meet.

### 2. Manual add is a single-job provider fetch

`add_job_from_url` consults, in order: a configured portal whose host matches →
a known site (`known_sites::lookup`) → an ATS adapter → generic HTML → Tier 2.
For a known Eightfold site, it calls the provider's list and detail directly, so
the stored row has the same `external_id`, the same `source_url`, and a
description. That fixes the duplicate and the empty description in one change.

The company name comes from the page (`hiringOrganization.name`) or the matching
portal, never the literal `"Manual"`.

### 3. Never clobber a better description

`update` keeps the stored `raw_text` when the incoming text is empty or is a
strict prefix of it (the site truncating its own teaser). A description that is
already good can no longer be replaced by a card summary. An explicit refresh may
still replace it, because that is the user asking.

### 4. A cursor, not a crawl

`portals.last_page_fetched` (migration 0003) stores how far a portal has been
read. **Fetch more** starts at `last_page_fetched + 1` and reads up to the
configured `max_pages` from there; a normal **Fetch** still starts at page 1.
When a provider reports the end of the list, the next page is cleared so the next
"Fetch more" starts over rather than requesting past the end.

Adapters that own pagination internally (Ashby, Lever, Greenhouse,
SmartRecruiters) have no cursor and disable **Fetch more**; their whole board
arrives in one call.

### 5. Per-job refresh is the repair tool

`refresh_job(id)` re-fetches one stored posting and updates its text in place. It
infers the provider from `external_id` first (`eightfold:{domain}:{id}` carries
both the engine and its arguments), then from the URL via the registry, then
falls back to generic HTML. It never inserts.

This is how a person fills the ten gaps in an NVIDIA fetch without paying for a
second full fetch, and how a Waymo teaser becomes a description.

### 6. A status needs a writer

Adding `NotAMatch` without a way to set it would ship a dead badge. The detail
view gains **Mark not a match**, which creates the application row if the job has
none and then transitions it. Phase 3 will set the same status from
`Recommendation::NoMatch`; the manual action stays.

### 7. Search is a filter input

The Pipeline page is bounded (`PIPELINE_LIMIT`), so search is an in-memory,
case-insensitive substring over title and company, composed with the status
filter. No new query layer, no full-text index. The search text lives in the view
model so a reload does not clear it.

### 8. Storage keeps what the site served; display normalizes

`raw_text` stays exactly what the provider returned. The detail view shows
`plain_text(raw_text)` by default and offers a **Raw text** toggle for the stored
markup. `core::text::plain_text` is the Phase 3 normalizer, implemented here
because the UI needs it first; Phase 3 then consumes it for prompts.

---

## Workstream 1 — Identity, dedup, and provider-aware manual add

**Priority:** First. Every later workstream depends on a posting not duplicating.
**Estimated duration:** 1.5–2 days.

### Deliverables

1. `core::acquisition::canonical_url(&Url) -> String` with the rules in decision 1,
   plus `#[cfg(test)]` cases for query strings, fragments, ports, case, and
   trailing slashes.
2. Migration `0003_job_identity.sql`:
   - `ALTER TABLE job_descriptions ADD COLUMN canonical_url TEXT;`
   - index `ux_jd_canonical_external (canonical_url, external_id)` and
     `ux_jd_canonical_hash (canonical_url, content_hash)`.
   The SQL files stay append-only, and sqlx only runs SQL; the backfill is a
   guarded Rust step in `db` that runs after migration while any row still has a
   NULL `canonical_url` (idempotent, row ids unchanged).
   Existing `source_url` is kept as the human-facing URL.
3. `job_descriptions::find_existing` switches to the canonical rules, including
   the unconditional title/company fallback (decision 1).
4. `update` applies the never-clobber rule (decision 3), with tests for empty,
   prefix, and genuinely different text.
5. `manual::add_job_from_url` gains provider dispatch (decision 2), including the
   Eightfold list/detail path and the company-name fix.
6. `jsonld::extract` prefers `hiringOrganization.name` over the caller's
   placeholder when the caller passed none or `"Manual"`.

### Acceptance criteria

| Criterion | Verification |
| :--- | :--- |
| Canonicalization normalizes host case, fragment, port, trailing slash, tracking params | Unit test table |
| Adding the URL of a portal-fetched posting updates that row | Integration test with a scripted source: one row, same id, description present |
| Re-adding the same posting twice inserts once | Integration test |
| An empty or teaser text never replaces a stored description | Unit test on `update` |
| A manual add of a known Eightfold posting stores `eightfold:{domain}:{id}` | Unit test with a stubbed provider |
| Existing databases migrate and backfill | Migration test on a seeded 2.4 database |

**Implementation note (2026-11-03).** `core::acquisition::identity::canonical_url`
lowercases the host (via `Url`), drops the fragment, a non-default port, one
trailing slash, and the known tracking parameters, then sorts the surviving
query pairs. Paths stay case-sensitive, and a query that identifies a posting
(`?jobId=7`) survives.

`job_descriptions` gained `canonical_url` (migration 0003) plus non-unique
indexes. They are deliberately **not** unique: a database written before this
migration may already contain two rows that canonicalize to the same identity,
and a unique index would fail the migration instead of leaving the duplicate for
the user to delete. `upsert` enforces identity on write. The backfill lives in
`db::open`, is idempotent (only NULL rows), and falls back to the raw string when
a URL cannot be parsed.

Dedup now tries, in order: the provider-scoped id alone (`eightfold:` ids are
unique within their engine, so a renamed slug no longer creates a second row —
this was a latent gap in the 2.3 exit criterion); canonical URL plus external id;
canonical URL plus content hash; canonical URL plus title and company,
**regardless of external id**. The last rule is the one that reproduces the
reported bug when it is absent.

`update` merges text instead of overwriting it: an empty extraction, or one that
is a strict prefix of the stored text (the site truncating its own teaser), keeps
the stored text, and the content hash is recomputed from what is stored. An
explicit refresh in WS2 will be allowed to replace it.

Manual add now resolves the host through the user's portals (longest matching
path wins) and then the registry. Selectors configs are used for the fetch but
only when the pasted URL is the portal's own page (a list config applied to a
posting page would store the wrong job), and they fall through to the old chain
when they produce nothing, so Tier 2 escalation is unchanged for
portal-configured sites. Eightfold URLs are read for a position id and fetched
through `pcsx/position_details`, producing the same `external_id` and source URL
the list path would. JSON-LD takes the company from `hiringOrganization.name`
only when the caller passed nothing or `"Manual"`.

Tests: `job_dedup.rs` (7: tracking parameter, renamed slug, second extraction,
empty refetch, teaser prefix, richer text, backfill), `manual_entry.rs` (3:
known Eightfold host, known selectors host, portal-configured host), `eightfold`
unit tests (5: detail posting, missing name, fallback URL, URL ids, detail URL),
and `jsonld` (2: page company for a manual fetch, portal name preserved).

---

## Workstream 2 — Continuation and per-job refresh

**Estimated duration:** 1–1.5 days.

### Deliverables

1. Migration `0004_portal_cursor.sql` adds
   `portals.last_page_fetched INTEGER NOT NULL DEFAULT 0`. It is separate from
   `0003` so WS1 can ship on its own; migrations are append-only.
2. Provider paging contract: `next_page_start(page) -> Request` alongside the
   existing pagination config. Selectors: page 1 is the portal URL, ≥2 uses
   `url_template`; Eightfold: `start = (page - 1) * page_size`; adapters declare
   "no cursor".
3. `router::FetchOutcome` gains `next_page: Option<usize>` and
   `end_of_list: bool`. `portals::service` advances or clears the cursor on
   success only.
4. `PortalsAction::FetchMore(id)` and a **Fetch more** button beside **Fetch**,
   disabled for portals whose provider has no cursor and while a task is running.
   The result message says where it resumed: `Fetched 50 jobs from pages 6-10.`
5. `manual::refresh_job(pool, job_id, source, browser)` and a **Refresh
   description** button in the Pipeline detail. It reports the byte count change.
6. `WAYMO_CONFIG` raises `detail.max_jobs` so every posting on the fetched
   pages is enriched (90 at the current three-page cap, well under the 200
   ceiling), documented in `docs/known-sites.md` as a coverage choice with a WAF
   cost. Test configs keep their small caps; the number that shipped was a test
   value, not a product one. A failed detail keeps the teaser (WS1's
   never-clobber rule).

### Acceptance criteria

| Criterion | Verification |
| :--- | :--- |
| **Fetch more** starts after the last page and advances the cursor | Core test with a scripted `PageSource` |
| A normal **Fetch** still starts at page 1 | Core test |
| Reaching the end of the list clears the cursor | Core test |
| Adapters without a cursor disable the action | `egui_kittest` |
| **Refresh description** updates in place and never inserts | Integration test |
| A failed detail keeps the existing text | Unit test |

**Implementation note (2026-11-03).** Migration 0004 adds
`portals.last_page_fetched` (default 0). `FetchSource` grew a `start_page`
argument on both list paths, and `html::fetch_pages_from` builds the first URL
from the config's `url_template` when the start is past one — a chain of `next`
links cannot be jumped into, so such a config has no cursor. Eightfold maps a
page to `(page - 1) * 10`; the page size is a constant because the API ignores
`num`. If the vendor changes it, dedup keeps the result correct and only the
cursor can drift.

`router::fetch_portal_more` passes `FetchStart::Resume`, which reads the cursor.
The cursor policy is deliberately conservative: a fetch that hit the configured
page cap (`LimitReached`) stores `next_page - 1`, the end of the list clears it
to 0, and a partial failure leaves it untouched so the next attempt retries the
range that failed. A resumed fetch whose first page yields nothing is an ending,
not a failure — otherwise tapping **Fetch more** past the end would mark the
portal Broken. `FetchOutcome` carries `first_page`, `next_page`, and
`end_of_list`, and the result line names a resumed range
(`Fetched 50 jobs from pages 6-10.`).

In the UI, **Fetch more** appears only when `can_fetch_more` finds an Eightfold
provider or a selectors config with a URL template; ATS adapters paginate
internally and a next-link chain has no cursor. The button is disabled while any
portal task runs.

`manual::refresh_job` re-fetches one posting without inserting: an Eightfold id
routes straight to `pcsx/position_details`, otherwise the posting page is fetched
and JSON-LD or the matching portal's `detail` selector is used. The new text goes
through `job_descriptions::set_raw_text`, which bypasses the merge rule (the user
asked for it) and keeps `fetched_at` so a description refresh does not reorder
the pipeline. The Pipeline detail shows the byte count on success and keeps the
old text on failure.

Waymo's embedded `detail.max_jobs` is now 90, which covers all 30 cards on each
of its three fetched pages. The test configs keep `max_jobs: 2`; that number was
a test value, not a product one.

Tests: `router.rs` (3: resume after the cursor with an Eightfold portal, a URL
template portal resuming at page 3, and a next-link portal ignoring
`FetchMore`), `manual_entry.rs` (3: refresh fills a missing Eightfold
description, replaces a teaser from JSON-LD, and leaves text alone on failure),
`html` (1: a resumed page past the end is an ending), `portals_screen.rs` (2:
the button appears for an offset provider and not for a next-link one),
`pipeline_screen.rs` (2: the action and the busy state), an app test that runs
the refresh through the real storage and pipeline, and the `fetch_summary` range
case.

---

## Workstream 3 — `Not a match` status

**Estimated duration:** 0.5–1 day.

### Deliverables

1. `ApplicationStatus::NotAMatch` (`"not_a_match"`), `StatusLabel::NotAMatch`
   (`"Not a match"`), `JobFilter::NotAMatch`, and a `StatusBadge` variant. The
   status column is free-form `TEXT`, so no migration is needed.
2. Transition rules: allowed from `discovered`, `evaluating`, and `ready`; not
   from `applied`, `interviewing`, `offer`, or `rejected`.
3. `applications::ensure(pool, job_id)` — create if absent — then `transition`.
4. **Mark not a match** in the detail view, plus a **Not a match** filter button.
   `JobFilter::NotApplied` excludes the new status, like it excludes rejected.
5. `PipelineViewModel` tests: the filters still partition the statuses.

### Acceptance criteria

| Criterion | Verification |
| :--- | :--- |
| The status round-trips through the database and parses | Unit test |
| The detail action creates the application row when there is none | Integration test |
| Invalid transitions are rejected | Unit test |
| The filter shows exactly the not-a-match rows | Unit test |
| Every status has a unique, non-empty label | Existing label test extended |

**Implementation note (2026-11-03).** `ApplicationStatus::NotAMatch` serializes as
`not_a_match`; the column is free text, so no migration was needed. It is
reachable from `discovered`, `evaluating`, and `ready`, cannot be reached from
`applied`, `interviewing`, `offer`, or `rejected`, and reopens to `discovered`
like `rejected` does. The view mirrors it as `StatusLabel::NotAMatch` and
`StatusBadge::NotAMatch` ("Not a match", muted slate).

Until this phase nothing in production created an `applications` row — only a
test did — so the status column was effectively unused. `applications::ensure`
finds the row for a job or creates it, and `mark_not_a_match` uses it before
transitioning; a second call is a no-op that returns the existing row rather
than failing on `NotAMatch -> NotAMatch`.

`JobFilter::NotAMatch` is the fifth filter, and **Not applied** now excludes the
new status the same way it excludes `rejected`. The detail view offers **Mark
not a match** whenever the status is absent or still `discovered`, `evaluating`,
or `ready`; the app updates the row in place and reports
`Marked as not a match.`

Phase 3 can set the same status from `Recommendation::NoMatch` (Q-20); the manual
action stays, because an evaluation is an opinion.

Tests: repo unit tests for the transition predicate and reachability, a new
`applications.rs` integration suite (create when absent, idempotent, refuses a
submitted application), view-model tests for the mapping and the filter
partition, `egui_kittest` for the action, its disabled states, and the filter
button, and an app test that runs the action through the real storage and
pipeline.

---

## Workstream 4 — Delete a job and Pipeline search

**Estimated duration:** 0.5–1 day.

### Deliverables

1. `job_descriptions::delete(pool, id)`. A fetched-only row deletes immediately:
   nothing user-authored is lost and the portal can fetch it again. A row the user
   has touched — an application status beyond `discovered`, an evaluation report,
   or a cover letter — is behind a typed **`delete me`** gate that names the
   title, the company, and exactly what is lost. The type-to-confirm gate exists
   because evaluations cost real tokens and cannot be re-created by fetching.
2. `PipelineViewModel.search: String`, `PipelineAction::SetSearch(String)`, and a
   search field in the Pipeline header. `apply_filter` applies both the status
   filter and the query (case-insensitive, title or company).
3. A distinct empty state for "No jobs match this search." and a **Clear**
   affordance.
4. The search text survives a reload; the detail view is dismissed when the
   query changes, the same way a filter change dismisses it.

### Acceptance criteria

| Criterion | Verification |
| :--- | :--- |
| Deleting a fetched-only job needs no confirmation | `egui_kittest` |
| A job with an evaluation requires the typed `delete me` phrase | `egui_kittest` |
| The phrase gate names the job and what is deleted with it | `egui_kittest` |
| A wrong phrase keeps the button disabled | `egui_kittest` |
| Deleting a job removes the row and its application | Integration test |
| Search matches title or company, case-insensitively | Unit test |
| Search composes with the status filter | Unit test |
| A reload keeps the search text | Unit test |
| The empty state distinguishes "no jobs" from "no matches" | `egui_kittest` |

**Implementation note (2026-11-05).** `job_descriptions::delete` relies on the
schema's existing `ON DELETE CASCADE`, so one statement removes the posting, its
application, its evaluation reports, and its cover letters. The Pipeline learns
which rows are gated from `job_descriptions::user_touched_ids`: a status other
than `discovered`, a report, or a letter. A fetched-only row deletes from the
detail view with no dialog; a touched row opens a window that names the posting
and what goes with it, and its **Delete job** button stays disabled until the
user types `delete me`. The window is view state, so only the confirmed
`DeleteJob` reaches the app, which deletes and removes the row in place; a
failure leaves the gate open with the error in the header.

Search lives on `PipelineViewModel`, so it survives a reload the same way the
filter does. `apply_filter` composes the status filter with a case-insensitive
substring over title and company, and the screen has its own "No jobs match
this search" empty state with a **Clear search** action. The detail view is
dismissed when the query changes, mirroring a filter change.

Tests: repo integration tests for the cascade and the touched-id query,
view-model tests for search matching and composition, `egui_kittest` for the
field, the empty states, the no-gate path, the gate's wording, the wrong-phrase
lockout, the typed phrase, and Cancel, and an app test that deletes through the
real storage and pipeline.

---

## Workstream 5 — Plain-text display with a raw toggle

**Estimated duration:** 0.5 day.

### Deliverables

1. `core::text::plain_text(raw, max_chars)` exactly as Phase 3's WS3 specifies:
   idempotent, entity-safe, block-aware, truncation on a character boundary,
   plain text unchanged.
2. The Pipeline detail renders `plain_text(raw_text)` with paragraph breaks.
3. A **Raw text** toggle shows the stored markup in monospace, with a note when
   the stored text differed (`Stored as HTML from the source.`).
4. Phase 3's plan gets a one-line note that the normalizer exists; its acceptance
   criteria are unchanged and become regression tests for this function.

### Acceptance criteria

| Criterion | Verification |
| :--- | :--- |
| HTML fixtures render without tags and keep paragraph breaks | Unit test with real provider fixtures |
| Plain text is unchanged and normalization is idempotent | Unit test |
| The toggle reveals the stored markup unchanged | `egui_kittest` |
| Phase 3 can call the same function | Function is `pub` with its plan's tests |

**Implementation note (2026-11-05).** `core::text::plain_text(raw, max_chars)`
returns `PlainText { text, truncated, source_chars }` exactly as Phase 3
specifies. It parses the stored text with `scraper` (no regex), drops
`script`/`style`/`noscript`/`head`, turns block boundaries into line breaks,
renders list items as `"- "`, decodes entities through the parser, collapses
runs of spaces and blank lines to at most one, and truncates with
`chars().take(..)` so a multibyte character is never split. `source_chars` is
the input's character count.

The Pipeline computes the normalized text once per row when the view model is
built (`rows_from_jobs`), so the detail view never re-parses on repaint, and
records whether the stored text was markup (it contains `<` and normalization
changed it). The detail shows the formatted text; a **Raw text** checkbox appears
only when the row is markup, switches to the stored text in monospace, and shows
`Stored as HTML from the source.` The toggle resets when the selection
changes.

Tests: unit tests for entity decoding, block boundaries, list bullets, dropped
elements, whitespace collapsing, idempotence, and truncation on a character
boundary, plus a real Waymo JSON-LD description from the existing fixture;
view-model tests for the computed text and markup flag; `egui_kittest` for the
formatted view, the toggle, the source note, and the absence of a toggle on a
plain paste.

---

## Test Strategy

| Layer | Covers |
| :--- | :--- |
| Core unit | Canonical URLs, dedup matrix, never-clobber, cursor arithmetic, status transitions, search, `plain_text` |
| Core integration | Manual add of a known posting updates the portal row; refresh updates in place; delete cascades |
| Migration | `0003` applies to a 0002 database, backfills `canonical_url`, and keeps row ids |
| `egui_kittest` | Fetch more gating, status action, delete confirmation, search field, raw toggle |
| Live (ignored) | The finding-2 regression: fetch NVIDIA, then add one of its URLs manually; one row, with a description |

---

## Risks

| ID | Risk | Impact | Mitigation |
| :--- | :--- | :--- | :--- |
| P2.5-R1 | **Dedup changes merge postings a user wanted separate.** | Medium | The merge rules never cross URLs: same canonical URL plus the same identity is the same posting. Manual pastes keep their synthetic URL and stay separate. |
| P2.5-R2 | **Cursor drift.** A reordered board can cause a page to be skipped. | Medium | Dedup prevents duplicates; a skipped page is found by fetching again from an earlier page. The cursor is a convenience, not a promise of completeness. |
| P2.5-R3 | **"Fetch all" reappears as a feature request.** | Low | The cursor plus `max_pages` covers the need without the crawl; documented in the manual script. |
| P2.5-R4 | P2.5-R4 | **Delete is destructive.** | Medium | A fetched-only row deletes without ceremony; anything user-authored sits behind a typed phrase. No keyboard shortcut. |
| P2.5-R5 | **Status inflation.** A new status that nothing sets is noise. | Low | The manual action ships with it, and Phase 3 sets it from the evaluation. |

---

## Exit Criteria

1. Re-adding the URL of a portal-fetched posting updates that row (same id) and
   fills its description; no duplicate exists.
2. A later fetch cannot replace a non-empty description with an empty or strictly
   shorter teaser; an explicit refresh can.
3. **Fetch more** continues after the last page, advances the cursor, and stops at
   the end of the list. There is still no one-click "fetch all".
4. **Refresh description** updates a stored posting in place and never inserts.
5. `Not a match` exists, is settable from the detail view, has a filter, and is
   excluded from "Not applied".
6. A fetched-only job deletes without a confirmation; a job with an evaluation
   (or another user-authored state) requires typing `delete me`.
7. The Pipeline has a search field that composes with the filters and survives a
   reload.
8. The detail view shows plain text by default and offers the stored markup on
   demand; the stored text is unchanged.
9. Waymo's entry enriches every posting on the pages it fetched, and a failed
   detail keeps the teaser.
10. Migrations 0003 and 0004 apply to an existing database, the full suite and
    clippy are clean, and a retrospective is recorded.

---

## Open Questions

| ID | Question | Position |
| :--- | :--- | :--- |
| Q-18 | **Should there ever be a "fetch all"?** | No. **Fetch more** plus `max_pages` is bounded and resumable; a full NVIDIA walk is 269 requests against an unofficial API. If a user needs everything, they can raise the configured cap knowingly. |
| Q-19 | **Merge existing duplicates automatically?** | No. The new delete action removes the worse copy. An automatic merge would have to choose which text, status, and evaluations survive, and that is a product decision, not a cleanup script. |
| Q-20 | **Should Phase 3 set `Not a match` automatically from `Recommendation::NoMatch`?** | Yes, but the status stays visible and editable by hand; an evaluation is an opinion, not a verdict. |

---

## What Phase 2.5 Is Not

- Not a crawler or a scheduler. Every fetch remains a user action.
- Not a duplicate-merging tool.
- Not a status workflow editor; one status, one action.
- Not a job-detail renderer. Plain text plus raw text; no HTML layout.
- Not a change to the storage policy: `raw_text` remains what the source served.
