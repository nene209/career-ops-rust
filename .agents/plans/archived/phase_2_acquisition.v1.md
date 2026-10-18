# Phase 2: Acquisition — Detailed Execution Plan

**Parent plan:** Draft Execution Plan: Career-Ops Rust Desktop Application (v0.4), §12
**Status:** Draft
**Last updated:** 2026-10-12
**Duration:** 3–5 weeks
**Depends on:** Phase 1 completion (see Entry Conditions)

---

## Purpose

Phase 2 makes the application **useful for the first time**. It implements the tiered job acquisition pipeline defined in §4 of the main plan: a lightweight HTTP path (Tier 1) that handles the majority of portals via ATS JSON APIs and HTML parsing, and a feature-gated headless browser path (Tier 2) for JavaScript-heavy or bot-protected sites.

The phase delivers three things:

1. **A working acquisition pipeline** that can take a configured portal, fetch its job listings, normalize them, and store them in `job_descriptions`.
2. **A portals configuration UI** that lets users add, edit, and monitor acquisition health per portal.
3. **A tier routing system** driven by `portals.render_mode` with runtime escalation, so the application self-tunes its own configuration over time.

Critically, Phase 2 is where the `BrowserBackend` trait from Phase 1 gets its first real implementation. Spike B's outcome determines whether Tier 2 is a genuine escalation path or a required component — and the phase plan accounts for both cases.

---

## Entry Conditions

Phase 2 begins after Phase 1 exit criteria are met. The following Phase 1 outputs are consumed:

| Phase 1 Output | How Phase 2 Uses It |
| :--- | :--- |
| `BrowserBackend` trait definition | Implemented by the Tier 2 backend |
| `PdfBackend` trait definition | Not used in Phase 2; implemented in Phase 4 |
| `portals` table and repository | Source of portal configuration; destination for health metadata |
| `job_descriptions` table and repository | Destination for acquired listings |
| GPUI shell with navigation | Host for the Portals Configuration screen |
| Unlock/keystore | Gates access to portal credentials (if any) and acquired data |
| Fixture corpus from Spike B | Seed for acquisition regression tests |
| Test/CI infrastructure | Extended with acquisition tests |

**Phase 2 does not depend on Phase 0 Spike A or C outcomes.** Spike A informs a post-v1 PDF backend; Spike C informs which CDP client implements `BrowserBackend`. Phase 2 can begin with the Spike C recommendation in hand, or with `rust-headless-chrome` as the initial choice if Spike C is inconclusive.

---

## Scope

| In scope | Out of scope |
| :--- | :--- |
| Tier 1a: ATS JSON API adapters (Greenhouse, Lever, Ashby, + 1–2 more) | AI evaluation of job postings |
| Tier 1b: `reqwest` + `scraper` HTML extraction for custom career pages | Cover letter generation |
| Tier 2: `rust-headless-chrome` (or chosen backend) behind `browser-backend` feature | PDF rendering of CV or cover letters |
| Portal configuration UI (add, edit, delete, render_mode selector, health display) | `rig` integration and LLM calls |
| Tier routing logic with escalation and `fetch_tier` recording | Prompt seeding and management |
| Acquisition tests using saved HTML/JSON fixtures | File-based data migration |
| `fetch_tier` and `last_verified_at` updates | Full CV editor |
| Manual job entry (paste a URL, fetch it) | Bulk import from external sources |
| | Browser stealth / anti-detection beyond defaults |

---

## Sequencing Rationale

The build order follows the dependency graph from the main plan:

```
W1: Tier 1a (ATS JSON adapters) + Tier 1b (HTML extraction)
    └── W2: Tier routing, normalization, storage
        └── W3: Portals Configuration UI
            └── W4: Tier 2 (browser backend) behind feature flag
                └── W5: Integration, manual entry, polish
```

Tier 1a comes first because it is the highest-leverage path: a single ATS JSON adapter covers every company on that platform. Tier 1b is second because it handles the long tail. Tier 2 is deliberately last, because Spike B's outcome may reduce its scope significantly — if Tier 1 covers ≥ 70% of target portals, Tier 2 becomes a true escalation path and receives correspondingly less investment.

