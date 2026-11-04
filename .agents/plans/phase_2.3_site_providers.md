# Phase 2.3: Site Providers and Known Sites — Detailed Execution Plan

**Parent plan:** Draft Execution Plan: Career-Ops Rust Desktop Application (v0.8), §12
**Status:** Complete — WS1–WS4 done; retrospective recorded in
`.agents/plans/phase_2.3_retrospective.md`
**Last updated:** 2026-11-03
**Duration:** 4–6 days
**Depends on:** Phase 2.2 (pagination and detail enrichment freeze the selector-config schema)

---

## Change Log

| Version | Change |
| :--- | :--- |
| v0.1 | Initial plan. Written after probing `https://jobs.nvidia.com/careers`, which is an Eightfold site: its content is served by two JSON endpoints and it cannot be fetched by either extraction tier today, with or without a selector config. That turned "a list of known configs" into "known sites with providers", because the second site the user wants is not a config at all. |
| v0.2 | Provider scope clarified after review: a **provider is an engine** (`selectors`, `eightfold`), while a **known site is an embedded profile** that names an engine and carries its payload. Waymo is therefore a first-class known site backed by the selectors engine, not a new provider variant; NVIDIA is the same structure backed by Eightfold. The registry moves from a JSON file to typed `KnownSite` constants in `known_sites.rs`, so both sites share one validation, prefill, fixture, live-test, and documentation path. |
| v0.3 | Workstream 1 implemented: `core::acquisition::config` owns the `Provider` discriminator and `ScraperConfig::parse`. A missing or null `provider` means `selectors`; an unknown provider is a clear config error. The router parses once, fails invalid configs as `InvalidRequest` (previously logged and ignored), and dispatches `eightfold` before any HTML fetch. `EightfoldConfig` carries the domain and reuses the Phase 2.2 `pagination`/`detail` caps; the Eightfold fetch itself is the WS2 placeholder. |
| v0.4 | Workstream 2 implemented: the `eightfold` engine (list/detail parsing, paging by `start`, stable `eightfold:{domain}:{id}` ids, `PageReport` stop reasons), `FetchSource::fetch_eightfold`, router dispatch through a `DetailSource` that reuses the Phase 2.2 enrichment loop, and default-on enrichment for Eightfold because its list carries no text. Live NVIDIA: 20 jobs from 2 pages, 2 of 2 descriptions, longest 6,253 bytes. |
| v0.5 | Workstream 3 implemented: the typed registry (`known_sites::{WAYMO, NVIDIA, all, lookup}`), the provider combo and Eightfold fields in the portal form, prefill that runs once per site and only into empty provider fields, and an explicit **Use known config** button. Save and Test config now share one validity gate, which also makes invalid JSON block saving rather than failing later. |
| v0.6 | Workstream 4 implemented: the gated registry live check, `docs/known-sites.md` with the curation rules and onboarding checklist, and the retrospective. The first full registry run verified both entries: Waymo 90 jobs / 3 pages / 2 of 2 descriptions (longest 3,611 bytes, 3.7 s) and NVIDIA 50 jobs / 5 pages / 40 of 40 descriptions (longest 7,723 bytes, 27.2 s). |
| v0.7 | The registry skip rules extracted into `registry_entry`, a sequencer shared by the live check and four offline tests. The live check could only be assessed by running it, and the rules it depends on — challenged page means skip without fetching, served page plus failed fetch means fail — are exactly the part that can drift silently. |

---

## Purpose

Phase 2.2 makes selector-configured sites scrape well. This phase answers two
different questions:

1. **Must every user write their own config?** No. If the app knows the site, it
   should offer the config, while still letting the user change it.
2. **What about sites that are not HTML at all?** A growing number of career
   sites are a shell around a JSON API. A selector config cannot help them, at
   either tier, because there is no job markup to select.

The plan separates **providers** (fetch-and-parse engines) from **known sites**
(embedded profiles that name an engine). That difference is what keeps a growing
registry from turning into a growing codebase, and it is the structure every new
site and engine follows.

### Evidence: NVIDIA

