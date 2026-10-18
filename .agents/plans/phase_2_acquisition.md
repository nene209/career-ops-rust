# Phase 2: Acquisition — Detailed Execution Plan

**Parent plan:** Draft Execution Plan: Career-Ops Rust Desktop Application (v0.4), §12
**Status:** Draft — revised for Obscura backend
**Last updated:** 2026-10-17
**Duration:** 3–4 weeks
**Depends on:** Phase 1 completion

---

## Change Log

| Version | Change |
| :--- | :--- |
| v0.1 | Initial Phase 2 plan; browser backend feature-gated, `rust-headless-chrome` as default. |
| v0.2 | Browser backend is now a **required** dependency (not feature-gated). Obscura is the chosen backend. `rust-headless-chrome` retained as documented fallback. Tier 1 remains the default path where it works, but is no longer expected to cover the majority of target portals. |

---

## Purpose

Phase 2 makes the application **useful for the first time**. It implements the tiered job acquisition pipeline: Tier 1 (HTTP/JSON) handles portals that return complete content without JavaScript, and Tier 2 (headless browser via Obscura) handles the portals that require JS execution or are otherwise unreachable via plain HTTP.

**A material change from the v0.1 plan:** Spike B was not run, but its central question was answered directly by the project owner — **`reqwest` + `scraper` are not sufficient for the target portals.** JavaScript execution is required. This means:

1. The browser backend is **not** a feature-gated escalation. It is a **required dependency** for basic acquisition against the intended targets.
2. The "lean build" story from the main plan §9 does not apply to this deployment. The application ships with a browser backend.
3. Tier 1 is still valuable — it is faster, cheaper, and more robust for the portals it covers — but the expected coverage is lower than the v0.1 plan assumed.

---

## Entry Conditions

Phase 2 begins after Phase 1 exit criteria are met. Consumed outputs:

| Phase 1 Output | How Phase 2 Uses It |
| :--- | :--- |
| `BrowserBackend` trait definition | Implemented by the Obscura backend |
| `PdfBackend` trait definition | Not used in Phase 2 |
| `portals` table and repository | Source of portal configuration |
| `job_descriptions` table and repository | Destination for acquired listings |
| GPUI shell with navigation | Host for the Portals Configuration screen |
| Keystore and unlock flow | Gates access to portal configuration and acquired data |
| Test/CI infrastructure | Extended with acquisition and browser backend tests |

**No Phase 0 output is consumed** because Phase 0 was skipped. The decisions that would have come from Phase 0 are recorded in the Decisions Addendum and are treated as settled inputs to this phase.

---

## Scope

| In scope | Out of scope |
| :--- | :--- |
| Tier 1: ATS JSON API adapters (Greenhouse, Lever, Ashby, SmartRecruiters) | AI evaluation of job postings |
| Tier 1: `reqwest` + `scraper` HTML extraction for static career pages | Cover letter generation |
| **Tier 2: Obscura-based `BrowserBackend`** (required, not feature-gated) | PDF rendering of CV or cover letters |
| Portal configuration UI (add, edit, delete, render_mode selector, health) | `rig` integration and LLM calls |
| Tier routing with escalation and `fetch_tier` recording | Prompt seeding and management |
| Acquisition tests using saved HTML/JSON fixtures | File-based data migration |
| `fetch_tier` and `last_verified_at` updates | Full CV editor |
| Manual job entry (paste a URL, fetch it) | Bulk import from external sources |
| Browser process lifecycle management (sidecar, isolation) | Browser stealth/anti-detection beyond Obscura defaults |
| Fallback path to `rust-headless-chrome` if Obscura proves unsuitable | |

---

## Sequencing Rationale

```
W1: Obscura integration spike (BrowserBackend trait implementation)
    └── W2: Tier 1a (ATS JSON) + Tier 1b (HTML extraction)
        └── W3: Tier routing, normalization, storage
            └── W4: Portals Configuration UI, manual entry, integration
```

**Obscura integration comes first.** In the v0.1 plan, Tier 1 came first because it was the highest-leverage path. With the browser now required, the browser backend is the load-bearing dependency. If Obscura cannot be integrated, or its rendering fidelity for scraping is insufficient, the entire acquisition strategy needs a fallback — and that is better to discover in Week 1 than Week 4.

Tier 1 work follows because it is still valuable: ATS JSON APIs are faster and more reliable than browser-based scraping for the portals they cover, and they reduce the load on the browser process.

---

## Workstream 1 — Obscura `BrowserBackend` Integration

**Priority:** First. Load-bearing dependency.
**Estimated duration:** 1 week (includes integration spike)

