# Phase 2.7: CV / Profile Editor — Detailed Execution Plan

**Parent plan:** Draft Execution Plan: Career-Ops Rust Desktop Application (v0.12), §12
**Status:** In progress — WS1 and WS2 implemented, WS3 (docs, verification,
retrospective) pending
**Last updated:** 2026-11-06
**Duration:** 3–5 days
**Depends on:** Phase 1 (profile and `cv_entries` schema/repositories), Phase 2.1 (shell, components, `async_bridge`)
**Blocks:** Phase 3 (candidate context for `{{cv_summary}}`)

---

## Change Log

| Version | Change |
| :--- | :--- |
| v0.1 | Initial plan. Phase 3 needs candidate context, but the CV / Profile screen is still a placeholder and first launch never creates a profile row. This phase ships the editor, ensures the singleton lazily, and defines one canonical `cv_summary` renderer in core so the UI preview and Phase 3 cannot drift. No schema change, no new dependency. |
| v0.2 | WS1 implemented: `ProfileDetails` (with `PartialEq` for action comparisons) and `update_details`; `render_cv_summary` / `cv_summary`; ten renderer unit tests in `profile.rs` and six integration tests in `crates/core/tests/cv_profile.rs`, which also give the `cv_entries` CRUD and reorder paths their first coverage. Every kind shares one entry line, so a title-only skill renders as `- Kubernetes`. |
| v0.3 | WS2 implemented: `view_models/cv_profile.rs` (form, rows, drafts, grouping, move arithmetic, preview through the renderer) and `screens/cv_profile.rs` (identity grid, summary, gated **Save profile**, three entry groups with add/edit forms, two-step inline delete, move gating, preview). `app.rs` loads the singleton on first visit, runs save and entry writes as tasks, and resets the screen on unlock. 10 kittest cases in `crates/app/tests/cv_profile_screen.rs`, three app-level database tests, and the sidebar test now reaches **CV / Profile**. |

---

## Purpose

Phase 3's evaluation and cover-letter prompts receive a `{{cv_summary}}`
variable. The data model for it already exists:

- `profile` is a singleton row with name, email, phone, location, and summary.
- `cv_entries` holds experience, education, and skill rows with `sort_order`.
- `repo::profile::{ensure_singleton, get, update_identity}` and
  `repo::cv_entries::{list_for_profile, create, update, delete, reorder}` exist.
  The `cv_entries` CRUD and reorder paths have no integration tests yet; this
  phase adds them.

What is missing is the **user-facing way to fill them**, and one **canonical
rendering** of the result. Today:

- `MainScreen::CvProfile` renders a placeholder (`screens/shell.rs`).
- First launch does not call `ensure_singleton`, so a fresh database has no
  profile row at all.
- There is no `{{cv_summary}}` renderer; Phase 3 would have to invent the format
  inside prompt assembly.

Phase 2.7 closes those three gaps. It is deliberately small: it does not import
the original project's `cv.md`/`profile.yml`, does not render a CV, and does not
call an LLM. Those belong to Phase 6, Phase 4, and Phase 3 respectively.

---

## Scope

| In scope | Out of scope |
| :--- | :--- |
| Edit the singleton profile: name, email, phone, location, summary | Importing `cv.md` / `config/profile.yml` from the original repo (Phase 6) |
| Add, edit, delete, and reorder CV entries (experience, education, skill) | CV/PDF rendering and templates (Phase 4) |
| One canonical `cv_summary` renderer in core, used by the UI preview and Phase 3 | AI rewriting, tailoring, or scoring of the CV (Phase 3) |
| Lazy `ensure_singleton` when the screen loads | Editing `archetypes` / `preferences` JSON (later) |
| Loading, saving, and error states; a summary preview | Multiple profiles, multiple CVs, attachments |
| `egui_kittest` and app-level tests; manual script steps; retrospective | Writing samples / voice DNA / interview story bank |

No migration: every column already exists in `0001_initial.sql`.

---

## Design decisions

### 1. One profile, one summary renderer

`profile` is a singleton (`id = 1`). The screen calls
`profile::ensure_singleton` on load, so the row exists after the first visit and
never has to be created by Phase 3.

The summary format lives in **one pure function** in core:

```rust
/// `None` when the profile has no name, no summary, and no entries.
pub fn render_cv_summary(profile: &Profile, entries: &[CvEntry]) -> Option<String>;
```

A repository helper loads the singleton and entries, then calls it:

```rust
pub async fn cv_summary(pool: &SqlitePool) -> Result<Option<String>>;
```

The UI's preview and Phase 3's `{{cv_summary}}` both call the repository helper
(or the pure function with fixture data in tests). No second format, no
formatting in the prompt layer.

**Format (deterministic, plain text):**

```
{name}
{location} | {email} | {phone}

Summary:
{summary}

Experience:
- {title} — {organization} ({start} - {end})
  {description}

Education:
- {title} — {organization} ({start} - {end})

Skills:
- {title}
```

Every kind uses the same entry line: title, optional organization, optional
date range, and an optional indented description. Empty parts are omitted, so a
skill that only has a title renders as `- Kubernetes`.

