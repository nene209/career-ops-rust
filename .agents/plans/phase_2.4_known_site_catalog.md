# Phase 2.4: Known-Site Catalog — Detailed Execution Plan

**Parent plan:** Draft Execution Plan: Career-Ops Rust Desktop Application (v0.9), §12
**Status:** In progress — WS1–WS2 complete; WS3 not started
**Last updated:** 2026-11-03
**Duration:** 1–2 days
**Depends on:** Phase 2.3 (registry, `apply_known_site`, prefill, provider combo)
**Blocks:** nothing. Phase 3 (AI Integration) is independent of this phase.

---

## Change Log

| Version | Change |
| :--- | :--- |
| v0.1 | Initial plan. Phase 2.3 ships an embedded registry and working prefill, but the only way to reach either is to type a URL the app already knows. This phase makes the registry discoverable: a chooser in the add flow, an "based on" attribution in the form, and a visible restore action. No storage change, no migration, no new dependency. |
| v0.2 | Workstream 1 implemented: `KnownSite::url()` in core, `PortalsView::{open_add, open_blank_add, choose_known_site}`, `PortalModal::ChooseSite`, and the chooser rendering. `OpenAdd` now opens the list instead of a blank form. Attribution and restore are unchanged until WS2. |
| v0.3 | Workstream 2 implemented: the chooser's notes moved to hover text, **Use known config** renamed to **Restore known config**, and a `Modified from the known config.` marker added when the form diverges from the entry. The marker and the action share `matches_known_site`, so whitespace and key order never count as edits. |

---

## Purpose

Phase 2.3 answered "must every user write their own config?" with no: two sites
ship with embedded configs, and typing their URL prefills the form. What it did
not answer is **how a user learns the app knows a site**. Today the path is:

```
Portals → Add portal → blank form → type a URL → (only then) "Known site: …"
```

A user who does not already know `https://jobs.nvidia.com/careers` gets no
benefit from the entry. The registry is, in practice, a hidden feature.

This phase makes the repository of known sites visible and attributable:

1. **Visible.** Adding a portal starts with the list of known sites and an
   explicit "Other site" choice. Nothing is typed to discover them.
2. **Attributable.** The form says which entry it was built from and its
   verification date, in add *and* edit.
3. **Reversible.** A config that diverges from the entry is marked as modified,
   and one action restores the shipped default.

It is deliberately **not** a place to store user-defined configs. The portal row
already is that: a saved portal *is* the user's config, and the registry is the
default it started from.

### Evidence

- `PortalsAction::OpenAdd` sets `modal = Some(PortalModal::Add(PortalForm::default()))`
  (`app.rs`), so the form is always blank.
- `known_sites::lookup` is only called while rendering the form
  (`portal_form_modal`), from the URL field the user has already typed.
- `known_sites::all()` has no other caller outside tests and the live check.

The data model already supports everything this phase needs:

```rust
pub struct KnownSite {
    pub host: &'static str,
    pub path_prefix: Option<&'static str>,
    pub name: &'static str,
    pub provider: Provider,
    pub config: &'static str,
    pub verified_at: Option<&'static str>,
    pub notes: &'static str,
}
```

---

## Scope

| In scope | Out of scope |
| :--- | :--- |
| A known-site chooser at the start of the add flow, including "Other site" | A remote or signed registry (2.3 Q-12 is settled: embedded) |
| Choosing an entry prefills URL, name, provider, and config through the existing `apply_known_site` | Per-user defaults or presets stored in the database |
| "Based on <entry> - verified <date>" attribution in add and edit | Recording `known_site` provenance on the `portals` row |
| A modified marker and a **Restore known config** action | Search/filter in the chooser (deferred until the registry is large) |
| Offline unit + `egui_kittest` coverage for each path | Auto-detection of platforms or providers |
| Docs: manual script, `docs/known-sites.md`, retrospective | A company directory for ATS boards (see Open Questions) |

---

## Design decisions

### 1. A catalog of sites, not providers

A provider is an engine (`selectors`, `eightfold`) and has no default payload of
its own: "Eightfold" does not imply `nvidia.com`. Only a site profile carries a
config. The chooser therefore lists **sites**, and the provider combo stays where
it is for hosts the registry does not know.

### 2. The catalog is a view over `all()`, not new storage

The entries ship in the binary. The chooser iterates `known_sites::all()`,
sorted by name so the order does not depend on declaration order. No table, no
seeding, no migration, and no second copy of a config.