---

## Workstream 1 — Tier 1a: ATS JSON API Adapters

**Priority:** First. Highest leverage per unit of effort.
**Estimated duration:** 1 week

### Deliverables

1. A `core::acquisition::ats` module with a trait for ATS adapters:

```rust
#[async_trait]
trait AtsAdapter: Send + Sync {
    fn name(&self) -> &'static str;
    fn detect(&self, url: &Url) -> Option<String>; // returns board slug
    async fn fetch_jobs(
        &self,
        client: &reqwest::Client,
        slug: &str,
    ) -> Result<Vec<NormalizedJob>>;
}

struct NormalizedJob {
    external_id: String,
    title: String,
    company: String,
    location: Option<String>,
    department: Option<String>,
    employment_type: Option<String>,
    source_url: Url,
    raw_text: String,
    posted_at: Option<DateTime<Utc>>,
}
```

2. Concrete adapters for at minimum:
   - **Greenhouse** — `https://boards-api.greenhouse.io/v1/boards/{slug}/jobs?content=true`[reference:0]
   - **Lever** — `https://api.lever.co/v0/postings/{slug}?mode=json`[reference:1]
   - **Ashby** — `https://api.ashbyhq.com/posting-api/job-board/{slug}`[reference:2]
   - **SmartRecruiters** — `https://api.smartrecruiters.com/v1/companies/{slug}/postings?limit=100`[reference:3]
3. Auto-detection: given a portal URL, determine which ATS it belongs to and extract the slug. Known host patterns: `boards.greenhouse.io/{slug}`, `jobs.lever.co/{slug}`, `jobs.ashbyhq.com/{slug}`, `jobs.smartrecruiters.com/{slug}`.

### Critical Implementation Notes

1. **Prefer JSON APIs over HTML scraping where they exist.** A single API call returns the entire board with structured fields, including full descriptions when `?content=true` is used. This is dramatically more robust than DOM parsing.
2. **Handle payload size.** Greenhouse with `content=true` returns full descriptions, which can be large for big boards. Stream or paginate if needed. A 168-role board with descriptions is roughly 100KB without content and significantly more with it.
3. **Normalize into a common shape early.** Every adapter returns `NormalizedJob`. Downstream code never sees ATS-specific fields.
4. **Record the adapter used.** Store the adapter name in `job_descriptions.fetch_tier` (e.g. `tier1a:greenhouse`) so failures can be attributed to a specific adapter.
5. **Respect rate limits.** ATS APIs are generally permissive, but add a configurable inter-request delay (default 500ms) and exponential backoff on 429/503.

### Acceptance Criteria

| Criterion | Verification |
| :--- | :--- |
| Each adapter fetches and parses a real board correctly | Integration test against live endpoint (environment-gated) |
| Auto-detection identifies the correct adapter from a portal URL | Unit tests with URL fixtures |
| `NormalizedJob` contains all required fields for each adapter | Unit tests per adapter |
| Adapter handles empty boards gracefully | Unit test with empty response fixture |
| Adapter handles malformed JSON without panicking | Unit test with truncated JSON |
| Rate limiting respects configured delay | Unit test with mock client |

---

## Workstream 2 — Tier 1b: HTML Extraction for Custom Career Pages

**Priority:** After Tier 1a adapters are proven.
**Estimated duration:** 1 week

### Deliverables

1. `core::acquisition::html` module using `reqwest` + `scraper`
2. Portal-level scraper configuration stored in `portals.scraper_config` (JSON)
3. A default extraction strategy for common patterns (JSON-LD `JobPosting` schema, `<article>` containers, link heuristics)
4. Fallback: if no structured data found, extract links matching job-posting URL patterns

### Critical Implementation Notes

