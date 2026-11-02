# Phase 2.2: Acquisition Completeness — Detailed Execution Plan

**Parent plan:** Draft Execution Plan: Career-Ops Rust Desktop Application (v0.7), §12
**Status:** In progress — Workstream 1 implemented (2026-10-25)
**Last updated:** 2026-10-25
**Duration:** 4–6 days (two workstreams deliver usable scraping; the rest is quality)
**Depends on:** Phase 2 (acquisition) and Phase 2.1 (UI shell)

---

## Change Log

| Version | Change |
| :--- | :--- |
| v0.1 | Initial plan. Written after the Phase 2.1 verification run against `https://careers.withwaymo.com/jobs/search`, which proved that a server-rendered page with a `scraper_config` works on Tier 1b (30 jobs, no sidecar) but exposed three gaps that would starve Phase 3: no pagination, no job descriptions for page-based portals, and no description at all from the SmartRecruiters adapter. |
| v0.2 | Workstream 1 implemented: `PaginationConfig` on `SelectorConfig` (`next`, `url_template`, `max_pages`), the page loop in `html::fetch_static_jobs`, and thirteen unit tests covering every stop condition. Verified live against Waymo: 90 jobs from 3 pages via `rel=next` (Tier 1b). Also learned that the site's AWS WAF answers sustained fetching with `202`/empty body, which the manual script now documents. |

---

## Purpose

Phase 2 answers "can the app fetch a job board?" Phase 2.1 answers "can a person
configure one?". Neither answers "does the app have enough of the posting text to
evaluate it?", and the difference only became visible when a real, non-ATS site
was configured and fetched.

The specific evidence, from 2026-10-25:

1. **Tier 1b stops at page one.** Waymo's search page returns 30 cards and
   paginates to 13 pages (~390 postings). `selectors::extract` has no notion of a
   second page.
2. **Tier 1b stores a summary, not a description.** The card carries a two-line
   teaser; the job description lives on the posting page, which nothing fetches.
3. **The SmartRecruiters adapter stores no description and a board-level URL.**
   `smartrecruiters::normalize` sets `raw_text: String::new()` and
   `source_url = https://jobs.smartrecruiters.com/{slug}` for every posting; the
   description requires one detail request per posting. A 237-job board fetched
   today therefore contributes 237 rows that Phase 3 cannot evaluate.

Phase 3 (AI integration) can be built against thin job rows, but the evaluations
produced from two-sentence summaries would be poor and the dashboard would look
empty. This phase is the smallest piece of work that makes "fetch a company's
careers page and evaluate what is there" true end to end.

**Sequencing matters more than completeness here.** Workstreams 1 and 2 are the
usable scraping milestone and should land first; 3 and 4 improve quality and
ergonomics and can follow immediately after.

---

## Entry Conditions

Already in place and reused unchanged:

| Asset | Where |
| :--- | :--- |
| Tier routing, escalation, classified failures | `crates/core/src/acquisition/router.rs` |
| CSS selector extraction with `SelectorConfig` | `crates/core/src/acquisition/html/selectors.rs` |
| JSON-LD `JobPosting` extraction (reusable on detail pages) | `crates/core/src/acquisition/html/jsonld.rs` |
| Dedup that matches on `(source_url, external_id)` before content hash | `crates/core/src/repo/job_descriptions.rs` |
| `scraper_config` field in the portal form, validated and stored | `crates/app/src/screens/portals.rs` |
| Live test harness with URL and config overrides | `app::tests::live_fetch_reaches_the_pipeline` |
| Fixture test pinning a real card-page selector shape | `crates/core/tests/ats_adapters.rs` |

No schema migration is expected. `portals.scraper_config` is JSON, and
`job_descriptions.raw_text` already exists.

---

## Scope

### In scope

