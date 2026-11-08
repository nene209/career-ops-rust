# Phase 3.1 Retrospective and Verification Record

**Phase:** LLM Usage Tracking
**Parent plan:** Draft Execution Plan: Career-Ops Rust Desktop Application (v0.35), §12
**Phase plan:** `.agents/plans/phase_3.1_llm_usage_tracking.md` (v0.4)
**Period:** 2026-11-07
**Estimate:** 3–4 days
**Actual:** three workstreams in one session, plus the small WS0
**Status:** Code complete and offline-green. Manual script §6e needs the owner's
run; every step that spends tokens is marked 🌐.

---

## Summary

Phase 3 made provider calls possible but left their cost invisible: reports and
letters stored character counts, and `rig`'s reported token counters were read
and discarded. Phase 3.1 adds the missing record:

1. **A ledger, not a statistic.** One row per provider call, including the
   repair retry, failures, and cancellations. The row carries the task, the
   posting/application, the counters, the cost, the outcome, and the duration.
2. **Reported or estimated, never silently one or the other.** DeepSeek reports
   real counters on both the unary and the streamed path; anything else is a
   four-characters-per-token estimate and says so.
3. **Cost only from a known price.** The provider's own figure wins; otherwise
   `rig`'s built-in catalog prices the reported counters; otherwise the cost is
   `NULL`. The screen draws `est.`, `≥`, or an em dash — never `$0.00`.
4. **A read-only Usage screen.** Totals by provider/model and by task, a range
   selector, and the last 50 calls joined to their posting.
5. **`deepseek-flash` as the default.** The old alias is not in the price
   catalog, so it could be counted but never costed.

---

## Exit criteria status

| # | Criterion | Status | Evidence |
| :--- | :--- | :--- | :--- |
| 1 | `llm_usage` exists; every provider call writes exactly one row | ✅ | `an_evaluation_records_the_provider_reported_counters`, `a_repair_writes_a_second_row`, `a_cover_letter_and_its_usage_commit_together`, `migration_0007_applies_to_a_0006_database_and_preserves_rows` |
| 2 | Reported usage stored verbatim; a missing one estimated and flagged | ✅ | `an_evaluation_records_the_provider_reported_counters`, `an_unreported_usage_is_estimated_from_the_prompt_and_reply`, `an_unreported_usage_is_unknown` |
| 3 | Cost `NULL` when unknown; a partial price is a lower bound | ✅ | `an_unknown_model_keeps_the_counters_and_leaves_the_cost_empty`, `a_missing_cost_is_counted_but_never_summed_as_zero`, `a_partial_cost_is_a_lower_bound`, `an_unpriced_group_is_not_drawn_as_zero` |
| 4 | Evaluation writes usage in the report transaction; failure/cancel recorded, original error returned | ✅ | `an_evaluation_records_the_provider_reported_counters`, `a_failed_call_is_recorded_with_its_kind`, `a_cancelled_call_is_recorded_as_such`, `a_backend_failure_restores_the_previous_status_and_stores_nothing` |
| 5 | Cover letter writes the letter and its usage together | ✅ | `a_cover_letter_and_its_usage_commit_together`, `a_failed_cover_letter_is_recorded_and_stores_no_letter` |
| 6 | Deleting a job leaves its usage rows | ✅ | `deleting_a_job_keeps_the_record_of_what_it_cost`, `deleting_a_job_keeps_its_usage_rows` |
| 7 | Usage is in the sidebar and shows totals, tasks, range, recent calls, empty state | ✅ | the eleven `usage_screen` tests; `app::tests::the_usage_screen_loads_the_ledger_and_keeps_the_range` |
| 8 | A fresh vault defaults to `deepseek-flash`; a stored `deepseek-chat` is preserved | ✅ | `the_default_is_deepseek_flash`, `saving_normalizes_an_empty_model`, `the_llm_config_round_trips_through_settings` |
| 9 | Migration 0007 applies to a real 0006 database and preserves rows | ✅ | `migration_0007_applies_to_a_0006_database_and_preserves_rows` |
| 10 | Offline suite, fmt, and clippy are clean | ✅ | the commands below |
| 11 | Manual §6e, `docs/usage.md`, parent plan, retrospective | ✅ (manual pending) | `docs/manual-test-script.md` §6e, `docs/usage.md`, this document |

