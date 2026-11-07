# Phase 2.9: Acquisition Preferences and Job Queue — Detailed Execution Plan

**Parent plan:** Draft Execution Plan: Career-Ops Rust Desktop Application (v0.16), §12
**Status:** Planned — not started
**Last updated:** 2026-11-06
**Duration:** 3–4 days
**Depends on:** Phase 2.5 (`applications::ensure`, `Not a match`, the Pipeline detail's raw/plain toggle, `manual::refresh_job`), Phase 2.8 (fetch controls, per-portal detail opt-in), Phase 2.8.1 (the **Logs** screen and drawer, where the batch's per-job lines are read)
**Blocks:** Phase 3 (evaluation writes `evaluated` into the queue this phase creates) and Phase 3.2 (triage fills the queue automatically).

---

## Change Log

| Version | Change |
| :--- | :--- |
| v0.1 | Initial plan. Adds a preference layer (companies, locations, keywords, exclusions) and a manual job queue state on `job_descriptions`. The queue is what makes "the user steers the fetch and evaluation" possible without an LLM, and it is exactly the structure Phase 3.2's triage will fill. Also adds a bounded batch "fetch descriptions for flagged details" action, which pairs with Phase 2.8's detail opt-in. One migration. |
| v0.2 | Renumbered from Phase 3.1 to **Phase 2.9**, so it lands before Phase 3. Phase 3 and this phase had come to depend on each other: Phase 3's entry conditions assumed the queue existed, while this plan listed Phase 3 as a dependency. Sequencing it first resolves that, and Phase 3 can then write `queue_state = 'evaluated'` unconditionally. Its only real inputs (`applications::ensure`, `Not a match`, the raw/plain toggle, `refresh_job`) are Phase 2.5 deliverables, so nothing from `rig` was ever needed here. Migration renumbered 0006 → **0005** (Phase 3 takes 0006, Phase 3.2 keeps 0007). |
| v0.3 | Phase 2.8.1 (Logging and Diagnostics) inserted before this phase, so the order is **2.8 → 2.8.1 → 2.9 → Phase 3 → 3.2**. Nothing in this plan's content changes: the batch in WS4 is a long, partially-failing operation across many jobs, which is exactly what the Logs screen and drawer are for, so this phase now names it as a dependency and reads its per-job progress in that screen rather than inferring it from a final count. |

---

## Purpose

Phase 2.8 makes the fetch actions honest. Phase 2.9 adds the state that tells the app which postings deserve attention, and Phase 3 makes evaluation possible. Without the queue and the preferences, neither of the user's two standing questions has an answer:

1. **"Which of these 200 new postings do I care about?"** — a preference layer that lets the user say "fintech, remote, staff-or-above" and see the pipeline sorted and badged accordingly, without hiding anything.
2. **"Do I have to look at every one of them?"** — a manual queue where the user marks a row for details, for evaluation, or dismisses it, and a batch action to fetch the descriptions for everything they flagged.

The second is what Phase 3.2's triage will fill in automatically. Building it manually first means the assisted triage has a well-defined state machine to write into, and it means the user can already steer without an LLM.

The phase adds three things:

- **Preferences**, stored in `profile.preferences` (JSON, already exists). Preferred companies, locations, keywords, and exclusions.
- **A queue state** on `job_descriptions`: `new`, `details_needed`, `evaluate`, `dismissed`, `evaluated`. Manual marking from the detail view; filtering in the Pipeline.
- **Batch "fetch descriptions"** for everything marked `details_needed`, cancellable and bounded, reusing the `refresh_job` path.

### Evidence

- `profile.preferences` exists in the schema (Phase 1) and is unused. It is the natural home for the preference layer.
- `job_descriptions` has no queue state; the Pipeline filters by application status only.
- `manual::refresh_job` (Phase 2.5) exists for one-row description refresh.
- The assisted triage (Phase 3.2) will need a state to write into. If that state does not exist, triage decisions land in a new table or a new column, which is more churn than creating the queue now.

---

## Scope

| In scope | Out of scope |
| :--- | :--- |
| Preference fields in `profile.preferences`: `preferred_companies`, `preferred_locations`, `keywords`, `excluded_companies`, `seniority` | LLM-assisted preference inference (Phase 3.2 or later) |
| Preference-aware Pipeline filter and badge | Preference-aware portal suggestions (Phase 3.2) |
| Queue state on `job_descriptions` (`new` / `details_needed` / `evaluate` / `dismissed` / `evaluated`) | Automatic triage (Phase 3.2) |
| Detail-view actions to mark a row | Automatic description fetching during fetch (Phase 2.8 explicitly made this opt-in) |
| Batch "fetch descriptions for flagged" with cancel and progress | Aggregator or web search (out of scope entirely) |
| Migration 0005 for the queue columns | A new status column on `applications` — the queue is separate from the application status |
| Preferences UI (Settings section), Pipeline filters | A queue that drives autonomous action |
| Tests, docs, retrospective | A redesign of the Pipeline layout |