`https://jobs.nvidia.com/careers` is an Eightfold site (`/gen/js/ef-*.js`,
`static.vscdn.net` assets). The page is client-rendered: no `JobPosting` JSON-LD,
no `#root`/`#app`/`__NEXT_DATA__`, and no job links in the static HTML. Probed on
2026-10-25:

| Endpoint | Result |
| :--- | :--- |
| `GET /api/pcsx/search?domain=nvidia.com&start=0` | `200`, `data.count = 2681`, 10 `positions[]` per page; `num` is ignored |
| `GET /api/pcsx/position_details?position_id={id}&domain=nvidia.com&hl=en` | `200`, `data.jobDescription` (HTML, ~4.5 KB of text for the sampled job), plus `publicUrl`, `department`, `locations`, `efcustomTextTimeType` |
| `GET /api/apply/v2/jobs?...` | `403` (not the endpoint this site uses) |

So NVIDIA is cleanly fetchable with an adapter, and not fetchable with a config.
The adapter has leverage beyond NVIDIA: Eightfold hosts `jobs.<company>.com` for
many employers.

---

## Scope

### In scope

1. A **provider** concept inside the existing `scraper_config` JSON: `selectors`
   (default, the Phase 2.2 behaviour) or `eightfold`.
2. An **Eightfold provider**: list, pagination, and per-posting description.
3. A **known-site registry**: embedded, typed `KnownSite` constants keyed by
   host, each with a name, a provider, the config, a verification date, and
   notes.
4. **Lookup and prefill** in the portal form, with the user's edits always
   winning.
5. A **curation rule**: a site enters the registry only after a live
   verification test passes, and the live test iterates the registry.

### Out of scope

- A general "map any JSON API with jq-style paths" engine. Tempting and wrong:
  it is a config language with no testable contract. Add providers deliberately.
- Auto-detecting Eightfold for unknown hosts. Detection is a guess; the registry
  names the provider explicitly, and the form lets the user pick it.
- Workday, Phenom, iCIMS, SuccessFactors adapters. Each is the same shape of work
  and gets its own entry in this phase's follow-up list, not a speculative
  framework.
- Caching, scheduling, background refresh, robots.txt enforcement (see the Open
  Questions carried over from Phase 2.2).
- AI evaluation, PDF rendering.

---

## Design decisions

### A provider is an engine; a known site is a profile

These are two concepts and the plan keeps them separate:

- **Provider** — the fetch-and-parse engine. `selectors` (HTML + CSS, the Phase
  2.2 engine) and `eightfold` (JSON API) are the two this phase knows. A provider
  knows nothing about a specific company.
- **Known site** — an embedded profile for one careers site. It names a provider
  and carries the payload that provider needs (a selector config, or an Eightfold
  domain), plus the verification date and notes.

Waymo is a known site whose provider is `selectors`; NVIDIA is a known site whose
provider is `eightfold`. Both are first-class: typed constant, embedded payload,
fixture test, live test, docs row.

**Waymo does not get `provider: "waymo"`.** There is no Waymo-specific fetch or
parse step — it is the selector engine plus a config. A provider variant per
company would grow the enum with every site, encourage a copy of the selector
engine per company, and make the engine boundary meaningless (P2.3-R5). The
question to ask before adding a provider is "does this need code that the
existing engines cannot express?", not "is this a new company?".

### Adding a site or a provider

One checklist, two entry points:

1. **An existing engine can fetch it** (Waymo). Add a `KnownSite` constant with
   the embedded config, a fixture for its payload, a live test, and a docs row.
2. **No engine can** (NVIDIA). Add a provider module (endpoints, parsing,
   fixtures), then the `KnownSite` constant. The provider is reusable across
   every site that runs on it; the site constant is not code.

### The provider lives inside `scraper_config`

No migration. `portals.scraper_config` is already JSON, the router already
parses it, and a missing `provider` means `selectors`, so every existing row
keeps working:

```json
{ "provider": "selectors", "list": "article.job-card", "fields": { "title": "h3 a" } }
```

```json
{ "provider": "eightfold", "domain": "nvidia.com" }
```

A `provider` column would be cleaner SQL and a worse migration for a value that
is meaningless without the JSON beside it.

