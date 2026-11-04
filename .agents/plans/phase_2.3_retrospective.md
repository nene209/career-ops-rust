# Phase 2.3 Retrospective and Verification Record

**Phase:** Site Providers and Known Sites
**Parent plan:** Draft Execution Plan: Career-Ops Rust Desktop Application (v0.8), §12
**Phase plan:** `.agents/plans/phase_2.3_site_providers.md` (v0.6)
**Period:** 2026-11-03
**Estimate:** 4–6 days
**Actual:** one day, four commits
**Status:** Complete against all nine exit criteria. The first full registry run
verified both entries with no skips: Waymo 90 jobs from three pages with two
descriptions, NVIDIA 50 jobs from five pages with forty descriptions.

---

## Summary

Phase 2.3 answered the question Phase 2.2 left open: what happens when the jobs
are not in the HTML at all. NVIDIA's careers site is an Eightfold shell — two
JSON endpoints, no markup — so no selector config could ever fetch it. The phase
introduced exactly one new engine (`eightfold`) and two site profiles, and it
drew the line that keeps the design from decaying: **a provider is an engine, a
known site is a profile**. Waymo stayed on the `selectors` engine and became a
registry entry; adding it required no code in the fetch path.

The registry is embedded (`known_sites.rs`), typed, and offered by the portal
form rather than imposed: typing a known URL prefills the provider and config
only into empty fields, shows the verification date, and never overwrites an
edit. The curation rule is that an entry ships only with a passing live run, and
that run is a test anyone can repeat.

Two things did not go to plan, both small and both recorded below: the registry
live check became a sibling test rather than a mode inside the existing one, and
the form's save gate was tightened so invalid JSON can no longer be saved.

---

## Exit criteria status

| # | Criterion | Status | Evidence |
| :--- | :--- | :--- | :--- |
| 1 | `scraper_config` accepts a provider; a missing provider still means selectors | ✅ | `config.rs` tests: `a_missing_provider_means_selectors`, `an_explicit_selectors_provider_is_accepted`, `an_eightfold_config_carries_its_domain`, `an_unknown_provider_is_a_config_error`, `a_null_provider_means_selectors` |
| 2 | NVIDIA can be added and fetched: list from `pcsx/search`, descriptions from `position_details`, within the caps | ✅ | Live 2026-11-03: `Fetched 50 jobs from 5 pages; 40 of 40 descriptions.` longest 7,723 bytes. Router tests `an_eightfold_config_fetches_and_enriches`, `an_eightfold_portal_enriches_by_default`, `preview_an_eightfold_config_uses_the_provider` |
| 3 | Re-fetching NVIDIA is idempotent, even if a slug changes | ✅ | `eightfold.rs` id test (stable `eightfold:{domain}:{id}`, URL-independent); `fetching_same_portal_twice_dedups` covers the storage half |
| 4 | The registry contains Waymo and NVIDIA, each with a passing live run and `verified_at: Some(date)` | ✅ | Registry run 2026-11-03: `verified ["Waymo", "NVIDIA"]; skipped []`. Both constants carry `2026-11-03` |
| 5 | Both are `KnownSite` constants with a provider, embedded config, fixture test, live test, and docs row; no site-specific fetch code | ✅ | `WAYMO`, `NVIDIA` in `known_sites.rs`; eightfold/selector fixtures; `live_registry_sites_reach_the_pipeline`; `docs/known-sites.md`. The router branches on the stored `provider`, never on the host |
| 6 | Typing a known host offers the config/provider without overwriting what the user typed | ✅ | `a_known_site_shows_its_label_and_prefills_an_empty_form`, `a_selectors_known_site_takes_the_same_prefill_path`, `editing_after_prefill_is_preserved`, `use_known_config_replaces_a_divergent_config`, `a_known_site_prefills_an_empty_form_once`, `a_known_site_does_not_prefill_a_non_empty_form` |
| 7 | An unknown host can still be configured manually (selectors or Eightfold) | ✅ | `a_manually_chosen_provider_fetches_an_unknown_host` (form → storage → router → pipeline); `save_is_gated_on_an_eightfold_domain` |
| 8 | `docs/known-sites.md` documents the entries and the curation rule | ✅ | `docs/known-sites.md`: entries table, what "verified" means, running the check, curation rules, onboarding checklist |
| 9 | Phase 2.3 retrospective recorded | ✅ | This document |

