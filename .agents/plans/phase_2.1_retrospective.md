# Phase 2.1 Retrospective and Verification Record

**Phase:** UI Shell
**Parent plan:** Draft Execution Plan: Career-Ops Rust Desktop Application (v0.5), §12
**Phase plan:** `.agents/plans/phase_2.1_ui_shell.md` (v0.8)
**Period:** 2026-10-24 → 2026-10-25
**Estimate:** 1.5–2 weeks
**Actual:** two calendar days of implementation plus one verification pass
**Status:** Complete against all eighteen exit criteria, with two items that need a
human with a network (the 🌐 rows in `docs/manual-test-script.md`) and one item
(Criterion 16) that cannot be observed from an offline workstation.

---

## Summary

Phase 2.1 is complete. The application runs: it opens to the right lock screen,
unlocks, renders the shell, lists and manages portals, lists and filters jobs,
and accepts manual job entry from either screen. The view layer was written once
against egui 0.37, and the framework-independent layers written for GPUI —
`state.rs`, `navigation.rs`, `async_bridge.rs`, and the view models — were not
rewritten.

The phase was **much faster than estimated**, and the estimate was wrong for an
instructive reason: it priced the work as a human week, but most of the
uncertainty had already been bought down in Phase 2.01. The screens are small
(1,765 lines including inline tests); the effort went into making them honest —
tests that fail when the behaviour is wrong, a modal that has exactly one
implementation, and a load path whose results cannot land out of order.

The verification pass found four gaps. Three were real (untested view-model
functions, no end-to-end recovery test, a missing manual script); one was
documentation drift. All four are closed in this phase, and each is recorded
below because "the test count went up" is not the same as "the criterion is
met".

---

## Exit criteria status

| # | Criterion | Status | Evidence |
| :--- | :--- | :--- | :--- |
| 1 | Opens to unlock / first-launch / recovery as appropriate | ✅ | `state::tests::{no_files_goes_to_first_launch, db_without_wrapped_goes_to_recovery, wrapped_without_db_goes_to_recovery}`; `state_flow::first_launch_then_unlock_then_ready` (restart → `Unlocking`); `wrong_passphrase_reaches_recovery_not_ready` |
| 2 | `cargo run -p career-ops-app` works on a clean checkout with no environment variables | ✅ | Ran with no env vars against the default data directory: process launched, stayed alive for the full observation window, and logged no Rust panic (only macOS `LSNotification`/`hiservices` noise). `data_dir()` resolution is by inspection; see F5. |
| 3 | Unlock, first launch, and recovery resolution all reach the main layout | ✅ | `unlock_through_the_app_reaches_ready`, `complete_first_launch_reaches_ready_through_the_app`, and the new `recovery_restores_a_key_file_and_reaches_ready` (Recovery → `LocateKeyFile` → `Unlocking` → passphrase → `Ready`) |
| 4 | Sidebar navigation works between the placeholder screens | ✅ (reworded) | `ui_shell::sidebar_navigation_switches_the_rendered_screen`. The navigation model has **five** screens; the GPUI-era "Dashboard" was folded into Pipeline. Criterion reworded in plan v0.8. |
| 5 | Portals lists portals with health, last attempt, suggested action | ✅ | `portals_screen::renders_rows_health_and_sidecar_status`; `from_entries_maps_the_four_health_states`; core `list_with_health_returns_one_entry_per_portal_with_correct_states` |
| 6 | Portals add, edit, delete, fetch one, batch fetch with progress | ✅ automated for CRUD; 🌐 for the fetch halves | `app::portals_crud_and_paste_through_the_app`; `portals_screen::row_fetch_returns_the_portal_id`; core `fetch_one_returns_*`, `fetch_batch_*`. Real fetch needs network → manual script §4. |
| 7 | Batch fetch can be cancelled | ✅ | Core `fetch_batch_cancellable_stops_between_portals`; app `cancel_batch_sets_the_shared_flag_and_marks_progress`; screen `running_batch_offers_cancel` |
| 8 | Pipeline lists jobs with tier labels and timestamps, filters by status | ✅ | `pipeline_screen::{renders_rows_with_tier_and_status, filter_buttons_emit_the_selected_filter, a_filter_with_no_matches_explains_itself}`; `filters_partition_the_statuses` |
| 9 | Job detail renders the raw description with disabled placeholders | ✅ | `pipeline_screen::selecting_a_row_shows_the_detail_and_placeholder_buttons` (asserts both buttons are disabled) |
| 10 | Manual entry works from both Portals and Pipeline, URL and paste | ✅ for paste; 🌐 for URL | Paste: `portals_crud_and_paste_through_the_app`. Pipeline entry point and shared modal: `pipeline_add_job_opens_the_shared_manual_modal`, `pipeline_joins_statuses_and_manual_add_invalidates_it`. URL fetch: core `manual_entry` suite offline (mock source) + manual script §5.2. |
| 11 | Sidecar status reflects the last Tier 2 outcome | ✅ automated; 🌐 for real Tier 2 | `apply_fetch_result_updates_the_row_and_marks_a_tier2_sidecar_running`; `apply_fetch_result_marks_a_browser_failure_as_sidecar_stopped`; rendered by `renders_rows_health_and_sidecar_status`. Manual script §4.6. |
| 12 | Every view-model function has a unit test | ✅ after F1 | Added `replace_entries_preserves_the_sidecar_and_batch_progress`, `row_finds_a_row_by_id` (both view models), `health_badge_cause_is_only_present_for_failures`, `tier_labels_are_unique_and_non_empty` — five new tests in total |
| 13 | `patterns.md` documents list, form, and async patterns incl. `request_repaint_after` | ✅ | `crates/app/docs/patterns.md`: screen structure, list, status, form, async, idle CPU, Portals orchestration, Pipeline, shared modal |
| 14 | `docs/manual-test-script.md` documents the flows and was executed on a clean machine | ⚠️ document written, execution pending | `docs/manual-test-script.md` (new). Sections 1–3, 5.4–5.7, 6, 7 can be run offline; the 🌐 rows need a human with network and a real job board. |
| 15 | `labelled_input` produces an accessible label, or the limitation is recorded | ✅ resolved, not deferred | `Response::labelled_by` exists at egui 0.37. `ui_shell::labelled_input_is_queryable_by_its_label` asserts the field is findable by label and that `unnamed_widgets()` is empty. |
| 16 | CI is green on all platforms | ⚠️ not observable offline | `.github/workflows/ci.yml` runs the workspace suite on ubuntu/macos/windows, the no-keychain build, keychain failure injection, the keychain sentinel, fmt + clippy, and the service-layer jobs. Local equivalents pass; the remote result must be read from the CI run, not from this memo. |
| 17 | Verification pass complete, every criterion checked against the code | ✅ | This table. Each row names the test or the reason the check is weaker than it looks. |
| 18 | Retrospective memo records P2-R9, egui findings, async bridge, estimate accuracy | ✅ | This document, sections below. |