### Registry entries are typed Rust

`crates/core/src/acquisition/known_sites.rs` holds typed constants rather than a
JSON file. One code address per site, an exhaustive provider enum, and no
separate data file that can drift from the enum:

```rust
pub struct KnownSite {
    pub host: &'static str,
    /// Optional path prefix when one host serves several boards.
    pub path_prefix: Option<&'static str>,
    pub name: &'static str,
    pub provider: Provider,
    /// The full `scraper_config` offered to the user, `provider` included.
    pub config: &'static str,
    /// ISO date of the last passing live run. `None` until one passes.
    pub verified_at: Option<&'static str>,
    pub notes: &'static str,
}

pub const WAYMO: KnownSite = KnownSite {
    host: "careers.withwaymo.com",
    path_prefix: Some("/jobs/search"),
    name: "Waymo",
    provider: Provider::Selectors,
    config: WAYMO_CONFIG,
    verified_at: Some("2026-11-03"),
    notes: "AWS WAF intermittently answers 202 with an empty body; the live test probes and skips.",
};

pub const NVIDIA: KnownSite = KnownSite {
    host: "jobs.nvidia.com",
    path_prefix: Some("/careers"),
    name: "NVIDIA",
    provider: Provider::Eightfold,
    config: r#"{"provider":"eightfold","domain":"nvidia.com"}"#,
    verified_at: None, // set by the WS4 live run
    notes: "Client-rendered Eightfold site; content comes from /api/pcsx/*.",
};

pub fn all() -> &'static [KnownSite] {
    &[WAYMO, NVIDIA]
}
```

- Keyed by `host`, with an optional `path_prefix` so a host can serve several
  boards (`shopify.com/careers` vs `shopify.com/university`) without a wildcard
  language.
- `WAYMO_CONFIG` is the Phase 2.2 selector config embedded as a JSON string with
  `"provider":"selectors"`. It stays a string so the form can prefill exactly
  the text the user may edit, and so the config has one representation.
- `verified_at` is `Some` only after a passing live run, never the date the entry
  was written. The registry test validates the date format when present; WS4's
  exit check requires both shipped entries to be `Some`.
- The `config` string is the single source of truth. A test parses every entry
  and asserts that the `provider` inside the JSON matches the typed field, so
  the two cannot disagree.
- A site with no engine yet (NVIDIA before WS2) stays in the file with
  `verified_at: None`; it can be previewed but not presented as verified.

---

## Workstream 1 — Provider model and dispatch

**Priority:** First.
**Estimated duration:** 0.5 day.

### Deliverables

1. `Provider` enum in a new `core::acquisition::config` module (shared by the
   config parser, the router, and the registry): `Selectors` (default) and
   `Eightfold`, deserialised from `scraper_config.provider`.
2. The router dispatches on the provider instead of assuming selectors. The
   existing `SelectorConfig` becomes the `Selectors` payload unchanged, so Phase
   2.2's tests keep passing without edits.
3. An unknown `provider` value is a classified config error, surfaced to the
   user the same way an invalid URL is: fix the config, don't retry.
4. `Selectors` is site-agnostic: Waymo is `Provider::Selectors` with an embedded
   config, not a provider variant of its own. `Provider::Eightfold` exists
   because its fetch and parse shape is different, not because its company is.

### Acceptance criteria

| Criterion | Verification |
| :--- | :--- |
| A config without `provider` still works | Existing selector tests unchanged |
| `provider: "eightfold"` reaches the Eightfold path | Router unit test |
| Unknown provider is a clear config error | Unit test on deserialisation |
| `provider` survives add → edit → fetch | App integration test |
| Waymo's registry entry dispatches to `Selectors` | Registry/dispatch test |

**Implementation note (2026-11-03).** `core::acquisition::config` now owns the
provider discriminator. `ScraperConfig::parse` reads `provider` (missing or null
means `selectors`), then deserialises the same object into `SelectorConfig` or
`EightfoldConfig`; `SelectorConfig` itself is unchanged, so every Phase 2.2 row
and test still works. `EightfoldConfig` carries `domain` plus the existing
`pagination` and `detail` blocks, so caps mean the same thing for every engine.

