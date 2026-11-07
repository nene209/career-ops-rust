# Phase 2.9 Retrospective and Verification Record

**Phase:** Acquisition Preferences and Job Queue
**Parent plan:** Draft Execution Plan: Career-Ops Rust Desktop Application (v0.22), §12
**Phase plan:** `.agents/plans/phase_2.9_acquisition_preferences_and_job_queue.md` (v0.7)
**Period:** 2026-11-06
**Estimate:** 3–4 days
**Actual:** five workstreams in one session, plus a manual-testing pass still to come
**Status:** Code complete. Manual script §6b is written and **not yet verified** by
the owner; everything else — preferences, queue, batch — is covered offline.

---

## Summary

Phase 2.8 made fetching legible. Phase 2.9 makes the result answerable. Two
questions a 200-row Pipeline raises had no answer before this phase:

1. **"Which of these do I care about?"** — a preference layer (preferred
   companies, locations, keywords, exclusions, seniority) in the
   `profile.preferences` column that has existed since Phase 1 and was unused.
   Preferences **badge**; only an exclusion **hides**, and the `Excluded` filter
   shows what it hid.
2. **"Do I have to look at every one?"** — a five-value queue state on
   `job_descriptions`, three manual actions in the detail view, and a bounded
   batch that fetches the full text for everything flagged for details.

The two are deliberately different in character: preferences are a display hint
that can be wrong for free, and the queue is a decision that is written down and
that Phase 3 and Phase 3.2 read.

---

## Exit criteria status

| # | Criterion | Status | Evidence |
| :--- | :--- | :--- | :--- |
| 1 | Preferences save and reload; the matching rules are covered by the unit test table | ✅ | `preferences_round_trip_through_the_profile_row`, `saving_an_empty_object_clears_every_list`, `a_malformed_stored_value_reads_as_empty_and_is_left_alone`, plus the `core::preferences` match-table unit tests |
| 2 | The Pipeline badges preferred rows and filters by Preferred; `All` hides excluded companies and `Excluded` shows them | ✅ | `rows_are_classified_by_the_preferences`, `all_hides_exclusions_and_only_the_excluded_filter_shows_them`, `a_preferred_row_shows_the_badge_and_others_do_not`, `the_excluded_filter_is_the_only_view_that_shows_an_excluded_row` |
| 3 | `job_descriptions` has a queue state with the five values, populated by the detail view's manual actions | ✅ | `every_queue_state_round_trips_through_the_repository`, `the_queue_actions_emit_their_states`, `queue_actions_update_the_row_and_the_database` |
| 4 | The queue-state badge renders next to the status badge | ✅ | `the_queue_badge_renders_beside_the_status_badge`, `a_new_row_has_no_queue_badge` |
| 5 | **Fetch descriptions (N)** runs a bounded, cancellable batch, advances successful rows to `evaluate`, and reports failures | ✅ | `the_batch_is_bounded_per_click`, `the_cancel_flag_stops_between_postings`, `a_failure_leaves_the_row_marked_for_details`, `fetch_descriptions_updates_the_rows_and_advances_the_queue`, `description_batch_summary_names_what_is_left` |
| 6 | Migration 0005 applies to a 0004 database and preserves rows | ✅ | `migration_0005_applies_to_a_0004_database_and_preserves_rows` |
| 7 | No new dependency and no change to acquisition semantics | ✅ | `Cargo.toml` unchanged; the router, tiers, and failure classification untouched. `fetch_descriptions_batch` calls the existing `manual::refresh_job` |
| 8 | Offline suite, fmt, and clippy are clean | ✅ | See the command table below |
| 9 | Manual script §6b, parent plan, and retrospective are updated | ✅ | §6b written (eight steps), §6.2 and §6.3 extended, `main.md` v0.22, Phase 3.2's entry conditions, this document. §6b is not yet exercised. |

**Verification commands and results (2026-11-06):**

| Command | Result |
| :--- | :--- |
| `cargo fmt --all --check` | clean |
| `cargo clippy --workspace --all-targets --offline -- -D warnings` | clean |
| `cargo test --workspace --features career-ops-app/keychain --offline -- --skip obscura` | all passed; 41 pipeline-screen tests, 6 job-queue tests, 5 description-batch tests |

No live test was added: the batch reuses `refresh_job`, whose live path
`docs/LIVE_TESTS.md` already covers.

---

## What the implementation found

### F1 — A preference field with no column behind it (design correction)

The plan said preferred **locations** would match a "location field". There is no
location column: 0001 created `job_descriptions` without one. Matching only a
field that does not exist would have shipped a settings box that silently never
matched. A location preference now checks the description text as well. That is
safe precisely because a preference is a badge: a false positive costs a
highlight, not a hidden row. When a location is extracted, the text fallback can
be retired. Recorded as plan v0.5.

### F2 — Two hiding decisions need two escape hatches (bug found by a test)