- Empty fields are skipped; an empty section is omitted.
- Entries are ordered by `sort_order` within each kind; sections are emitted in
  the fixed order Experience, Education, Skills.
- Fields are trimmed; descriptions keep their internal line breaks but lose
  leading/trailing blank lines.
- No HTML, no markdown decoration beyond the plain list markers.
- `None` when name, summary, and all entries are empty. Phase 3 turns `None`
  into an actionable "add your CV" error rather than an empty prompt section.

### 2. Free-text dates, minimal validation

The schema stores dates as `TEXT`. Phase 2.7 keeps them free text (`2021`,
`Jan 2021`, `Present`) and validates only that an entry's **title** is
non-empty. A date picker would reject legitimate CV formats and is out of scope.

### 3. Display groups, persisted order

Entries are displayed grouped by kind (Experience, Education, Skills), each group
in `sort_order` order. **Move up/down** operates within the group; saving the new
order rewrites the complete list in display order with
`cv_entries::reorder` (Experience entries first, then Education, then Skills).
This keeps the persisted `sort_order` identical to what the screen shows, so a
reload never reshuffles.

New entries use `cv_entries::create`, which appends to the global order; the
grouped display places them at the end of their kind, and the next reorder
normalizes the stored order.

### 4. Save model

- Profile fields (identity + summary) are edited in one form and saved with an
  explicit **Save profile** button. The button is disabled until something
  changes, and a status line reports success or the typed error.
- Each entry has its own form (add or edit) with **Save** and **Cancel**. Entry
  saves are immediate; a failed save keeps the form open with the error.
- Delete is two-step inline (**Delete** → **Confirm delete** / **Cancel**) rather
  than a modal. The row is user-authored but small; the two-step affordance
  prevents a stray click without adding another window.

### 5. The editor is the empty state

A first-time user sees the form with placeholders and a short hint, not a
separate "add your CV" gate. The summary preview explains what Phase 3 will see.

### 6. Data is private by default

CV data is user data. It is not logged, exported, or sent anywhere in this phase.
Phase 3's Settings disclosure is the first place it leaves the machine.

---

## Workstream 1 — Core: profile updates and `cv_summary`

**Estimated duration:** 1–1.5 days.

### Deliverables

1. `profile::update_details` (or an equivalent struct-based update) covering
   name, email, phone, location, and summary. `update_identity` stays for
   compatibility or becomes a thin wrapper.
2. `profile::ensure_singleton` is safe to call on every screen load and from
   `cv_summary`.
3. `render_cv_summary(&Profile, &[CvEntry]) -> Option<String>` (pure) and
   `cv_summary(&SqlitePool) -> Result<Option<String>>` (loads and renders).
4. Unit tests:
   - Exact fixture output for a profile with all three kinds.
   - Sections omitted when a kind has no entries; empty fields skipped.
   - Ordering by `sort_order` within each kind and fixed section order.
   - `None` for the empty profile; `None` when only blank fields exist.
   - Trimmed fields; descriptions keep internal newlines; no HTML.
   - Deterministic: two calls with the same data produce the same string.
5. Repository tests: update round-trip; `ensure_singleton` is idempotent;
   `cv_summary` reads the freshly saved data; and the existing `cv_entries`
   create/update/delete/reorder paths get their first integration tests.

### Acceptance criteria

| Criterion | Verification |
| :--- | :--- |
| Profile identity, contact fields, and summary round-trip | Integration test |
| `ensure_singleton` is idempotent and safe on a fresh database | Unit test |
| The summary renderer produces the documented format | Unit test with a fixture |
| Empty profile returns `None`, not an empty string | Unit test |
| Section order and per-kind ordering are deterministic | Unit test |
| Phase 3 can call `cv_summary` | Function is `pub` and tested |

---

## Workstream 2 — CV / Profile screen

**Estimated duration:** 1.5–2 days.

### Deliverables

1. `view_models/cv_profile.rs`: `CvProfileViewModel`, `CvEntryRow`, form state,
   and pure grouping/ordering helpers (grouped by kind, sorted by `sort_order`).
2. `screens/cv_profile.rs` with `CvProfileAction` and `CvProfileRenderState`:
   - Identity form: name, email, phone, location.
   - Summary multiline.
   - **Save profile** (disabled until dirty), status line, loading spinner.
   - Summary preview (read-only, monospace or wrapped) generated by
     `render_cv_summary`.
   - Three entry groups; each row shows title, organization, dates, and actions
     **Edit**, **Delete**, **Move up**, **Move down**.
   - Add entry form per group with kind fixed by the group, title required.
   - Delete’s two-step inline confirm.
3. App wiring: on `MainScreen::CvProfile`, load the profile and entries once
   (`ensure_singleton` + `list_for_profile`); spawn save/entry tasks through
   `async_bridge`; render state from the tasks; clear the view on vault reset.
4. Replace the `CvProfile` placeholder in `screens/shell.rs` with the real
   screen; keep the placeholder path for the other screens.
