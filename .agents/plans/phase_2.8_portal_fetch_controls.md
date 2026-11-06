# Phase 2.8: Portal Fetch Controls — Detailed Execution Plan

**Parent plan:** Draft Execution Plan: Career-Ops Rust Desktop Application (v0.14), §12
**Status:** Complete — retrospective recorded in
`.agents/plans/phase_2.8_retrospective.md`. Manual script §4.3e and §4.4a–c are
written and await the owner's pass.
**Last updated:** 2026-11-06
**Duration:** 2–3 days
**Depends on:** Phase 2.5 (the page cursor, `fetch_portal_more`, `fetch_batch_cancellable`, `refresh_job`)
**Blocks:** Phase 2.9 (the per-portal detail toggle) and Phase 3 (the fetch semantics it assumes).

---

## Change Log

| Version | Change |
| :--- | :--- |
| v0.1 | Initial plan. Splits the current `Fetch` / `Fetch more` into three actions with explicit names, and makes detail enrichment a per-portal opt-in. The intent is to keep the app's fetch semantics legible as the triage agent (Phase 3.2) starts to reason about jobs the user has not looked at. No schema change. |
| v0.2 | Review decisions recorded. **Naming:** the per-portal actions are **Refresh** / **Continue** / **Fetch all pages**; the screen already has a header **Refresh** (reload the list) and **Fetch all** (every portal), so those become **Reload list** and **Fetch all portals**, and the new action type is `FetchAllPages` because `PortalsAction::FetchAll` already means the batch. Also corrects the variant name (`FetchOne`, not `Fetch`). **Cursor:** Refresh never touches `portals.last_page_fetched`; today a page-1 fetch rewinds a deeper cursor (40 → 5), so this is a behavior change, and the `end_of_list` reset moves to Continue exclusively. **Loop safety:** Fetch all pages is gated on the existing `can_fetch_more` and stops when a chunk makes no progress, because a portal without a cursor never reports `end_of_list`. **Progress:** cumulative pages/jobs after each chunk, since `fetch_portal_more` returns per chunk. **Toggle:** it applies only while the config JSON is unedited; hand-editing the field disables it and points at `detail.enabled`. |
| v0.3 | WS1 and WS2 implemented. Two refinements the code forced, both recorded here rather than left as drift. **Cursor:** Refresh is *monotonic*, not inert — it raises the cursor to the deepest page it read and never lowers it. Writing nothing at all would break the Phase 2.5 flow (Refresh pages 1–5, then Continue would re-read 1–5), while the rewind it fixes is the lowering half. Continue remains the only run that clears the cursor on `end_of_list`. **Eightfold default:** a board with no `detail` block still enriches, because that has always been its only source of descriptions; the toggle therefore shows *on* for a fresh Eightfold portal and writes `enabled: false` when turned off. Selector portals default to off, as designed. |
| v0.4 | WS3 implemented: manual script §4.3e (toggle, including the hand-edit case) and §4.4/§4.4a–c rewritten for the new labels, `docs/known-sites.md` gains the enrichment rules, the retrospective, and the parent-plan close-out. Phase complete; the manual pass is the owner's. |

---

## Purpose

The current fetch UI has two buttons:

- **Fetch** — pages 1..=`pagination.max_pages`, resets to page 1 every time, honors `detail.max_jobs` enrichment.
- **Fetch more** — continues from `portals.last_page_fetched` through `max_pages`, honors `detail.max_jobs`.

Both work, but the labels are doing too much work. A returning user reading "Fetch" expects "get me new postings" (which is page 1), and a user reading "Fetch more" expects "get me more pages" (which is continuation). Once triage (Phase 3.2) starts depending on the pipeline containing fresh and complete postings, the ambiguity becomes a problem for the user rather than a small labeling quirk.

This phase:

1. **Renames the two actions** to their actual behavior and adds a third: **Refresh**, **Continue**, **Fetch all pages**. The screen's existing header actions are renamed so no two buttons share a label.
2. **Makes detail enrichment per-portal opt-in.** `detail.max_jobs` continues to bound enrichment when it happens, but the fetch actions no longer request details automatically. The user chooses when, in Phase 2.9's queue and Phase 3's evaluation.
3. **Adds a bounded "Fetch all pages" loop**, answering Q-18 from Phase 2.5 and the D1 deferral in its retrospective.

The phase is deliberately small. It changes the app's fetch affordance and its enrichment default, but not the acquisition pipeline. Every change is a UI label, a service-level loop, or a config field read.

### Evidence

- `crates/app/src/screens/portals.rs` renders **Fetch** and **Fetch more** per row (`PortalsAction::FetchOne` / `FetchMore`); the second is gated on `can_fetch_more`. The header already has **Refresh** (reload the portal list) and **Fetch all** (`PortalsAction::FetchAll`, every portal).
- `router::fetch_portal` returns `FetchOutcome { jobs_fetched, tier, first_page, pages_fetched, pagination_stopped, next_page, end_of_list, details_attempted, details_enriched }` — the fields "Fetch all pages" needs are already present.
- `router::record_fetch_outcome` advances the cursor from `next_page` for a page-1 run too, so today a Refresh rewinds a deeper cursor (a portal at page 40 with `max_pages: 5` drops to 5).
- `core::acquisition::router` enriches details whenever `scraper_config.detail` is present; there is no per-portal toggle, only the JSON block.
- `portals::service::fetch_batch_cancellable` already demonstrates the shape a bounded loop should follow (per-portal progress, cancel between items, one entry per completion).

---

## Scope

| In scope | Out of scope |
| :--- | :--- |
| Rename **Fetch** → **Refresh** (from page 1) | Preference-aware filtering (Phase 2.9) |
| Rename **Fetch more** → **Continue** (from cursor) | Job queue states (Phase 2.9) |
| New **Fetch all pages** action with a bounded loop | LLM triage (Phase 3.2) |
| Rename the header **Refresh** → **Reload list** and **Fetch all** → **Fetch all portals** | A change to the header's reload or batch behavior |
| Per-portal **Fetch descriptions during fetch** toggle, default off | LLM-assisted portal suggestions (Phase 3.2) |
| Progress and cancel for **Fetch all pages** | Aggregator or web search (out of scope entirely) |
| Refresh never rewinds the continuation cursor | A change to list-fetch semantics, pagination caps, or the cursor itself |
| Tests, docs, retrospective | A change to the router's tiers or failure classification |

---

## Design decisions

### 1. Three per-portal actions, three clear meanings

| Button | What it does | Cursor effect |
| :--- | :--- | :--- |
| **Refresh** | Reads pages 1..=`pagination.max_pages`, updates existing rows, inserts new ones. | **Never lowered.** The cursor tracks the deepest page ever fetched: a page-one fetch raises it to the deepest page it read and leaves a deeper value alone. A returning user gets new top-of-list postings without losing their crawl position, and Continue still picks up where the first Refresh ended. |
| **Continue** | Reads pages `cursor + 1`..=`cursor + pagination.max_pages`, advances the cursor. Disabled when the portal cannot paginate (`can_fetch_more`); after `end_of_list` the cursor resets, so the next Continue starts from page one. | Advances to the highest page read; cleared to 0 on `end_of_list`. |
| **Fetch all pages** | Repeats Continue in-process until `end_of_list`, the user cancels, no progress is made, or the per-click budget is exhausted. | Advanced by each chunk through Continue; cleared to 0 on `end_of_list`, unchanged on cancel, no-progress, or budget hit. |

Two consequences worth stating: Refresh is the "get me new jobs" action a returning user expects, and does not disturb a crawl in progress. Fetch all pages is the bounded walk Q-18 rejected as a default — here it is an explicit, cancellable click with a visible budget.