The router parses the config once and fails an unusable config as
`InvalidRequest` instead of logging and ignoring it. That is deliberate: a
config the user wrote should not be silently dropped. The Eightfold arm is a
WS1 placeholder — it returns "the eightfold provider is not available in this
build" and never touches the selector path; WS2 replaces it with the fetch. The
registry half of the last criterion lands in WS3; WS1 already proves that an
explicit `provider: "selectors"` dispatches to the HTML path.

---

## Workstream 2 — Eightfold provider

**Priority:** Second. This is the capability the user wants.
**Estimated duration:** 2 days.

### Endpoints and shapes (probed 2026-10-25)

- List: `GET https://{host}/api/pcsx/search?domain={domain}&start={offset}`
  → `data.count`, `data.positions[]` (10 per page; `num` is ignored).
  Position: `id`, `name`, `locations[]`, `standardizedLocations[]`,
  `department`, `workLocationOption`, `positionUrl`, `postedTs` (epoch seconds),
  `atsJobId`/`displayJobId`. No description.
- Detail: `GET /api/pcsx/position_details?position_id={id}&domain={domain}&hl=en`
  → `data.jobDescription` (HTML), `publicUrl`, `department`, `locations`,
  `efcustomTextTimeType`.

### Deliverables

1. `EightfoldProvider` implementing the same "return jobs" contract as the tier
   paths, so the router can store its results unchanged.
2. Pagination by `start += 10`, bounded by Phase 2.2's `max_pages` (default 5 ⇒
   50 jobs from a 2681-posting site). Stop on an empty page or the cap; never
   walk `count` blindly.
3. `external_id = "eightfold:{domain}:{id}"` so re-fetch is idempotent even if
   the site changes the URL slug.
4. `source_url` from `publicUrl`, falling back to `positionUrl` resolved against
   the host.
5. Descriptions: one detail request per posting, bounded by Phase 2.2's
   `detail.max_jobs`, sequential with the same small delay, cancellable between
   postings. Reuse 2.2's enrichment loop rather than a second implementation.
6. `raw_text` from `jobDescription`, HTML-to-text the same way the other
   adapters do, truncated at 200 KB.
7. `department`, `employment_type` (from `efcustomTextTimeType`), `locations`
   (first entry) mapped into `NormalizedJob`.

### Acceptance criteria

| Criterion | Verification |
| :--- | :--- |
| A fixture list response yields the expected jobs | Fixture test (3 positions) |
| Paging stops at the cap and on an empty page | Mock/fixture test |
| `external_id` is stable and URL-independent | Unit test: same id, renamed slug |
| Description comes from the detail endpoint | Fixture test |
| Detail failures keep the list rows | Mock test: one 404 |
| Cancellation stops between postings | Core test with a pre-set flag |
| Re-fetch is idempotent | Fetch twice, one row per position |
| Live NVIDIA board stores jobs with text | Live test (§Phase 2.3 exit criteria) |

**Implementation note (2026-11-03).** The engine lives in
`core::acquisition::eightfold`: `parse_page`, `normalize`, `detail_url`,
`detail_from_body`, and a paging loop behind a private `PageSource` seam (the
same shape as the selector path). `FetchSource::fetch_eightfold` carries the
host, domain, company name, and page cap; the router dispatches to it before
`decide_initial_route`, then reuses the Phase 2.2 enrichment loop through a
`DetailSource` enum (ATS adapter vs. Eightfold).

Four decisions worth knowing:

1. **Enrichment is default-on for Eightfold** (cap 40) because the list has no
description at all; selectors stay opt-in through a `detail` block. The block
only changes the cap.
2. **`FetchTier::Tier1aAts`** is reused. Eightfold is a structured provider API,
not a new tier; adding a tier variant would ripple through storage, UI labels,
and the closed failure vocabulary for no user-visible gain.
3. **`source_url` comes from the list's `positionUrl`**, resolved against the
portal host. The detail's `publicUrl` duplicates it; using it would make the
dedup key depend on whether enrichment ran.
4. **Descriptions are stored as HTML**, matching SmartRecruiters and JSON-LD, and
truncated at 200 KB. The plan said "HTML-to-text"; no adapter strips markup
today, and a lone stripper would make Eightfold rows the odd ones out. Phase 3
should treat `raw_text` as markup for every provider.