`KnownSite::url()` is added in core so the chooser, the live registry test, and
future callers derive `https://{host}{path_prefix}` in one place instead of three.

### 3. One prefill path

The chooser calls the same `PortalForm::apply_known_site(site)` that the URL
prefill uses. There is no second way to build a known form, so the two entry
points cannot drift. A unit test asserts the equivalence directly: a form built
by choosing an entry stores exactly the same `PortalFormData` as a form built by
typing the entry's URL and letting prefill run.

### 4. The URL stays the key; no provenance column

Attribution and restore are derived from `lookup(form.url)` at render time, as
they are today. Storing `known_site` on the portal row would duplicate the URL,
require a migration, and need reconciliation when a URL changes.

The consequence is documented rather than engineered away: if a user applies a
known config and then points the portal at a different host, the attribution and
the restore action disappear with the match. The saved config is unaffected. If
that case turns out to matter, the column can be added later; it is not needed
for v1.

### 5. "Other site" is always visible

Discoverability must not turn into a gate. The chooser always offers **Other
site**, which opens exactly today's blank form, and the manual fields remain
unchanged for unknown hosts. A user who knows their own config never has to
interact with the catalog beyond one extra click.

### 6. Search is deferred, the component is not hard-coded to two rows

Two entries need a list, not a search box. The chooser takes a slice of
`KnownSite` and renders every item, so adding a filter later is local to the
chooser. The trigger for adding it is the same one `docs/known-sites.md` uses
for curation: when scanning the list stops being instant (roughly 15+ entries).

### 7. Navigation lives in the view

`PortalsView` gains `open_add`, `open_blank_add`, and `choose_known_site`.
`CareerOpsApp` keeps calling `view.open_add()` for `PortalsAction::OpenAdd`;
there is no I/O in these transitions, so no new `PortalsAction` variant is
needed. The methods are directly testable without rendering.

---

## Workstream 1 — The chooser

**Estimated duration:** 0.5–1 day.

### Deliverables

1. `core::acquisition::known_sites::KnownSite::url() -> String`, returning
   `https://{host}{path_prefix or "/"}`, with a unit test covering a prefixed
   entry (Waymo, NVIDIA) and the no-prefix case. The live registry test stops
   building the URL inline.
2. `PortalsView::open_add()`, `open_blank_add()`, `choose_known_site(&KnownSite)`:
   - `open_add` opens `PortalModal::ChooseSite` (a new variant; no payload).
   - `open_blank_add` opens `PortalModal::Add(PortalForm::default())`.
   - `choose_known_site` builds `PortalForm { url: site.url(), ..default }`,
     calls `apply_known_site(site)`, and opens it as `PortalModal::Add`.
3. `PortalModal::ChooseSite` rendering:
   - A window titled "Add portal".
   - One button per entry whose label is `site.label()`
     (`"NVIDIA (Eightfold) - verified 2026-11-03"`), sorted by name, with
     `site.notes` as a weak secondary line.
   - **Other site** → `open_blank_add`; **Cancel** → close.
   - The window returns a local choice (`Option<Choice>`) that is applied after
     `modals()` stops borrowing `view.modal`. Assigning `view.modal` from inside
     the match arm would be a second mutable borrow of the same field, so the
     transition is deferred rather than fought.
   - Direct `view` mutation for the transitions (decision 7).
4. `CareerOpsApp::handle_portals_action(OpenAdd)` calls `view.open_add()`.
5. Update `config_preview_reports_and_never_writes`, which calls `OpenAdd` and
   then expects a form: it must call `view.open_blank_add()` first (or assert
   the chooser and then choose). This is the only existing app test the change
   touches.

### Acceptance criteria

| Criterion | Verification |
| :--- | :--- |
| `OpenAdd` opens the chooser, not a blank form | App/state test |
| The chooser lists every registry entry plus "Other site" | `egui_kittest` |
| Entries are listed alphabetically by name, independent of declaration order | Unit test on the sorted helper (`NVIDIA` before `Waymo`) |
| Choosing an entry prefills URL, company name, provider, and config | `egui_kittest` + state assertion |
| Choosing an entry stores the same config as typing its URL | Unit test comparing `PortalFormData` for every entry |
| "Other site" opens the blank form with empty provider fields | `egui_kittest` |
| `KnownSite::url()` matches the URL the live registry test uses | Unit test; live test updated to call it |

