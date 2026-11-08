# Phase 3 Retrospective and Verification Record

**Phase:** AI Integration
**Parent plan:** Draft Execution Plan: Career-Ops Rust Desktop Application (v0.31), §12
**Phase plan:** `.agents/plans/phase_3_ai_integration.md` (v0.16)
**Period:** 2026-11-06 – 2026-11-07
**Estimate:** 3–4 weeks
**Actual:** seven workstreams across several sessions
**Status:** Code complete and offline-green. Manual script §6c (Prompts) and
§6d (Evaluation and cover letters) need the owner's run; the live fixture and
cover-letter checks need a key and spend tokens.

---

## Summary

Phase 3 makes the application intelligent without delegating to an external
CLI. Four things define it:

1. **One provider boundary.** `core::llm` is the only module that names `rig`.
   DeepSeek is reached through its OpenAI-compatible dialect; keys live in a
   dedicated keychain namespace; `status()` names the first reason a call cannot
   run.
2. **Prompts are data.** Two compact prompts (`evaluate`, `cover_letter`)
   replace the 27–93 KB agent originals as call input. They are seeded,
   read-only, versioned per owner, and editable only through a duplicate. Every
   report records the prompt version and hash that produced it.
3. **One deterministic pipeline.** Both flows normalize with
   `text::plain_text`, resolve through `active_for_use`, render strictly,
   budget the CV and job text, and store provenance. Evaluation adds a
   structured contract, a lifecycle, and a transaction; cover letters add
   append-only drafts.
4. **Untrusted input is fenced.** The posting is sent inside an explicit
   untrusted block whose own marker cannot be forged, the prompts state the
   rule, output is validated, and the client has no tools.

The UI follows: a **Prompts** screen for the version history, a three-tab
Pipeline detail for the report, history, and letters, and a score badge on the
row.

---

## Exit criteria status

| # | Criterion | Status | Evidence |
| :--- | :--- | :--- | :--- |
| 1 | DeepSeek integrated and selectable; structured result; abstraction ready for a second provider | ✅ | `the_default_is_deepseek_chat`, `a_deepseek_chat_model_takes_json_mode_and_tools`, `an_evaluation_stores_provenance_and_moves_discovered_to_ready`, the Settings AI section kittest, and `deepseek_accepts_the_configured_key` (live) |
| 2 | Keys in a dedicated keychain namespace, never the database | ✅ | `llm::keys` (write-only; `keychain`-gated), the Settings key round trip, and `the_llm_config_round_trips_through_settings` (no key in `settings`) |
| 3 | Compact prompts seeded on unlock, read-only, under their ceilings; originals stay as reference | ✅ | `seeding_creates_the_compact_prompts_and_is_idempotent`, `the_compact_prompts_are_single_shot_and_bounded`, the Prompts screen's read-only system row |
| 4 | User prompts created, edited, versioned, reverted; scoped by owner; `active_for_use` prefers the copy | ✅ | `prompt_manager_duplicates_edits_and_reverts`, `a_user_copy_is_the_one_in_use`, `resolution_falls_back_to_the_system_prompt`, `a_system_update_does_not_collide_with_a_user_version` |
| 5 | Every report records version, hash, model, input/source chars, truncation, score, recommendation | ✅ | `an_evaluation_stores_provenance_and_moves_discovered_to_ready`, `a_truncated_input_is_recorded_on_the_report` |
| 6 | Structured output validated; application ensured | ✅ | `the_evaluation_output_schema_is_stable`, `an_out_of_range_score_is_rejected_before_anything_is_written`, `a_reply_that_stays_unusable_is_an_error_with_no_report`, `each_fixture_scores_in_its_expected_band` |
| 7 | Text normalized deterministically; no markup; `input_chars` recorded | ✅ | `plain_text` unit tests, `the_job_description_is_normalized_before_the_prompt`, `each_fixture_scores_in_its_expected_band` (no `<p>`) |
| 8 | Streaming reaches the UI and can be cancelled | ✅ | `an_evaluation_streams_and_can_be_stopped`, `stopping_an_evaluation_restores_the_status_and_stores_nothing`, `a_pre_set_cancel_never_calls_the_provider_and_restores_the_status` |
| 9 | Cover letters generate as markdown with provenance | ✅ | `a_cover_letter_is_stored_with_provenance`, `regenerating_adds_a_row_and_preserves_the_first`, `the_cover_letter_cassette_stays_within_the_prompt_rules` |
| 10 | Injection defenses implemented and tested | ✅ (mechanics; no red-team corpus) | `a_hostile_posting_is_fenced_as_untrusted_data`, `a_posting_cannot_forge_the_end_marker`, the schema tests; `docs/prompt-injection.md` |
| 11 | `NoMatch` marks an eligible application; a submitted one is left alone | ✅ | `a_no_match_report_marks_a_ready_application_not_a_match`, `a_no_match_report_on_a_submitted_application_leaves_its_status_alone`, `a_match_report_on_a_submitted_application_leaves_its_status_alone` |
| 12 | Migration 0006 applies to an existing 0005 database and preserves rows | ✅ | `migration_0006_applies_to_a_0005_database_and_preserves_rows` |
| 13 | **Amended:** the prompt-quality suite runs in the default suite against cassettes | ✅ (amended) | `rig` 0.46 has no `evals` module, so `evaluation_fixtures.rs` replays hand-authored cassettes; `docs/llm-testing.md` records the substitution. The shipped cassettes are marked `synthetic` until recorded with a key. |
| 14 | CI green on Linux, macOS, and Windows | ⚠️ Not run here | The existing `ci.yml` already runs `cargo test --workspace --features career-ops-app/keychain` on the three OSes, which includes every offline Phase 3 test. No separate job was needed. The live checks stay out by `#[ignore]`. |
| 15 | A short Phase 3 retrospective | ✅ | This document |
| 16 | The report write sets `queue_state = 'evaluated'` in the same transaction; the badge updates without a reload | ✅ | `set_queue_state_tx_writes_inside_the_callers_transaction`, `an_evaluation_stores_provenance_and_moves_discovered_to_ready`, `evaluating_a_row_updates_the_queue_status_and_gate` |
| 17 | The lifecycle moves `discovered`/`ready` through `evaluating`, restores on failure, never rewrites a submitted application | ✅ | `a_backend_failure_restores_the_previous_status_and_stores_nothing`, the submitted-application tests, `finish_evaluation_tx_...` |