5. `egui_kittest` cases:
   - Identity fields are queryable by their labels.
   - Saving emits the profile action; the button is disabled until dirty.
   - Empty title keeps the entry save disabled.
   - Adding an entry updates the group.
   - Delete asks for confirmation; Cancel keeps the row.
   - Move up/down changes the displayed order.
   - The preview matches the renderer's output.
6. App-level test: save identity + summary, add an entry, reload the screen, and
   assert the data and `cv_summary` survive.

### Acceptance criteria

| Criterion | Verification |
| :--- | :--- |
| The screen renders the identity, summary, and three entry groups | `egui_kittest` |
| Profile saves are explicit and disabled when unchanged | `egui_kittest` |
| Entry create, edit, delete, and reorder persist | App integration test |
| Move up/down rewrites the full order and survives reload | App integration test |
| The preview is generated by the same function Phase 3 uses | `egui_kittest` + unit test |
| A failed save keeps the form and shows the error | App test |
| The CV screen is reachable from the sidebar | Existing navigation test extended |

---

## Workstream 3 — Docs, verification, retrospective

**Estimated duration:** 0.5 day.

1. Manual script: new section **6a. CV / Profile** (between Pipeline and Idle
   behaviour, so existing section numbers stay stable):
   - Fill identity and summary, save, reload.
   - Add one entry of each kind; edit; move; delete with confirm.
   - Preview shows the canonical summary.
   - A fresh database shows the editor, not an empty placeholder.
2. Document the canonical summary format in the renderer's doc comment and
   reference it from Phase 3's plan. No separate user-facing doc is needed.
3. Full validation: `cargo fmt --all --check`, `cargo clippy --workspace
   --all-targets --offline -- -D warnings`, `cargo test --workspace --features
   career-ops-app/keychain --offline -- --skip obscura`.
4. Short retrospective: `.agents/plans/phase_2.7_retrospective.md`.
5. Parent plan: Phase 2.7 row, changelog, and the Phase 3 dependency updated.

---

## Test Strategy

| Layer | Covers |
| :--- | :--- |
| Core unit | `render_cv_summary` format, empty/None, section and per-kind order, trimming |
| Core integration | Profile update round-trip; `ensure_singleton`; `cv_summary` reads saved data |
| View model | Grouping by kind; move up/down arithmetic; dirty state |
| `egui_kittest` | Fields, save gating, entry forms, delete confirm, preview |
| App | Load → edit → save → reload through the real database |
| Manual | §6a on a fresh and a populated data directory |

No network. No new dependency. No migration.

---

## Risks

| ID | Risk | Impact | Mitigation |
| :--- | :--- | :--- | :--- |
| P2.7-R1 | **Summary format drift** between the preview and Phase 3. | Medium | One `render_cv_summary` function; the UI and Phase 3 both call it; exact-output tests. |
| P2.7-R2 | **Reordering across groups** produces a confusing persisted order. | Low | Grouped display; move within a group; persist the full display order on every move; reload test. |
| P2.7-R3 | **Sensitive data leaks into logs.** | Medium | No prompt/value logging; errors carry field names, not values; no export in this phase. |
| P2.7-R4 | **Free-text dates are inconsistent.** | Low | Accepted by design; the summary prints what the user typed. A future import can normalize. |
| P2.7-R5 | **Scope creep into CV import or rendering.** | Medium | Explicitly out of scope; Phase 6 owns import, Phase 4 owns rendering. |
| P2.7-R6 | **An empty profile reaches Phase 3.** | Low | `cv_summary` returns `None`; Phase 3 turns that into an actionable "add your CV" error. |

---

## Exit Criteria

1. The **CV / Profile** screen is reachable from the sidebar and replaces the
   placeholder.
2. Profile identity, contact fields, and summary save and reload.
3. Entries of all three kinds can be added, edited, deleted (with confirm), and
   reordered; the order survives a reload.
4. One canonical `render_cv_summary` / `cv_summary` exists in core and is used by
   both the preview and (later) Phase 3.
5. An empty profile returns `None`, and the format is documented and tested.
6. No schema change and no new dependency.
7. The offline suite, fmt, and clippy are clean.
8. Manual script §6a, the parent plan, and a short retrospective are updated.

---

## Open Questions

| # | Question | Position |
| :--- | :--- | :--- |
| Q-30 | **Should the app import the original `cv.md` / `profile.yml`?** | Not now. Phase 6 owns file migration; an importer can call the same repositories later. |
| Q-31 | **Should `archetypes` / `preferences` be editable?** | Not now. They are prompt-influencing metadata with no agreed UI; the summary and entries are what Phase 3 needs. |
| Q-32 | **Should deleting a CV entry affect prior evaluations?** | No. Reports are historical records; a later evaluation uses the current CV. |

---

## What Phase 2.7 Is Not

- Not a CV importer or a file migration tool.
- Not a CV renderer or PDF generator (Phase 4).
- Not an AI CV writer or tailoring feature (Phase 3).
- Not a multi-profile or multi-CV system.
- Not a date parser, rich-text editor, or attachment manager.