1. Pagination for selector-configured portals (Tier 1b), with hard caps.
2. Detail-page enrichment: fetch the posting URL and store the description text.
3. The SmartRecruiters adapter gap (job URL and description).
4. Configuration ergonomics: a "Test config" preview in the portal form so a
   user can see what a config extracts before saving/fetching.
5. Live verification against at least two real sites, including Waymo.

### Out of scope

- Any AI evaluation, prompt work, or scoring (Phase 3).
- PDF rendering (Phase 4).
- Background crawling, scheduling, or monitoring. Fetches remain user-initiated;
  the "no background crawl" rule from Phase 2 still holds.
- Anti-bot evasion, proxy rotation, or captcha solving. If a site blocks us, that
  is reported, not worked around.
- robots.txt policy enforcement (see Open Questions; at minimum, document the
  posture).
- Pagination for ATS APIs: Greenhouse, Lever, Ashby, and (after WS3)
  SmartRecruiters paginate inside their adapters already.

---

## Workstream 1 — Pagination for selector-configured portals

**Priority:** First. Without it, a configured board is one page of jobs.
**Estimated duration:** 1–1.5 days.

### Config shape

`SelectorConfig` gains one optional block. Both mechanisms are supported because
sites use both; `next` wins when present.

```json
{
  "list": "article.job-search-results-card-col",
  "fields": { "title": "h3.job-search-results-card-title a" },
  "pagination": {
    "next": "a[rel='next'], nav a.next",
    "url_template": "https://careers.withwaymo.com/jobs/search?page={page}",
    "max_pages": 5
  }
}
```

- `next` — selector for the link to the next page. The href is resolved against
  the page URL (relative paths work, as with job URLs).
- `url_template` — used when the site only exposes numbered links; `{page}` is
  substituted starting at 2.
- `max_pages` — default 5, hard ceiling 20. A cap is not optional: an unbounded
  loop against a calendar-style site is how a scraper gets blocked.

### Deliverables

1. `pagination: Option<PaginationConfig>` on `SelectorConfig`.
2. `html::fetch_static_jobs` loops pages (bounded by `max_pages`), accumulating
   `NormalizedJob`s. The existing single-page `extract` stays pure and testable;
   the loop lives beside it.
3. **Stop conditions**: no next link/template match, zero jobs on a page (a
   repeated page is a loop, not data), a job URL that already appeared (the site
   ignored `?page`), or the cap.
4. Dedup within the fetch: two pages containing the same card must not produce
   two rows. `job_descriptions::upsert` already dedups on
   `(source_url, external_id)`, but the in-memory list should not grow either.
5. Page-level failures degrade, not fail: if page 4 errors, pages 1–3 are still
   returned. Record the partial outcome (loop count, pages fetched) for the UI
   message.

### Acceptance criteria

| Criterion | Verification |
| :--- | :--- |
| `next` pagination follows relative and absolute links | Fixture test with two pages linked `rel=next` |
| `url_template` pagination builds `page=2..N` | Fixture test + unit test on the URL builder |
| `max_pages` is honoured and defaults to 5 | Unit test, cap=3 fixture |
| A repeated page stops the loop | Fixture test: page 2 identical to page 1 |
| A failing middle page returns the earlier pages | Mock `FetchSource`/fixture test |
| Duplicate cards across pages yield one job | Fixture test + `upsert` count assertion |
| Waymo fetches more than 30 jobs | Live test (§8 of the manual script) |

**Implementation note (2026-10-25).** Landed as `PaginationConfig` in
`crates/core/src/acquisition/html/selectors.rs` and a page loop in
`html::fetch_static_jobs`. The loop sits behind a private `PageSource` trait with
an HTTP implementation and a scripted one for tests, because every stop
condition (failure after page one, a repeated page, the cap) needs a server that
misbehaves on purpose. Thirteen unit tests cover them. Verified live against
Waymo through the app: 90 jobs from 3 pages via `rel=next` (6.7 s), and 60 jobs
from 2 pages via `url_template` (4.9 s).