**Verification commands and results (2026-11-07):**

| Command | Result |
| :--- | :--- |
| `cargo fmt --all --check` | clean |
| `cargo clippy --workspace --all-targets --offline -- -D warnings` | clean |
| `cargo test --workspace --features career-ops-app/keychain --offline -- --skip obscura` | all passed; 169 app lib tests, 47 pipeline-screen tests, 21 evaluation, 12 cover-letter, 4 fixture, 5 prompts-screen, 3 prompt-manager |
| `cargo test -p career-ops-core --test llm_eval_live --offline` | 2 ignored, as intended |

---

## Decisions worth remembering

- **The provider lives behind one module.** Upgrading `rig` touches
  `core::llm` and nothing else. The capability table is ours, not the
  provider's, so a renamed model does not silently lose JSON mode.
- **JSON mode with local validation, not strict schemas.** DeepSeek has no
  schema-constrained output. The contract is enforced by our parser, with one
  repair call, and the schema is asserted by a test so the prompt and the struct
  cannot drift.
- **One transaction for the report.** The report, the application link and
  score, and the queue state are written together; the transaction-scoped repo
  helpers exist because the plain `set_queue_states` opens its own transaction.
- **The application lifecycle is visible.** `evaluating` exists so a slow call
  is not a frozen UI, and a failure restores the previous status rather than
  leaving a row in a state the user did not choose.
- **Cancellation is cooperative.** A `CancelToken` raced against the provider
  future restores the status and stores nothing; dropping the task would skip
  the restore.
- **Same-primitives reuse, not a second pipeline.** Cover letters use the
  evaluation's normalization, rendering, budgets, context check, and backend
  trait; `CompletionBackend` moved to `core::llm` once there were two callers.
- **The fence, not `rig` documents.** The prompt contract is a single templated
  string that user prompts rely on; `untrusted_block` gives explicit boundaries
  and neutralizes a forged marker. `docs/prompt-injection.md` records the
  trade-off and the residual risk.
- **Curation over volume.** Three fixtures with clear bands, one reference
  letter, and a small judge beat a large ambiguous set.

## Friction and deviations

- **`rig` 0.46 is newer than the plan assumed.** It requires rustc 1.97.1 (the
  workspace MSRV moved from 1.83) and duplicates `reqwest` (0.13 beside
  acquisition's 0.12) — plan risk P3-R16. It also has **no `evals` module**,
  which invalidated WS7's first deliverable; the fixture/cassette suite is the
  substitute.
- **The completion request is streamed, not structured.** WS3 parses the reply
  assembled by `stream.finish()` and uses JSON mode; the plan's `Extractor`
  suggestion does not apply to DeepSeek.
- **Cover letters are not streamed.** The UI shows a spinner and Stop; the
  request shape has no `stream` field. Add it when the letter tab wants live
  text.
- **`cover_letters.input_chars` is the assembled prompt size**, while the
  report's value is the job text; the 0006 column comments say so.
- **Hand-authored cassettes.** They are labelled `synthetic` and are a
  placeholder until a key-enabled recording run replaces them.

## Open follow-ups

1. **Record the cassettes** with a real key and review the diff, then remove
   `synthetic` from the docs' caveat. This is the only way the fixture bands
   become evidence about the model.
2. **Run the live checks** (`llm_eval_live`) and the owner's manual §6c/§6d.
3. **A red-team fixture corpus.** The current injection tests prove the
   mechanics, not resistance to a corpus of attacks; `docs/prompt-injection.md`
   says so.
4. **Cost display.** `input_chars` and `model_used` are recorded; a per-run cost
   estimate is not shown. Phase 3.2's triage introduces a cost line; reuse it.
5. **Prompt tooling stops at substitution.** No conditionals, loops, or A/B
   testing (P3-R9). Keep resisting that until a real need appears.
6. **CI confirmation.** Push and confirm the three-OS matrix; the offline suite
   is the only gate, and the live checks are excluded by `#[ignore]`.

## What Phase 3 Is Not (confirmed)

- No PDF rendering of CVs or letters (Phase 4).
- No acquisition changes; Phase 3 consumes `job_descriptions` as stored.
- No batch-evaluation UI; the headless `run_batch` exists for tests.
- No second provider; the boundary is ready, DeepSeek is the only
  constructible one.
- No local model.