Live verification 2026-11-03: NVIDIA returned 2,750 positions; two pages stored
20 jobs, and 2 of 2 requested details produced text (longest 6,253 bytes). The
core test took 1.9 s, the app path 5.5 s including startup.

`PageReport`/`PaginationStop` moved from `html` to `acquisition::report` in the
same step: two engines now share the report, so the type should not live inside
one of them.

---

## Workstream 3 — Known-site registry and prefill

**Priority:** After WS2.
**Estimated duration:** 1–1.5 days.

### Deliverables

1. `crates/core/src/acquisition/known_sites.rs` with typed `KnownSite`
   constants and `all()`, embedded in the binary (not seeded into the database):
   it is app content, not user data, and a bad entry must be fixable by a
   release, not by a migration. The `Provider` enum comes from
   `core::acquisition::config` (WS1).
2. Lookup by host + optional path prefix, and prefill in the portal form: typing
   a URL that matches an entry shows "Known site: NVIDIA (Eightfold) — verified
   <date>" and, **only when the provider/config fields are empty**, prefills
   them. A "Use known config" button appears when the fields differ from the
   entry; the user's edits are never overwritten automatically.
3. A provider selector in the form (Selectors / Eightfold) for sites the
   registry does not know, with the domain field shown for Eightfold.
4. The existing scraper-config field remains the escape hatch for `selectors`,
   so "known" is a convenience, not a gate.
5. **One shape for every site.** Waymo (`Selectors`) and NVIDIA (`Eightfold`)
   must traverse the same validation, prefill, fixture, live-test, and docs path.
   A site-specific branch in the fetch path is a design error.

### Acceptance criteria

| Criterion | Verification |
| :--- | :--- |
| Every entry parses, and its JSON `provider` matches its typed `provider` | Unit test over `all()` |
| Lookup matches host and path prefix | Unit test (hit, miss, prefix mismatch) |
| Prefill happens only when the fields are empty | `egui_kittest` |
| Editing after prefill is preserved | `egui_kittest` |
| "Use known config" replaces a divergent config | `egui_kittest` |
| A manually chosen provider works for an unknown host | App integration test |
| Waymo and NVIDIA take the same path | Registry test: both produce a valid `scraper_config` and prefill identically |

**Implementation note (2026-11-03).** `core::acquisition::known_sites` holds
`KnownSite { host, path_prefix, name, provider, config, verified_at, notes }`,
the `WAYMO`/`NVIDIA` constants, `all()`, and `lookup(&Url)`. Lookup compares the
host case-insensitively and requires the path prefix, so one host can serve
several boards; the longest matching prefix wins.

`PortalForm` gained `provider`, `domain`, `eightfold_config`, and `prefilled_for`.
`data()` always produces one JSON object: selectors stores the field verbatim,
and Eightfold merges the advanced JSON under the `provider`/`domain` that the
fields own. That keeps one storage shape for both engines, which is what lets
`ScraperConfig::parse` stay the only validator.

Three decisions worth knowing:

1. **Prefill is a render-time side effect, guarded by `prefilled_for`.** It runs
   only while the provider fields are empty and only once per site, so it cannot
   overwrite an edit and cannot resurrect itself after the user clears a field.
   "Use known config" is the explicit, repeatable path and appears only while
   the form differs from the entry.
2. **Save and Test config share `provider_fields_valid()`.** This is stricter
   than Phase 2.2, where invalid JSON was reported inline but still sent on save.
   A config that cannot parse can never be correct, so it should not reach the
   network or the database. `valid()` stays the URL/company gate.
3. **A selectors config that names another provider is refused.** The form points
   at the provider combo instead of silently dispatching elsewhere; the fetch
   would classify it, but only after a save.

