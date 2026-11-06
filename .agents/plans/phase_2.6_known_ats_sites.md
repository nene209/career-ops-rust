# Phase 2.6: Known ATS Boards and a Filtered Chooser — Detailed Execution Plan

**Parent plan:** Draft Execution Plan: Career-Ops Rust Desktop Application (v0.11), §12
**Status:** In progress — WS1–WS2 complete; WS3 verification and docs pending
**Last updated:** 2026-11-05
**Duration:** 1–1.5 days
**Depends on:** Phase 2.3 (providers, ATS adapters, registry), Phase 2.4 (chooser, attribution, restore), Phase 2.5 (registry live check and docs conventions)
**Blocks:** nothing. Phase 3 (AI Integration) is independent of this phase.

---

## Change Log

| Version | Change |
| :--- | :--- |
| v0.1 | Initial plan. The registry exposes two hand-curated sites (Waymo, NVIDIA) while four ATS boards are verified by live tests and invisible in the UI. This phase adds those boards and the chooser filter that Phase 2.4 deferred. Decisions from review: model an ATS board as a site with an adapter **engine** (option B, an `Engine` enum), not a new `Provider`; ship the four live-tested boards; the SmartRecruiters detail cap is an upper bound with a default of 25; search matches name, host, and engine label. |
| v0.2 | Review decisions recorded: the union is named `SiteEngine` (not `Engine`) to keep it distinct from the existing `Provider`, which already means the config-selected engine. The SmartRecruiters cap of 25 is approved. WS1 starts. |
| v0.3 | WS1 implemented: `SiteEngine`, six entries, offline detection/lookup/label tests, and the registry live run. Live results: Figma 143 jobs (143 with text), Spotify 86 (72), Linear 31 (31), Ubisoft 207 (25 enriched), NVIDIA 50 (40), Waymo skipped on the last run by its WAF after passing earlier. Greenhouse now redirects `boards.greenhouse.io` to `job-boards.greenhouse.io` and Figma's board page to `figma.com/careers`; detection accepts both hosts, the entry uses the canonical host, and the live probe follows the redirect. |
| v0.4 | WS2 implemented: `KnownSite::matches` over name, host, and engine label; the chooser filter lives in `PortalModal::ChooseSite { filter }` so it starts empty on every open; a bounded scroll area, a no-match state, and **Clear**. Other site and Cancel stay outside the filter. kittest covers filtering by name/host/engine, the empty state, Clear, and choosing from a filtered list. |

---

## Purpose

Phase 2.3 built site providers and an embedded registry. Phase 2.4 made the
registry visible. Both left the same gap: **the only boards a user can discover
are the two sites we hand-wrote entries for**, even though the app supports
Greenhouse, Lever, Ashby, and SmartRecruiters boards through their adapters, and
those four boards are exercised by live tests on every run.

A user who opens **Add portal** sees:

```
Waymo (Selectors) - verified 2026-11-03
NVIDIA (Eightfold) - verified 2026-11-03
Other site
```

They do not see that a Greenhouse board works with no configuration. The
knowledge exists, but only in test files.

This phase closes that gap and prepares the chooser for the registry it will
become:

1. **Verified boards are selectable.** The four boards the live adapter tests
   cover (Figma/Greenhouse, Spotify/Lever, Linear/Ashby, Ubisoft/SmartRecruiters)
   become known sites with the same attribution, date, and restore behaviour as
   Waymo and NVIDIA.
2. **The chooser is searchable.** A filter over name, host, and engine keeps the
   list usable as it grows. Phase 2.4 explicitly deferred this "until the
   registry is large"; six entries and a scrollable list is that point.

It is deliberately **not** a directory of every ATS customer, a remote catalog,
or a new fetch engine. A known ATS board is a **site profile** whose engine
happens to be selected by URL instead of by a config payload.

### Evidence