1. **`scraper` wraps `html5ever`**, the same parser used in Servo, so it follows the HTML spec rather than treating markup as text to regex over. This matters on real pages with malformed markup.[reference:4]
2. **Neither `reqwest` nor `scraper` executes JavaScript.** Client-rendered pages return markup with zero job records. This is the Tier 1 failure boundary, and it must be detected explicitly, not silently treated as "no jobs found."[reference:5]
3. **Prefer JSON-LD when present.** Many career pages embed `application/ld+json` blocks with `JobPosting` schema. This is more reliable than CSS selectors and survives layout changes.
4. **CSS selectors are the fallback, not the primary path.** Store selectors in `portals.scraper_config` so users can tune them without a rebuild. Provide sensible defaults for common patterns.
5. **Detect JS-required pages.** If the initial HTML contains no job links and the page has a known SPA root element (`#root`, `#app`, `__NEXT_DATA__` absent), classify the failure as `JS_REQUIRED` rather than `NO_JOBS`. This distinction drives escalation.

### Acceptance Criteria

| Criterion | Verification |
| :--- | :--- |
| JSON-LD extraction works on pages with `JobPosting` schema | Unit test with saved fixture |
| CSS selector extraction works on pages without structured data | Unit test with saved fixture |
| JS-required pages are classified distinctly from empty pages | Unit test with SPA fixture |
| `scraper_config` round-trips through the database | Integration test |
| Extraction handles malformed HTML without panic | Unit test with truncated markup |
| HTML fixture corpus from Spike B is fully exercised | Test coverage check |

---

## Workstream 3 — Tier Routing, Normalization, and Storage

**Priority:** After Tier 1a and 1b are individually functional.
**Estimated duration:** 1 week

### Deliverables

1. `core::acquisition::router` implementing the escalation logic from §4 of the main plan:

```
1. Read render_mode from portals row
2. If static → run Tier 1 (1a, then 1b)
3. On empty/failed extraction → escalate to Tier 2 if compiled in
4. If browser → go directly to Tier 2
5. Record successful tier in job_descriptions.fetch_tier
6. Update portals.last_verified_at
```

2. Deduplication: before inserting, check `(source_url, external_id)` against existing rows. Update if changed, skip if identical, insert if new.
3. `fetch_tier` recording per job listing
4. `last_verified_at` update per portal on successful fetch
5. Optional: `render_mode` suggestion logic (if Tier 1 keeps failing, suggest `browser`; if Tier 2 succeeds where Tier 1 would have, keep as-is)

### Critical Implementation Notes

1. **Escalation is explicit, not silent.** When Tier 1 fails and Tier 2 is available, the UI should indicate that escalation occurred. Users should know which tier succeeded — it affects troubleshooting and cost (browser fetches are slower).
2. **Deduplication is content-aware.** A job reposted with a new `external_id` but identical content should not create a duplicate. Compare a content hash as a secondary check.
3. **`fetch_tier` is a diagnostic, not a routing key.** It records what happened. The routing decision is made from `render_mode` plus runtime state.
4. **Escalation should be rate-limited.** If a portal consistently fails Tier 1, do not escalate on every fetch — respect a backoff window before retrying Tier 2.
5. **Failed fetches are recorded, not discarded.** Store the failure cause (classified: `js_required`, `blocked`, `timeout`, `parse_error`) so the Portals UI can display actionable health information.

### Acceptance Criteria

| Criterion | Verification |
| :--- | :--- |
| `static` portal routes to Tier 1 | Unit test with mock backends |
| `browser` portal routes to Tier 2 directly | Unit test with mock backends |
| Tier 1 failure escalates to Tier 2 when compiled in | Unit test with mock backends |
| Tier 1 failure surfaces an error when Tier 2 not compiled in | Unit test |
| Successful fetch records `fetch_tier` correctly | Integration test |
| Duplicate listings are deduplicated by `(source_url, external_id)` | Integration test |
| Content-identical listings with different IDs are deduplicated | Integration test |
| `last_verified_at` updates on success, not on failure | Integration test |
| Failure causes are classified and stored | Integration test |

---

## Workstream 4 — Tier 2: Browser Backend Implementation

**Priority:** After Tier 1 routing works. Scope depends on Spike B outcome.
**Estimated duration:** 1 week (if Spike B showed Tier 2 is escalation), 1.5–2 weeks (if Tier 2 is required)

### Deliverables

