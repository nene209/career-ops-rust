# Phase 3.4 — Prompt Version Pointer and Cleanup: Retrospective

**Date:** 2026-11-12
**Plan:** `.agents/plans/phase_3.4_prompt_version_pointer_and_cleanup.md` (v0.6)
**Outcome:** all five workstreams implemented; the offline gates are green and the
manual §6c pass is the owner's.

---

## What shipped

- **Schema and repository (WS1).** Migration **0010** adds `is_current` and
  `retired_at`, backfills the newest version per thread, and creates
  `ux_prompts_current_per_thread` (partial unique). `Prompt` carries both fields.
  `repo::prompts` gains `current`, `newest_system` (renamed from `active_system`),
  `set_current`, `references`/`PromptReferences`, `delete_version`/`DeleteOutcome`,
  `retire_version`, and `restore_version`; `usage` includes triage;
  `list_threads` follows the pointer and counts non-retired versions; and
  `create_version`/`insert_system_prompt` clear-then-set the pointer in one
  transaction. A hard delete splices `supersedes_id` children to the deleted
  row's parent and promotes the newest remaining non-retired version.
- **Resolution and seeding (WS2).** `seed_one` compares against the newest system
  version; seeding activates the new system version without touching a user
  thread; `active_for_use` resolves the user's current row, then the system's.
  Tests pin the older-user-current case, both seeding cases, and an evaluation
  recording the activated version's `prompt_version`/`prompt_hash`.
- **View model and screen (WS3).** `current()` became the editor, default-pane,
  diff, and save baseline; `newest()` is only for ordering. The detail's category
  comes from the current row. The version list marks the current row, hides
  retired rows behind **Show retired (N)**, and the header labels
  **Current version (vN)** / **Viewing vN** / **Retired vN**. **Make current**
  replaces **Revert to this version**; **Delete…**, **Retire…**, and **Restore**
  lead to a confirmation that names the promoted version or the usage count.
- **App wiring (WS4).** One `PromptOp` task performs create/activate/delete/
  retire/restore and returns a typed outcome. The app updates the message,
  reloads both panes, resets the historical selection, selects the new user
  thread after a duplicate, and falls back to the system thread when the only
  user copy is deleted.
- **Docs (WS5).** Manual §6c is rewritten for the pointer and the cleanup
  actions (including the used-version retire after a §6d run);
  `crates/app/docs/patterns.md` gains the versioning rules; the parent plan is
  updated; this retrospective records the deviations.

## What held up

- **A pointer instead of a copy.** Reverting to v1 now moves `is_current`; no
  row is inserted and no hash is duplicated. The version count is unchanged,
  which is exactly what the manual review asked for.
- **The database invariant.** The partial unique index makes "one current per
  thread" a rule, not a convention. Every mutation clears then sets the pointer
  in one transaction, so an interrupted write cannot leave two current rows.
- **Provenance-first cleanup.** `references()` counts evaluation reports, cover
  letters, and triage by hash. A referenced version is retired and
  `get_by_hash` still resolves it; an unreferenced one can be deleted.
- **The FK splice.** `supersedes_id` has a foreign key and no `ON DELETE`
  action, so the hard delete repoints children before deleting. The three-version
  chain test proves v3 adopts v1 when v2 is removed.
- **The current-vs-newest split in the view model.** The screen never has to
  reason about "latest"; it asks `current()` for the baseline and the marker.
  `newest()` exists only where the list order matters.
- **The reused operation task.** `PromptOp` kept one `writing` flag and one
  reload path for five actions, so the UI cannot show two operations at once.

## What was harder, and the deviations

- **`is_current` is the pointer, not a literal `current_version_id`.** A literal
  pointer needs an owner row; on every version row it is redundant, and in a new
  `prompt_threads` table it is a larger migration and a join on every read.
  `is_current` plus the partial index is the smallest representation that still
  makes the invariant enforce itself.