**Verification commands and results (2026-10-25):**

| Command | Result |
| :--- | :--- |
| `cargo fmt --all --check` | clean |
| `cargo clippy --workspace --all-targets --offline -- -D warnings` | clean |
| `cargo test -p career-ops-app --offline --features keychain` | 55 lib + 9 pipeline + 10 portals + 5 lock + 5 state-flow + 4 shell = **88 passed** |
| `cargo test --workspace --features career-ops-app/keychain --offline -- --skip obscura` | all passed (`obscura` skipped: the sandbox forbids binding a port) |
| `cargo test -p career-ops-core --no-default-features --offline -- --skip obscura` | all passed |
| `cargo run -p career-ops-app --offline` (no env vars) | launched, no panic, killed after the observation window |

88 app tests, 28 of them `egui_kittest` harness tests. The rest are view-model,
state-machine, recovery, and app-orchestration tests.

---

## What the verification pass found

### F1 — Five small functions had no unit test (low severity, real)

Criterion 12 was false as written. `PortalsViewModel::replace_entries`,
`PortalsViewModel::row`, `PipelineViewModel::row`, `HealthBadge::cause`, and
`TierLabel::label` were exercised only indirectly or not at all.

`replace_entries` was the one that mattered. It preserves `sidecar` and `batch`
across a reload — exactly the same class of bug as the Pipeline filter reset
fixed in Workstream 4 — and nothing asserted it. The new test constructs a view
model with a running sidecar and an in-progress batch, replaces the rows, and
asserts both survive.

Fixed with five unit tests. The criterion could have been marked "met" by
reading the code; that is precisely the proxy reasoning the phase's own
verification discipline forbids.

### F2 — No test reached `Ready` through the recovery path (medium severity)

Criteria 1 and 3 were each covered in halves: `state.rs` tests prove a database
without a wrapped key reaches `Recovery`; `recovery.rs` tests prove
`install_key_file` copies and validates a bundle; `state_flow` proves
`try_passphrase` reaches `Ready`. Nothing drove the three together, so a wiring
mistake in `CareerOpsApp::handle_lock_action` — for example failing to
re-bootstrap after installing the key — would not have failed a test.