---

## Design decisions

### 1. Preferences live in `profile.preferences`

The field is already there. The JSON shape:

```json
{
  "preferred_companies": ["Anthropic", "Figma"],
  "preferred_locations": ["Remote", "Berlin"],
  "keywords": ["compiler", "developer tools", "Rust"],
  "excluded_companies": ["RecruiterCo"],
  "seniority": "staff"
}
```

Every field is optional and defaults to empty. A `ProfilePreferences` struct deserialises from the JSON and has a `matches(&JobRow) -> PreferenceMatch` method.

**Matching is case-insensitive substring** on the specific field, not regex, not word boundary. Substring is predictable, testable offline, and consistent with how the Pipeline search works.

### 2. Preferences boost, exclusions filter

- **Preferred companies / locations / keywords** produce a `Preferred` badge and a `Preferred` filter option in the Pipeline. They do **not** hide anything by default.
- **Excluded companies** are a hard filter: an excluded company's rows are hidden in the `All` view and visible only under an explicit `Excluded` filter, so the user can see and reverse the exclusion if they change their mind.
- **Seniority** is a soft hint used only by Phase 3.2's triage; Phase 2.9 records it and shows it in the Settings screen without applying any filter.

The reasoning: "hide what I do not want" is a decision the user makes once (in `excluded_companies`), while "show me what I want" is an order-of-attention problem, not a visibility problem. Making `preferred_*` a filter would hide large parts of the pipeline by default, which is not what "steer me towards" means.

### 3. Queue state is separate from application status

`applications.status` is the pipeline's lifecycle: `discovered`, `evaluating`, `ready`, `applied`, etc. It exists because an evaluation or a submission changed something.

Queue state is different: it is a **pre-evaluation work state**. `new` (untouched), `details_needed` (user wants full text), `evaluate` (user wants evaluation), `dismissed` (user decided against looking further), `evaluated` (an evaluation report exists).

The two are related but not the same. A row can be `new` with no application, and only becomes `evaluating` after the user (or, later, the agent) starts an evaluation. Keeping them separate means the queue works whether or not the user ever moves a row into the application lifecycle.

`evaluated` is set automatically when Phase 3 writes a report, which is why Phase 3 lands after this phase. The other four are manual in Phase 2.9; Phase 3.2's triage will write `details_needed`, `evaluate`, or `dismissed` from the agent.

### 4. Manual actions in the detail view

The Pipeline detail view gains three actions:

- **Mark for details** → `details_needed`
- **Mark for evaluation** → `evaluate`
- **Dismiss** → `dismissed`

Each is a state change, not a starting action. The actual work (fetching details, evaluating) happens from a separate button or batch action. This is the same separation Phase 2.5 established with its delete gate: the marking is cheap and reversible; the work happens on confirmation.

`dismissed` rows are hidden from the default Pipeline view (the `All` filter excludes them) but visible under a `Dismissed` filter. A `details_needed` or `evaluate` row shows its queue state as a badge next to the status badge.

### 5. Batch "Fetch descriptions for flagged"

A Pipeline header action: **Fetch descriptions (12)**. It runs `manual::refresh_job` sequentially for each `details_needed` row, cancellable between jobs, with per-row progress. Bounded at 20 jobs per click for the same reason Fetch all is bounded — a click is a decision.

On success, each row moves to `evaluate` (the user flagged it for details, so once details exist the natural next step is evaluation) with a badge on the detail view. On failure, the row stays `details_needed` and reports the error in the header.

### 6. Migration 0005

```sql
ALTER TABLE job_descriptions ADD COLUMN queue_state TEXT NOT NULL DEFAULT 'new';
ALTER TABLE job_descriptions ADD COLUMN queue_reason TEXT;
ALTER TABLE job_descriptions ADD COLUMN queued_at TEXT;
CREATE INDEX ix_job_descriptions_queue_state ON job_descriptions(queue_state);
```

`queue_state` is free text (consistent with `applications.status` and `job_descriptions.fetch_tier`). The index makes the filter fast even at the 500-row pipeline limit. `queue_reason` is a short note the user (and later the agent) can attach: "seniority mismatch", "wants details before deciding", etc.

No backfill: `new` is correct for every existing row.

---

## Workstream 1 — Preferences model and UI