One operational finding: the site's AWS WAF answers sustained automated fetching
with `202` and an empty body. A run that fails with `Broken { cause: "other" }`
after a previous run succeeded is a challenge, not a pagination bug; the manual
script now documents the `curl` check and the wait. This is risk P2.2-R1
happening in practice.

Two deliberate deviations from the plan text: partial-page outcomes are logged
with `tracing::warn!` rather than surfaced in the portal message (the message
belongs with the enrichment counts in WS2/WS5), and `max_pages` is clamped to
`1..=20` so an accidental `0` cannot mean "fetch nothing".

---

## Workstream 2 — Detail-page enrichment

**Priority:** Second. This is what makes a fetched job evaluable.
**Estimated duration:** 1.5–2 days.

### Behaviour

After the list (and pagination) is extracted, for each job up to a cap, fetch
`job.source_url` and extract the description. Two extraction paths, in this
order:

1. **JSON-LD `JobPosting` on the detail page.** Most ATS-hosted detail pages and
   many custom sites emit it; `jsonld::extract` already handles it. When it
   yields a job whose title matches, take its `description` as `raw_text`.
2. **Selector config.** A new optional `detail.fields` block (reusing
   `FieldSelectors`) for sites that have no JSON-LD.

If both fail, keep the list-level row (with whatever text the card had) and count
it as a detail failure. A failed detail fetch must never lose the job.

### Config shape

```json
{
  "list": "article.job-search-results-card-col",
  "fields": { "title": "h3.job-search-results-card-title a" },
  "detail": {
    "fields": {
      "description": ".job-description",
      "title": "h1"
    },
    "max_jobs": 40
  }
}
```

`detail` absent means no enrichment for that portal. `max_jobs` defaults to 40,
hard ceiling 200. The cap exists because each job is one HTTP request: 390 jobs
× 30 s timeout is not a fetch, it is an incident.

### Deliverables

1. `detail: Option<DetailConfig>` on `SelectorConfig`.
2. `html::fetch_detail(client, job, config)` returning
   `Option<String>` (the description) — tries JSON-LD, then selectors.
3. Sequential enrichment with a small inter-request delay (start at 150 ms,
   configurable in code, not per portal). Sequential is deliberate: a desktop
   app fetching a user's chosen board should look like one careful client, not
   a burst. Revisit only if a real board makes it too slow.
4. **Cancellation.** Enrichment must stop between jobs when the user cancels, or
   a 40-job pass becomes uninterruptible. This needs a cancellation token in the
   router/service path — the same `AtomicBool` pattern the batch fetch uses,
   threaded one level deeper (test-first).
5. **Size cap.** Truncate `raw_text` at 200 KB per job, with a warning recorded
   when it happens. Postings with embedded base64 images exist.
6. **Progress.** The portal-level spinner is enough for v1. Add a message with
   `jobs_enriched / jobs_attempted` on completion. Do not build per-job progress
   UI in this phase.
7. Re-fetch semantics: enrichment must be idempotent. The stored row keeps its
   `external_id` (the job URL for Tier 1b), so `upsert` updates `raw_text`
   instead of inserting a duplicate. Add a test that fetches twice and asserts
   one row per job with the enriched text.

### Acceptance criteria

| Criterion | Verification |
| :--- | :--- |
| A JSON-LD detail page enriches a list row | Fixture test (list + detail HTML) |
| A selector-configured detail page enriches a list row | Fixture test |
| A detail failure keeps the list row | Fixture/Mock test: 404 on one job, row still stored |
| `max_jobs` caps the number of detail requests | Mock counter assertion |
| Cancellation stops between jobs | Core test with a pre-set cancel flag |
| Enrichment is idempotent | Fetch twice, assert one row and updated text |
| Oversize descriptions are truncated | Unit test with a >200 KB fixture |
| Waymo rows carry the posting text, not the card teaser | Live test asserts a minimum `raw_text` length |

---

## Workstream 3 — SmartRecruiters description and job URL