1. `BrowserBackend` implementation for `rust-headless-chrome` (or `chromiumoxide`, per Spike C)
2. Behind the `browser-backend` Cargo feature flag
3. `fetch_rendered` method: navigate, wait for page settle, return post-JS DOM
4. `print_to_pdf` method: render HTML to PDF via CDP `Page.printToPDF`
5. Browser lifecycle management: one browser instance, reused across fetches within a session
6. Profile directory isolation: browser profile in a user-inspectable, user-deletable location

### Critical Implementation Notes

1. **Blocking API handling.** `rust-headless-chrome` is synchronous. All browser calls must be wrapped in `spawn_blocking` or a dedicated worker thread to avoid stalling the GPUI event loop. `chromiumoxide` is async-first and integrates more naturally, but requires verifying `PrintToPdfParams` coverage.
2. **`print_to_pdf` is included in Phase 2 even though PDF rendering is Phase 4.** The reason is that the same browser instance serves both acquisition and rendering. Implementing both methods in the `BrowserBackend` impl now means Phase 4 only needs the `PdfBackend` wrapper, not a new browser dependency.
3. **Browser binary size.** Bundling Chrome adds 100–150 MB. This is feature-gated, so the default build is unaffected. Users who enable the feature accept the size.
4. **GPUI integration.** Use a bridge crate (e.g. `gpui-tokio-bridge` or Zed's `gpui_tokio`) to spawn browser tasks from the GPUI context without blocking the UI.[reference:6]
5. **Spike B outcome determines scope.** If Tier 1 covered ≥ 70% of target portals, Tier 2 handles the long tail: fewer features, less polish, lower priority. If Tier 1 covered < 70%, Tier 2 moves into the critical path and warrants full implementation, including more robust navigation and waiting strategies.

### Acceptance Criteria

| Criterion | Verification |
| :--- | :--- |
| `fetch_rendered` returns post-JS DOM for a JS-heavy fixture | Integration test (environment-gated) |
| `print_to_pdf` returns valid PDF bytes | Integration test |
| Browser calls do not stall the GPUI event loop | UI responsiveness test |
| Browser profile directory is in a user-inspectable location | Manual inspection |
| Feature-disabled build has no browser dependency in `Cargo.toml` | Build assertion |
| Feature-enabled build compiles and passes integration tests | CI job with feature enabled |
| Browser instance is reused across multiple fetches in a session | Integration test |

---

## Workstream 5 — Portals Configuration UI

**Priority:** After routing logic works. Can overlap with Tier 2 if Tier 2 is not critical-path.
**Estimated duration:** 1–1.5 weeks

### Deliverables

1. **Portals list screen** showing:
   - Company name, URL, `render_mode`
   - `last_verified_at` (relative time)
   - Health indicator (last fetch status: success, partial, js_required, blocked, timeout)
   - Job count (total, new since last fetch)
2. **Add/edit portal form**:
   - URL input with ATS auto-detection (shows detected adapter or "custom")
   - `render_mode` selector (`static` / `browser`)
   - `scraper_config` JSON editor (advanced, collapsible)
3. **Delete confirmation** (with option to keep or remove acquired job listings)
4. **Manual fetch trigger** per portal ("Fetch now") and per all portals ("Fetch all")
5. **Fetch progress display**: per-portal status during a batch fetch
6. **Health drill-down**: for failed portals, show the classified failure cause and a suggested action

### Critical Design Rules

1. **Backend availability is visible.** If Tier 2 is not compiled in, the `browser` option is greyed out with a tooltip explaining why. The user is not silently blocked.
2. **Failure causes are actionable.** "JS required" should suggest enabling the browser feature. "Blocked" should suggest checking the URL. "Timeout" should suggest retrying. Never show a raw error string without context.
3. **`render_mode` auto-suggestion is opt-in.** The app can recommend switching a portal from `static` to `browser` after repeated Tier 1 failures, but it must ask, not silently change.
4. **Manual fetch is idempotent.** Fetching the same portal twice does not create duplicates.

### Acceptance Criteria

| Criterion | Verification |
| :--- | :--- |
| Portal CRUD round-trips through the database | UI integration test |
| ATS auto-detection populates the adapter correctly | UI test with known URLs |
| `browser` option is greyed out when feature disabled | UI test with feature disabled build |
| Fetch progress displays per-portal status | UI smoke test |
| Failure drill-down shows classified cause and suggestion | UI test |
| Manual fetch does not create duplicates | Integration test |

---

## Workstream 6 — Manual Job Entry

**Priority:** Can be developed in parallel with Tier 2 or Portals UI.
**Estimated duration:** 2–3 days

### Deliverables

1. **"Add job by URL"** action in the Pipeline screen
2. Fetch the URL via the tiered pipeline, normalize, and store
3. **"Paste job description"** fallback for URLs that cannot be fetched
4. Manual entry creates a `job_descriptions` row with `fetch_tier = 'manual'`

### Rationale

Not every job comes from a configured portal. Users find postings via referrals, newsletters, or direct links. Manual entry ensures the pipeline does not become a bottleneck for these cases.

### Acceptance Criteria

| Criterion | Verification |
| :--- | :--- |
| URL fetch routes through the tiered pipeline | Integration test |
| Paste fallback stores raw text correctly | Unit test |
| Manual entries appear in the pipeline | Integration test |
| Manual entries are deduplicated against portal-fetched entries | Integration test |

---

## Test Strategy

### Fixture-Based Testing

All Tier 1 acquisition tests run against **saved fixtures**, not live network calls. The Spike B fixture corpus is the seed. Every real-world portal failure becomes a regression fixture.

| Fixture type | Source | Used by |
| :--- | :--- | :--- |
| Greenhouse JSON response | Spike B sample | Tier 1a adapter test |
| Lever JSON response | Spike B sample | Tier 1a adapter test |
| Ashby JSON response | Spike B sample | Tier 1a adapter test |
| Custom career page HTML | Spike B sample | Tier 1b extraction test |
| SPA shell HTML (no jobs) | Spike B sample | JS-required classification test |
| Malformed JSON | Hand-crafted | Error handling test |
| Truncated HTML | Hand-crafted | Error handling test |

### Mock-Based Testing

`BrowserBackend` tests use the mock implementation from Phase 1. A small number of environment-gated tests exercise the real backend against a local fixture HTML file.

### Integration Tests

End-to-end flows:

1. Add a Greenhouse portal → fetch → verify `job_descriptions` rows created with `fetch_tier = 'tier1a:greenhouse'`.
2. Add a custom career page → fetch → verify rows with `fetch_tier = 'tier1b:html'`.
3. Add an SPA portal → fetch → verify failure classified as `js_required` → enable browser feature → fetch → verify escalation and `fetch_tier = 'tier2'`.
4. Fetch the same portal twice → verify no duplicates.
5. Manual URL entry → verify it appears in the pipeline alongside portal-fetched entries.

### CI Jobs

| Job | Purpose |
| :--- | :--- |
| `acquisition-fixtures` | Tier 1a and 1b tests against saved fixtures (always runs) |
| `acquisition-live` | Tier 1a tests against real ATS endpoints (environment-gated, runs nightly) |
| `browser-backend` | Tier 2 integration tests with `browser-backend` feature enabled (runs on PR when feature files change) |
| `ui-portals` | Portals UI tests with GPUI test harness |

---

## Phase 2 Risks

| ID | Risk | Impact | Mitigation |
| :--- | :--- | :--- | :--- |
| P2-R1 | **Spike B shows Tier 1 covers < 70% of target portals.** Tier 2 becomes required, not escalation. | High | Phase 2 scope adjusts: Tier 2 moves to critical path, Portals UI prioritizes `browser` mode, and the plan's default-build framing is revisited. |
| P2-R2 | **ATS APIs change or rate-limit unexpectedly.** Adapters break without warning. | Medium | Fixture-based tests catch regressions on known responses. Live tests (nightly) catch API changes early. Adapter errors are classified and surfaced in the UI. |
| P2-R3 | **`rust-headless-chrome` blocking API causes UI stalls despite `spawn_blocking`.** | Medium | Use `gpui-tokio-bridge` for task spawning. Add UI responsiveness test. If stalls persist, evaluate `chromiumoxide` async alternative. |
| P2-R4 | **Browser binary size makes feature-enabled builds unattractive.** | Medium | Feature is opt-in. Default build ships without it. Bundle size is documented in the UI before the user enables it. |
| P2-R5 | **Portal configuration UI scope creep.** | Medium | Ship the minimum: list, add, edit, delete, fetch, health. Advanced features (scraper selector editor, batch operations) are Phase 5 polish. |
| P2-R6 | **HTML extraction on custom pages is brittle.** Every career page is different. | Medium | JSON-LD first, CSS selectors second, link heuristics third. Store selectors in `portals.scraper_config` so users can tune without a rebuild. Accept that some pages require manual entry. |
| P2-R7 | **JS-required detection produces false positives.** A page with no jobs is classified as JS-required. | Low | Distinguish "no job links found" from "SPA root element present." False positives cause unnecessary escalation, not data loss. |

---

## Weekly Budget

| Week | Focus | Deliverable |
| :--- | :--- | :--- |
| **1** | W1 Tier 1a ATS adapters (Greenhouse, Lever, Ashby, SmartRecruiters) | All adapters fetch and normalize real boards; fixture tests pass |
| **2** | W2 Tier 1b HTML extraction, JSON-LD, JS-required detection | Custom career pages extract correctly; SPA pages classified distinctly |
| **3** | W3 Tier routing, deduplication, `fetch_tier` recording, failure classification | End-to-end: portal → fetch → store → dedupe |
| **4** | W4 Portals Configuration UI, manual job entry | Portal CRUD, fetch progress, health display; manual URL entry works |
| **5** | W5 Tier 2 browser backend behind feature flag; integration | Browser backend fetches JS-heavy pages; escalation works; UI responsive |

**Total:** 3–5 weeks. The lower end (3 weeks) assumes Spike B showed Tier 1 covers ≥ 80% of portals and Tier 2 is a thin escalation path. The upper end (5 weeks) assumes Tier 2 is required and receives full implementation.

---

## Exit Criteria

Phase 2 is complete when **all** of the following are true:

1. At least four ATS adapters (Greenhouse, Lever, Ashby, SmartRecruiters) fetch and normalize real job listings correctly.
2. HTML extraction handles at least JSON-LD and CSS-selector-based career pages.
3. JS-required pages are classified distinctly from empty pages.
4. Tier routing follows `render_mode` with explicit, rate-limited escalation.
5. `fetch_tier` is recorded per listing; `last_verified_at` is updated per portal on success.
6. Duplicate listings are deduplicated by `(source_url, external_id)` and content hash.
7. Portals UI supports CRUD, fetch progress, and health drill-down.
8. Manual job entry (URL and paste) works and deduplicates against portal-fetched entries.
9. Tier 2 browser backend works behind `browser-backend` feature, with no UI stalls.
10. Fixture corpus from Spike B is fully exercised in tests.
11. CI is green on Linux, macOS, and Windows, including acquisition fixture and browser backend jobs.
12. A short Phase 2 retrospective memo exists, recording Spike B's actual outcome vs. the plan's assumption and Tier 2's realized scope.

---

## What Phase 2 Is Not

Phase 2 acquires job postings. It does not evaluate them, score them, generate cover letters, or produce PDFs. Specifically:

- There is no AI evaluation, no `rig` integration, no match scoring.
- There is no cover letter generation.
- There is no PDF rendering — `print_to_pdf` is implemented on the `BrowserBackend` because it shares the browser instance, but no `PdfBackend` wrapper exists yet.
- There is no prompt seeding or management.
- There is no file-based data migration.

The temptation to "just add a quick match score" before Phase 2 is complete is the most common way acquisition phases overrun. Resist it. Phase 3 is where the application becomes intelligent.

---

The single most important thing to take from this plan: **run Tier 1a adapters first and measure their coverage against your real target portals before investing in Tier 2.** The ATS JSON APIs are the highest-leverage path in the entire acquisition pipeline — one adapter covers every company on that platform, with structured data and no browser required. If they cover the majority of your targets, Tier 2 becomes a genuinely optional escalation, and the phase finishes in three weeks instead of five.