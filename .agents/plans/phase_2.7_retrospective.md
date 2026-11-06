# Phase 2.7 Retrospective and Verification Record

**Phase:** CV / Profile Editor
**Parent plan:** Draft Execution Plan: Career-Ops Rust Desktop Application (v0.13), §12
**Phase plan:** `.agents/plans/phase_2.7_cv_profile_editor.md` (v0.4)
**Period:** 2026-11-06
**Estimate:** 3–5 days
**Actual:** one session across three workstreams
**Status:** Complete against the automated criteria; manual script §6a is ready for the owner's pass.

---

## Summary

Phase 3 asks for a `{{cv_summary}}` variable, and the schema for one had been
sitting unused since Phase 1: a `profile` singleton and ordered `cv_entries`
rows. Nothing wrote to them, nothing displayed them, and first launch never even
created the profile row. Phase 2.7 closed all three gaps without a migration and
without a new dependency.

Two decisions shaped the work:

1. **One renderer, in core.** `repo::profile::render_cv_summary` is the only
   function that turns a profile and its entries into text. The screen's preview
   calls it and so will Phase 3, so the two cannot drift. It returns `None` for
   an empty profile, which is the signal Phase 3 needs to say "add your CV"
   instead of sending an empty section to a model.
2. **The screen is the empty state.** There is no gate in front of the editor.
   A first-time user sees the form, three empty groups, and a preview that
   explains what Phase 3 will see.

The editor covers identity and summary (explicit save, gated on dirty), three
entry groups with add/edit forms, a two-step inline delete, and move up/down
within a group. Every entry write reloads the list from the database, which is
what keeps the persisted `sort_order` identical to what the screen shows.

---

## Exit criteria status

| # | Criterion | Status | Evidence |
| :--- | :--- | :--- | :--- |
| 1 | **CV / Profile** is reachable from the sidebar and replaces the placeholder | ✅ | `sidebar_navigation_switches_the_rendered_screen` selects it; `app.rs` routes `MainScreen::CvProfile` to `screens::cv_profile::show`. The leftover arm in `shell::placeholder` is only reachable from `shell::show`, which the app no longer uses for content. |
| 2 | Identity, contact fields, and summary save and reload | ✅ | `cv_profile_saves_entries_and_reloads_through_the_app` saves, reloads from SQLite, and asserts every field. |
| 3 | Entries of all three kinds can be added, edited, deleted (with confirm), and reordered; the order survives a reload | ✅ | `a_draft_needs_a_title_and_trims_the_rest`, `moving_down_swaps_within_the_group_and_returns_the_full_order`, `delete_asks_for_confirmation_and_cancel_keeps_the_row`, `moving_a_cv_entry_persists_the_display_order` |
| 4 | One canonical `render_cv_summary` / `cv_summary` used by the preview and later by Phase 3 | ✅ | `the_preview_matches_the_renderer`, `the_preview_is_the_renderer_output`, and the app test compares the stored `cv_summary` with the editor's preview |
| 5 | An empty profile returns `None`; the format is documented and tested | ✅ | `an_empty_profile_has_no_summary`, `contact_details_alone_are_not_candidate_data`, `an_empty_editor_has_no_preview`; format in the renderer's doc comment |
| 6 | No schema change and no new dependency | ✅ | `migrations/` unchanged; `crates/app/Cargo.toml` unchanged |
| 7 | Offline suite, fmt, and clippy are clean | ✅ | See the command table below |
| 8 | Manual script §6a, the parent plan, and a short retrospective are updated | ✅ | `docs/manual-test-script.md` §6a, `main.md` v0.13, this document |

**Verification commands and results (2026-11-06):**

| Command | Result |
| :--- | :--- |
| `cargo fmt --all --check` | clean |
| `cargo clippy --workspace --all-targets --offline -- -D warnings` | clean |
| `cargo test --workspace --features career-ops-app/keychain --offline -- --skip obscura` | all passed; 107 lib tests, 10 CV-screen tests, 6 core CV integration tests |
| `cargo test -p career-ops-app --features career-ops-app/keychain --offline --test cv_profile_screen` | 10 passed |