**Verification commands and results (2026-11-03):**

| Command | Result |
| :--- | :--- |
| `cargo fmt --all --check` | clean |
| `cargo clippy --workspace --all-targets --offline -- -D warnings` | clean |
| `cargo test --workspace --features career-ops-app/keychain --offline -- --skip obscura` | all passed (`obscura` skipped: the sandbox forbids binding a port) |
| `CAREER_OPS_LIVE_REGISTRY=1 cargo test -p career-ops-app --features keychain --offline -- --ignored live_registry_sites_reach_the_pipeline --nocapture` | both entries verified, none skipped, 33.4 s total |
| `cargo test -p career-ops-app --features keychain --offline --lib -- registry` | 4 offline registry-sequencing tests passed; the live check filtered as ignored |
| `cargo test -p career-ops-app --features keychain --lib -- --ignored live_fetch_with_pagination_and_details` | Phase 2.2's Waymo variant, unchanged; re-verified 90 jobs / 3 pages / 2 of 2 descriptions during the registry run |

---

## Live measurements

| Site | Provider | Jobs | Text | Longest | Time | Result line |
| :--- | :--- | ---: | ---: | ---: | ---: | :--- |
| Waymo | `selectors` | 90 (3 pages) | 90 | 3,611 B | 3.7 s | `Fetched 90 jobs from 3 pages; 2 of 2 descriptions.` |
| NVIDIA | `eightfold` | 50 (5 pages) | 40 | 7,723 B | 27.2 s | `Fetched 50 jobs from 5 pages; 40 of 40 descriptions.` |

The pacing is 400 ms plus up to 400 ms of jitter per host. NVIDIA's 27 s is 45
paced requests (5 list + 40 detail) and is the cost of the default caps, not of
the provider; Waymo's 3.7 s is three list pages plus two details. No WAF
challenge appeared on either site in this run; the previous day's Waymo
challenges were the reason the live test probes and skips.

A later run the same day exercised the other branch for real: Waymo's probe was
challenged and the entry was skipped without spending a fetch, while NVIDIA
verified again (50 jobs, 40 descriptions, 27.3 s). That is the behavior the
skipping rules are supposed to produce, observed rather than assumed.

---

## What the verification pass found

### F1 — A full registry run is the strongest evidence the phase produced (design confirmed)

Both entries were created exactly the way a user creates them — a `KnownSite`
constant's config written into a portal — and both fetched through the real
router to stored rows. The test asserts storage, not just health: at least one
job with non-empty text, and every `source_url` absolute. That is the only check
that would catch a provider that returns an empty list with a `200`.

### F2 — "Verified" is a claim with a date, not a freshness signal (operational, recorded)

An entry that is WAF-challenged on every run keeps its last passing date
indefinitely, because a challenge skips rather than fails. The UI shows the date,
so a user can see how old the claim is, but nothing warns when it is stale.

**Response.** Documented in `docs/known-sites.md` ("A skipped run is not a pass",
"Do not copy a date from an earlier run"). A staleness hint in the form is a
candidate for Phase 5 polish, not a correctness gap: the fetch itself still
reports a challenge or a failure when the user tries the site.

### F3 — The registry mode is a sibling test, not a branch (deviation from the plan)

The plan said `live_fetch_reaches_the_pipeline` would gain a
`CAREER_OPS_LIVE_REGISTRY=1` mode. Implementing it there would have meant a
fail-fast test growing a skip path per entry while sharing a vault with the
default URL list, so the registry walk lives in
`live_registry_sites_reach_the_pipeline` behind the same environment variable.
Both run under `--ignored live_fetch`, and neither is affected by the other's
flags.

### F4 — The save gate tightened (behavior change, documented)

`PortalForm::provider_fields_valid()` now gates Save and Test config, so invalid
JSON cannot be saved and re-classified at fetch time. This came out of WS3: the
prefill path composes JSON that must parse, and allowing a known-bad value to be
stored contradicted the rest of the form. `valid()` remains the URL/company gate.
Recorded in `docs/manual-test-script.md` §4.3b/§4.3d.

### F5 — Prefill is a render-time side effect (design note, tested)