**Refresh's cursor rule is a fix, not a rename.** `router::record_fetch_outcome`
currently derives the cursor from `next_page` whatever the run's start was, so a
page-1 fetch on a portal at cursor 40 with `max_pages: 5` writes `5` back: the
next Continue re-reads pages 6–10. `record_fetch_outcome` therefore takes the
start kind (`Beginning` / `Resume`): a `Beginning` run raises the cursor with
`portals::set_last_page_fetched_at_least` and never lowers it, and only a
`Resume` run may clear it on `end_of_list`. This is the one behavior change in
the phase, and it is the reason the phase is worth doing.

### 2. Naming: no two buttons share a label

The screen already has two header actions, and the obvious names for the new
row actions collide with both. The labels are therefore:

| Where | Labels | `PortalsAction` |
| :--- | :--- | :--- |
| Per portal row | **Refresh** · **Continue** · **Fetch all pages** | `FetchOne` (unchanged) · `FetchMore` (unchanged) · `FetchAllPages(id)` (new) |
| Header | **Reload list** (was **Refresh**) · **Fetch all portals** (was **Fetch all**) | `Reload` (unchanged) · `FetchAll` (unchanged, batch) |
| Running Fetch all pages | **Stop** under the row | `CancelFetchAllPages` (new) |

`PortalsAction::FetchAll` already exists and means "fetch every portal", so the
new variant is `FetchAllPages(i64)`; reusing the name would not compile and
would make the two flows indistinguishable at a call site. Renaming the header
labels is what keeps `get_by_role_and_label` unambiguous in the screen tests as
well as the UI honest: "Fetch all" for one board and "Fetch all" for every
board cannot both be right.

### 3. Fetch all pages has a per-click budget, not a total cap

The loop is bounded by a **per-click budget of 50 pages** (not per-portal `pagination.max_pages`, which stays as the depth of a normal click). Fifty pages on a 10-per-page board is 500 jobs; the actual NVIDIA board would be five clicks. This is deliberate:

- The user asked for a bounded action, not a crawl. Each click is a decision.
- The budget is visible in the button tooltip ("Fetch all pages (up to 50 pages)") and in the result message ("Fetched 500 jobs across 50 pages. There is more; Continue or Fetch all pages again.").
- A hard-coded ceiling means a board with 269 pages cannot be accidentally walked to completion in a single click by a user who did not read the tooltip.

The tooltip and the result message are the whole disclosure. No second confirmation dialog.

**The loop must also stop when it cannot make progress.** `end_of_list` is only
reported by providers that paginate: an ATS adapter or a selector config without
a `url_template` returns `pagination_stopped: None` and never advances the
cursor, so a loop that only watched `end_of_list` would re-fetch the same page 50
times. Two guards: the button is enabled only when the existing
`can_fetch_more(row)` says the portal can continue, and the service stops with
`NoProgress` when a chunk returns neither a new cursor nor `end_of_list`.

### 4. Detail enrichment is a per-portal toggle

`scraper_config.detail` continues to mean "how to extract a description"; a new
`scraper_config.detail.enabled` means "do it during fetch." The toggle appears in
the portal form, labeled **Fetch descriptions during fetch (slower, ~25 requests
per page)**.

Serialization: the form writes `enabled: false` explicitly when the toggle is
off and `enabled: true` when it is on, so a portal's intent is never ambiguous. A
serialized field that is **absent** means the config predates this phase and is
treated as `true` when a `detail` block exists — implemented as
`#[serde(default = "enabled_by_default")]`, not as an `Option`. That keeps Waymo
and Ubisoft (whose registry entries carry a `detail` block) enriching with no
edit, and keeps the registry entries unchanged.

The rule is keyed on the **presence of `detail`**, not on the portal's age: a new
user who pastes a config containing a `detail` block gets enrichment until they
see the toggle and turn it off. That is the intended reading of "existing configs
keep working", and the form shows the toggle's current state either way.