### Deliverables

1. `ObscuraBrowserBackend` implementing the `BrowserBackend` trait from Phase 1:

```rust
pub struct ObscuraBrowserBackend {
    config: ObscuraConfig,
    // process handle, session state, etc.
}

#[async_trait]
impl BrowserBackend for ObscuraBrowserBackend {
    async fn fetch_rendered(
        &self,
        url: &Url,
        opts: RenderOpts,
    ) -> Result<RenderedPage>;

    async fn print_to_pdf(
        &self,
        html: &str,
        opts: PdfOptions,
    ) -> Result<Vec<u8>>;
}
```

2. **Process lifecycle management:**
   - Obscura runs as a **sidecar process** (preferred) rather than being linked into the application binary. This isolates crashes and keeps the main binary lean.
   - The sidecar is spawned on first acquisition request and reused across fetches within a session.
   - Graceful shutdown on application exit.
   - Health check: if the sidecar dies, the next fetch restarts it.

3. **CDP or native API integration.** Obscura's interaction model needs to be verified during the first day of this workstream:
   - If Obscura exposes a CDP server, use a CDP client to drive it.
   - If it exposes a native Rust API, use that directly.
   - The `BrowserBackend` trait abstracts the difference for the rest of the application.

4. **`print_to_pdf` implementation.** Even though PDF rendering is Phase 4, the method is implemented now because the same sidecar serves both acquisition and rendering. If Obscura's PDF rendering fidelity is insufficient, the fallback is `rust-headless-chrome`.

5. **Fallback path.** If Obscura integration fails or its scraping fidelity proves insufficient against real target portals:
   - Switch the `BrowserBackend` implementation to `rust-headless-chrome`.
   - The rest of the application is unaffected because it depends on the trait, not the concrete type.
   - Document the reason for the switch.

### Critical Implementation Notes

1. **Verify Obscura's CDP compatibility or native API on day one.** Do not assume. Check the repository for the actual integration surface. If it is a CLI binary with a CDP server, the integration is a CDP client. If it is a Rust library, the integration is direct.
2. **Rendering fidelity for scraping vs PDF generation.** The search results note that Obscura's rendering engine is "an evolving independent engine" where long-tail CSS and some Web APIs may differ from Chromium. For **scraping**, what matters is JS execution and DOM access, not pixel-perfect rendering. For **PDF generation**, pixel fidelity matters more. If Obscura works for scraping but not for PDF, the fallback for PDF is `rust-headless-chrome` while Obscura remains the scraping backend. The `BrowserBackend` trait can be implemented twice, with different backends for different methods, if necessary.
3. **GPUI event loop must not block.** Wrap all browser calls in `spawn_blocking` or a dedicated worker thread. Use `gpui-tokio-bridge` if the calls are async.
4. **Sidecar binary distribution.** Obscura must be bundled with the application or downloaded on first use. Bundling is simpler but grows the artifact. Document the choice.
5. **Fallback is not a failure.** Choosing `rust-headless-chrome` after evaluating Obscura is a legitimate outcome. The trait exists precisely to make that switch cheap.

### Acceptance Criteria

| Criterion | Verification |
| :--- | :--- |
| Obscura sidecar starts and accepts a fetch request | Integration test |
| `fetch_rendered` returns post-JS DOM for a JS-heavy fixture | Integration test |
| `print_to_pdf` returns valid PDF bytes | Integration test |
| Sidecar crashes are recovered on the next request | Fault injection test |
| Sidecar is shut down cleanly on application exit | Integration test |
| GPUI event loop is not blocked during fetch | UI responsiveness test |
| Fallback to `rust-headless-chrome` compiles and passes the same tests | Compile + test with alternate backend |

---

## Workstream 2 — Tier 1a: ATS JSON API Adapters

**Priority:** After the browser backend is functional. Still valuable for speed and reliability.
**Estimated duration:** 1 week

### Deliverables

Same as the v0.1 Phase 2 plan:

1. `core::acquisition::ats` module with the `AtsAdapter` trait
2. Concrete adapters for Greenhouse, Lever, Ashby, SmartRecruiters
3. Auto-detection from portal URL
4. Normalization into `NormalizedJob`
5. Rate limiting and retry policy

### Critical Change from v0.1

**Tier 1a is no longer expected to cover the majority of portals.** It is an optimization: when a portal is ATS-hosted and the JSON API returns complete data, use it. When it does not, escalate to Tier 2 without hesitation. The browser is not a last resort; it is a normal path.

### Acceptance Criteria

Same as v0.1, with one addition:

| Criterion | Verification |
| :--- | :--- |
| (existing criteria) | |
| Tier 1a is used when available, but its absence does not block acquisition | Router test |

---

## Workstream 3 — Tier 1b: HTML Extraction

**Priority:** After Tier 1a. Same as v0.1.
**Estimated duration:** 1 week

### Deliverables

Same as the v0.1 Phase 2 plan:

1. `core::acquisition::html` using `reqwest` + `scraper`
2. Portal-level scraper configuration in `portals.scraper_config`
3. JSON-LD `JobPosting` extraction as the primary path
4. CSS selectors as the fallback
5. JS-required detection (now a **routing signal**, not a terminal failure)

### Critical Change from v0.1

**JS-required detection now triggers Tier 2 escalation, not an error.** A portal classified as `js_required` is routed to the browser backend. This is the normal flow, not a failure mode.

### Acceptance Criteria

Same as v0.1, with the JS-required criterion reframed:

| Criterion | Verification |
| :--- | :--- |
| (existing criteria) | |
| JS-required pages are routed to Tier 2, not surfaced as errors | Router test |

---

## Workstream 4 — Tier Routing, Normalization, and Storage

**Priority:** After Tier 1a, Tier 1b, and the browser backend are individually functional.
**Estimated duration:** 1 week

### Deliverables

1. `core::acquisition::router` implementing:

```
1. Read render_mode from portals row
2. If static → run Tier 1 (1a, then 1b)
3. On failure or js_required classification → escalate to Tier 2 (always available)
4. If browser → go directly to Tier 2
5. Record successful tier in job_descriptions.fetch_tier
6. Update portals.last_verified_at
```

2. **Deduplication** by `(source_url, external_id)`, then by content hash, then by `(source_url, title, company)` for entries without an external ID. The hash is computed by the Phase 1 job_descriptions::upsert — Phase 2 does not need to compute or pass it.
3. **Failure recording** with classified causes (`js_required`, `blocked`, `timeout`, `parse_error`)
4. **`fetch_tier` recording** per job listing
5. **`last_verified_at` update** per portal on success
6. **`render_mode` suggestion logic** (optional): if Tier 1 consistently fails and Tier 2 succeeds, suggest changing `render_mode` to `browser`

### Critical Change from v0.1

**Escalation is unconditional.** There is no "Tier 2 not compiled in" case. The browser is always available.

### Acceptance Criteria

Same as v0.1, with the "Tier 2 not compiled in" test removed and the "escalation is unconditional" property added.

---

## Workstream 5 — Portals Configuration UI

**Priority:** After routing works.
**Estimated duration:** 1 week

### Deliverables

Same as v0.1, with one addition:

1. **Portal list** showing health status
2. **Add/edit form** with `render_mode` selector
3. **Fetch progress** per portal
4. **Health drill-down** with classified failure causes and suggested actions
5. **Browser status indicator** showing whether the Obscura sidecar is running and healthy

### Critical Change from v0.1

**The `browser` option is always available.** There is no greyed-out state for a feature-disabled build. The health drill-down should mention the tier that succeeded or failed, so users understand whether Tier 1 or Tier 2 is doing the work.

### Acceptance Criteria

Same as v0.1, with the feature-disabled test removed and a sidecar health indicator test added.

---

## Workstream 6 — Manual Job Entry

**Priority:** Parallel with Portals UI.
**Estimated duration:** 2–3 days

### Deliverables

Same as v0.1:

1. "Add job by URL" action
2. Paste job description fallback
3. `fetch_tier = 'manual'` for manual entries
4. Deduplication against portal-fetched entries

### Critical Change

**None.** Manual entry was already tier-agnostic. It uses the same routing pipeline.

---

## Test Strategy

### Fixture-Based Testing

Same as v0.1, plus:

| Fixture type | Source | Used by |
| :--- | :--- | :--- |
| JS-heavy SPA shell HTML | Curated | Obscura integration, routing |
| ATS JSON responses | Curated | Tier 1a adapters |
| Static career page HTML | Curated | Tier 1b extraction |
| Malformed JSON / HTML | Hand-crafted | Error handling |

### Mock-Based Testing

- `BrowserBackend` tests use the mock from Phase 1 for routing logic tests.
- A small number of environment-gated tests exercise the real Obscura sidecar.

### Integration Tests

End-to-end flows:

1. Add a Greenhouse portal → fetch → verify `fetch_tier = 'tier1a:greenhouse'`.
2. Add a static career page → fetch → verify `fetch_tier = 'tier1b:html'`.
3. Add an SPA portal → fetch → verify Tier 1 fails → escalate to Tier 2 → `fetch_tier = 'tier2:obscura'`.
4. Fetch the same portal twice → verify no duplicates.
5. Manual URL entry → verify it appears alongside portal-fetched entries.
6. Kill the Obscura sidecar mid-batch → verify the next fetch restarts it and succeeds.

### CI Jobs

| Job | Purpose |
| :--- | :--- |
| `acquisition-fixtures` | Tier 1a and 1b tests against saved fixtures |
| `browser-obscura` | Obscura integration tests (environment-gated) |
| `browser-fallback` | Same tests with `rust-headless-chrome` backend, verifying the trait abstraction |
| `ui-portals` | Portals UI tests |
| `routing` | Tier routing and escalation tests with mock backends |

---

## Phase 2 Risks

| ID | Risk | Impact | Mitigation |
| :--- | :--- | :--- | :--- |
| P2-R1 | **Obscura integration proves harder than expected.** Its interaction model is not CDP, or its scraping fidelity is insufficient. | High | Spike it in Week 1. Fallback to `rust-headless-chrome` is a trait swap, not a rewrite. |
| P2-R2 | **Obscura rendering fidelity is insufficient for PDF generation.** | Medium | Scraping and PDF rendering can use different backends. Obscura for scraping, `rust-headless-chrome` for PDF. |
| P2-R3 | **Browser sidecar introduces process management complexity.** Crashes, orphaned processes, resource leaks. | Medium | Health checks, restart-on-failure, graceful shutdown. Integration test the crash path. |
| P2-R4 | **Tier 1 is less valuable than expected.** If ATS JSON APIs do not cover many targets, the browser does most of the work. | Low | Accepted. Tier 1 is an optimization, not the primary path. |
| P2-R5 | **Portal configuration UI scope creep.** | Medium | Ship the minimum: list, add, edit, delete, fetch, health. Advanced features are Phase 5. |

---

## Weekly Budget

| Week | Focus | Deliverable |
| :--- | :--- | :--- |
| **1** | Obscura integration spike, `BrowserBackend` implementation, sidecar lifecycle | Obscura fetches a JS-heavy fixture and returns rendered HTML |
| **2** | Tier 1a ATS adapters, Tier 1b HTML extraction | ATS JSON and static HTML extraction work |
| **3** | Tier routing, deduplication, `fetch_tier` recording, failure classification | End-to-end: portal → route → fetch → store |
| **4** | Portals Configuration UI, manual entry, integration, crash recovery | Portal CRUD, health display, manual entry, sidecar restart |

**Total:** 3–4 weeks. The lower end assumes Obscura integrates cleanly in Week 1. The upper end accounts for integration friction or a fallback switch.

---

## Exit Criteria

Phase 2 is complete when:

1. The Obscura `BrowserBackend` implementation fetches JS-heavy pages and returns post-JS DOM.
2. The sidecar starts, is reused across fetches, and restarts after a crash.
3. `print_to_pdf` works (even if not used until Phase 4).
4. At least four ATS adapters fetch and normalize real job listings.
5. HTML extraction handles JSON-LD and CSS-selector-based career pages.
6. JS-required pages are routed to Tier 2, not surfaced as errors.
7. Tier routing follows `render_mode` with unconditional escalation.
8. `fetch_tier` is recorded per listing; `last_verified_at` is updated on success.
9. Duplicate listings are deduplicated.
10. Portals UI supports CRUD, fetch progress, health drill-down, and sidecar status.
11. Manual job entry works.
12. A fallback build using `rust-headless-chrome` passes the same acquisition tests, proving the trait abstraction.
13. CI is green on all platforms.
14. A retrospective memo records Obscura integration findings and whether the fallback was needed.

---

## What Phase 2 Is Not

Phase 2 acquires job postings. It does not evaluate them, score them, generate cover letters, or produce PDFs. Specifically:

- There is no AI evaluation, no `rig` integration, no match scoring.
- There is no cover letter generation.
- There is no PDF rendering UI, even though `print_to_pdf` is implemented on the backend.
- There is no prompt seeding or management.

The browser backend being required does not change the phase boundary. It only changes the acquisition pipeline's internal dependency graph.

---

The single most important thing to take from this plan: **the Obscura integration spike in Week 1 is the gate.** The browser is now required, so if Obscura cannot be integrated or cannot scrape the target portals, the entire acquisition strategy needs the `rust-headless-chrome` fallback. That fallback is cheap because of the `BrowserBackend` trait, but it is not free — it changes the bundle size, the process model, and potentially the rendering fidelity for Phase 4. Discover that in Week 1, not Week 4.