`maybe_prefill_known_site` runs while the modal renders, guarded by
`prefilled_for` and by "provider fields are empty". It is idempotent, cannot
overwrite an edit, and cannot resurrect itself after a user clears a field.
`editing_after_prefill_is_preserved` and `a_known_site_prefills_an_empty_form_once`
pin both halves. The alternative — an explicit "apply" on URL blur — adds an
event the app has no other use for.

### F6 — The skip rules needed offline tests (found after the phase closed) — fixed

The registry check is `#[ignore]`d and network-bound, so the only way to know the
skip logic worked was to run it and wait for a challenge. That is backwards for
the part most likely to drift: the ordering (probe, fetch only a served page,
re-probe only on failure) and the three outcomes.

**Fix.** The logic is now `registry_entry`, a sequencer generic over its context.
The live check passes the app; four ordinary unit tests pass a call recorder and
assert the call order as well as the outcome — including that a challenged page
is never fetched and that a healthy fetch is not re-probed. The names are in
`docs/known-sites.md`. A later live run confirmed the refactor drives the real
path: Waymo skipped on a challenged probe, NVIDIA verified.

---

## Cap values after measurement

| Cap | Value | Verdict | Reasoning |
| :--- | :--- | :--- | :--- |
| `pagination.max_pages` | 5 (ceiling 20) | Keep | Five Eightfold pages is ten requests' worth of latency, not a depth problem. The NVIDIA board reports ~2,750 positions, so the cap is what makes a fetch finite. |
| `detail.max_jobs` | 40 (ceiling 200) | Keep | NVIDIA's 40 details took ~20 s of the 27 s run. It is also the honest coverage boundary: 40 of 50 stored rows carry text, and the result line says `40 of 40 descriptions` because it reports attempts, not coverage. |
| Per-site caps | Waymo 3 pages / 2 details | Keep | The embedded entry can be tighter than the defaults; that is the point of a profile. |
| Request pacing | 400 ms + 0–400 ms | Keep | 45 requests in 27 s is exactly the configured pacing. |

If a first fetch of NVIDIA feels slow, the lever is the entry's `detail.max_jobs`,
not the global default: a user who wants speed can edit the advanced options.

---

## Estimate accuracy

| Workstream | Plan | Actual |
| :--- | :--- | :--- |
| WS1 provider model and dispatch | 1 day | Part of 2026-11-03 |
| WS2 Eightfold engine | 1.5–2 days | Part of 2026-11-03 |
| WS3 registry and prefill | 1–1.5 days | Part of 2026-11-03 |
| WS4 curation and live verification | 1 day | Part of 2026-11-03 |
| **Total** | **4–6 days** | **One day, four commits** |

The same overestimate shape as Phases 2.1 and 2.2: the probing had already
happened (NVIDIA's endpoints were captured before the plan was written), every
decision had a test seam, and the live sites cooperated on the first full run.
The parts that took real thought were the boundaries — what a provider is, what
a known site may claim, and what "verified" means — not the code.

---

## What to carry into Phase 3

1. **`raw_text` is markup for every provider.** Eightfold descriptions are HTML,
   as are SmartRecruiters and JSON-LD. The evaluation input either accepts tags
   or the pipeline strips them; do not assume plain text.
2. **Coverage is partial and visible.** NVIDIA's default fetch gives 40 of 50
   rows a description. Evaluation should record whether it read a description or
   a teaser, so a weak score cannot be mistaken for a weak match.
3. **`source_url` is per posting for both engines.** Dedup keys on it; the
   Eightfold id is URL-independent, so a slug change re-uses the row.
4. **A provider is a dispatch decision, not a special case.** If a new site needs
   code, it needs an engine with fixtures — not a `if host == …` branch.
5. **The registry is data with a date.** Extending it is a `KnownSite` constant
   plus a docs row, and the run that proves it is one command.

---

## Open items for the human

- Execute the 🌐 rows of `docs/manual-test-script.md`, especially §4.3b.2/4.3b.3
  (the prefill paths) and §8 (the live checks), and record the sign-off table.
- Re-run the registry check periodically; a challenge skips rather than fails, so
  a red run means extraction broke, not that the site was busy.
- Add sites only through the checklist in `docs/known-sites.md`; a new engine
  needs its own fixtures and justification.