**The toggle applies only while the config is not hand-edited.** The form already
offers the config as editable JSON for selector portals. Two representations of
the same bytes would drift, so: while the JSON field still matches what the form
generated, the toggle drives it (rewriting the text); once the user edits the
JSON by hand, the toggle is disabled and the form says "This config was edited by
hand; set `detail.enabled` in the JSON." Choosing the toggle again after a hand
edit is not offered, because silently rewriting a hand-written config is worse
than asking the user to type one field. For providers the form builds itself
(Eightfold), the toggle always applies.

**Consequence for the pipeline.** Most new postings arrive with only a card
teaser. That is fine for Phase 3.2's triage (which wants titles and briefs
anyway), but Phase 3's evaluation needs full text. Phase 3 handles this with a
combined "fetch description then evaluate" action (WS3 item 11), so the user is
not stuck with "no description" on a job they want to evaluate.

### 5. Cancellation and progress reuse the Phase 2.5 pattern

`fetch_all_pages` is a service function in `portals::service` with the same shape
as `fetch_batch_cancellable`:

- A shared `AtomicBool` cancel flag, checked between chunks.
- A progress callback (`FnMut(&FetchAllProgress)`) invoked after each chunk with
  **cumulative** totals: `pages_fetched`, `jobs_stored`, `chunks_done`. The
  router reports pages per call, not per page, so the UI line reads "Page 10 of
  50 · 200 jobs · Stop" and updates once per chunk rather than lying about
  per-page timing.
- One `FetchAllPagesReport` at the end: `pages_fetched`, `jobs_stored`,
  `stopped_reason` (`EndOfList` | `Cancelled` | `BudgetExhausted` | `NoProgress`
  | `Error { .. }`).

The existing batch cancellation (`fetch_batch_cancellable`) is unchanged.
`fetch_all_pages` is a new per-portal operation, not a modification of the batch.

### 6. The button UI is a single row per portal

Three buttons is right at the boundary where a toolbar becomes clutter. They render as a compact row in each portal entry, with Fetch all pages slightly de-emphasized (outline style) to reflect that it is the more consequential action. The detail toggle is inside the advanced form, not on the row. While a run is in progress, the row's three buttons are replaced by the progress line and **Stop**.

### 7. No migration

`portals.last_page_fetched` already exists. `scraper_config` is JSON. Nothing changes at the schema level.

---

## Workstream 1 — Backend

**Estimated duration:** 1 day.

### Deliverables

1. `portals::service::fetch_all_pages(pool, portal_id, source, browser, cancel, progress) -> Result<FetchAllPagesReport>`. Loops `router::fetch_portal_more` until `end_of_list`, the cancel flag is set, a chunk makes no progress, or 50 pages have been read. Progress is reported after each chunk with cumulative totals; the function never writes the cursor itself.
2. `DetailConfig` (in `acquisition::html::detail`, shared by `SelectorConfig` and `EightfoldConfig`) gains `enabled: bool` with `#[serde(default = "enabled_by_default")]` returning `true`. `fetch_static_jobs`, `fetch_pages`, and `eightfold::fetch` honor it: `enabled` runs the existing enrichment pass; `enabled == false` or an absent `detail` skips it entirely. Existing `detail` blocks without the field keep enriching.
3. `record_fetch_outcome` takes the run's `FetchStart`. A `Beginning` run raises the cursor to the deepest page it read (`set_last_page_fetched_at_least`) and never lowers it; only a `Resume` run clears it on `end_of_list`. `fetch_portal` (Refresh) and `fetch_portal_more` (Continue) otherwise keep their current behavior.
4. Unit tests:
   - `fetch_all_pages` stops at `end_of_list`.
   - `fetch_all_pages` stops when the cancel flag is set.
   - `fetch_all_pages` stops at the per-click budget with `BudgetExhausted`.
   - `fetch_all_pages` stops with `NoProgress` for a portal that cannot paginate.
   - `fetch_all_pages` reports cumulative `pages_fetched` and `jobs_stored`.
   - Refresh never lowers `portals.last_page_fetched` (the regression this phase fixes).
   - Refresh still raises it when it reads deeper, so Refresh → Continue works.
   - A Refresh that reaches the list end does not clear the cursor.
   - Continue still advances the cursor and clears it on `end_of_list`.
   - A `detail` block without `enabled` still enriches (regression test for existing configs).
   - A `detail` block with `enabled: false` skips enrichment.
   - An Eightfold board with no `detail` block still enriches.
