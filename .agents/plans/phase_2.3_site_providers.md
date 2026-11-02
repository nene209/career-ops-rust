# Phase 2.3: Site Providers and Known Sites — Detailed Execution Plan

**Parent plan:** Draft Execution Plan: Career-Ops Rust Desktop Application (v0.8), §12
**Status:** Draft — not started
**Last updated:** 2026-10-25
**Duration:** 4–6 days
**Depends on:** Phase 2.2 (pagination and detail enrichment freeze the selector-config schema)

---

## Change Log

| Version | Change |
| :--- | :--- |
| v0.1 | Initial plan. Written after probing `https://jobs.nvidia.com/careers`, which is an Eightfold site: its content is served by two JSON endpoints and it cannot be fetched by either extraction tier today, with or without a selector config. That turned "a list of known configs" into "known sites with providers", because the second site the user wants is not a config at all. |

---

## Purpose

Phase 2.2 makes selector-configured sites scrape well. This phase answers two
different questions:

1. **Must every user write their own config?** No. If the app knows the site, it
   should offer the config, while still letting the user change it.
2. **What about sites that are not HTML at all?** A growing number of career
   sites are a shell around a JSON API. A selector config cannot help them, at
   either tier, because there is no job markup to select.

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
3. A **known-site registry**: embedded data keyed by host, with a name, a
   provider, the config, a verification date, and notes.
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

### Registry entries are minimal and verifiable

```json
{
  "host": "careers.withwaymo.com",
  "name": "Waymo",
  "provider": "selectors",
  "path_prefix": "/jobs/search",
  "verified_at": "2026-10-25",
  "config": { "list": "article.job-search-results-card-col", "fields": { "title": "h3.job-search-results-card-title a" }, "pagination": { "url_template": "https://careers.withwaymo.com/jobs/search?page={page}", "max_pages": 5 }, "detail": { "fields": { "description": ".job-description" } } }
}
```

- Keyed by `host`, with an optional `path_prefix` so a host can serve several
  boards (`shopify.com/careers` vs `shopify.com/university`) without a wildcard
  language.
- `verified_at` is the date the live test last passed, not the date it was
  written. An entry without a passing live test does not ship.

---

## Workstream 1 — Provider model and dispatch

**Priority:** First.
**Estimated duration:** 0.5 day.

### Deliverables

1. `Provider` enum in `core::acquisition::html` (or a new
   `core::acquisition::config` module): `Selectors` (default) and `Eightfold`,
   deserialised from `scraper_config.provider`.
2. The router dispatches on the provider instead of assuming selectors. The
   existing `SelectorConfig` becomes the `Selectors` payload unchanged, so Phase
   2.2's tests keep passing without edits.
3. An unknown `provider` value is a classified config error, surfaced to the
   user the same way an invalid URL is: fix the config, don't retry.

### Acceptance criteria

| Criterion | Verification |
| :--- | :--- |
| A config without `provider` still works | Existing selector tests unchanged |
| `provider: "eightfold"` reaches the Eightfold path | Router unit test |
| Unknown provider is a clear config error | Unit test on deserialisation |
| `provider` survives add → edit → fetch | App integration test |

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

---

## Workstream 3 — Known-site registry and prefill

**Priority:** After WS2.
**Estimated duration:** 1–1.5 days.

### Deliverables

1. `crates/core/src/acquisition/known_sites.json` embedded with `include_str!`,
   deserialised once at startup, with a lookup by host + optional path prefix.
   Embedded rather than seeded into the database: it is app content, not user
   data, and a bad entry must be fixable by a release, not by a migration.
2. Lookup in the portal form: typing a URL that matches an entry shows
   "Known site: NVIDIA (Eightfold) — verified 2026-10-25" and, **only when the
   provider/config fields are empty**, prefills them. A "Use known config"
   button appears when the fields differ from the entry; the user's edits are
   never overwritten automatically.
3. A provider selector in the form (Selectors / Eightfold) for sites the
   registry does not know, with the domain field shown for Eightfold.
4. The existing scraper-config field remains the escape hatch for `selectors`,
   so "known" is a convenience, not a gate.

### Acceptance criteria

| Criterion | Verification |
| :--- | :--- |
| `known_sites.json` parses and every entry is valid for its provider | Unit test over the registry (all-or-nothing) |
| Lookup matches host and path prefix | Unit test (hit, miss, prefix mismatch) |
| Prefill happens only when the fields are empty | `egui_kittest` |
| Editing after prefill is preserved | `egui_kittest` |
| "Use known config" replaces a divergent config | `egui_kittest` |
| A manually chosen provider works for an unknown host | App integration test |

