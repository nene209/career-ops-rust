# Phase 2.6 Retrospective and Verification Record

**Phase:** Known ATS Boards and a Filtered Chooser
**Parent plan:** Draft Execution Plan: Career-Ops Rust Desktop Application (v0.11), §12
**Phase plan:** `.agents/plans/phase_2.6_known_ats_sites.md` (v0.5)
**Period:** 2026-11-05
**Estimate:** 1–1.5 days
**Actual:** under a day across three workstreams
**Status:** Complete against the automated criteria; manual script §4.2a verified by the owner on 2026-11-06.

---

## Summary

The registry grew from two hand-written sites to six without adding a provider,
a dependency, a migration, or a stored column. The enabling change was
conceptual: a known site's engine is either a **config-selected provider**
(`selectors`, `eightfold`) or a **URL-selected ATS adapter** (`greenhouse`,
`lever`, `ashby`, `smartrecruiters`). One `SiteEngine` enum now carries that
distinction, so the chooser, the attribution line, and the form all describe an
ATS board as a board rather than pretending it is an HTML scrape.

The four new entries are the boards the adapter live tests already exercise:
Figma, Spotify, Linear, Ubisoft. Their `verified_at` dates come from the
registry check, not from the adapter tests, and the live run confirmed all four
on 2026-11-05. The chooser also gained the filter Phase 2.4 deferred: name, host,
or engine, with the query living and dying with the dialog.

The run found one upstream migration worth knowing: Greenhouse moved its board
host and Figma's board page now redirects to the company site. Detection accepts
both Greenhouse hosts; the entry uses the canonical one; the registry probe
follows redirects while the adapter keeps using the Greenhouse API.

---

## Exit criteria status

| # | Criterion | Status | Evidence |
| :--- | :--- | :--- | :--- |
| 1 | `KnownSite` distinguishes a provider engine from an ATS adapter with one `SiteEngine`; Waymo and NVIDIA behave as before | ✅ | `site_engine_labels_are_display_names`, `waymo_is_a_site_profile_backed_by_selectors`, `nvidia_is_a_site_profile_backed_by_eightfold`; labels unchanged |
| 2 | The four verified boards are known sites with engine label, URL, config, and a date from a passing registry run | ✅ | Six entries in `known_sites::all()`; `verified_at: 2026-11-05` for Figma, Spotify, Linear, Ubisoft |
| 3 | An ATS entry's URL is detected by the adapter it names, and its config has no selector list | ✅ | `every_ats_entry_is_detected_by_the_adapter_it_names`, `an_ats_entry_carries_no_selector_list` |
| 4 | The chooser lists six entries, filters by name/host/engine case-insensitively, shows a no-match state, and keeps Other site and Cancel | ✅ | `matches_filters_by_name_host_and_engine_only`, `the_site_filter_narrows_by_name_and_engine`, `the_site_filter_matches_the_host`, `a_filter_with_no_matches_still_offers_other_site` |
| 5 | The filter starts empty on every open and does not leak between dialogs | ✅ | `open_add_starts_with_an_empty_site_filter`; the query lives in `PortalModal::ChooseSite { filter }` |
| 6 | Choosing any entry, filtered or not, builds the same form as typing its URL | ✅ | `choosing_a_site_builds_the_same_form_as_typing_its_url` iterates every entry; `choosing_a_filtered_site_prefills_the_form` |
| 7 | Offline suite, fmt, clippy clean; the registry live check passed for the dated entries | ✅ | See the command table and live results below |
| 8 | Docs, parent plan, and retrospective updated | ✅ | `docs/known-sites.md`, `docs/LIVE_TESTS.md`, manual §4.2a/§4.3a.1, `main.md`, this document |