- **Activation is not logged.** Reverting moves the pointer; it does not record
  *when* the switch happened. That is the accepted tradeoff of the decision. A
  `prompt_activations` table is Q-60 if the history is ever needed.
- **Deleting the current version with only retired siblings is refused.** The
  plan did not name this state. Without the refusal the thread would have rows
  but no current version; the error tells the user to restore one first, and the
  screen disables the action.
- **`DeleteOutcome::Deleted.promoted` is a version number.** The first
  implementation returned the promoted row id, which produced "v20 is now
  current" in the app message. The repository now selects `(id, version)` and
  returns the version, so the dialog and the message agree.
- **The app wiring landed with WS3.** The screen could not emit the new actions
  without handlers, so WS3 and WS4 were implemented together; the plan records
  them as separate workstreams for the changelog and tests.
- **`active_system` became `newest_system`.** "Active" now means the pointer;
  seeding and version numbering need the highest number, not the current row.
  The seeding caller and its tests were updated in WS1 to keep the build green.
- **Triage was missing from `usage()`.** A version used only by the sorter looked
  unused. The display and the delete guard now count
  `job_descriptions.triage_prompt_hash`.

## Test strategy as built

| Layer | What it pins |
| :--- | :--- |
| Migration | 0010 on a 0009 schema; one current per thread; the partial index refuses a second |
| Repository | activation without a new row; create/activate; delete/splice/promote; retire/restore; triage references; system refusals; the sibling-hash rule; the retired-sibling refusal |
| Resolution | an older user current beats a newer system version; seeding moves only the system pointer |
| Evaluation | a run records the activated version and not the newer non-current one |
| `egui_kittest` | current marker, Make current, delete/retire dialogs, Show retired, Restore, Back to current, system safety |
| App | duplicate → edit → activate (no new row); delete/retire/restore with messages; fallback to the system thread |
| Manual | §6c.1–6c.11 |

No network in the offline suite. No new dependency. One migration.

## Follow-ups

1. **Manual §6c** is the owner's pass. 6c.10 needs one evaluation from §6d
   with the `evaluate` prompt current, so the retire dialog has a usage count;
   6c.11 verifies that the retired version's **Used by** entry still resolves.
2. **Q-59:** the cross-entity Danger Zone is defined in the plan and still
   deferred. It needs its own phase, an operation inventory, an export-first
   step, and the rule that referenced prompt versions are not deleted without
   their artifacts.
3. **Q-60:** if "when did I switch to this version?" matters, add a
   `prompt_activations` table. The current pointer deliberately keeps only the
   present state.
4. **Q-61:** `supersedes_id` stays chronological. A `derived_from_id` would be
   needed to record that v4 was edited from v1 while v3 was the newest row.
5. **Q-63:** deleting the only user copy falls back to the system thread. The
   app selects it and the detail follows; the test covers it.
6. **A repeated-name `get_by_hash`** returns the oldest row with that hash. The
   sibling-hash delete rule uses this: a referenced hash remains resolvable if
   any other row still carries it.

## Exit criteria

All eleven are met; the manual execution is the owner's:

1. Reverting activates an existing version and adds no row. ✅
2. Exactly one current version exists per `(name, is_system)`. ✅
3. `active_for_use` returns the user's current version, else the system's. ✅
4. An unreferenced version can be deleted; the chain is repaired and a current
   is promoted. ✅
5. A referenced version is retired and restored; `get_by_hash` still resolves it. ✅
6. Triage counts as usage and in the delete guard. ✅
7. System rows are read-only. ✅
8. Migration 0010 applies to a 0009 database and preserves rows. ✅
9. The UI shows the current marker, Make current, Delete/Retire, Show retired,
   and Restore; the editor and diff use the current version. ✅
10. The offline suite, fmt, and clippy are clean. ✅
11. Manual §6c, `patterns.md`, the parent plan, and this retrospective are
    updated. ✅ (the manual run is the owner's)