**Implementation note (2026-11-03).** `KnownSite::url()` lives in core and is
now the single way the chooser, the app integration test, and the live registry
check build a board URL. `PortalsView::{open_add, open_blank_add,
choose_known_site}` hold the transitions, so they are testable without a
renderer; `CareerOpsApp::handle_portals_action(OpenAdd)` calls `open_add`, and
no new `PortalsAction` variant was needed because none of this is I/O.

The chooser returns a local `SiteChoice` that `modals()` applies after its
`match &mut view.modal` ends. Assigning `view.modal` inside the arm would be a
second mutable borrow of the same field; deferring one decision is cheaper than
restructuring the match. Entries are sorted by name at render time, so the
constant declaration order never reaches the UI.

One existing test changed: `config_preview_reports_and_never_writes` called
`OpenAdd` and then expected an open form. It now calls `open_blank_add()` first,
which is the only place the old assumption was baked in.

Tests added: `open_add_opens_the_chooser_and_other_site_opens_the_blank_form`,
`known_sites_are_listed_alphabetically`,
`choosing_a_site_builds_the_same_form_as_typing_its_url` (the anti-drift test),
plus four `egui_kittest` cases in `portals_screen.rs` for the list, choosing
NVIDIA, choosing "Other site", and cancelling.

---

## Workstream 2 — Attribution, modified state, and restore

**Estimated duration:** 0.5 day.

### Deliverables

1. Rename the form's action from **Use known config** to **Restore known
   config**. It already does exactly that, and "restore" matches the modified
   state it appears next to. Update the button, the kittest query, the manual
   script, and `docs/known-sites.md`.
2. When the form's effective config differs from the entry, show
   **"Modified from the known config."** next to the source line, together with
   the restore action. The marker is derived from `matches_known_site`, which
   compares parsed JSON, so whitespace and key order do not count as edits.
3. Keep the source line in both add and edit:
   `Known site: NVIDIA (Eightfold) - verified 2026-11-03`. An unverified entry
   already renders `- unverified`; that phrasing stays.
4. Confirm the restore path also works in the edit modal for a saved known
   portal (it does today through `lookup(form.url)`; this phase adds the test).

### Acceptance criteria

| Criterion | Verification |
| :--- | :--- |
| A divergent config shows the modified marker and the restore action | `egui_kittest` |
| Restoring rewrites provider, domain, and config, and clears the marker | `egui_kittest` |
| A matching config shows neither the marker nor the action | `egui_kittest` |
| Whitespace/key-order changes do not count as modified | Unit test on `matches_known_site` |
| Editing a saved known portal shows its source and can restore | `egui_kittest` |
| An unknown host shows no source line, marker, or restore action | `egui_kittest` |

**Implementation note (2026-11-03).** The source line, the modified marker, and
the restore action are one block driven by `matches_known_site`, so a form can
never claim to match while offering restore. `matches_known_site` compares parsed
JSON, which makes whitespace and key order irrelevant; the unit tests pin both
directions (`a_reformatted_known_config_is_not_modified`, `changing_a_known_value_is_modified`).

Restore calls `apply_known_site`, the same function the URL prefill and the
chooser use, so "restore" and "prefill" cannot produce different configs.

The block is rendered before the provider fields, so the marker reflects the
previous frame's keystrokes in those fields. That is one frame of lag on a
human-scale interaction, and the alternative — moving the attribution below the
fields — reads worse. Tests render after the edit, so they see the settled state.

**Chooser notes on hover.** The entry notes were visible lines under each
button; with two entries that is informative, with twenty it is noise. They now
appear as `on_hover_text`, which leaves the accessible name of the button as the
`site.label()` the tests query.

Tests added: `restore_known_config_replaces_a_divergent_config` (renamed, and now
asserts the marker clears), `a_matching_config_shows_no_modified_marker`,
`editing_a_known_portal_shows_its_source_and_can_restore`,
`an_unknown_site_has_no_known_site_affordances`, plus the two unit tests above.
The edit case seeds a saved config with `pagination.max_pages: 2` so restore has
something to remove.

---

## Workstream 3 — Tests, docs, and verification

**Estimated duration:** 0.5 day.

### Deliverables

1. The unit and `egui_kittest` tests named in WS1/WS2, plus an app-level test
   that `OpenAdd` produces the chooser state.