5. An integration test: seed a portal and a `MockFetchSource` producing 5 pages, run Refresh then Continue then Fetch all pages, and assert the cursor, row count, and enrichment calls follow the table in design decision 1.

### Acceptance criteria

| Criterion | Verification |
| :--- | :--- |
| `fetch_all_pages` reaches the end of the list and clears the cursor | Core test with a scripted source |
| Cancel stops between chunks | Core test with a pre-set flag |
| Budget stops at 50 pages | Core test |
| A portal that cannot paginate stops immediately instead of repeating | Core test (`NoProgress`) |
| Refresh never lowers `last_page_fetched`, and still raises it when it reads deeper | Core test |
| `detail.enabled` gates enrichment without breaking old configs | Unit + integration tests |

---

## Workstream 2 — UI

**Estimated duration:** 1 day.

### Deliverables

1. Replace the two row buttons with three: **Refresh**, **Continue** (disabled unless `can_fetch_more(row)`), **Fetch all pages** (same gate). Rename the header actions to **Reload list** and **Fetch all portals** so no two buttons share a label.
2. Per-portal progress on Fetch all pages: a one-line status under the row (`Page 10 of 50 · 200 jobs`) plus **Stop**, replacing the row's buttons while it runs.
3. The **detail toggle** in the portal form, labeled as in design decision 4, enabled only while the config JSON is unedited; once hand-edited, it is disabled with a note pointing at `detail.enabled`.
4. `PortalsAction` gains `FetchAllPages(i64)` and `CancelFetchAllPages`; `FetchOne` and `FetchMore` keep their names and gain the new labels. `FetchAll` keeps meaning "every portal".
5. `egui_kittest`:
   - Three row buttons render with their distinct labels, and the two header labels are unchanged in meaning.
   - Continue and Fetch all pages are disabled for a portal that cannot paginate (`can_fetch_more` false) and enabled for one that can.
   - Stop cancels a running Fetch all pages.
   - The detail toggle is present and off for a fresh selector portal.
   - The detail toggle is on for an old portal whose config has a `detail` block and no `enabled`.
   - Editing the config JSON by hand disables the toggle and shows the note.

### Acceptance criteria

| Criterion | Verification |
| :--- | :--- |
| Three row actions are present per portal with distinct labels, and no header label collides | `egui_kittest` |
| Continue and Fetch all pages are gated on `can_fetch_more` | `egui_kittest` |
| Stop cancels a running Fetch all pages | App test |
| The detail toggle defaults off for a new portal, stays on for an old config, and disables itself after a hand edit | `egui_kittest` |

---

## Workstream 3 — Docs, verification, retrospective

**Estimated duration:** 0.5 day.