`JobFilter::matches` rejected an excluded row first, for every filter, and only
then looked at the queue. A row that was both excluded **and** dismissed was
therefore invisible in `Dismissed` — the one view that exists to show a
dismissal. Each decision now answers for its own view first, and the shared
"hidden" rule applies to the remaining filters, so the row is findable in either
escape hatch and nowhere else. `a_dismissed_row_that_is_also_excluded_is_findable_under_both_filters`
pins it.

### F3 — The migration test had to build the old schema itself (test design)

`create_and_migrate` only knows the newest schema, so a "0005 applies to 0004"
test cannot use it. The test replays migrations 0001–0004 from `include_str!`,
inserts a pre-queue row, applies 0005, and asserts the row survives with
`queue_state = 'new'`. That is more literal than using the migrator, and it is
the only shape that actually tests the transition.

### F4 — The view model must not trust the database (forward compatibility)

`queue_state` is free text so Phase 3.2 can add a value without a migration.
`QueueState::parse` returns `None` for a value this build does not know, and the
view model reads that as `new`. An older build therefore renders a newer build's
rows instead of failing to open the Pipeline.

### F5 — Deleting and dismissing are different, and stay ungated

The existing typed delete gate is about work that cannot be re-fetched: an
evaluation, a cover letter, a saved status. A queue state is none of those — it
is cheap and reversible — so dismissing does **not** trigger the gate. A
dismissed row is still a real row: deleting it is a separate action with its own
confirmation.

### F6 — A batch that updates text exposed a stale-plain-text bug (found in WS4)

The per-job **Refresh description** set `row.raw_text` and left `row.plain_text`
alone, so the detail view kept showing the old text until the next reload. A
batch makes that visible immediately (the whole point is to replace a teaser),
so the derivation moved into `PipelineViewModel::set_description`, which is now
the one place that turns stored text into plain text plus a markup flag. Both
paths use it, and `set_description_rederives_the_plain_text_and_the_markup_flag`
pins the derivation.

### F7 — The batch entry had to carry text, not a count (deviation)

The plan's entry was `Ok(bytes_added)`. The Pipeline shows the description it
just fetched, so a count would have forced a reload or left the panel stale. The
entry carries the stored text; the summary counts entries. Recorded as plan
v0.7.

### F8 — A batch's progress is a cell, not a task result (pattern reuse)

The portal walk's pattern — an `Arc<Mutex<Option<Progress>>>` the UI reads each
frame plus an `AtomicBool` the task checks between items — transferred without
change. The alternative (returning progress through the task channel) would only
report after the batch finished, which is the one thing a progress line must not
do.

### F9 — The header count belongs to the page, not the filter (scope)

`Fetch descriptions (N)` counts every `details_needed` row in the loaded page,
not the rows visible under the current filter or search. The batch is a queue
operation: filtering the list to 2 rows must not silently shrink the work the
button reports, or a click would do something the label did not say.

### F10 — Pacing was already there (risk that did not materialize)

P2.9-R3 worried about a 20-request batch tripping a WAF. `acquisition::http`
already spaces requests per host (400 ms plus up to 400 ms jitter, process-wide),
so the batch inherits the protection without adding a second delay. Verified by
reading the spacer rather than by measuring, which is why the batch stays
sequential and has no pacing of its own.

---

## Estimate accuracy

| Workstream | Plan | Actual |
| :--- | :--- | :--- |
| WS1 preferences model, storage, Settings screen | 1 day | Part of 2026-11-06 |
| WS2 preference-aware Pipeline | 0.5 day | Part of 2026-11-06 |
| WS3 queue state, migration, detail actions, badge | 1 day | Part of 2026-11-06 |
| WS4 bounded description batch | 0.5 day | Part of 2026-11-06 |
| WS5 docs, manual script, retrospective | 0.5 day | Part of 2026-11-06 |
| **Total** | **3–4 days** | **One session per workstream; manual §6b outstanding** |

The three corrections (F1, F2, F6) were each found by a test written in the same
workstream, at no extra cost.

---

## Deferred, unchanged

- **Q-35** Excluded companies are not excluded from fetch. Fetch is the
  acquisition path; an exclusion is a view decision the user can reverse.
- **Q-36** The queue state is not exported yet. Phase 6's export story decides.
- **Q-37** **Fetch descriptions** does not evaluate. Phase 3.2 may combine the
  two for a supervised batch; Phase 2.9 keeps them apart so cost is a click.
- **Location extraction** — a real `location` column would retire the
  description-text fallback in `PreferenceFacts`.
- **A queue-state filter in the list** — the queue is visible as a badge and
  reachable through `Dismissed`; a per-state chip row would not fit beside the
  eight existing chips.

Phase 3 is next. Its entry conditions are met: the queue exists, evaluation
writes `queue_state = 'evaluated'` in the same transaction as the report, and
`docs/LIVE_TESTS.md` is unchanged because no new network path was added.
