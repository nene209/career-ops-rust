# Phase 3.4: Prompt Version Pointer and Cleanup — Detailed Execution Plan

**Parent plan:** Draft Execution Plan: Career-Ops Rust Desktop Application (v0.48), §12
**Status:** Planned — not started
**Last updated:** 2026-11-09
**Duration:** 2–3 days
**Depends on:** Phase 3 (prompt versioning and the Prompts screen), Phase 3.2 (`triage_prompt_hash` on `job_descriptions`), Phase 3.3 (none functionally; sequencing only)
**Blocks:** nothing. It changes how "current" is stored and adds cleanup actions; no other phase depends on it.

---

## Change Log

| Version | Change |
| :--- | :--- |
| v0.1 | Initial plan. Replaces "revert appends a copy" with a **current-version pointer** (`is_current` plus a partial unique index), so reverting moves the pointer and never duplicates a content hash. Adds **delete** for unreferenced versions and **retire/restore** for referenced ones, extending the usage check to triage. Defines the deferred cross-entity **Danger Zone** for irreversible deletes. One migration (**0010**), no new dependency, no AI change. |

---

## Purpose

The Prompt Manager currently treats "the latest row" as both the newest version and
the version in use. Two consequences the owner hit while reviewing the manual
script:

1. **Revert duplicates content.** `Revert to this version` calls `create_version`
   with the old text, so v3 is v1's content again. The database grows, and two
   rows share one `content_hash`. Because resolution is by hash, the duplicate is
   not merely wasteful: `get_by_hash` has to pick one of them (it picks the
   oldest), and a future consumer could assume one row per hash.
2. **There is no cleanup.** Versions are immutable by design, so a mistaken save
   lives forever. That is correct for versions that produced an evaluation or a
   letter, and unnecessary for ones that did not.

This phase separates **content** from **activation**. Version rows stay
immutable; a pointer says which one is current. Reverting moves the pointer.
Deleting is allowed only when no stored artifact references the content (or a
sibling row still provides it); otherwise the version is retired, which hides it
from the picker while keeping the text resolvable for historical review.

### Evidence

- `repo::prompts::create_version` inserts `MAX(version) + 1`; `Revert` reuses it
  with an older version's content (`crates/app/src/app.rs`, `revert_prompt`).
- `repo::prompts::active_for_use` resolves `ORDER BY version DESC LIMIT 1` per
  owner; `list_threads` selects the same row.
- `evaluation_reports.prompt_hash`, `cover_letters.prompt_hash`, and
  `job_descriptions.triage_prompt_hash` store `content_hash`; `get_by_hash`
  resolves text from the `prompts` table. Deleting the last row with a hash
  breaks that lookup.
- `prompts.supersedes_id` is `INTEGER REFERENCES prompts(id)` with foreign keys
  enabled (`db/mod.rs` sets `foreign_keys = ON`), so a hard delete must repair
  the chain.
- `repo::prompts::usage` counts evaluation reports and cover letters only; triage
  is missing.
- The manual script's §6c.6 pins the duplicate behavior and §6c.5 labels the
  baseline "latest", both of which change.

---

## Scope

| In scope | Out of scope |
| :--- | :--- |
| `is_current` pointer + partial unique index (migration 0010) | A cross-entity Danger Zone screen |
| Revert = activate an existing version, no new row | Any change to evaluation/letter/triage content |
| Hard delete for unreferenced versions (with `supersedes_id` repair) | A new templating language or prompt tooling |
| Retire/restore for referenced versions | Deleting system prompt rows |
| Triage added to `usage()` and the reference guard | A per-thread version-number counter |
| Prompts UI: current marker, Make current, Delete/Retire/Restore, Show retired | A global destructive-actions framework |
| Tests, manual §6c/§6d, docs, retrospective | Cross-device sync or export |

---

## Design decisions

### 1. "Current" is a pointer, represented as `is_current`

The smallest correct representation is a boolean on the version row plus a
partial unique index:

```sql
ALTER TABLE prompts ADD COLUMN is_current INTEGER NOT NULL DEFAULT 0;
ALTER TABLE prompts ADD COLUMN retired_at TEXT;

UPDATE prompts
SET is_current = 1
WHERE id IN (
    SELECT p.id FROM prompts p
    WHERE p.version = (SELECT MAX(q.version) FROM prompts q
                       WHERE q.name = p.name AND q.is_system = p.is_system)
);

CREATE UNIQUE INDEX ux_prompts_current_per_thread
    ON prompts(name, is_system) WHERE is_current = 1;
```

A literal `current_version_id` column needs an owner. Putting it on every row
would be redundant; putting it on a new `prompt_threads` table is a larger
migration and every read joins. `is_current` is the pointer, and the partial
index makes "exactly one current per `(name, is_system)`" a database invariant
rather than a convention. Activation is two updates in one transaction:

```sql
UPDATE prompts SET is_current = 0 WHERE name = ? AND is_system = ?;
UPDATE prompts SET is_current = 1 WHERE id = ?;
```

The backfill picks the highest version per thread, which is exactly the current
resolution rule, so no existing behavior changes on upgrade.

### 2. Content stays immutable; only `is_current` and `retired_at` move

No version row's `content`, `content_hash`, `version`, `created_at`, `category`,
or `supersedes_id` is ever rewritten except the `supersedes_id` repair described
in decision 6. This keeps every hash in every artifact meaningful.

### 3. The UI's baseline is "current", not "newest"

`PromptDetail` gains `current_id`; `PromptVersionRow` gains `is_current` and
`retired_at`. The default pane (no version explicitly selected) shows the
**current** version, the editor loads it, the diff compares against it, and
"save" is enabled when the editor differs from it. The version list is still
ordered newest-first, so the newest row may not be the current one.

`PromptThreadRow`'s `latest_id`/`latest_version` become
`current_id`/`current_version`; `version_count` counts **non-retired** versions
so the number matches what the list shows. The list's date is the current row's
`updated_at` (content age). The time a version became current is not stored
(see the tradeoff below).

### 4. Revert is `make current`, and it writes nothing

`PromptsAction::RevertToVersion` becomes `SetCurrentVersion`. The app calls a new
`repo::prompts::set_current(version_id)` which loads the row, refuses a retired
row, and flips the pointer in one transaction. The message is
"Now using `evaluate` v1." No version is added, the count does not change, and
`v2` remains listed.

`supersedes_id` still records **creation** order. If v1 is current and the user
edits it into v4, v4 supersedes v3 (the row created before it), not v1. The
pointer records activation, the chain records creation, and the two are
deliberately separate. A "derived from" column would be needed to record which
version was edited; that is deferred (Q-61).

### 5. Save and seed still append, and the new row becomes current

`create_version` and `insert_system_prompt` each become transactional:

1. compute the next version from `MAX(version)` over the remaining rows
   (retired rows included); a hard-deleted highest number can therefore be
   reused, which is acceptable because no artifact references it and the row
   id/hash is the identity;
2. clear `is_current` for the thread;
3. insert the new row with `is_current = 1`.

Seeding compares content against the **newest system version**, not the current
one. `active_system` is therefore renamed **`newest_system`** to stop "active"
from meaning two things. `active_for_use` keeps its name and changes its rule to
"the user's current row, else the system's current row".

### 6. Delete only when the content is still resolvable; otherwise retire

`repo::prompts::references(content_hash)` counts the three tables that store a
hash:

| Table | Column |
| :--- | :--- |
| `evaluation_reports` | `prompt_hash` |
| `cover_letters` | `prompt_hash` |
| `job_descriptions` | `triage_prompt_hash` |

`delete_version(version_id)` refuses a system row, runs the reference counts
inside its transaction, and:

- **references == 0** → hard delete.
- **references > 0 but another row with the same `content_hash` exists**
  (retired or not) → hard delete is still safe, because `get_by_hash` resolves
  the sibling. The UI uses the simpler rule below, but the repository is
  authoritative.
- **otherwise** → refuse with the counts; the UI offers **Retire**.

A hard delete must repair `supersedes_id` first, because foreign keys are on and
`ON DELETE` is not `SET NULL`:

```sql
UPDATE prompts SET supersedes_id = ? WHERE supersedes_id = ?; -- child adopts the deleted row's parent
DELETE FROM prompts WHERE id = ?;
```

