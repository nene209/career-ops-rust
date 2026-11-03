# Phase 2.2: Acquisition Completeness — Detailed Execution Plan

**Parent plan:** Draft Execution Plan: Career-Ops Rust Desktop Application (v0.8), §12
**Status:** Complete — all ten exit criteria met and verified live, including the final Waymo run (90 jobs from 3 pages with 2 of 2 descriptions). Retrospective: `.agents/plans/phase_2.2_retrospective.md`
**Last updated:** 2026-11-03
**Duration:** 4–6 days (two workstreams deliver usable scraping; the rest is quality)
**Depends on:** Phase 2 (acquisition) and Phase 2.1 (UI shell)

---

## Change Log

| Version | Change |
| :--- | :--- |
| v0.1 | Initial plan. Written after the Phase 2.1 verification run against `https://careers.withwaymo.com/jobs/search`, which proved that a server-rendered page with a `scraper_config` works on Tier 1b (30 jobs, no sidecar) but exposed three gaps that would starve Phase 3: no pagination, no job descriptions for page-based portals, and no description at all from the SmartRecruiters adapter. |
| v0.2 | Workstream 1 implemented: `PaginationConfig` on `SelectorConfig` (`next`, `url_template`, `max_pages`), the page loop in `html::fetch_static_jobs`, and thirteen unit tests covering every stop condition. Verified live against Waymo: 90 jobs from 3 pages via `rel=next` (Tier 1b). Also learned that the site's AWS WAF answers sustained fetching with `202`/empty body, which the manual script now documents. |
| v0.3 | Workstream 1b implemented before WS2: per-host request pacing (400 ms minimum + up to 400 ms jitter) inside `core::acquisition::http`, so pagination, adapter paging, batch runs, and the detail fetches WS2 adds are all spaced without per-caller code. Env overrides and README documentation added. |
| v0.4 | Workstream 2 implemented: `detail` config on `SelectorConfig`, JSON-LD-then-selector extraction, bounded and cancellable enrichment between list extraction and storage, 200 KB truncation, and `details_attempted`/`details_enriched` on `FetchOutcome` plus a one-line result message in the app. Live verification hit Waymo's WAF on the detail route (`202`/empty body), recorded as an operational limit rather than a code failure. |
| v0.5 | Workstream 3 implemented: `smartrecruiters::normalize` emits a stable per-posting URL (`https://jobs.smartrecruiters.com/{slug}/{id}`) and department/employment type from the list item; the adapter exposes `detail_url`/`detail_from_body` so the router's existing bounded enrichment pass reads `jobAd.sections` from the posting API and `compose_description` skips empty sections. Live against Ubisoft2: 206 unique postings, 2 of 2 descriptions, longest 6,031 bytes. |
| v0.6 | Workstream 4 implemented: **Test config** in the add/edit modal runs the draft URL and config through the router's real routing and extraction with no database handle, reporting page-1 counts, up to three titles with resolved URLs, and the first description length. Bounded to one list page and three detail requests regardless of the config; invalid JSON is classified as `InvalidRequest` instead of being silently ignored. |
| v0.7 | Workstream 5 implemented: the live test now asserts SmartRecruiters per-posting URLs and descriptions (attaching a detail-only default so the default run covers WS3), a dedicated paginated/detailed variant covers Waymo and skips on a WAF challenge, and the manual script records per-site counts and timings. Retrospective written; caps confirmed by measurement; two findings (silent partial pagination, Tier 1 cause masked by Tier 2) recorded for follow-up. |
| v0.8 | WS5 follow-up: both findings fixed. `fetch_pages` returns a `PageReport` (jobs, pages fetched, stop reason) that reaches `FetchOutcome` and the fetch message ("Fetched 60 jobs from 2 pages (page 3 could not be read)."). A Tier 2 failure no longer replaces the Tier 1 cause: the original cause is kept and the fallback's detail is appended, so an absent sidecar cannot mask a blocked page. A later live run completed criterion 8: 90 jobs from 3 pages, 2 of 2 descriptions, longest 3,611 bytes, 7.0 s. |

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

## Workstream 1b — Request pacing

**Priority:** Immediately after WS1, before detail enrichment.
**Estimated duration:** 0.5 day.