1. Manual script §4.4: replace the Fetch/Fetch more rows with Refresh/Continue/Fetch all pages, note the header renames, and add a detail-toggle step (including the hand-edit case).
2. `docs/known-sites.md`: note that Waymo and Ubisoft keep their existing enrichment because their configs already have a `detail` block and no `enabled` field; new entries default to off, and the registry entries are not rewritten.
3. Full validation: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --offline -- -D warnings`, `cargo test --workspace --features career-ops-app/keychain --offline -- --skip obscura`.
4. Parent plan: v0.14 changelog and a Phase 2.8 row.
5. Short retrospective in `.agents/plans/phase_2.8_retrospective.md`.

---

## Test Strategy

| Layer | Covers |
| :--- | :--- |
| Core unit | `fetch_all_pages` stop conditions (end, cancel, budget, no progress); `DetailConfig.enabled` gating; cursor invariance for Refresh |
| Core integration | Three actions in sequence against a scripted source |
| `egui_kittest` | Three row buttons, `can_fetch_more` gating, Stop, detail toggle, hand-edit case |
| App | Cancel and progress flow through `career_ops_app` |
| Manual | §4.4 on a real portal |

No network for any new test. No new dependency. No migration.

---

## Risks

| ID | Risk | Impact | Mitigation |
| :--- | :--- | :--- | :--- |
| P2.8-R1 | **Detail opt-in reduces evaluation quality.** The pipeline carries thin rows by default. | Medium | Phase 3's combined fetch-then-evaluate action (WS3 item 11) and Phase 3.2's triage (which wants titles anyway). |
| P2.8-R2 | **Fetch all pages runs into a WAF.** A 50-page walk is many requests. | Medium | Bounded per click, cancellable, honest result message. The WAF already skips and reports; no new evasion. |
| P2.8-R3 | **Refresh unexpectedly skips new jobs.** Some boards reorder on each fetch. | Low | Refresh reads from page 1 every time; the new-postings case is exactly what it is for. |
| P2.8-R4 | **Old `detail` blocks silently start enriching.** | Low | Backward-compatible default is `true` when `detail` exists; a regression test pins it. |
| P2.8-R5 | **Refresh's cursor rule changes existing behavior.** A deep crawl is no longer rewound by a page-1 fetch. | Medium | It is the bug being fixed, not a side effect: the write is monotonic, tests pin both halves (raise, never lower), and only Continue clears the cursor. |
| P2.8-R6 | **Two "Fetch all" meanings confuse the user.** The header batch and the per-row walk. | Low | Labels are distinct ("Fetch all portals" vs "Fetch all pages") and the row action is the one with a budget and a Stop. |

---

## Exit Criteria

1. **Refresh**, **Continue**, and **Fetch all pages** are present, distinct, and behave per design decision 1; the header actions read **Reload list** and **Fetch all portals**.
2. **Fetch all pages** stops at end-of-list, cancel, no progress, or 50 pages, and reports which.
3. **Fetch all pages** is cancellable and shows cumulative per-chunk progress.
4. Refresh never lowers `portals.last_page_fetched`; Continue is the only run that clears it.
5. The per-portal detail toggle exists, defaults off for new configs, is disabled after a hand edit, and is honored by all fetch paths.
6. Existing `detail` blocks (Waymo, Ubisoft) continue to enrich without changes.
7. No schema change and no new dependency.
8. Offline suite, fmt, and clippy are clean.
9. Manual script §4.4, `docs/known-sites.md`, parent plan, and a short retrospective are updated.

---

## Open Questions

| # | Question | Position |
| :--- | :--- | :--- |
| Q-33 | **Should the 50-page Fetch all pages budget be user-configurable?** | Not in this phase. A visible constant is a policy the user can reason about; a per-portal setting is a knob the user would have to discover. Revisit if the fixed budget turns out to be wrong for a real board. |
| Q-34 | **Should Refresh also enrich details for new postings?** | No. Refresh is the "cheap check for new work" action. Enrichment is Phase 2.9's queue action or Phase 3's combined fetch-then-evaluate. |
| Q-43 | **Should the header's "Fetch all portals" also move to the three-action vocabulary?** | Not now. The batch is still one click over every portal; the row actions describe how deeply one board is read. If the batch grows a budget or per-portal progress, it should be revisited together with this naming. |

---

## What Phase 2.8 Is Not

- Not a triage phase (Phase 3.2).
- Not a preference or queue layer (Phase 2.9).
- Not a change to list-fetch semantics: the tiers, the caps, and the failure classification are untouched. The one acquisition change is the cursor rule (Refresh stops lowering it), which is the bug this phase exists to fix.
- Not a change to the pipeline limit.
- Not a UI redesign of the Portals screen beyond the fetch row and the two header labels.