If the deleted row was current, the newest remaining non-retired version is
promoted in the same transaction. If no row remains, the thread disappears and
`active_for_use` falls back to the system thread (or `None`).

**Retire** sets `retired_at` and leaves the row and its hash in place. Retiring
the current version promotes the newest remaining non-retired version; retiring
the **only** active version is refused (the user must keep it or, if nothing used
it, delete it). **Restore** clears `retired_at`; it does not make the version
current (the user can then choose **Make current**). `get_by_hash` never filters
`retired_at`, so historical review keeps working.

System rows are never deleted, retired, or restored.

### 7. UI: one action per intent, and a visible retired set

The version list shows a `current` marker on the pointer's row. The viewed-version
header offers:

| Version state | Header | Actions |
| :--- | :--- | :--- |
| current | **Current version (vN)** | **Delete…** (user rows only) |
| historical | **Viewing vN** | **Make current**, **Delete…** |
| retired | **Retired vN** | **Restore**, **Delete…** (only when unused) |

**Delete…** opens a confirmation. For an unreferenced row it says it cannot be
undone and, if current, names the version that becomes current (or says that the
user copy disappears and the system prompt becomes active when it was the only
one). For a referenced row it says how many runs used it and offers **Retire**
instead, explaining that the text stays available to those runs. A
**Show retired (N)** checkbox in the Versions header reveals retired rows,
dimmed.

### 8. The Danger Zone is defined here but deferred

The owner wants one place for irreversible deletes across all data, not a
per-screen habit. This phase records the shape and does not build it:

- **Placement:** a dedicated **Danger zone** screen (not a modal inside Prompts),
  reachable from Settings, visually separated and never the default action.
- **Inventory:** delete all prompts and versions, delete all evaluations and
  cover letters, delete all jobs, delete all portals, delete all logs, reset the
  vault. Each row states what is deleted, what is kept, and the count.
- **Gate:** typing `delete me`, as the job delete already uses; no single click.
- **Constraint:** prompt versions referenced by an artifact are not deletable
  even from the Danger Zone without first deleting the artifact, because that is
  the provenance rule. The zone may offer "delete the artifact and its prompt
  reference together" as one explicit operation.
- **Backup:** every dangerous action offers the encrypted export first.

Recorded as Q-59 and deferred to Phase 5 (or its own phase) so 3.4 stays about
the pointer and per-version cleanup.

---

## Decision check (issues to be aware of)

These follow from the two decisions and are the places where the naive version
would be wrong:

1. **The pointer is `is_current`, not a literal `current_version_id`.** A literal
   pointer needs an owner row; on every version row it would be redundant, and in
   a new `prompt_threads` table it is a larger migration and a join on every read.
   `is_current` plus a partial unique index is the smallest representation that
   still makes "exactly one current per thread" a database invariant.
2. **Reverting loses the activation timestamp.** The pointer keeps the current
   state, not a log of switches. If "when did I activate this?" matters, a
   `prompt_activations` table is the future fix (Q-60).
3. **Hard delete must splice `supersedes_id`.** Foreign keys are on and the
   column has no `ON DELETE SET NULL`, so deleting a middle version without
   repointing its child fails. The delete transaction repoints first.
4. **Triage is missing from the existing usage count.** A version used only for
   triage would look deletable and its `triage_prompt_hash` would become
   unresolvable. `usage()` and the delete guard both add
   `job_descriptions.triage_prompt_hash`.
5. **`active_system` is renamed.** It returns the newest system version for
   seeding, not the current one; keeping the name would make "active" mean two
   things once `active_for_use` follows the pointer.
6. **Deleting the current version must promote.** The transaction selects the
   newest remaining non-retired row and activates it; deleting the only row
   removes the thread and falls back to the system prompt. Retiring the last
   active version is refused.
7. **A hard delete can free a version number.** The next save uses
   `MAX(version) + 1` over the remaining rows, so a deleted highest number can
   reappear. That is safe because nothing references the deleted row, but the
   version number is not a permanent identity; the row id and hash are.
8. **The Danger Zone is not a per-prompt dialog.** It is a cross-entity area with
   its own inventory, typed gate, and export-first rule. Referenced prompt
   versions cannot be deleted there without deleting the artifacts that reference
   them.

---