- `known_sites::all()` returns exactly `[WAYMO, NVIDIA]`.
- `crates/core/tests/ats_live.rs` verifies four boards by default:
  `boards.greenhouse.io/figma`, `jobs.lever.co/spotify`,
  `jobs.ashbyhq.com/linear`, `jobs.smartrecruiters.com/Ubisoft2`.
- `crates/app/src/screens/portals.rs::choose_site_modal` renders one button per
  entry with no filter and no scroll area.
- The Phase 2.4 scope table lists "Search/filter in the chooser" as deferred.

---

## Scope

| In scope | Out of scope |
| :--- | :--- |
| A `SiteEngine` model that distinguishes a config-selected provider from a URL-selected ATS adapter | A `Provider::Ats` variant or any second router dispatch path |
| Four known-site entries for the live-tested ATS boards | A directory of every company on those ATS platforms |
| A no-notes filter over name, host, and engine label | Full-text search over notes, docs, or configs |
| A scrollable chooser list with an empty state | Grouping by engine or sorting controls |
| Registry live verification of the new entries before they ship with a date | A remote or signed registry |
| Docs: `docs/known-sites.md`, `docs/LIVE_TESTS.md`, manual script §4.2a | Custom-domain aliases for ATS boards (see Q-21) |
| Offline tests for the engine model, detection, lookup, and filtering | Changing list-fetch semantics or truncating boards |

---

## Design decisions

### 1. An ATS board is a site with an adapter engine (option B)

`Provider` is the **config-selected** engine and is stored on the portal as
`scraper_config.provider`. An ATS board is different: the router detects its
adapter from the URL, and the stored config carries no payload. Modeling that as
`Provider::Ats` would conflate two dispatch mechanisms and add a second path
through the router.