**Registry live results (2026-11-05, in the session's sandbox):**

| Board | Jobs | With text | Notes |
| :--- | :--- | :--- | :--- |
| Figma | 143 | 143 | Board page redirects to `figma.com/careers`; adapter fetched the Greenhouse API |
| Spotify | 86 | 72 | |
| Linear | 31 | 31 | |
| Ubisoft | 207 | 25 | Detail cap of 25, as designed |
| NVIDIA | 50 | 40 | |
| Waymo | 90 | 3 of 90 on the first run | Passed earlier; the final run was WAF-skipped, so its date is unchanged |

**Verification commands and results (2026-11-05):**

| Command | Result |
| :--- | :--- |
| `cargo fmt --all --check` | clean |
| `cargo clippy --workspace --all-targets --offline -- -D warnings` | clean |
| `cargo test --workspace --features career-ops-app/keychain --offline -- --skip obscura` | all passed; 34 portals-screen tests, 14 registry tests |
| `CAREER_OPS_LIVE_REGISTRY=1 cargo test -p career-ops-app --features keychain -- --ignored live_registry_sites_reach_the_pipeline` | 5 of 6 verified on the final run (Waymo skipped by its WAF); all four new entries verified |

---

## What the implementation found

### F1 — Greenhouse moved its board host (real, upstream) — handled

`boards.greenhouse.io/figma` now answers `301` to
`job-boards.greenhouse.io/figma`, and that page answers `302` to
`figma.com/careers`. The adapter fetches `boards-api.greenhouse.io`, so the
portal works either way, but two things needed adjusting: detection now accepts
both Greenhouse hosts (a pasted old or new link is recognized), and the entry
uses the canonical `job-boards` host so the registry probe lands on a served
page. The probe follows redirects; the adapter never does.

**Lesson.** A known-site entry is a claim about a live page, and pages move.
Detection should be tolerant of host aliases even when the entry stores one URL.

### F2 — `Provider` already means "engine" (naming decision)

The union of "config-selected engine" and "URL-selected adapter" was first
drafted as `Engine`. Review renamed it `SiteEngine`, because `Provider` already
is the engine concept everywhere else and two things called "engine" would make
the next reader guess. `SiteEngine::provider()` keeps the form's stored value
obvious, and `label()` keeps the UI honest.

### F3 — An adapter entry must be unable to fall back to scraping (design note)

An ATS entry stores `{"provider":"selectors"}` with no `list` and no `fields`.
That is intentional: the router detects the adapter from the URL, and the empty
config means no code path can quietly treat the board page as a selector site.
The invariant is an offline test, not a comment, so a future edit that adds a
`list` to an adapter entry fails before a release.

### F4 — The detail cap is a bound, not a target (confirmed live)

Ubisoft's list returned 207 postings; the entry enriched 25. A board with ten
postings would have enriched all ten. The rule is `min(cap, fetched)`, so the
cap only matters for large boards, and 25 keeps a registry run to about two
minutes instead of an unbounded sequence of detail requests.

### F5 — The filter belongs to the dialog (design note)

Putting `filter: String` in `PortalModal::ChooseSite` means the query cannot
outlive the dialog and `open_add()` cannot forget to reset it. The alternative —
a `PortalsView` field — would need a reset in every entry point and would quietly
survive a close/reopen. `open_add_starts_with_an_empty_site_filter` locks the
chosen shape in.

### F6 — A WAF skip is not a failed entry (confirmed)

Waymo passed the registry check earlier in the session and was skipped by its
WAF on the final run. Its `verified_at` stayed at 2026-11-03, exactly as the
curation rule says: a skip is not a date update and not a failure. The live
check reports both lists (`verified`, `skipped`), so the distinction is visible
without reading logs.

**Operational note.** Behind a proxy, the registry run needs the redirect target
(`figma.com`) allowed, or the Figma probe looks like a transport failure. The
live-test doc records this.

---

## Estimate accuracy

| Workstream | Plan | Actual |
| :--- | :--- | :--- |
| WS1 engine model, four entries, offline tests, registry run | 0.5–1 day | Most of 2026-11-05, including two live runs to chase the Greenhouse redirect |
| WS2 filtered chooser and kittest | 0.5 day | Part of 2026-11-05 |
| WS3 validation, docs, retrospective | 0.25 day | Part of 2026-11-05 |
| **Total** | **1–1.5 days** | **Under one day** |

The extra live run was the only unplanned cost, and it produced the most useful
finding of the phase.

---

## Deferred, unchanged

- **Q-21 custom domains.** `careers.company.com` backed by an ATS still works
  through **Other site**; it gets no attribution.
- **Q-22 ATS templates.** A "any Greenhouse board" entry would need a slug field
  and a different prefill path. Concrete boards are enough for now.
- **Q-23 list caps.** The adapters return one board response; truncating it would
  hide postings. The expensive part, descriptions, is capped.

None of these blocks Phase 3.