## Workstream 1 — Schema, model, and repository

**Estimated duration:** 1 day.

### Deliverables

1. `crates/core/migrations/0010_prompt_current_version.sql`: `is_current`,
   `retired_at`, the backfill, and `ux_prompts_current_per_thread`.
2. `model::Prompt` gains `is_current: bool` and `retired_at: Option<DateTime<Utc>>`.
3. `repo::prompts`:
   - `current(pool, name, is_system) -> Option<Prompt>`;
   - `active_for_use` resolves the user's current row, then the system's;
   - `newest_system` (renamed from `active_system`) returns the highest system
     version for seeding;
   - `set_current(pool, version_id) -> Result<Prompt>` (refuses retired rows);
   - `create_version` and `insert_system_prompt` clear then set the pointer in
     one transaction and never reuse a version number;
   - `references(pool, content_hash) -> PromptReferences { reports, letters, triage }`;
   - `delete_version(pool, version_id) -> Result<DeleteOutcome>` with the
     `supersedes_id` repair and current-version promotion;
   - `retire_version` / `restore_version`;
   - `usage` gains triage, using `COALESCE(triaged_at, queued_at, fetched_at)`
     and `COALESCE(triage_model, 'unknown')` so the display row is always
     well-formed;
   - `get_by_hash` is unchanged and explicitly does **not** filter retired rows.
4. `PromptThread` returns `current_id`/`current_version` and an active
   `version_count`; `list_versions` returns all rows (retired included).
5. Tests in `crates/core/tests/prompts_repo.rs` and a migration test:
   - 0010 applies to a 0009 schema, preserves rows, and marks exactly one current
     per thread (system v2, user v1 in a mixed fixture);
   - the partial unique index rejects a second current row;
   - `set_current` flips without inserting and refuses a retired row;
   - `create_version` makes the new row current;
   - hard delete of a middle row splices the chain (v3 points at v1);
   - hard delete of the current row promotes the newest remaining row;
   - hard delete of the last row removes the thread and `active_for_use` falls
     back to system;
   - a referenced row is refused; a row with a referenced hash but a sibling row
     is deletable;
   - retire hides a row from `active_for_use`/`list_threads` but `get_by_hash`
     still resolves it; restore clears the flag;
   - the last active version cannot be retired;
   - system rows cannot be deleted, retired, or restored;
   - triage appears in `usage()` and in `references()`.

### Acceptance criteria

| Criterion | Verification |
| :--- | :--- |
| Exactly one current per thread after migration | Migration test + index |
| Revert/activation writes no new row | Repository test |
| A hard delete repairs `supersedes_id` | Chain test |
| A referenced version cannot be hard-deleted | Reference test |
| Retired rows remain resolvable by hash | `get_by_hash` test |
| System rows are immutable | Refusal tests |

---

## Workstream 2 — Resolution, seeding, and write paths

**Estimated duration:** 0.5 day.

### Deliverables

1. `prompts_seed::seed_one` uses `newest_system`; the `Updated`/`Unchanged`
   logic is unchanged.
2. `insert_system_prompt` activates the new system version. A later release
   moves the system thread's current pointer without touching a user copy.
3. `active_for_use`'s callers (evaluation, cover letter, triage, suggest) need no
   signature change; they now resolve the current row.
4. Tests:
   - a user copy with an older current version still wins over a newer system
     version;
   - after `set_current` on the user thread, `active_for_use` returns that
     content and `prompt_version` used by a run is the activated version;
   - seeding after a user copy exists does not change the user's pointer;
   - seeding a changed system prompt advances only the system pointer.

### Acceptance criteria

| Criterion | Verification |
| :--- | :--- |
| Resolution follows the pointer | Core tests |
| Seeding never changes a user's current version | Core test |
| A run records the activated version number | Evaluation/triage test |

---

## Workstream 3 — View model and screen

**Estimated duration:** 1 day.

### Deliverables

1. `PromptThreadRow`: `current_id`, `current_version`, active `version_count`.
2. `PromptVersionRow`: `is_current`, `retired_at`.
3. `PromptDetail`: `current_id`, `current()`, `newest()`, `version(id)`. The
   detail's `category` is taken from the **current** row (falling back to the
   newest), not from `versions.first()`; a revert to an older category must not
   leave the editor validating against the wrong variable set.