`KnownSite` therefore carries a `SiteEngine`:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SiteEngine {
    /// Selected by `scraper_config.provider`.
    Provider(Provider),
    /// Selected by the board URL; the string is `AtsAdapter::name()`.
    Ats(&'static str),
}

impl SiteEngine {
    /// The provider the form stores. An ATS entry stores the default engine
    /// with an empty config; the URL decides the adapter at fetch time.
    pub fn provider(self) -> Provider;

    /// The label the chooser and the attribution line show.
    pub fn label(self) -> &'static str;
}
```

`label()` renders known adapters with their display names (`greenhouse` →
"Greenhouse", and so on) and falls back to the raw name for anything new.

Waymo and NVIDIA become `SiteEngine::Provider(Provider::Selectors)` and
`SiteEngine::Provider(Provider::Eightfold)`; their labels do not change. The four
new entries use `SiteEngine::Ats("greenhouse")`, `"lever"`, `"ashby"`, and
`"smartrecruiters"`.

**Invariants, tested offline:**

- Every entry's config parses, and `entry.engine.provider()` matches the config's
  provider.
- A `SiteEngine::Ats` entry has a `url()` that the named adapter detects.
- A `SiteEngine::Ats` entry has no selector `list` (it cannot accidentally scrape
  the board page as HTML).

### 2. Only boards that pass the registry live check ship with a date

`verified_at` follows the rule in `docs/known-sites.md`: it is the date the
**registry** live check passed, not the adapter test. The adapter tests prove
parsing; the registry check proves the full portal path (create → fetch → store →
non-empty text).

Implementation order:

1. Add the entries with `verified_at: None`. They render as "- unverified".
2. Run `CAREER_OPS_LIVE_REGISTRY=1` against the six entries.
3. Set `verified_at` (and the docs table) only for the entries the run verified.

Hosts needed for the live run: `boards.greenhouse.io`, `jobs.lever.co`,
`jobs.ashbyhq.com`, `jobs.smartrecruiters.com`, plus the existing
`careers.withwaymo.com` and `jobs.nvidia.com`.

### 3. The detail cap is a bound, not a target

The board that needs detail enrichment is SmartRecruiters: its list payload
carries no description. The router already enriches `min(detail.max_jobs,
jobs_fetched)`, so a cap **larger** than the fetched count enriches every
posting, and a cap smaller than the count enriches that many.

Chosen default: **25**.

- A board that returns 10 postings enriches all 10.
- A board that returns 40 enriches the first 25; the rest keep their empty list
  text, and the user can raise the cap in the portal config or refresh a single
  job.
- 25 is two to three times a typical first page and keeps the registry live
  check bounded. The alternative — no cap — would issue one request per posting
  for a board that can return hundreds.

This phase does **not** add a list-size cap. ATS adapters return one board
response; truncating it would silently hide postings, and `pagination.max_pages`
does not apply to them.

### 4. Filter state belongs to the chooser

`PortalModal::ChooseSite` becomes `ChooseSite { filter: String }`. The query
therefore disappears when the modal closes, and `open_add()` starts a fresh one.
The alternative — a `PortalsView.site_filter` field — survives close/open and
would need reset calls in every entry point.

`KnownSite::matches(query)` is a pure function in core: trim, lower-case, and
substring over **name, host, and engine label**. Notes are deliberately excluded;
matching prose makes results feel arbitrary. Unit tests cover name, host, engine,
case, and no-match.

### 5. Filtering never hides the escape hatch

**Other site** and **Cancel** stay outside the filtered list and are always
visible. An empty result says "No sites match this filter." and still offers
**Other site**. The list sits in a bounded `ScrollArea`; the chooser is about to
double in size and will keep growing.

---

## Workstream 1 — Engine model and the four boards

**Estimated duration:** 0.5–1 day.

### Deliverables

1. `SiteEngine` in `known_sites.rs`; `KnownSite.provider` becomes
   `KnownSite.engine`. Update `SiteEngine::{provider, label}`, `KnownSite::label()`,
   and every caller (`PortalForm::apply_known_site`, registry tests, label
   tests). Waymo and NVIDIA keep their visible labels.
2. Four constants:

| Constant | Host / prefix | Engine | Config | Verified |
| :--- | :--- | :--- | :--- | :--- |
| `FIGMA` | `job-boards.greenhouse.io` / `/figma` | `Ats("greenhouse")` | `{"provider":"selectors"}` | 2026-11-05 |
| `SPOTIFY` | `jobs.lever.co` / `/spotify` | `Ats("lever")` | `{"provider":"selectors"}` | 2026-11-05 |
| `LINEAR` | `jobs.ashbyhq.com` / `/linear` | `Ats("ashby")` | `{"provider":"selectors"}` | 2026-11-05 |
| `UBISOFT` | `jobs.smartrecruiters.com` / `/Ubisoft2` | `Ats("smartrecruiters")` | `{"provider":"selectors","detail":{"max_jobs":25}}` | 2026-11-05 |

   Notes on each entry say what the board is, that the list carries (or does not
   carry) text, and that other boards on the same ATS work through **Other
   site**.
3. `all()` returns six entries; the chooser sorts them by name, so the order is
   Figma, Linear, NVIDIA, Spotify, Ubisoft, Waymo.
4. Offline tests:
   - Every entry parses and its engine's provider matches the config.
   - Every `SiteEngine::Ats` entry's `url()` is detected by an adapter whose
     `name()` equals the engine's adapter.
   - Every `SiteEngine::Ats` entry's config has no selector `list`.
   - `lookup` matches each new board URL, a sub-path (`/figma/jobs/123`), and
     does not match a different board on the same host.
   - `KnownSite::label()` for an ATS entry reads "Figma (Greenhouse) - …".
5. Run the registry live check; commit `verified_at` dates for verified entries
   only.
6. `docs/known-sites.md`: add the four rows, explain "adapter entry", and record
   the 25-cap decision.

### Acceptance criteria

| Criterion | Verification |
| :--- | :--- |
| The four boards appear in the chooser with their company, engine, and date | `egui_kittest` list assertion |
| Choosing one prefills the same form as typing its URL | Existing `choosing_a_site_builds_the_same_form_as_typing_its_url` covers every entry |
| An ATS entry stores a valid config and is fetched by its adapter | Offline detection test + registry live check |
| Waymo and NVIDIA labels are unchanged | Registry label tests |
| An ATS entry cannot be mistaken for a selector scrape | Offline "no selector list" test |
| Dates come only from a passing registry run | `docs/known-sites.md` rule; commit review |

**Implementation note (2026-11-05).** `SiteEngine` is in place and all six
entries parse, detect, and label as designed. The registry live run verified
Figma (143 jobs, 143 with text), Spotify (86, 72), Linear (31, 31), Ubisoft
(207, 25 enriched), and NVIDIA (50, 40); Waymo was skipped by its WAF on the
last run, so its existing 2026-11-03 date is unchanged.

The run found an upstream change worth recording: Greenhouse now redirects
`boards.greenhouse.io/figma` to `job-boards.greenhouse.io/figma`, and Figma's
board page in turn redirects to `figma.com/careers`. Detection accepts both
Greenhouse hosts, the entry uses the canonical `job-boards` host, and the live
probe follows the redirect to the final page while the adapter still fetches
`boards-api.greenhouse.io`. The entry's notes say so.

---

## Workstream 2 — Filtered chooser

**Estimated duration:** 0.5 day.

### Deliverables

1. `KnownSite::matches(query)` in core: case-insensitive substring over `name`,
   `host`, and `engine.label()`. Empty/whitespace query matches everything.
2. `PortalModal::ChooseSite { filter: String }`; `open_add()` starts empty;
   `modals()` passes the filter into `choose_site_modal`; closing the modal drops
   it.
3. Chooser UI:
   - A "Filter sites" `labelled_input` at the top.
   - A bounded `ScrollArea` around the entry buttons (the notes remain on hover).
   - "No sites match this filter." when the query excludes everything.
   - **Clear** shown while the query is non-empty.
   - **Other site** and **Cancel** below the list, outside the filter.
4. Optional polish: focus the filter when the chooser opens, so typing narrows it
   immediately. Skip if it complicates the focus tests.
5. `egui_kittest` cases:
   - Typing `lever` leaves Spotify visible and hides Figma, NVIDIA, and Waymo.
   - Typing a host (`ashby`) matches Linear.
   - Typing an engine (`greenhouse`) matches Figma.
   - A no-match query shows the empty line and **Other site** still opens the
     blank form.
   - **Clear** restores the full list.
   - Choosing a filtered entry still prefills the form.
6. Manual script §4.2a: the chooser now lists six boards and has a filter field.

### Acceptance criteria

| Criterion | Verification |
| :--- | :--- |
| The filter matches name, host, and engine, case-insensitively | Unit test on `KnownSite::matches` |
| The filter state does not survive closing the chooser | Unit test on `open_add()` and modal state |
| Other site and Cancel remain available under any query | `egui_kittest` |
| Choosing a filtered entry still prefills the form | `egui_kittest` |
| An empty result explains itself | `egui_kittest` |

---

## Workstream 3 — Verification and docs

**Estimated duration:** 0.25 day.

1. Full validation: `cargo fmt --all --check`; `cargo clippy --workspace
   --all-targets --offline -- -D warnings`; `cargo test --workspace --features
   career-ops-app/keychain --offline -- --skip obscura`.
2. Run the registry live check and record the result in the plan's
   implementation note.
3. Update `docs/LIVE_TESTS.md`: the registry run now covers six boards; note the
   SmartRecruiters detail cap and its effect on run time.
4. Update the parent plan (`main.md`): a v0.11 change-log entry and a Phase 2.6
   row with the plan link.
5. Short retrospective in `.agents/plans/phase_2.6_retrospective.md` once the
   manual and live checks pass.

---

## Test Strategy

| Layer | Covers |
| :--- | :--- |
| Core unit | `SiteEngine` labels/provider mapping; `KnownSite::matches`; label rendering; lookup with prefixes |
| Core registry | Every entry parses; ATS URL detected by its named adapter; ATS entries have no selector list |
| `egui_kittest` | Six entries listed; filter by name/host/engine; no-match state; Clear; choose-from-filter; Other site |
| App | The chooser transition and prefill still go through `open_add` / `apply_known_site` |
| Live (ignored) | `live_registry_sites_reach_the_pipeline` over six entries, to set `verified_at` |

No new dependency, no migration, no storage change.

---

## Risks

| ID | Risk | Impact | Mitigation |
| :--- | :--- | :--- | :--- |
| P2.6-R1 | **Board churn.** A company closes or moves its board. | Medium | `verified_at` is visible; the registry live check catches it; notes say the entry is one board, not the ATS. Other boards still work through **Other site**. |
| P2.6-R2 | **Large boards.** Figma/Spotify/Ubisoft can return hundreds of postings in one response. | Low | Existing behavior; the Pipeline loads the latest 500. The hover note says the board is large. No list truncation is added. |
| P2.6-R3 | **SmartRecruiters pacing.** Detail enrichment is one request per posting. | Medium | Cap at 25; the live check stays bounded; the cap is visible in the entry's docs row. |
| P2.6-R4 | **A merge date is mistaken for adapter verification.** | Low | `docs/known-sites.md` states the rule; the plan's implementation note records which run produced the dates. |
| P2.6-R5 | **The SiteEngine refactor misses a caller.** | Low | The compiler finds them; the registry tests assert the provider/config invariant for every entry. |
| P2.6-R6 | **Filtering hides the escape hatch.** | Low | Other site and Cancel live outside the filtered list and are asserted under a no-match query. |
| P2.6-R7 | **Custom ATS domains.** `careers.company.com` backed by Greenhouse will not match the `boards.greenhouse.io` entry. | Low | Out of scope (Q-21). Detection still fetches it once the user pastes the URL. |

---

## Exit Criteria

1. `KnownSite` distinguishes a provider engine from an ATS adapter with one
   `SiteEngine` enum; Waymo and NVIDIA behave exactly as before.
2. The four verified boards are known sites with their engine label, board URL,
   config, and a `verified_at` date from a passing registry run.
3. An ATS entry's URL is detected by the adapter it names, and its config carries
   no selector list; both are offline tests.
4. The chooser lists six entries, filters by name, host, or engine
   case-insensitively, shows a no-match state, and keeps **Other site** and
   **Cancel** available.
5. The filter starts empty on every open and does not leak between dialogs.
6. Choosing any entry — filtered or not — builds the same form as typing its URL.
7. Offline suite, fmt, and clippy are clean; the registry live check passes for
   the entries whose dates were committed.
8. `docs/known-sites.md`, `docs/LIVE_TESTS.md`, manual script §4.2a, the parent
   plan, and a short retrospective are updated.

---

## Open Questions

| ID | Question | Position |
| :--- | :--- | :--- |
| Q-21 | **Should a custom domain (`careers.company.com`) get its own entry?** | No. One entry per verified host. A custom domain is fetched correctly once the user pastes it; attribution is a convenience, not a requirement. Add an alias only if a real user hits it. |
| Q-22 | **Should the chooser offer "any Greenhouse board" templates?** | Not now. A template needs a slug field and a different prefill path. The concrete verified boards plus **Other site** cover the need. Revisit if the registry grows past a dozen entries. |
| Q-23 | **Should ATS list fetches be capped like `pagination.max_pages`?** | No. The adapters return one board response; truncating it hides postings. The expensive part (details) is capped. Raise the detail cap in the portal config if a board needs more. |

---

## What Phase 2.6 Is Not

- Not a company directory or a job-board marketplace.
- Not a remote catalog; entries still ship in the binary.
- Not a new provider, engine, or router path.
- Not a change to list-fetch semantics; no board is truncated.
- Not custom-domain detection or aliasing.
- Not a redesign of the chooser beyond a filter and a scroll area.