**Estimated duration:** 1 day.

### Deliverables

1. `core::profile::Preferences` struct with `deserialize`/`serialize` from `profile.preferences`, plus a `matches(&JobRow) -> PreferenceMatch` method.
2. `profile::update_preferences` repository helper.
3. A **Preferences** section in Settings with five input rows (companies, locations, keywords, exclusions, seniority), each a comma-separated list input, and a short explanation of what each does.
4. A unit test table for `matches`: preferred company hits a substring, preferred location hits "Remote" and "Berlin, Germany", an exclusion hits a substring, a keyword matches in title or description, and an empty preference object matches everything as "not preferred, not excluded."

### Acceptance criteria

| Criterion | Verification |
| :--- | :--- |
| Preferences round-trip through `profile.preferences` | Integration test |
| Every match case in the table behaves as documented | Unit test |
| The Settings section renders and saves | `egui_kittest` |
| An empty preferences object excludes nothing and badges nothing | Unit test |

---

## Workstream 2 — Preference-aware Pipeline

**Estimated duration:** 0.5 day.

### Deliverables

1. `JobRow` gains `preferred: bool` and `excluded: bool`, set by `PipelineViewModel::from_jobs` using `Preferences::matches`.
2. The Pipeline row shows a small **Preferred** badge when `preferred` is true.
3. `JobFilter` gains `Preferred` and `Excluded`. The `All` filter excludes `excluded` rows.
4. The Pipeline header shows a one-line summary when preferences are set: "3 preferred of 45 shown; 2 excluded companies hidden."
5. `egui_kittest`:
   - A row with a preferred company shows the badge.
   - The `Preferred` filter shows exactly the preferred rows.
   - The `Excluded` filter shows exactly the excluded rows.
   - `All` hides excluded rows.

### Acceptance criteria

| Criterion | Verification |
| :--- | :--- |
| Preferred badges appear on matching rows | `egui_kittest` |
| `All` hides excluded rows; `Excluded` shows them | `egui_kittest` + unit test |
| The header summary counts are accurate | Unit test |

---

## Workstream 3 — Queue state model and detail actions

**Estimated duration:** 1 day.

### Deliverables

1. `QueueState` enum (`new`, `details_needed`, `evaluate`, `dismissed`, `evaluated`) in `core::model`, with `as_str`/`parse`.
2. Migration 0005 (as above), and the `JobDescription` model gains `queue_state: String`, `queue_reason: Option<String>`, `queued_at: Option<DateTime<Utc>>`.
3. `job_descriptions::set_queue_state(id, state, reason: Option<&str>)` and `set_queue_states(ids, state, reason)` for batch use.
4. The Pipeline detail view gains three actions: **Mark for details**, **Mark for evaluation**, **Dismiss**. Each calls `set_queue_state`. A row already in the state shows the state's label instead of the action.
5. A queue-state badge next to the status badge in the Pipeline list.
6. Phase 3's `evaluation_reports` write path sets `queue_state = evaluated` on the row it evaluates, inside the same transaction as the report insert. Phase 3 lands after this phase, so the column exists by then and no conditional guard is needed.
7. `egui_kittest`:
   - Mark for details sets the state and shows the label.
   - A second click on the same action is a no-op.
   - Dismiss hides the row from `All` and shows it under `Dismissed`.

### Acceptance criteria