4. `PromptsView`: `select` and `show_version(None)` load the **current** content;
   `viewed_version(None)` is `current()`; the diff baseline and
   `editor_is_changed` use `current()`; add `show_retired: bool`.
5. Screen: **Current version (vN)** / **Viewing vN** / **Retired vN**, **Make
   current** (replaces **Revert to this version**), **Delete…**, **Retire**,
   **Restore**, **Show retired (N)**, and the delete/retire confirmation.
6. `egui_kittest`:
   - the current marker follows the pointer after an activation;
   - activation adds no row and does not change the count;
   - the delete dialog for an unused row names the promoted version;
   - the retire dialog names the usage count;
   - Show retired reveals a dimmed row, and Restore removes the retired state;
   - a system thread has no destructive actions;
   - the editor baseline is the current version after showing a historical one.

### Acceptance criteria

| Criterion | Verification |
| :--- | :--- |
| The pane and editor default to the current version | `egui_kittest` |
| Activation is one action and adds no version | `egui_kittest` |
| Delete vs Retire is explained before writing | `egui_kittest` |
| Retired rows are hidden by default and restorable | `egui_kittest` |

---

## Workstream 4 — App wiring

**Estimated duration:** 0.5 day.

### Deliverables

1. `PromptsAction::{SetCurrentVersion, RequestDeleteVersion,
   ConfirmDeleteVersion, CancelDeleteVersion, RetireVersion, RestoreVersion,
   ToggleRetired}`.
2. One polled prompt-operation task with an enum
   (`CreateVersion`, `Activate`, `Delete`, `Retire`, `Restore`) and a result that
   carries the message and the affected name. `PromptWriteKind::Reverted` becomes
   `Activated`, with the message "Now using `evaluate` v1."
3. After every operation, reload the thread list and the selected detail;
   `selected_version` resets to `None` so the pane shows the current version.
4. If deleting the only user version removes the thread, select the system
   thread of the same name; if there is none, clear the selection.
5. App tests:
   - duplicate → edit → activate v1 → the version count is unchanged and the
     editor shows v1;
   - delete an unused v3 → v2 becomes current;
   - retire a used v2 → the thread's current promotes and `get_by_hash` still
     resolves the retired row;
   - deleting the only user version falls back to the system thread;
   - system rows expose no destructive action.

### Acceptance criteria

| Criterion | Verification |
| :--- | :--- |
| Every operation reloads the list and detail consistently | App tests |
| A removed thread falls back predictably | App test |
| Messages name the version and the outcome | App tests |

---

## Workstream 5 — Docs, verification, retrospective

**Estimated duration:** 0.5 day.

1. Manual script:
   - §6c intro: version history is immutable and the current version is a
     pointer.
   - §6c.5: **Back to current** (not "latest"); the compare baseline is the
     current version.
   - §6c.6 rewrite: **Make current** on v1 reports "Now using `evaluate` v1.";
     the marker moves; **no new version is created** and the count stays the
     same; v2 remains.
   - New steps: delete an unused version (and the promotion when it was current);
     retire a version after §6d has produced a run, then **Show retired** →
     **Restore**, and confirm the run's **Used by** entry still resolves.
   - §6c.8 onward renumber as needed.
2. `crates/app/docs/patterns.md`: a short "Prompt versioning" paragraph
   (immutable content, `is_current` pointer, owner-scoped threads, retire vs
   delete, `supersedes_id` is creation order).
3. `docs/manual-test-script.md` header/date; `main.md` v0.47 changelog and the
   Phase 3.4 row.
4. Full validation.
5. `.agents/plans/phase_3.4_retrospective.md`: the pointer decision, the FK
   splice, the triage-usage gap, the activation-audit tradeoff, and the deferred
   Danger Zone.

---

## Test Strategy

| Layer | Covers |
| :--- | :--- |
| Migration | 0010 on a 0009 schema; one current per thread; the partial index |
| Repository | activation, create, delete (splice + promote), retire/restore, references incl. triage, `get_by_hash` on retired rows, system refusals |
| Resolution | `active_for_use` follows the pointer; `newest_system` drives seeding |
| View model | current vs newest; editor/diff baseline; retired filtering |
| `egui_kittest` | current marker, activation, delete/retire dialogs, Show retired, Restore, system read-only |
| App | duplicate → edit → activate; delete promotion; retire used; fallback to system |
| Manual | §6c (pointer, delete, retire/restore) and one used-version retire in §6d |