Pagination made the cost of repeated requests visible, and the live verification
hit Waymo's AWS WAF (`202`, empty body) after a handful of runs. Workstream 2
multiplies requests per host by the number of postings, so the pacing has to
exist before it lands, not after.

### Behaviour

Every request to the same host is spaced by a minimum interval plus random
jitter. The spacing lives inside `core::acquisition::http`, so it covers
pagination, adapter pagination (SmartRecruiters), manual URL fetches, batch
runs, and the detail fetches WS2 adds — no caller has to remember it.

- Default: **400 ms minimum + up to 400 ms of jitter**, per host.
- `CAREER_OPS_HTTP_MIN_INTERVAL_MS` / `CAREER_OPS_HTTP_JITTER_MS` override them;
  `0` disables pacing. A non-numeric value is ignored with a warning rather than
  silently becoming zero.
- Slots are booked under a lock and slept outside it, so queued callers get
  distinct times instead of firing together.
- The first request to a host in a process is never delayed; different hosts are
  paced independently, so a batch over distinct boards pays nothing.

### What it deliberately is not

- Not a retry-backoff change: failed attempts still back off as before, and the
  pacing applies to the retry as well because a retry is still a request.
- Not applied to the browser backend (Tier 2): one navigation per fetch, not a
  request stream.
- Not a rate-limit solver. A site with a per-minute quota can still refuse the
  application; pacing prevents bursts, it does not negotiate with a WAF.

### Acceptance criteria

| Criterion | Verification |
| :--- | :--- |
| The first request to a host is not delayed | Unit test |
| A second request to the same host waits the interval | Unit test |
| Different hosts are paced independently | Unit test |
| Queued reservations get distinct, increasing slots | Unit test |
| Jitter stays within its bound | Unit test |
| Zero spacing disables the wait | Unit test |
| A paginated live fetch still succeeds, with the expected extra time | Live run |

**Implementation note (2026-10-25).** Landed as `RequestSpacing` + `HostSpacer`
in `crates/core/src/acquisition/http.rs`, applied in `send_with_retry_policy`.
`send_with_retry`/`send_with_retry_policy` gained a `url` argument so the host is
explicit rather than guessed from the request builder; the only caller was
`get_with_retry`. Seven unit tests cover the scheduling. Live check: the Waymo
3-page fetch still returns 90 jobs and took 8.36 s instead of 6.67 s, which is
the two ~600 ms gaps plus jitter. README documents the env overrides.

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
3. Sequential enrichment. Raw spacing is already applied by
   `http::get_with_retry` (Workstream 1b: 400 ms + jitter per host), so this
   workstream adds no second delay. Sequential is deliberate: a desktop app
   fetching a user's chosen board should look like one careful client, not a
   burst. Revisit only if a real board makes it too slow.
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

**Implementation note (2026-10-25).** Landed as `crates/core/src/acquisition/html/detail.rs`
(`DetailConfig`, `extract_detail`) plus enrichment in
`router::fetch_portal_inner`, between the tier fetch and `store_jobs`. Three
deviations from the plan text, all for testability and honesty:

1. Fetching goes through a new `FetchSource::fetch_page`, not a free
   `fetch_detail(client, ...)`. The existing seam already exists for exactly
   this reason, it lets every enrichment test run without a network, and the
   HTTP pacing from Workstream 1b applies automatically. `extract_detail` is
   the pure part: JSON-LD first (preferring a title match, falling back to the
   only posting), then `detail.fields.description`.
2. `DetailConfig.fields` is optional, so `{"detail": {}}` is valid for sites
   whose detail pages emit JSON-LD. Requiring a selector merely to enable
   JSON-LD enrichment would have been a config tax.
3. Cancellation is a new `fetch_portal_cancellable`, used by the batch path.
   A successful enrichment is also reported: `FetchOutcome` gained
   `details_attempted`/`details_enriched`, and the app shows a one-line result
   ("Fetched 60 jobs; 2 of 2 descriptions.").