2. `docs/manual-test-script.md` §4: a chooser step (`4.2a`) and an update to
   `4.3` that starts with **Other site**; `4.3b.2`/`4.3b.3` keep testing the
   typed-URL path, and a new step covers choosing NVIDIA from the list.
3. `docs/known-sites.md`: an "In the app" paragraph — chooser, source line,
   modified marker, restore — and one sentence in the onboarding checklist that
   adding a constant is all it takes to appear in the chooser.
4. `main.md` phase row updated to complete when the phase closes.
5. A short `.agents/plans/phase_2.4_retrospective.md` (the same pattern as
   2.1–2.3), including whether the extra chooser click felt like friction.
6. Full validation: `cargo fmt --all --check`,
   `cargo clippy --workspace --all-targets --offline -- -D warnings`,
   `cargo test --workspace --features career-ops-app/keychain --offline -- --skip obscura`.

---

## Test Strategy

| Layer | Covers |
| :--- | :--- |
| Core unit | `KnownSite::url()` for prefixed and unprefixed hosts; `all()` sorted helper |
| Screen unit | `open_add`/`open_blank_add`/`choose_known_site` transitions; `matches_known_site` whitespace tolerance; chosen-vs-typed `PortalFormData` equivalence |
| `egui_kittest` | Chooser contents and order; choose Waymo/NVIDIA/Other; modified marker; restore; unknown host; edit attribution |
| App | `OpenAdd` → chooser; choosing → save → stored config parses with the entry's provider |
| Live | Unchanged. `live_registry_sites_reach_the_pipeline` keeps verifying the entries themselves; the catalog is UI over the same constants |

No network is required for any new test.

---

## Risks

| ID | Risk | Impact | Mitigation |
| :--- | :--- | :--- | :--- |
| P2.4-R1 | **Two prefill paths drift.** The chooser and the typed-URL prefill produce different forms over time. | Medium | The chooser calls `apply_known_site`; an equivalence test asserts identical `PortalFormData` for every entry. |
| P2.4-R2 | **The chooser hides manual configuration.** Users with unlisted sites get stuck at a list. | Medium | "Other site" is always present and opens the unchanged blank form. |
| P2.4-R3 | **An entry looks authoritative after it rots.** A catalog makes a stale config more convincing than a hidden one. | Medium | The source line carries the verification date, and unverified entries say so; the registry live check remains the gate. |
| P2.4-R4 | **The list stops scaling.** A flat list is fine at two entries and clumsy at fifty. | Low | The chooser takes a slice and sorts; search is additive when scanning stops being instant. |
| P2.4-R5 | **Terminology drift.** "Known provider" creeps back into the UI and the docs. | Low | The UI says known **site**; the provider combo stays an engine choice. Recorded in `docs/known-sites.md`. |

---

## Exit Criteria

1. **Add portal** opens a chooser listing every registry entry, sorted by name,
   with provider and verification date, plus **Other site**.
2. Choosing an entry stores exactly the same `scraper_config` as typing its URL
   and letting prefill run.
3. **Other site** opens the blank form, and unknown hosts behave exactly as in
   Phase 2.3.
4. The add and edit forms name the source entry and mark a diverging config as
   modified; **Restore known config** rewrites provider, domain, and config.
5. No schema change, no new dependency, and no second copy of any embedded
   config.
6. The new paths are covered offline; the suite and clippy are clean.
7. Manual script and `docs/known-sites.md` describe the chooser, the source line,
   and restore.
8. A short retrospective is recorded.

---

## Open Questions

| ID | Question | Position |
| :--- | :--- | :--- |
| Q-16 | **Should the catalog also list companies on supported ATS boards** (for example Spotify on Lever), even though they need no config? | Not in this phase. The catalog's contract is "an embedded config the app offers"; an ATS board has none, and a company directory is a different promise with a real maintenance cost (boards move). Revisit if users ask for discovery rather than configuration. |
| Q-17 | **Should the user be able to save a modified known-site config as their own default** for that site? | No. The saved portal is the user's config; a second per-site default would need a storage model and conflict resolution for no clear gain. Restore always returns to the shipped entry. |

---

## What Phase 2.4 Is Not

- Not a config marketplace, exporter, or importer.
- Not a remote or user-editable registry. Entries ship in the release.
- Not provider defaults. An engine has no payload; only a site does.
- Not auto-detection. The user chooses, or types a URL.
- Not a change to acquisition, storage, or the live verification rules.
