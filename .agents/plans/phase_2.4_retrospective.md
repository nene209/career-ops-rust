# Phase 2.4 Retrospective and Verification Record

**Phase:** Known-Site Catalog
**Parent plan:** Draft Execution Plan: Career-Ops Rust Desktop Application (v0.9), §12
**Phase plan:** `.agents/plans/phase_2.4_known_site_catalog.md` (v0.4)
**Period:** 2026-11-03
**Estimate:** 1–2 days
**Actual:** one day, three commits
**Status:** Complete against all eight exit criteria.

---

## Summary

Phase 2.3 made the registry work; Phase 2.4 made it findable. Before this phase
the only way to use an embedded profile was to type the exact URL the app
already knew, so the knowledge was invisible. Now **Add portal** opens with the
list, and the form names the profile it came from.

The phase added no storage, no schema change, no dependency, and no new provider.
It is a view over `known_sites::all()` plus two decisions that keep it honest:

1. **One prefill path.** The chooser and the typed-URL prefill both call
   `apply_known_site`, and a unit test compares the resulting `PortalFormData`
   for every entry, so the two cannot drift.
2. **Attribution is derived, not stored.** The source line, the modified marker,
   and the restore action all come from `lookup(form.url)` plus
   `matches_known_site`. Nothing was added to `portals`.

The only surprise was that a phase this small still found an assumption baked
into an existing test (F2), which is a useful reminder that defaults are part of
a contract.

---

## Exit criteria status

| # | Criterion | Status | Evidence |
| :--- | :--- | :--- | :--- |
| 1 | **Add portal** opens a chooser listing every entry, sorted by name, with provider and date, plus **Other site** | ✅ | `open_add_opens_the_known_site_chooser` (app), `the_add_chooser_lists_known_sites_and_other` (kittest), `known_sites_are_listed_alphabetically` (unit; asserts sorted and no entry dropped) |
| 2 | Choosing an entry stores the same config as typing its URL | ✅ | `choosing_a_site_builds_the_same_form_as_typing_its_url` compares `PortalFormData` for every registry entry |
| 3 | **Other site** opens the blank form; unknown hosts behave as in 2.3 | ✅ | `choosing_other_site_opens_the_blank_form`, `an_unknown_site_has_no_known_site_affordances` |
| 4 | Add and edit name the source, mark divergence as modified, and restore | ✅ | `a_matching_config_shows_no_modified_marker`, `restore_known_config_replaces_a_divergent_config`, `editing_a_known_portal_shows_its_source_and_can_restore`, `a_reformatted_known_config_is_not_modified`, `changing_a_known_value_is_modified` |
| 5 | No schema change, no new dependency, no second copy of a config | ✅ | `git diff` touches no migration and no `Cargo.toml`; the chooser reads `known_sites::all()` and the form stores via `data()`, which already existed |
| 6 | New paths covered offline; suite and clippy clean | ✅ | See the command table below |
| 7 | Manual script and `docs/known-sites.md` describe the chooser, source line, and restore | ✅ | Manual script §4.2a, §4.3, §4.3a, §4.3b.2; `docs/known-sites.md` "In the app" |
| 8 | A short retrospective is recorded | ✅ | This document |

**Verification commands and results (2026-11-03):**

| Command | Result |
| :--- | :--- |
| `cargo fmt --all --check` | clean |
| `cargo clippy --workspace --all-targets --offline -- -D warnings` | clean |
| `cargo test -p career-ops-app --features keychain --offline --test portals_screen` | 27 passed |
| `cargo test -p career-ops-app --features keychain --offline --lib -- screens::portals` | 13 passed |
| `cargo test --workspace --features career-ops-app/keychain --offline -- --skip obscura` | all passed (`obscura` skipped: the sandbox forbids binding a port) |

No network was needed: the registry entries are constants, and every new test
either renders the modal or inspects the form it produces.

---

## What the implementation found

### F1 — A borrow forced a deferred decision (design note)

`modals()` matches `&mut view.modal` to render the open dialog. The chooser's
buttons need to replace that modal, which would be a second mutable borrow of the
same field inside the match arm. Rather than restructure the match, the chooser
returns a local `SiteChoice` that is applied after the match ends.

**Why it matters.** The next person who adds a chooser-like dialog should follow
the same shape instead of fighting the borrow with `take()`, which would make the
dialog flash closed for a frame.

### F2 — An app test encoded the old add flow (small, real) — fixed

`config_preview_reports_and_never_writes` called `PortalsAction::OpenAdd` and then
expected `view.form()` to be `Some`. That was a hidden assumption about what
"Add" means, and changing the first step surfaced it immediately. The fix is one
line (`open_blank_add()` first), but the lesson is broader: when a flow's entry
point changes, grep the tests that drive it by intent, not by widget.

### F3 — Visible notes did not survive scale (design note) — fixed during WS2

The first implementation printed each entry's `notes` under its button. At two
entries that was helpful; at twenty it would be a wall of text. The notes moved
to `on_hover_text`, leaving the scan line as name, provider, and date. The rule
recorded in the plan: rows are for scanning, details are on demand.

### F4 — Attribution is URL-derived, and that is a choice (documented)

`lookup(form.url)` decides whether a form is "based on" an entry, so moving a
known config to a different host removes the source line and the restore action.
The config itself is unaffected. A `known_site` column would preserve
attribution across a URL change at the cost of a migration and a reconciliation
rule; the phase decided that case is not worth the schema. If it becomes real,
the column can be added without changing this design's shape.

### F5 — The marker lags the provider fields by one frame (accepted)

The source/restore block renders before the provider fields it describes, so a
keystroke in the domain field updates the marker on the next frame. That is
invisible at human speed, and moving the block below the fields would read worse.
Tests render after the edit, so they assert the settled state.

---

## Estimate accuracy

| Workstream | Plan | Actual |
| :--- | :--- | :--- |
| WS1 chooser | 0.5–1 day | Part of 2026-11-03 |
| WS2 attribution and restore | 0.5 day | Part of 2026-11-03 |
| WS3 tests, docs, retrospective | 0.5 day | Part of 2026-11-03 |
| **Total** | **1–2 days** | **One day, three commits** |

The estimate was generous for the same reason as the earlier phases: the design
was written down before implementation and every decision had a test seam. The
work that took thought was not code but the boundaries — what a catalog is, what
"modified" means, and where attribution comes from.

---

## What to carry into Phase 3

1. **The database is unchanged.** Phase 3 starts from the same schema; nothing
   in 2.4 needs migrating or backfilling.
2. **Attribution-by-derivation is the pattern.** The source line, the modified
   marker, and restore are computed, not stored. Any future "based on a default"
   UI (prompt templates, for example) should follow the same shape: show where a
   value came from, mark divergence, offer a restore.
3. **The registry grows by constants.** Adding a site means a `KnownSite`, a docs
   row, and a passing live check; the chooser picks it up with no UI change.
   Search is deferred until the list stops being scannable (about 15 entries).
4. **The add flow is now the front door.** Phase 3's evaluation and cover-letter
   screens should assume a portal exists — no additional discovery work belongs
   there.

---

## Open items for the human

- Run the 🌐 rows of `docs/manual-test-script.md`, plus the new §4.2a and §4.3a
  chooser steps, and record the sign-off table.
- Add sites through the checklist in `docs/known-sites.md`; a constant is all the
  chooser needs.
- Decide later whether the extra chooser click is friction worth removing (for
  example, remembering the last choice). No evidence either way from this phase.