---

## Workstream 4 — Curation workflow and live verification

**Priority:** Last.
**Estimated duration:** 1 day.

### Deliverables

1. `live_fetch_reaches_the_pipeline` gains a registry mode: with
   `CAREER_OPS_LIVE_REGISTRY=1`, it iterates every registry entry, configures a
   portal from it, fetches, and asserts at least one stored job with non-empty
   text. That is what "known" means: a test passes.
2. `docs/known-sites.md`: host, name, provider, verified date, caps, notes
   (including blocks and rate limits observed).
3. Rules in that document: an entry ships only with a passing live run; a
   failing entry is fixed or removed in the same commit that bumps the release.
4. Initial entries: Waymo (selectors, Phase 2.2's config) and NVIDIA
   (Eightfold).

---

## Test Strategy

- **Unit / fixture**: provider deserialisation, Eightfold list/detail parsing,
  paging stop conditions, idempotent ids, registry validation.
- **`egui_kittest`**: lookup, prefill, no-overwrite, provider selector.
- **App integration**: a scripted provider through the real router and storage.
- **Live (ignored)**: the existing default ATS boards, plus the registry
  iteration once entries exist.

---

## Risks

| ID | Risk | Impact | Mitigation |
| :--- | :--- | :--- | :--- |
| P2.3-R1 | **Unofficial API changes.** Eightfold's `pcsx` endpoints are not a public contract; a field rename breaks the adapter. | Medium | Fixture tests catch parsing changes at build time only if the fixture is refreshed; the live test is the real detector. Classify failures, never crash. |
| P2.3-R2 | **2681 postings invites a crawl.** Ten per request; a full walk is 269 requests. | High | `max_pages` default 5 (50 jobs), detail capped at 40. No "fetch everything" button. |
| P2.3-R3 | **Registry rot.** Sites change; an entry silently yields zero jobs. | Medium | `verified_at` + live iteration; zero-job results are reported as an extraction failure, not success. |
| P2.3-R4 | **Prefill surprises.** Auto-filling a config the user did not ask for. | Low | Prefill only into empty fields, show the source and date, never overwrite edits. |
| P2.3-R5 | **Provider sprawl.** Every new vendor becomes a PR-shaped special case. | Medium | This phase adds exactly one provider. The registry gives a place for configs; a second provider needs its own justification and fixtures. |
| P2.3-R6 | **ToS/robots ambiguity for JSON APIs.** | Medium | Same posture as Phase 2.2 Q-8: report blocks, never evade; document the user's responsibility. |

---

## Exit Criteria

1. `scraper_config` accepts a provider and a missing provider still means
   selectors.
2. NVIDIA can be added as a portal and fetched: list from `pcsx/search`,
   descriptions from `position_details`, up to the configured caps.
3. Re-fetching NVIDIA is idempotent, even if a slug changes.
4. The registry contains Waymo and NVIDIA, each with a passing live run and a
   `verified_at` date.
5. Typing a known host in the portal form offers the config/provider without
   overwriting what the user typed.
6. An unknown host can still be configured manually (selectors or Eightfold).
7. `docs/known-sites.md` documents the entries and the curation rule.
8. Phase 2.3 retrospective recorded.

---

## Open Questions

| ID | Question | Needed by |
| :--- | :--- | :--- |
| Q-12 | **Should the registry be updatable without a release** (seeded table or a signed remote list)? Embedded is simpler and offline-safe; a remote list is a supply-chain surface. | Before WS3 ships |
| Q-13 | **Should Eightfold be auto-detected** for unknown hosts? It would help users, but detection is a guess and Eightfold's markup is a vendor detail. | Phase 2.3 WS2 |
| Q-14 | **Page size.** The API ignores `num` and returns 10; is that stable, and does `start` have a ceiling? Confirm with the live run before fixing defaults. | Phase 2.3 WS2/WS4 |
| Q-15 | **Which provider next?** Workday is the most common for large employers; decide by measuring the user's actual targets, not by popularity. | After Phase 2.3 |

---

## What Phase 2.3 Is Not

- Not a generic JSON-extraction engine, and not a jq clone.
- Not auto-detection of every job platform.
- Not a crawler: every fetch remains a user action on a configured portal.
- Not a replacement for selector configs. Selectors remain the general answer for
  server-rendered sites; providers are for sites whose content is not in the
  HTML.