`app::recovery_restores_a_key_file_and_reaches_ready` now does the whole walk:
create a vault, move the key aside, bootstrap into Recovery, install the file
through the same `LockAction` the UI emits, unlock, assert `Ready` and a live
pool. This is the only recovery path a user can take that ends in a usable
application, so it deserved an end-to-end test.

### F3 — The manual test script did not exist (criterion 14)

The plan calls the script an exit criterion, and `docs/` did not contain it. It
now exists as `docs/manual-test-script.md`, with exact UI labels and expected
outcomes, a note on which steps need network, and a sign-off table.

Writing it exposed two facts worth recording:

- Export from Recovery is a **byte copy**, not a validation. A corrupt
  `dek.wrapped.json` still exports successfully. That is defensible for a
  backup tool, but it is not obvious, so the script says so explicitly.
- Step 3.1.3 in the script (export with a missing key) fails, while 3.2.2
  (export with a corrupt key) succeeds. Both behaviours come from
  `export_database`'s existence check, and the script now pins them.

Executing the 🌐 rows needs a human with network access; that is the only
remaining part of criterion 14.

*Post-pass follow-up:* `CareerOpsApp::with_fetch_source` now injects the Tier 1
source, so the orchestration half of those rows (router → storage → health →
Pipeline) runs in CI with a scripted source. The manual rows keep what only a
real page can prove: JSON-LD extraction against actual markup, Tier 2 through
the sidecar, and the UI feedback while a fetch is in flight.

A second follow-up closes most of the first of those: an ignored live test
(`live_fetch_reaches_the_pipeline`, URLs overridable with
`CAREER_OPS_LIVE_PORTALS`) runs the production `HttpFetchSource` through the
app. It defaults to one board per supported ATS — Ashby/linear (34 jobs),
Lever/spotify (90), Greenhouse/figma (139), SmartRecruiters/Ubisoft2 (237) —
fetches the first individually and the rest as a batch, and asserts portal
health, stored jobs per company, and the Pipeline page. On 2026-10-25 it stored
500 unique jobs across the four boards, which covers the Tier 1a half of manual
script §4.4 and the batch orchestration of §4.6. The Tier 1b and Tier 2 halves
still need a hand, because they depend on the markup a specific page emits.

The test creates its portals in a throwaway vault. Seeding these four boards
into the real database was considered and rejected: portals are user data, an
unannounced migration that inserts companies a user never chose is
indistinguishable from a bug, and a test that reads its fixtures from the user'
sdatabase is neither deterministic nor safe. If a starter kit is ever wanted, it
belongs to onboarding with explicit consent, not to a seed step.

That run also produced a finding worth carrying into Phase 3: Waymo's careers
search page is a JS-driven search form whose static HTML contains no
`JobPosting` JSON-LD. Tier 1b fails on it and escalates to Tier 2, which reuses
the *same* JSON-LD/selector extraction on the rendered DOM. So a general career
page with neither JSON-LD nor a `scraper_config` cannot be fetched by either
tier — and the Phase 2.1 portal form does not expose `scraper_config`, so the
selector path is currently unreachable from the UI. Phase 3 should either
expose that field or add a real extraction strategy for such pages; the current
error (`Parse: no jobs found in HTML`) is accurate but not actionable for a user.

### F4 — Criterion 4 named a screen that does not exist (documentation drift)

The criterion lists "Dashboard (placeholder)" alongside Pipeline, Portals,
CV/Profile, Prompts, and Settings. `MainScreen` has five variants and no
Dashboard; the closest equivalent is Pipeline, which is also the default screen.
This is leftover from the GPUI-era plan where a dashboard summarized the
pipeline. The criterion is reworded in plan v0.8 rather than adding a screen to
satisfy a stale sentence.

### F5 — `data_dir()` has no unit test (accepted)

The override and the `directories` fallback are both one expression. Testing
them means mutating process-global environment state, which is racy in a
parallel test binary and would make the suite order-dependent. The smoke run
exercised the default path; the README documents both. Recorded as accepted
rather than closed.

---

## P2-R9 — service/view divergence: resolved

The Phase 2 risk was that the view layer would need data the service layer does
not provide, tempting the next phase to compute it in the view.

**The shell did not reshape the service layer's outputs.** It made one
capability addition and zero shape changes:

| Need | Resolution | Which layer changed |
| :--- | :--- | :--- |
| The edit form must round-trip `render_mode` and `scraper_config` | Both were already fields on `Portal`; the view model gained them | View model |
| The Pipeline needs the application status per job | `applications::list` already exists; the app's load task joins it by `job_description_id` | App orchestration |
| Health, suggested action, sidecar state | `PortalHealth::from_portal` and `health::suggested_action` were reused verbatim | None |
| Batch fetch must be cancellable | `fetch_batch`'s callback cannot break its loop | **Core service**, test-first (`fetch_batch_cancellable` + core test) |

The one service change was a **capability**, not a shape, and it was made in the
service layer with a test rather than worked around in the view. That is the
mitigation working as intended.

A related lesson came from the Pipeline, not from P2-R9: a load that is in
flight when a write succeeds can return stale data *after* the write. The fix is
at the orchestration layer — a successful manual add drops the in-flight load
and marks the page stale — but the general rule belongs in the patterns doc for
Phase 3: **every write invalidates and, if necessary, abandons reads.**

---

## egui immediate mode on the real screens

**No performance concern observed; no profiling done.** The honest statement is
that the screens feel immediate at Phase 2.1's data volumes, and the volumes are
bounded by construction: portals are a dozen, and the Pipeline loads at most 500
rows (`PIPELINE_LIMIT`). Both use a plain `ScrollArea`; `TableBuilder` is the
documented swap point and is deliberately not used yet.

Four findings that are not about speed:

1. **Immediate mode made the shell trivially testable.** Screens are functions
   over `(&view_model, &mut view_state, render_state)` returning an action enum.
   There is no retained widget state to drift out of sync, so a harness test
   proves the same thing a user would see.
2. **`Harness::run()` never settles while a spinner is visible.** `Spinner`
   requests a repaint every frame, so the harness's "run until idle" loop never
   terminates. Tests with a spinner use `run_steps(2)`. This is a property of
   immediate mode, not a bug; it is recorded in `patterns.md`'s async section as
   a testing note.
3. **`Window` is a layer, which made the shared modal possible.** Moving the
   manual-entry dialog out of `portals::show` and rendering it after the active
   screen costs one call site and removes the need for Pipeline to know anything
   about `PortalsView`.
4. **Layout is explicit and cheap, but not free.** Row structure (label widths,
   truncation) is written by hand. That is fine for five screens and will become
   repetitive for Phase 3's denser job/evaluation views; extracting a row helper
   is the natural next step, not a table rewrite.

Risk P2.1-R1 (egui is the wrong framework) did not trigger. Nothing in the phase
required fighting the framework: every friction point had a documented API
answer (`Panel::left`, `labelled_by`, `Window` layers), and the one genuinely
awkward area — the async bridge — is framework-independent.

## Accessibility in the unlock flow

The Phase 2.01 gap was that text inputs had no accessible name. **It is closed,
not deferred:** egui 0.37 has `Response::labelled_by(Id)`, every input goes
through `components::labelled_input`, and `egui_kittest` fails a test that
renders an unnamed widget. A regression is therefore not possible without a test
failure.

What remains, and is still Phase 5 scope:

- **Focus order and keyboard navigation.** The unlock passphrase submits on
  Enter, but Tab order follows declaration order with no grouping, and the
  first-launch wizard does not move focus to the next step's field. A keyboard
  user can complete the flow; they cannot do it efficiently.
- **Modals do not trap focus.** `egui::Window` is a layer; Tab can leave it.
  The manual-entry and delete dialogs are still operable, but a screen-reader
  user can lose their place.
- **Row semantics on the Pipeline.** Each row is a `selectable_label` followed
  by separate labels for tier, status, and time. A screen reader reads them as
  unrelated nodes. Grouping them (a labelled container or one composed label)
  is a Phase 5 audit item.
- **Contrast** has not been audited. The status badge palette is defined in one
  place (`StatusBadge::color`), so a future contrast pass has a single target.

## The async bridge

**Verdict: ergonomic enough to keep, with three known friction points.**

What works:

- One poll point per task per frame, with `TaskPoll::{Ready, Pending,
  Cancelled}`. The same shape serves the lock transition, the four Portals task
  variants, and the pipeline load.
- `request_repaint_after(50 ms)` while pending and *nothing* while idle is the
  whole idle-CPU story. Because it is enforced in one place per screen, the
  default is correct.
- Moving `AppState` into the future and restoring it with the result avoided
  both `Arc<Mutex<_>>` and any borrow across an await.

What is friction:

1. **The UI thread is not a runtime thread.** `AsyncTask::spawn` panics without
   an entered runtime, so every spawn must go through `CareerOpsApp::spawn`.
   One helper, but easy to bypass in a new screen; it is documented in
   `patterns.md` and asserted by a test.
2. **Each new task type adds an enum variant, a poll variant, and a match arm.**
   `portals_task` has four shapes and the plumbing is straightforward but
   mechanical. Phase 3 will add evaluation and generation tasks; if the count
   grows past two more, a small `Pending<T>` wrapper or a task registry keyed by
   id is worth the abstraction. Not before.
3. **Cancellation only exists where the service supports it.** Dropping an
   `AsyncTask` discards the result but does not stop the work; `AppState`'s lock
   operations cannot be interrupted at all. That is acceptable because both are
   short, but it is the reason the batch fetch needed an explicit `AtomicBool`
   rather than cancellation of the future.

## Estimate accuracy and the framework swap

| Workstream | Plan | Actual |
| :--- | :--- | :--- |
| WS1 shell infrastructure | 2–3 days | Part of the first day |
| WS2 unlock / first launch / recovery | 2–3 days | Part of the first day |
| WS3 Portals | 3–4 days | Most of the first day and the second |
| WS4 Pipeline | 2 days | Second day |
| WS5 verification | 1 day | This pass |
| **Total** | **10–13 days (1.5–2 weeks)** | **Two calendar days of implementation + one verification pass** |

The estimate was not wrong about scope — every workstream produced what it said
it would — it was wrong about the unit of work. The plan priced human days for
design, implementation, and debugging; with the design already recorded in the
plan, the view models already specified, and a test harness (`egui_kittest`)
that makes a UI assertion as cheap as a function assertion, the implementation
is no longer the dominant cost. **The dominant cost has moved to deciding what
"verified" means.** All four findings above were about that, not about missing
features.

**How the framework swap affected the total.** Phase 2.01 landed on 2026-10-24
and Phase 2.1 started the same day, so no view code was ever written twice
inside this phase. The layers that survived the swap unchanged are the ones the
architecture said would: `state.rs` (211 lines), `navigation.rs` (145),
`async_bridge.rs` (169), and 899 lines of view models. The view layer was
written once, against egui. Had the swap happened after Phase 2.1 instead of
before it, the entire view layer would have been rework. That is the strongest
argument for having front-loaded the evaluation, and it belongs in the next
retrospective as evidence, not as a claim. The view layer is 1,765 lines: the
lock screen (550), Portals (700), Pipeline (226), the shell (102), and shared
components (187), tests included in each file.

What the swap did cost is visible in one place: the plan needed eight versions
(v0.1–v0.8). v0.2 and v0.4 are pure framework corrections — the first rewrote the
plan for egui, the second fixed API facts the egui 0.37 docs would have answered
(`eframe::App::ui`, `Panel::left`, `Response::labelled_by`). v0.7's app-level
modal is also egui-shaped: `Window` renders into a layer, so one call after the
active screen replaces a screen-specific coupling. Those are documentation costs
for a framework that moves, and they are now paid down in `patterns.md`.

---

## What to carry into Phase 3

1. **Every write invalidates reads.** The Pipeline already does this. Evaluation
   and cover-letter generation will write reports while a detail view is open;
   use the same invalidation rule instead of comparing timestamps.
2. **Screens return actions; the app owns side effects.** Extend
   `PipelineAction` rather than giving the detail view a database handle. The
   Evaluate button is already in place; wiring it should not need a new
   orchestration pattern, only a new task type.
3. **Add a harness test alongside every new screen, and a unit test for every
   view-model function.** The gaps F1 and F2 closed were all "the code is
   obviously correct" cases. That is the failure mode to expect.
4. **Run `docs/manual-test-script.md` before starting Phase 3.** The 🌐 rows are
   the accumulated network debt from Phase 2.1 (portal fetch, batch, manual URL,
   Tier 2 sidecar).
5. **Do not add virtualization until the 500-row cap is removed.** If Phase 3
   needs more than the cap, replace the loop with `egui_extras::TableBuilder` in
   the same change that raises `PIPELINE_LIMIT`, not before.

## Open items for the human

- Execute the 🌐 sections of `docs/manual-test-script.md` and record the result
  in its sign-off table.
- Confirm the CI run for the Phase 2.1 commits (criterion 16) is green on all
  three operating systems.
- Decide whether the idle-CPU check (script §7) should be automated later; the
  agent sandbox forbids process inspection, so it can only be run by hand today.