**Priority:** Third. It fixes the one adapter that produces unevaluable rows.
**Estimated duration:** 0.5–1 day.

### Deliverables

1. `smartrecruiters::normalize` sets `source_url` to the posting page,
   `https://jobs.smartrecruiters.com/{slug}/{id}` (prefer `refNumber` for the
   identifier but use the API `id` in the URL), not the board root.
2. Fetch the posting detail
   (`GET https://api.smartrecruiters.com/v1/companies/{slug}/postings/{id}`) and
   build `raw_text` from `jobAd.sections` (company description, job
   description, qualifications, additional information), in that order.
3. Bound the detail pass with the same `max_jobs` rule as WS2, because it is one
   request per posting and Ubisoft2 alone has 237.
4. Department and employment type, if the detail response carries them.

### Acceptance criteria

| Criterion | Verification |
| :--- | :--- |
| `source_url` is per-posting and stable across fetches | Unit test on `normalize` + fixture |
| `raw_text` composes the four sections | Fixture test with all four, and with a missing section |
| Detail failures do not fail the fetch | Mock test: 404 on one posting |
| The live SmartRecruiters board stores non-empty text | Live test assertion |

---

## Workstream 4 — Config ergonomics: "Test config"

**Priority:** After WS1–WS2. This is what makes scraping usable without guessing.
**Estimated duration:** 0.5–1 day.

### Deliverables

1. **Test config** button in the portal modal (add and edit). It fetches the
   page with the current URL + config, without saving, and renders:
   - "N jobs on page 1; M pages followed"
   - the first three titles with their resolved URLs
   - the description length of the first enriched job
   - the extraction error, classified the same way the list shows it
2. The preview runs through the same core functions as a real fetch
   (`fetch_static_jobs` with the draft config), so what the preview shows is
   what the save will do. No parallel implementation.
3. Preview is bounded: page 1 only, at most 3 detail fetches, regardless of the
   config caps.

### Acceptance criteria

| Criterion | Verification |
| :--- | :--- |
| Preview shows counts and titles for a valid config | `egui_kittest` with a scripted source |
| An invalid config or a bad URL shows the classified error | `egui_kittest` |
| Preview performs one page and three details at most | Core test with a counting mock |
| Preview never writes to the database | App test: row count unchanged |

---

## Workstream 5 — Verification and retrospective

**Priority:** Last.
**Estimated duration:** 1 day.

1. Extend `live_fetch_reaches_the_pipeline` and add a paginated/detailed variant:
   - Waymo with the selector config: assert more than 30 jobs, and that the
     first N rows have `raw_text` longer than the card teaser.
   - SmartRecruiters: assert `raw_text` is non-empty and `source_url` contains
     the posting id.
2. Record per-site observations (job counts, times, blocks) in the manual script.
3. Retrospective: what the caps should be after real measurements, whether
   sequential enrichment is fast enough, and whether Phase 3 should enrich
   lazily at evaluation time instead of eagerly at fetch time.

---

## Test Strategy

- **Unit / fixture** (no network): pagination stop conditions, URL templating,
  detail extraction (JSON-LD and selectors), truncation, dedup idempotency, cap
  enforcement, cancellation.
- **`egui_kittest`**: the Test config preview and its error states.
- **App integration**: a fetch with a scripted `FetchSource` that returns pages
  and details, asserting stored counts and text; the preview writes nothing.
- **Live (ignored)**: the existing four ATS boards stay the fast default;
  pagination and details are asserted with the Waymo config and SmartRecruiters.

---

## Risks