No network. No new dependency. One migration.

---

## Risks

| ID | Risk | Impact | Mitigation |
| :--- | :--- | :--- | :--- |
| P3.4-R1 | **A thread ends up with zero or two current rows.** | High (resolution breaks) | Partial unique index; clear-then-set in one transaction; refuse retiring the last active version; promote on delete. |
| P3.4-R2 | **Hard delete violates the `supersedes_id` foreign key.** | High (delete fails) | Splice children to the deleted row's parent first; a three-version chain test. |
| P3.4-R3 | **Deleting the last row with a hash breaks provenance.** | High | `references()` guard inside the transaction; retire is the fallback. |
| P3.4-R4 | **The activation history is lost.** | Low | Accepted tradeoff: the pointer keeps only the current state. A `prompt_activations` table (Q-60) is the future fix if "when did I switch?" matters. |
| P3.4-R5 | **The UI still uses `versions.first()` as the baseline.** | Medium | Rename to `current()`/`newest()` and switch `select`, `show_version`, `set_compare`, and `editor_is_changed`; view-model tests pin it. |
| P3.4-R6 | **Seeding compares against the current instead of the newest.** | Medium | `newest_system` for `seed_one`; a test where a user copy is current and the system thread advances. |
| P3.4-R7 | **A hard delete reuses a version number.** | Low | Next version is `MAX(version) + 1` over all remaining rows; gaps are allowed and the row id/hash is the identity. Documented. |
| P3.4-R8 | **Retire is confused with delete.** | Low | The dialog states what each does; retire is offered only for referenced rows; a `retired` badge is visible. |

---

## Exit Criteria

1. Reverting activates an existing version and adds no row.
2. Exactly one current version exists per `(name, is_system)` at all times.
3. `active_for_use` returns the user's current version, else the system's.
4. An unreferenced version can be hard-deleted; its `supersedes_id` children are
   repaired and a promoted current is chosen when needed.
5. A referenced version cannot be hard-deleted; it can be retired and restored,
   and `get_by_hash` still resolves it.
6. Triage runs count as usage in both the display and the delete guard.
7. System rows are read-only.
8. Migration 0010 applies to a 0009 database, preserves rows, and enforces the
   pointer invariant.
9. The Prompts UI shows the current marker, Make current, Delete/Retire, Show
   retired, and Restore; the editor and diff use the current version.
10. Offline suite, fmt, and clippy are clean.
11. Manual §6c/§6d, `patterns.md`, the parent plan, and the retrospective are
    updated.

---

## Open Questions

| # | Question | Position |
| :--- | :--- | :--- |
| Q-59 | **What belongs in the cross-entity Danger Zone, and when?** | Defined in decision 8 and deferred to Phase 5 or its own phase. It needs an inventory, a typed gate, an export-first step, and a rule that referenced prompts are not deleted without their artifacts. |
| Q-60 | **Do we need an activation audit?** | Not now. The pointer records only the current state; a revert is not timestamped. Add a `prompt_activations` table if users need "when did I switch?". |
| Q-61 | **Should `supersedes_id` record the version that was edited?** | No. It stays chronological. A separate `derived_from_id` would be needed to record derivation from a non-newest version. |
| Q-62 | **Can a retired version be activated directly?** | No. Restore first, then Make current. Two explicit steps keep the state legible. |
| Q-63 | **What happens when the last user version is deleted?** | The thread disappears and `active_for_use` falls back to the system prompt; the UI selects the system thread of the same name. |

---

## What Phase 3.4 Is Not

- Not a Danger Zone implementation (defined and deferred).
- Not a change to prompt content, rendering, or variables.
- Not a change to evaluation, cover-letter, triage, or suggestion behavior beyond
  which version they resolve.
- Not a schema rewrite: two columns, a backfill, and a partial index; no table
  rebuild.
- Not a version-renumbering or history-compaction feature.
- Not a cross-entity delete framework.