Seven router tests and nine detail-extraction tests cover the acceptance table.
Live verification on the Waymo detail route was blocked by the site's WAF
(`202`, empty body) after a handful of requests, which is the P2.2-R1 risk in
practice: the list route still returns 200 while detail routes are challenged.
A browser-saved copy of the real posting page (2026-10-25) was trimmed to its
JSON-LD block and committed as
`crates/core/tests/fixtures/waymo_job_posting.html`;
`the_real_waymo_detail_page_yields_its_description` asserts that the extractor
turns it into the full ~4 KB description, so the markup is verified even while
the live route is challenged. The manual script records the symptom and the
wait.

The first live attempt also found a shape bug, now fixed: `SelectorConfig`
required `list` and `fields`, so a detail-only config (`{"detail": {...}}`) was
rejected before enrichment could run. That is the natural way to add
descriptions to an ATS board, which already has full list data, so both fields
are now optional and a config may carry only `detail`. Re-verified live on
Lever: `Fetched 85 jobs; 1 of 1 description.`, longest description 4,888 bytes
(the job page's `JobPosting` JSON-LD).

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

**Implementation note (2026-11-02).** The detail pass lives in the router, not in
`fetch_jobs`. The adapter answers two questions — where the detail lives
(`detail_url`) and how to read it (`detail_from_body`) — and the router keeps the
`detail.max_jobs` bound, the per-host pacing, and the cancellation check it
already applies to every other portal. Without a `detail` block a SmartRecruiters
portal stores titles and URLs but no text, exactly like every other ATS board.

Two consequences worth remembering. The URL is built from the API `id`, not from
the API's own `postingUrl`: the latter appends a slugified title, so it changes
when a posting is retitled and differs between language copies. And
`additionalInformation` is routinely empty on Ubisoft's board, so
`compose_description` skips empty sections instead of emitting a bare heading.

Department and employment type are now populated on `NormalizedJob` (list first,
detail as the fallback), but `job_descriptions` has no columns for them yet, so
they are not persisted — a storage concern for a later phase, not a parsing one.

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

**Implementation note (2026-11-02).** `router::preview` takes no `SqlitePool`,
so "never writes" is a property of the signature rather than a promise held by
tests. It shares `decide_initial_route`, `fetch_static_jobs` (through
`FetchSource`), `extract_from_html`, and `job_detail` with the real fetch; only
the bounds differ. The pagination block is stripped for the list call, and the
detail cap is 3 independently of `detail.max_jobs`.

Two things are deliberately reported rather than hidden. A Tier 1a preview
fetches the whole board, because the adapter owns pagination, so the panel says
"the adapter paginates internally" instead of inventing a page count. And a
`pagination` block with neither `next` nor `url_template` reports one page,
matching `fetch_pages`, rather than its unused default of five.

A preview result is tied to the inputs it was run with: editing the URL, render
mode, or config clears the panel on the next frame. Without that, a saved
config could be judged against a report for a different one.

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

**Outcome (2026-11-03).** Four ATS boards stored 467 unique jobs in about 6.4 s,
including two SmartRecruiters detail requests and a re-fetch with no duplicates.
Waymo's final run stored 90 jobs from three pages in 7.0 s with 2 of 2
descriptions (longest 3,611 bytes), so criterion 8 is fully verified; earlier
attempts were WAF-challenged and the test skipped them. Caps were confirmed
rather than changed (5 pages, 40 details). The two findings from the pass — a
later page failing left the portal Healthy with no visible partial state, and a
Tier 1 cause was replaced by the Tier 2 error when both tiers failed — were fixed
in the follow-up: a selector fetch now reports its page count and stop reason,
and a failed fallback preserves the original cause. Full record:
`.agents/plans/phase_2.2_retrospective.md`.

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

**Resolutions (2026-11-03).**

- **Q-8** — documented posture, no enforcement: the app fetches on a user action,
  reports blocks, and never evades; the user is responsible for the sites they
  configure.
- **Q-9** — stay eager and bounded. Measured cost is about half a minute for the
  default 40 details; lazy would make evaluation depend on a live request and
  would leave the list unable to show what it has. Revisit when the detail view
  gains an explicit refresh action.
- **Q-10** — keep 5 pages / 40 details. One page costs 0.4–0.8 s of pacing plus
  one response; 40 details cost roughly half a minute. The ceiling stays 200.
- **Q-11** — still open. Recommended answer: a per-job "Refresh description"
  action in Phase 3, not automatic refetching.

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