| Criterion | Verification |
| :--- | :--- |
| Migration 0005 applies to a 0004 database and preserves rows | Migration test |
| Each queue-state transition round-trips | Integration test |
| The detail actions set the state and are idempotent | `egui_kittest` |
| The queue-state badge renders next to the status badge | `egui_kittest` |
| Phase 3 sets `evaluated` on evaluation | Integration test (Phase 3's own suite extends to assert this) |

---

## Workstream 4 — Batch fetch descriptions

**Estimated duration:** 0.5 day.

### Deliverables

1. `manual::fetch_descriptions_batch(pool, ids, source, browser, cancel, progress) -> Result<BatchReport>`: sequential `refresh_job` for each id, cancel between jobs, bounded at 20 ids per call. One entry per attempt: `Ok(bytes_added)` or `Err(cause)`.
2. Pipeline header action **Fetch descriptions (N)** where N is the count of `details_needed` rows, disabled when N is 0. Progress and Stop in the header, following the Phase 2.5 batch progress pattern.
3. On success, each row's `queue_state` advances from `details_needed` to `evaluate`. A failure leaves the state alone and reports the error in the header.
4. `egui_kittest`:
   - The action is disabled with zero flagged rows.
   - The action fetches each flagged row in sequence.
   - Stop cancels between jobs.
   - Successful rows move to `evaluate`.
5. An app integration test through `career_ops_app` with a `MockFetchSource` returning descriptions for the flagged ids.

### Acceptance criteria

| Criterion | Verification |
| :--- | :--- |
| The action is gated on the flagged count | `egui_kittest` |
| Progress and Stop work | App test |
| Successful rows move to `evaluate`, failures stay `details_needed` | Integration test |
| The batch is bounded at 20 per click | Core test |

---

## Workstream 5 — Docs, verification, retrospective

**Estimated duration:** 0.5 day.

1. Manual script: new section **6b. Preferences and Queue** (between §6a CV and the existing filters section, keeping the following sections numbered):
   - Set preferences, save, see the badges and the header summary.
   - Flag a row for details, run Fetch descriptions, see the row move to `evaluate`.
   - Dismiss a row; verify it hides from All and appears under Dismissed.
2. `docs/manual-test-script.md` §6 filters section: add `Preferred` and `Excluded`.
3. Parent plan: v0.14 changelog, Phase 2.9 row, and the Phase 3 row updated to depend on 2.9.
4. Full validation, retrospective, and a note in Phase 3.2's Entry Conditions that the queue exists.

---

## Test Strategy

| Layer | Covers |
| :--- | :--- |
| Core unit | `Preferences::matches` table; `QueueState::parse`/`as_str` |
| Core integration | Migration 0005; every queue-state transition; batch fetch descriptions |
| View model | `PipelineViewModel` preferred/excluded flags; filter partitioning |
| `egui_kittest` | Preferences UI; badges; queue actions; batch header |
| App | Preferences → Pipeline → mark → batch fetch through the real database |
| Manual | §6b on a real portal |

No network for any new test. No new dependency. One migration (0005).

---

## Risks

| ID | Risk | Impact | Mitigation |
| :--- | :--- | :--- | :--- |
| P2.9-R1 | **Preference matching surprises.** A substring like "Berlin" matches "Berlin, Germany" and "New Berlin." | Low | Document substring behavior in the Settings hint. An explicit `excluded_companies` lets the user un-hide a false positive. |
| P2.9-R2 | **Queue state drifts from application status.** A row can be `evaluate` in the queue and `discovered` in the application. | Medium | The two are intentionally orthogonal; a row's queue state does not change its application status, and evaluation moves it from `evaluate` to `evaluated` while the application moves from `discovered` to `evaluating` to `ready`. The badge row shows both. |
| P2.9-R3 | **Batch fetch descriptions runs into a WAF.** | Low | Sequential with pacing (from Phase 2.2's `http` pacing), bounded at 20, cancellable. |
| P2.9-R4 | **The new columns change rows Phase 3 depends on.** | Low | `new` is a safe default; Phase 3 only reads `raw_text`, the application link, and now `queue_state`. |
| P2.9-R5 | **Preferences leak into prompts.** | Low | Phase 3 does not read preferences; Phase 3.2 will, with its own provenance. |

---

## Exit Criteria

1. Preferences save and reload; the matching rules are covered by the unit test table.
2. The Pipeline badges preferred rows and can filter by Preferred; `All` hides excluded companies, and `Excluded` shows them.
3. `job_descriptions` has a queue state with the five values, populated by the detail view's manual actions.
4. The queue state badge renders next to the status badge.
5. **Fetch descriptions (N)** runs a bounded, cancellable batch, advances successful rows to `evaluate`, and reports failures.
6. Migration 0005 applies to a 0004 database and preserves rows.
7. No new dependency and no change to acquisition semantics.
8. Offline suite, fmt, and clippy are clean.
9. Manual script §6b, parent plan, and retrospective are updated.

---

## Open Questions

| # | Question | Position |
| :--- | :--- | :--- |
| Q-35 | **Should excluded companies also be excluded from fetch?** | No. Fetch is the acquisition path; the exclusion is a Pipeline view filter. Fetching is cheap and the user may reverse an exclusion; filtering at fetch time would hide data the user cannot recover without re-fetching. |
| Q-36 | **Should the queue state be exposed in the CSV/export later?** | Not in this phase. Phase 6's migration/export story is the right place to decide. |
| Q-37 | **Should "Fetch descriptions" also evaluate?** | No. The two are separate actions so the user controls cost. Phase 3.2's triage may combine them for a supervised batch; Phase 2.9 keeps them apart. |

---

## What Phase 2.9 Is Not

- Not a change to fetch semantics (Phase 2.8 owns those).
- Not an evaluation pipeline (Phase 3 owns that).
- Not a preference-inference system; the user types their preferences.
- Not a change to application status; the queue is a separate pre-evaluation work state.