Tests: the registry has seven unit tests (every entry parses and matches its
typed provider, a present `verified_at` is a valid date, an unverified entry is
labelled as such, lookup hit/miss/prefix, and one test per entry shape).
`egui_kittest` covers the label and prefill for both entries, the no-overwrite
rule, "Use known config", and the domain gate. Two app tests go through the
real storage and router: a hand-chosen provider for a host the registry does not
know, and every registry entry from form to stored row to fetch, proving the
dispatch follows the stored config rather than a site-specific branch.

Known-site labels are ASCII only (a hyphen, not an em dash) because
`egui_kittest` queries the exact string.

---

## Workstream 4 — Curation workflow and live verification

**Priority:** Last.
**Estimated duration:** 1 day.

### Deliverables

1. `live_fetch_reaches_the_pipeline` gains a registry mode: with
   `CAREER_OPS_LIVE_REGISTRY=1`, it iterates `known_sites::all()`, configures a
   portal from each entry, fetches, and asserts at least one stored job with
   non-empty text. That is what "known" means: a test passes. A WAF challenge is
   probed the same way as the Waymo live test and reported as a **skip for that
   entry**, not a failure; a served page that yields no text fails.
2. `docs/known-sites.md`: host, name, provider, verified date, caps, notes
   (including blocks and rate limits observed).
3. Rules in that document: an entry ships only with a passing live run; a
   failing entry is fixed or removed in the same commit that bumps the release.
   The onboarding checklist is the one in "Adding a site or a provider" above.