| ID | Risk | Impact | Mitigation |
| :--- | :--- | :--- | :--- |
| P2.2-R1 | **A site blocks automated fetches** (AWS WAF, Cloudflare). Waymo already serves an AWS WAF challenge script. | Medium | Report the classified failure; do not attempt evasion. Tier 2 with a real browser is the only legitimate escalation. |
| P2.2-R2 | **Detail fetching is slow** (one request per job). | Medium | Caps (`max_jobs` 40, ceiling 200), sequential with a delay, cancellation between jobs, measurable live timings. |
| P2.2-R3 | **Selector drift breaks a configured portal silently.** A changed class name yields zero jobs. | Medium | Explicit "zero jobs extracted" outcome, the Test config preview, and the fixture test for the pinned shape. |
| P2.2-R4 | **Enrichment duplicates rows.** Changing `raw_text` changes the content hash. | Medium | Dedup matches `(source_url, external_id)` first; the Tier 1b external id is the job URL. Idempotency test required. |
| P2.2-R5 | **Unbounded storage** from very large postings. | Low | 200 KB truncation with a recorded warning. |
| P2.2-R6 | **Scope creep into an extraction engine** (LLM extraction, JS heuristics). | High | Selectors + JSON-LD only. Anything else is a new phase with its own rationale. |
| P2.2-R7 | **Pagination loop** against a site that ignores `?page`. | Medium | Stop on repeated job URLs, zero-job page, or cap. |

---

## Exit Criteria

1. A portal with a `pagination` config stores jobs from more than one page, and
   stops at `max_pages`.
2. A portal with a `detail` config stores the posting description as `raw_text`,
   truncated at 200 KB.
3. A failed detail fetch keeps the list row.
4. Enrichment can be cancelled between jobs.
5. Re-fetching the same portal is idempotent: one row per job, text updated.
6. SmartRecruiters rows have a per-posting `source_url` and non-empty `raw_text`.
7. The portal form's Test config preview shows counts, sample titles, and the
   classified error for a bad config, and writes nothing.
8. Waymo's search page is fetched end to end: all pages up to the cap, with
   posting-level text, verified by the live test.
9. `docs/manual-test-script.md` records the per-site results and the caps.
10. Retrospective recorded, including whether eager enrichment should become
    lazy in Phase 3.

---

## Open Questions

| ID | Question | Needed by |
| :--- | :--- | :--- |
| Q-8 | **robots.txt / terms posture.** Should the app fetch and honour `robots.txt` for Tier 1b/2, or document that the user is responsible for the sites they configure? | Phase 2.2 WS1, before the first multi-page fetch |
| Q-9 | **Eager versus lazy enrichment.** Eager fills the pipeline for Phase 3 but costs N requests per fetch. Lazy (fetch text when the user opens or evaluates a job) is cheaper and fresher but needs a per-job action and an error path. Recommendation: eager and bounded now, revisit in Phase 3 when the detail view gains actions. | Phase 3 planning |
| Q-10 | **Cap values.** `max_pages` 5 / `max_jobs` 40 are defaults from judgement, not measurement. Confirm with the live timings in WS5. | End of Phase 2.2 |
| Q-11 | **Should detail text be re-fetched on demand later** (postings change)? No mechanism exists today; decide in Phase 3 alongside the Evaluate flow. | Phase 3 |

---

## What Phase 2.2 Is Not

- Not AI evaluation. No prompts, no scoring, no cover letters.
- Not a crawler. Every fetch is a user action on a configured portal.
- Not an anti-bot effort. If a site blocks the app, the app says so.
- Not pagination for ATS boards. Their adapters already paginate.
- Not a new extraction engine. Selectors and JSON-LD, exactly as Phase 2
  designed them; if a site supports neither, the answer is a better config, not
  a heuristic.

---

## Milestone: usable scraping

After Workstreams 1–3 (and the live verification in WS5), the following is true
and is the point at which the user can start scraping for real:

- Configure a career page with a selector config in the app.
- Fetch every page up to the cap, in one action.
- Get posting-level descriptions for the first `max_jobs` postings.
- See the jobs in the Pipeline with real text.
- Re-fetch without creating duplicates.

Workstream 4 (Test config) and the remaining polish can land after that without
blocking Phase 3 preparation.