No live tests. The phase touches no network path.

---

## What the implementation found

### F1 — Clippy caught the test helper, not the code (tooling) — handled

The renderer's fixture helper took eight arguments, one past the lint's limit.
It became a small `EntrySpec` builder (`entry(id, kind, title).org(..).dates(..)`)
rather than an `#[allow]`, which also made the fixtures read as data. The lesson
is the usual one: a lint on test code is still a lint, and a builder is a better
answer than a suppression.

### F2 — A screen that emits actions does not mutate data (test-design lesson)

The first kittest for "move down" asserted the reordered rows right after the
click and failed: `screens::cv_profile::show` only returns `CvProfileAction`, and
the app applies `CvProfileViewModel::move_entry`. The test now applies the action
the way the app does, then asserts both the view-model order and the rendered
order (comparing the rows' `rect().top()`). That is a more honest test of the
contract than teaching the screen to reorder locally.

### F3 — Dirty state is a comparison, not a flag (design note)

`CvProfileViewModel` keeps the persisted `Profile` next to the editable
`ProfileForm`, so `is_dirty()` is `form != ProfileForm::from_profile(&row)`. A
save adopts the row the repository returned, which is the trimmed one, so a
whitespace-only edit saves once and then resets instead of staying dirty forever.
`a_whitespace_only_edit_is_dirty_until_it_is_saved` locks that in.

### F4 — `reorder` renumbers the whole table (design note)

`cv_entries::reorder` writes `0..n` over the ids it is given, so a move must
persist the **full** display order, not a pair swap. `move_entry` returns exactly
that list (Experience, then Education, then Skills) and renumbers the in-memory
rows the same way, which is why the next frame shows the new order without a
reload. Every other entry write reloads the list for the same reason: the
database is the canonical order.

### F5 — An edit must not clear `metadata` (data-safety)

`cv_entries.metadata` is JSON that the editor does not expose. `update` writes
every column, so a draft built with `metadata: None` would silently erase it.
`CvEntryDraft::to_new_entry` takes the stored value and parses it back, and
`an_edit_preserves_metadata` covers it. No writer sets metadata today, but Phase
6's importer will.

### F6 — Forcing a save failure needs a seam (testing)

"A failed save keeps the edits" is an exit criterion, and there is no natural
failure. The app-level test closes the pool (`SqlitePool::close`) instead of
corrupting the schema, so the write fails like any other I/O error and the test
can assert that the typed text survives and the error names the action.

### F7 — Unknown entry kinds are hidden, not guessed (design note)

`CvEntryKind::parse` returns `None` for an unknown string. The view-model drops
such a row instead of mapping it to a default, because showing it as Experience
and then saving would rewrite `kind`. The editor is the only writer today; an
import path must use the enum.

---

## Estimate accuracy

| Workstream | Plan | Actual |
| :--- | :--- | :--- |
| WS1 core: profile updates and `cv_summary` | 1–1.5 days | Part of 2026-11-06 |
| WS2 screen, app wiring, tests | 1.5–2 days | Part of 2026-11-06 |
| WS3 validation, manual script, retrospective | 0.5 day | Part of 2026-11-06 |
| **Total** | **3–5 days** | **One session** |

The estimate assumed the original project's `cv.md`/`profile.yml` import would
appear here; it did not (Q-30, Phase 6), and the screen is smaller than a
rendered CV would be.

---

## Deferred, unchanged

- **Q-30 `cv.md` / `profile.yml` import.** Phase 6 owns file migration, and an
  importer can call `profile::update_details` and `cv_entries::create` directly.
- **Q-31 `archetypes` / `preferences`.** Prompt-influencing metadata with no
  agreed UI; the summary and entries are what Phase 3 needs.
- **Q-32 deleting an entry and prior evaluations.** Reports are historical
  records; a new evaluation uses the current CV.
- **CV rendering.** Phase 4. This phase stores and summarizes, it does not
  typeset.

None of these blocks Phase 3. Phase 3 can call `profile::cv_summary` as soon as
WS1 starts.