4. Initial entries: Waymo (`Selectors`, Phase 2.2's config) and NVIDIA
   (`Eightfold`). Both are `KnownSite` constants. `verified_at` is committed by
   hand after a passing registry run, not generated: a skip leaves it `None`.

**Implementation note (2026-11-03).** The check is
`live_registry_sites_reach_the_pipeline`, an ignored test gated on
`CAREER_OPS_LIVE_REGISTRY=1`. It is a sibling of
`live_fetch_reaches_the_pipeline` rather than a mode inside it: a challenge must
skip *one entry* without aborting the run, and that test is fail-fast over a
single URL list. Both still run under `--ignored live_fetch`, and neither is
affected by the other's environment variable.

Per entry the test probes the page, creates the portal from the `KnownSite`
constant, fetches it within a 180 s bound, and checks storage: at least one job
with non-empty text and every `source_url` absolute. A challenge before or after
the fetch skips the entry and leaves `verified_at` alone; a served page that
stores nothing fails. It never writes `known_sites.rs`, so a date can only be
committed by a person reading the run's last line.

`docs/known-sites.md` carries the entries, what "verified" means, the curation
rules (an entry ships only with a passing run; a failure is fixed or removed in
the release commit), and the onboarding checklist from "Adding a site or a
provider". In production nothing reads `verified_at`: it is a claim in the UI, a
gate in review, and a unit test that a present date parses.

The skip rules do not depend on a live failure to be trusted. `registry_entry`
is a small sequencer — probe, fetch only a served page, re-probe only to explain
a failed fetch — that the live check and four ordinary unit tests share, with a
call recorder in place of the app. They pin the ordering (a challenged page is
never fetched, a healthy fetch is not re-probed) and every branch of the outcome
enum. The second live run exercised the skip path for real: Waymo was challenged
on the probe and skipped without a fetch while NVIDIA verified again.

First full registry run (2026-11-03, no skips):

| Entry | Result | Stored | Text | Longest | Time |
| :--- | :--- | ---: | ---: | ---: | ---: |
| Waymo | `Fetched 90 jobs from 3 pages; 2 of 2 descriptions.` | 90 | 90 | 3,611 B | 3.7 s |
| NVIDIA | `Fetched 50 jobs from 5 pages; 40 of 40 descriptions.` | 50 | 40 | 7,723 B | 27.2 s |

Waymo's 90 jobs carry text on every row because the card selector fills a teaser;
NVIDIA's 40-of-50 is the default `detail.max_jobs` cap, which is the documented
coverage boundary rather than a missing description. The 27 s is 45 paced
requests (5 list + 40 detail) at 400–800 ms each.

---

## Test Strategy

- **Unit / fixture**: provider deserialisation, Eightfold list/detail parsing,
  paging stop conditions, idempotent ids, registry validation.
- **`egui_kittest`**: lookup, prefill, no-overwrite, provider selector.
- **App integration**: a scripted provider through the real router and storage.
- **Live (ignored)**: the existing default ATS boards, the Waymo pagination
  variant, and the registry iteration (`CAREER_OPS_LIVE_REGISTRY=1`).

---

## Risks

| ID | Risk | Impact | Mitigation |
| :--- | :--- | :--- | :--- |
| P2.3-R1 | **Unofficial API changes.** Eightfold's `pcsx` endpoints are not a public contract; a field rename breaks the adapter. | Medium | Fixture tests catch parsing changes at build time only if the fixture is refreshed; the live test is the real detector. Classify failures, never crash. |
| P2.3-R2 | **2681 postings invites a crawl.** Ten per request; a full walk is 269 requests. | High | `max_pages` default 5 (50 jobs), detail capped at 40. No "fetch everything" button. |
| P2.3-R3 | **Registry rot.** Sites change; an entry silently yields zero jobs. | Medium | `verified_at` + live iteration; zero-job results are reported as an extraction failure, not success. |
| P2.3-R4 | **Prefill surprises.** Auto-filling a config the user did not ask for. | Low | Prefill only into empty fields, show the source and date, never overwrite edits. |
| P2.3-R5 | **Provider sprawl.** Every new company becomes a provider. | Medium | Providers are engines; known sites are constants. This phase adds exactly one engine (Eightfold) and two site profiles (Waymo, NVIDIA). A second engine needs its own justification and fixtures. |
| P2.3-R6 | **ToS/robots ambiguity for JSON APIs.** | Medium | Same posture as Phase 2.2 Q-8: report blocks, never evade; document the user's responsibility. |

---

## Exit Criteria

1. `scraper_config` accepts a provider and a missing provider still means
   selectors.
2. NVIDIA can be added as a portal and fetched: list from `pcsx/search`,
   descriptions from `position_details`, up to the configured caps.
3. Re-fetching NVIDIA is idempotent, even if a slug changes.
4. The registry contains Waymo and NVIDIA, each with a passing live run and a
   `verified_at: Some(date)`.
5. Waymo and NVIDIA are both `KnownSite` constants with a provider, embedded
   config, fixture test, live test, and docs row. Waymo names `Selectors`;
   neither site has site-specific code in the fetch path.
6. Typing a known host in the portal form offers the config/provider without
   overwriting what the user typed.
7. An unknown host can still be configured manually (selectors or Eightfold).
8. `docs/known-sites.md` documents the entries and the curation rule.
9. Phase 2.3 retrospective recorded.

---

## Open Questions

| ID | Question | Needed by |
| :--- | :--- | :--- |
| Q-12 | **Should the registry be updatable without a release** (seeded table or a signed remote list)? The Rust module is embedded and offline-safe; a remote list is a supply-chain surface. | Resolved for Phase 2.3: embedded (`known_sites.rs`), per v0.2. Revisit only if stale entries become a real support load. |
| Q-13 | **Should Eightfold be auto-detected** for unknown hosts? It would help users, but detection is a guess and Eightfold's markup is a vendor detail. | Phase 2.3 WS2 |
| Q-14 | **Page size.** The API ignores `num` and returns 10; is that stable, and does `start` have a ceiling? Confirm with the live run before fixing defaults. | Partly answered by the 2026-11-03 live run (10 per page, ~2,750 total); the `start` ceiling is untested and the defaults stay at `max_pages` ≤ 5. |
| Q-15 | **Which provider next?** Workday is the most common for large employers; decide by measuring the user's actual targets, not by popularity. | After Phase 2.3 |

---

## What Phase 2.3 Is Not

- Not a generic JSON-extraction engine, and not a jq clone.
- Not a provider per company. A provider is an engine; a known site is data.
- Not auto-detection of every job platform.
- Not a crawler: every fetch remains a user action on a configured portal.
- Not a replacement for selector configs. Selectors remain the general answer for
  server-rendered sites; providers are for sites whose content is not in the
  HTML.