**Verification commands and results (2026-11-07):**

| Command | Result |
| :--- | :--- |
| `cargo fmt --all --check` | clean |
| `cargo clippy --workspace --all-targets --offline -- -D warnings` | clean |
| `cargo test --workspace --features career-ops-app/keychain --offline -- --skip obscura` | all passed; 181 app lib tests, 11 usage-screen, 8 usage capture, 8 usage repository/migration, 21 evaluation, 12 cover-letter, 4 fixture |
| `cargo test -p career-ops-core --test llm_eval_live --offline` | 2 ignored, as intended |

---

## Decisions worth remembering

- **The ledger is a statement, not a child of the job.** No foreign key and no
  cascade: deleting a posting must not delete the record of what it cost. Both
  tests enforce it.
- **Usage travels with the text.** `CompletionBackend` returns
  `Completion { text, usage }`. A side channel would let a caller forget it or a
  fake omit it; the compiler now finds every call site.
- **The provider's cost wins over the catalog.** A gateway that bills in dollars
  is more authoritative than a list price. The catalog is only a fallback, and a
  model it does not list is unpriced, not free.
- **The pricing lives inside `core::llm`.** `rig`'s catalog is a private detail
  like the client itself; no other module learns that `rig` has a catalog.
- **The ledger is written twice, deliberately.** Success rows share the
  report/letter transaction, so a stored report always has its cost; failure
  rows are best-effort, so a ledger write can never replace the error the user
  needs to see.
- **The screen is a report, not a dashboard.** No charts, no auto-refresh, a
  fixed 50-row recent list. The ledger is the record; the screen is a glance.
- **The load is tagged with the range it answered.** A slow 30-day query cannot
  overwrite a 7-day selection. This is the third time a single-flight load needed
  a tag (Prompts, Pipeline detail, now Usage), which is a pattern worth naming
  in the conventions.

## Friction and deviations

- **`Call` omits the subject ids.** The plan put them on the call; the code puts
  them in `NewLlmUsage::from_call`, so the ledger stays a repository concern and
  `llm::usage` stays pure.
- **Two counters were dropped.** `tool_use_prompt_tokens` and
  `cache_creation_input_tokens` have no column because DeepSeek reports neither;
  a column that is always `NULL` is noise. They remain in `rig`'s struct if a
  later provider needs them.
- **`RecentCall` gained `error_kind`.** The screen needs to say *why* a call
  failed, not only that it did.
- **The empty cover letter is recorded as `ok`.** The provider answered; the
  pipeline refused the empty result. The outcome describes the call, not the
  letter.
- **`rig`'s `Usage` is `#[non_exhaustive]`.** A struct literal does not compile
  outside the crate; the tests build with `Usage::new().input_tokens(…)`, and
  that is the only shape a future test should use.
- **Clippy's `is_multiple_of`.** The digit-grouping helper needed the method form
  under `-D warnings`; a reminder that the workspace's clippy is the style
  authority, not habit.
- **kittest duplicate labels.** The model name and the task name appear in two
  grids each, so the screen tests assert on values unique to one grid (the
  token columns) rather than on the names.

## Open follow-ups

1. **Run manual §6e.** The one thing the offline suite cannot prove is that
   DeepSeek still reports usage through the streamed request. If a step shows
   `estimated`, record it in `docs/usage.md` rather than changing the ledger.
2. **Q-46: a user-maintained price table.** The catalog is a list-price estimate.
   Revisit only if a real invoice disagrees.
3. **Q-47: pruning and export.** The ledger is small now; the Phase 5
   backup/export path should include it, and deletion should be deliberate.
4. **Q-49: a per-job usage line.** The recent-calls list names the job. If the
   Pipeline detail wants "what did this posting cost", it is a small read, not a
   second screen.
5. **Phase 3.2 writes `task = 'triage'`** through the same seam, with no
   migration. The `Task` enum already names it.

## What Phase 3.1 Is Not (confirmed)

- No budgets, quotas, or a hard stop on spend.
- No billing reconciliation against the provider's balance endpoint.
- No per-span tracing; `task` plus the subject is the association. Option C of
  Phase 2.8.1 (Q-44) remains the place for spans.
- No charts or a per-day bar; no auto-refresh.
- No change to evaluation or cover-letter behavior beyond keeping the usage.
- No second provider; only DeepSeek is constructible and priced.
