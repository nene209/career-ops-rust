# Phase 2: Acquisition — Detailed Execution Plan

**Parent plan:** Draft Execution Plan: Career-Ops Rust Desktop Application (v0.5), §12
**Status:** Draft — revised for Phase 1 outcomes and current Obscura integration options
**Last updated:** 2026-10-19
**Duration:** 3–5 weeks
**Depends on:** Phase 1 completion (confirmed 2026-10-19)

---

## Change Log

| Version | Change |
| :--- | :--- |
| v0.1 | Initial Phase 2 plan; browser backend feature-gated, `rust-headless-chrome` as default. |
| v0.2 | Browser backend is now a **required** dependency (not feature-gated). Obscura is the chosen backend. `rust-headless-chrome` retained as documented fallback. Tier 1 remains the default path where it works, but is no longer expected to cover the majority of target portals. |
| v0.3 | Revised against Phase 1 outcomes and current ecosystem. Removed the `print_to_pdf` deliverable — Obscura cannot render PDFs, and Phase 1's `BrowserBackend` trait provides a `BackendUnsupported` default. Added evaluation of Obscura's embedded Rust API alongside the sidecar model. Added `chromiumoxide` as a second fallback. Made test-support and mock-backend dependencies explicit. Added toolchain requirement to entry conditions. Added a verification-pass exit criterion, per the Phase 1 retrospective lesson. |

---

## Purpose

Phase 2 makes the application **useful for the first time**. It implements the tiered job acquisition pipeline: Tier 1 (HTTP/JSON) handles portals that return complete content without JavaScript, and Tier 2 (headless browser via Obscura) handles the portals that require JS execution or are otherwise unreachable via plain HTTP.

**A material change from the v0.1 plan:** Spike B was not run, but its central question was answered directly by the project owner — **`reqwest` + `scraper` are not sufficient for the target portals.** JavaScript execution is required. This means:

1. The browser backend is **not** a feature-gated escalation. It is a **required dependency** for basic acquisition against the intended targets.
2. The "lean build" story from the main plan §9 does not apply to this deployment. The application ships with a browser backend.
3. Tier 1 is still valuable — it is faster, cheaper, and more robust for the portals it covers — but the expected coverage is lower than the v0.1 plan assumed.

**A second material change in v0.3:** Phase 1 established that `BrowserBackend::print_to_pdf` has a default implementation returning `BackendUnsupported`, and the Phase 1 backend tests pin that property. Obscura has no layout or paint engine, so it cannot render PDFs. Phase 2 therefore **does not implement `print_to_pdf`** on the Obscura backend. PDF rendering is a Phase 4 concern, using a separate `PdfBackend` implementation based on `rust-headless-chrome`.

---

## Entry Conditions

Phase 2 begins after Phase 1 exit criteria are met. Phase 1 is confirmed complete as of 2026-10-19. Consumed outputs:

| Phase 1 Output | How Phase 2 Uses It |
| :--- | :--- |
| `BrowserBackend` trait with `print_to_pdf` default | Implemented by the Obscura backend; `print_to_pdf` inherits the `BackendUnsupported` default |
| `PdfBackend` trait definition | Not used in Phase 2 |
| `MockBrowserBackend` in `core::backend` | Used for routing and escalation tests |
| `career-ops-test-support` crate | Provides persistent keychain backend for integration tests |
| `portals` table and repository | Source of portal configuration |
| `job_descriptions` table and repository, including `content_hash` and `upsert` | Destination for acquired listings; dedup logic already implemented |
| `applications` table and repository | Not directly used in Phase 2, but `job_descriptions` rows become attachable |
| Keystore and unlock flow | Gates access to portal configuration and acquired data |
| `AppState` state machine | Host for acquisition progress and portals UI state |
| GPUI shell with navigation | Host for the Portals Configuration screen |
| Test/CI infrastructure | Extended with acquisition and browser backend tests |

**Toolchain requirement:** Rust 1.95.0 or newer. The GPUI pin requires `std::hint::cold_path`, stabilized in 1.95.0. Phase 2 does not itself depend on GPUI, but the workspace build does, and CI runs the workspace.

**No Phase 0 output is consumed** because Phase 0 was skipped. The decisions that would have come from Phase 0 are recorded in the Decisions Addendum and are treated as settled inputs to this phase.

---

## Scope

| In scope | Out of scope |
| :--- | :--- |
| Tier 1a: ATS JSON API adapters (Greenhouse, Lever, Ashby, SmartRecruiters) | AI evaluation of job postings |
| Tier 1b: `reqwest` + `scraper` HTML extraction for static career pages | Cover letter generation |
| **Tier 2: Obscura-based `BrowserBackend`** (required, not feature-gated) | PDF rendering of CV or cover letters |
| Evaluation of Obscura sidecar vs. embedded integration model | `rig` integration and LLM calls |
| Portal configuration UI (add, edit, delete, `render_mode` selector, health) | Prompt seeding and management |
| Tier routing with escalation and `fetch_tier` recording | File-based data migration |
| Acquisition tests using saved HTML/JSON fixtures | Full CV editor |
| `fetch_tier` and `last_verified_at` updates | Bulk import from external sources |
| Manual job entry (paste a URL, fetch it) | Browser stealth/anti-detection beyond Obscura defaults |
| Browser process or instance lifecycle management | |
| Fallback path to `rust-headless-chrome` or `chromiumoxide` if Obscura proves unsuitable | |

---

## Sequencing Rationale

```
W1: Obscura integration spike (sidecar vs. embedded; BrowserBackend implementation)
    └── W2: Tier 1a (ATS JSON) + Tier 1b (HTML extraction)
        └── W3: Tier routing, normalization, storage
            └── W4: Portals Configuration UI, manual entry, integration
                └── W5: Verification pass, fallback confirmation
```

**Obscura integration comes first.** In the v0.1 plan, Tier 1 came first because it was the highest-leverage path. With the browser now required, the browser backend is the load-bearing dependency. If Obscura cannot be integrated, or its scraping fidelity is insufficient, the entire acquisition strategy needs a fallback — and that is better to discover in Week 1 than Week 5.

Tier 1 work follows because it is still valuable: ATS JSON APIs are faster and more reliable than browser-based scraping for the portals they cover, and they reduce the load on the browser instance.

**A verification pass is added as Week 5.** The Phase 1 retrospective's central lesson was that a test passing is not the same as a test testing the right thing. Phase 2 should budget time for a verification pass that checks each exit criterion against the code, rather than assuming the green suite implies completeness.

---

## Workstream 1 — Obscura `BrowserBackend` Integration

**Priority:** First. Load-bearing dependency.
**Estimated duration:** 1–1.5 weeks (includes integration spike and sidecar-vs-embedded evaluation)

### Deliverables

1. `ObscuraBrowserBackend` implementing `BrowserBackend`:

```rust
pub struct ObscuraBrowserBackend {
    config: ObscuraConfig,
    // sidecar handle or embedded Browser, depending on integration model
}

#[async_trait]
impl BrowserBackend for ObscuraBrowserBackend {
    async fn fetch_rendered(
        &self,
        url: &Url,
        opts: RenderOpts,
    ) -> Result<RenderedPage>;

    // print_to_pdf is deliberately NOT overridden.
    // The trait default returns BackendUnsupported.
}

fn name(&self) -> &'static str { "obscura" }
```

2. **Integration model evaluation.** Obscura provides both a CDP server (sidecar) and an embeddable Rust crate. The spike must evaluate both before committing:

| Model | How it works | Pros | Cons |
| :--- | :--- | :--- | :--- |
| **Sidecar + CDP** | Spawn `obscura serve` on a local port; drive over CDP | Crash isolation; app survives browser death; standard protocol | IPC overhead; process lifecycle management; port allocation |
| **Embedded Rust API** | Add `obscura` as a dependency; construct a `Browser` in-process | No IPC; simpler lifecycle; lower latency; smaller process count | Browser crash is an app crash; V8 linked into the app binary; binary size grows |

Selection criteria: binary size delta, crash isolation, GPUI event-loop interaction (whether the embedded API can run off the main thread), and whether the embedded API exposes the `Page`/`Element` surface needed for scraping. Document the choice and the evidence.

3. **Lifecycle management.** Regardless of model:
   - Spawn or construct on first acquisition request; reuse across fetches within a session.
   - Graceful shutdown or drop on application exit.
   - Health check: if the sidecar dies or the embedded browser panics, the next fetch restarts or reconstructs it.
   - If sidecar: allocate a free port rather than a fixed one, to avoid collisions.

4. **No `print_to_pdf` implementation.** This is deliberate and load-bearing:
   - Obscura cannot render PDFs. It has no layout or paint engine.
   - The `BrowserBackend` trait's default `print_to_pdf` returns `CoreError::BackendUnsupported`.
   - `ObscuraBrowserBackend` inherits that default and must not override it.
   - PDF rendering is a Phase 4 concern, via `rust-headless-chrome` as a separate `PdfBackend`.
   - A test asserts that `ObscuraBrowserBackend::print_to_pdf` returns `BackendUnsupported`.

5. **Fallback path.** If Obscura integration fails or its scraping fidelity proves insufficient against real target portals:
   - **First fallback:** `rust-headless-chrome`. It is mature and complete, but less actively maintained than it once was.
   - **Second fallback:** `chromiumoxide`. More actively maintained, async-first, tokio-native. Verify `PrintToPdfParams` coverage before choosing, though PDF rendering is Phase 4.
   - The rest of the application is unaffected because it depends on the trait, not the concrete type.
   - Document the reason for the switch and record the maintenance status of both crates as of the spike date.

### Critical Implementation Notes

1. **Verify Obscura's integration surface on day one.** Do not assume. Check whether the CDP server and the embedded crate expose the same capabilities, and whether either is feature-gated or platform-limited. The existence of an embedded API changes the sidecar-vs-embedded decision; the plan does not pre-commit to sidecar.
2. **Scraping needs JS execution and DOM access, not pixel fidelity.** Obscura's rendering engine is an evolving independent engine; long-tail CSS and some Web APIs may differ from Chromium. For scraping, this is acceptable. For PDF generation it would not be, which is why PDF is not this backend's job.
3. **GPUI event loop must not block.** Wrap all browser calls in `spawn_blocking` or a dedicated worker thread. If using the embedded API, verify that V8 execution can run off the GPUI main thread; if not, the embedded model is disqualified. Use `gpui-tokio-bridge` if the calls are async.
4. **Distribution.** If sidecar, the binary must be bundled or downloaded on first use. If embedded, V8 is linked into the application binary. Both choices affect the final artifact size; document the trade-off in the Phase 6 bundle accounting.
5. **Fallback is not a failure.** Choosing `rust-headless-chrome` or `chromiumoxide` after evaluating Obscura is a legitimate outcome. The trait exists precisely to make that switch cheap.

### Acceptance Criteria

| Criterion | Verification |
| :--- | :--- |
| Obscura backend starts (sidecar or embedded) and accepts a fetch request | Integration test |
| `fetch_rendered` returns post-JS DOM for a JS-heavy fixture | Integration test |
| `print_to_pdf` returns `BackendUnsupported` | Unit test |
| Integration model choice (sidecar vs. embedded) is documented with evidence | Spike memo |
| Browser death or panic is recovered on the next request | Fault injection test |
| Browser is shut down or dropped cleanly on application exit | Integration test |
| GPUI event loop is not blocked during fetch | UI responsiveness test |
| Fallback to `rust-headless-chrome` or `chromiumoxide` compiles and passes the same scraping tests | Compile + test with alternate backend |

---

## Workstream 2 — Tier 1a: ATS JSON API Adapters

**Priority:** After the browser backend is functional. Still valuable for speed and reliability.
**Estimated duration:** 1 week

### Deliverables

1. `core::acquisition::ats` module with the `AtsAdapter` trait
2. Concrete adapters for Greenhouse, Lever, Ashby, SmartRecruiters
3. Auto-detection from portal URL
4. Normalization into `NormalizedJob`
5. Rate limiting and retry policy

### Critical Change from v0.1

**Tier 1a is not expected to cover the majority of portals.** It is an optimization: when a portal is ATS-hosted and the JSON API returns complete data, use it. When it does not, escalate to Tier 2 without hesitation. The browser is not a last resort; it is a normal path.

### Acceptance Criteria

| Criterion | Verification |
| :--- | :--- |
| Each adapter fetches and parses a real board correctly | Integration test (environment-gated) |
| Auto-detection identifies the correct adapter from a portal URL | Unit tests with URL fixtures |
| `NormalizedJob` contains all required fields for each adapter | Unit tests per adapter |
| Adapter handles empty boards and malformed JSON without panic | Unit tests with edge-case fixtures |
| Rate limiting respects configured delay | Unit test with mock client |
| Tier 1a is used when available, but its absence does not block acquisition | Router test |

---

## Workstream 3 — Tier 1b: HTML Extraction

**Priority:** After Tier 1a.
**Estimated duration:** 1 week

### Deliverables

1. `core::acquisition::html` using `reqwest` + `scraper`
2. Portal-level scraper configuration in `portals.scraper_config`
3. JSON-LD `JobPosting` extraction as the primary path
4. CSS selectors as the fallback
5. JS-required detection as a routing signal, not a terminal failure

### Critical Change from v0.1

**JS-required detection triggers Tier 2 escalation, not an error.** A portal classified as `js_required` is routed to the browser backend. This is the normal flow, not a failure mode.

### Acceptance Criteria

| Criterion | Verification |
| :--- | :--- |
| JSON-LD extraction works on pages with `JobPosting` schema | Unit test with saved fixture |
| CSS selector extraction works on pages without structured data | Unit test with saved fixture |
| JS-required pages are classified distinctly from empty pages | Unit test with SPA fixture |
| `scraper_config` round-trips through the database | Integration test |
| Extraction handles malformed HTML without panic | Unit test with truncated markup |
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

2. **Deduplication.** `job_descriptions::upsert` already implements the full dedup order: `(source_url, external_id)`, then `(source_url, content_hash)`, then `(source_url, title, company)` for entries without an external ID. Phase 2 does not compute or pass the hash; the repository does. The router only calls `upsert`.
3. **Failure recording** with classified causes (`js_required`, `blocked`, `timeout`, `parse_error`)
4. **`fetch_tier` recording** per job listing, using the existing `FetchTier` enum values: `tier1a:ats`, `tier1b:html`, `tier2:obscura`, `manual`
5. **`last_verified_at` update** per portal on success
6. **`render_mode` suggestion logic** (optional): if Tier 1 consistently fails and Tier 2 succeeds, suggest changing `render_mode` to `browser`

### Critical Change from v0.1

**Escalation is unconditional.** There is no "Tier 2 not compiled in" case. The browser is always available.

### Acceptance Criteria

| Criterion | Verification |
| :--- | :--- |
| `static` portal routes to Tier 1 | Unit test with mock backends |
| `browser` portal routes to Tier 2 directly | Unit test with mock backends |
| Tier 1 failure or `js_required` escalates to Tier 2 | Unit test with mock backends |
| Successful fetch records `fetch_tier` correctly | Integration test |
| Duplicate listings are deduplicated via `upsert` | Integration test |
| `last_verified_at` updates on success, not on failure | Integration test |
| Failure causes are classified and stored | Integration test |

---

## Workstream 5 — Portals Configuration UI

**Priority:** After routing works.
**Estimated duration:** 1 week

### Deliverables

1. **Portal list** showing health status, `render_mode`, and the tier that succeeded or failed last time
2. **Add/edit form** with `render_mode` selector
3. **Fetch progress** per portal
4. **Health drill-down** with classified failure causes and suggested actions
5. **Browser status indicator** showing whether the Obscura backend is running and healthy

### Critical Change from v0.1

**The `browser` option is always available.** There is no greyed-out state for a feature-disabled build. The health drill-down should mention the tier that succeeded or failed, so users understand whether Tier 1 or Tier 2 is doing the work.

### Acceptance Criteria

| Criterion | Verification |
| :--- | :--- |
| Portal CRUD round-trips through the database | UI integration test |
| ATS auto-detection populates the adapter correctly | UI test with known URLs |
| Fetch progress displays per-portal status | UI smoke test |
| Failure drill-down shows classified cause and suggestion | UI test |
| Manual fetch does not create duplicates | Integration test |
| Browser status indicator reflects backend health | UI test |

---

## Workstream 6 — Manual Job Entry

**Priority:** Parallel with Portals UI.
**Estimated duration:** 2–3 days

### Deliverables

1. "Add job by URL" action in the Pipeline screen
2. Fetch the URL via the tiered pipeline, normalize, and store
3. "Paste job description" fallback for URLs that cannot be fetched
4. Manual entry creates a `job_descriptions` row with `fetch_tier = 'manual'`
5. Deduplication against portal-fetched entries via the existing `upsert`

### Critical Change

**None.** Manual entry was already tier-agnostic. It uses the same routing pipeline and the same `upsert` path.

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

| Fixture type | Source | Used by |
| :--- | :--- | :--- |
| JS-heavy SPA shell HTML | Curated | Obscura integration, routing |
| ATS JSON responses | Curated | Tier 1a adapters |
| Static career page HTML | Curated | Tier 1b extraction |
| Malformed JSON / HTML | Hand-crafted | Error handling |

### Mock-Based Testing

- Routing logic tests use `MockBrowserBackend` from `core::backend` (Phase 1).
- A small number of environment-gated tests exercise the real Obscura backend.
- Keychain-dependent integration tests use `career-ops-test-support::install()`, not `keyring::mock`. The persistent test backend is the only keychain backend in use across the workspace.

### Integration Tests

End-to-end flows:

1. Add a Greenhouse portal → fetch → verify `fetch_tier = 'tier1a:ats'`.
2. Add a static career page → fetch → verify `fetch_tier = 'tier1b:html'`.
3. Add an SPA portal → fetch → verify Tier 1 fails or is classified `js_required` → escalate to Tier 2 → `fetch_tier = 'tier2:obscura'`.
4. Fetch the same portal twice → verify no duplicates via `upsert`.
5. Manual URL entry → verify it appears alongside portal-fetched entries.
6. Kill the browser backend mid-batch (sidecar process or injected panic) → verify the next fetch recovers and succeeds.
7. Confirm `ObscuraBrowserBackend::print_to_pdf` returns `BackendUnsupported`, so no code path accidentally depends on it.

### CI Jobs

| Job | Purpose |
| :--- | :--- |
| `acquisition-fixtures` | Tier 1a and 1b tests against saved fixtures |
| `browser-obscura` | Obscura integration tests (environment-gated) |
| `browser-fallback` | Same scraping tests with `rust-headless-chrome` or `chromiumoxide`, verifying the trait abstraction |
| `routing` | Tier routing and escalation tests with mock backends |
| `ui-portals` | Portals UI tests |

---

## Phase 2 Risks

| ID | Risk | Impact | Mitigation |
| :--- | :--- | :--- | :--- |
| P2-R1 | **Obscura integration proves harder than expected.** Its interaction model is unsuitable, or its scraping fidelity is insufficient. | High | Spike both sidecar and embedded models in Week 1. First fallback: `rust-headless-chrome`. Second fallback: `chromiumoxide`, which is more actively maintained. Record maintenance status of both as of the spike date. The `BrowserBackend` trait makes each swap contained. |
| P2-R2 | **Obscura cannot render PDFs.** Confirmed by its architecture; not a risk to Phase 2. | Resolved | PDF rendering is Phase 4, via `rust-headless-chrome` as `PdfBackend`. Phase 2 must not implement `print_to_pdf`; a test pins the `BackendUnsupported` default. |
| P2-R3 | **Browser lifecycle introduces process or instance management complexity.** Crashes, orphaned processes, resource leaks, or V8 panics on the main thread. | Medium | Health checks, restart-on-failure, graceful shutdown. For embedded, verify V8 runs off the main thread; if not, use sidecar. Integration test the crash path. |
| P2-R4 | **Tier 1 is less valuable than expected.** If ATS JSON APIs do not cover many targets, the browser does most of the work. | Low | Accepted. Tier 1 is an optimization, not the primary path. |
| P2-R5 | **Portal configuration UI scope creep.** | Medium | Ship the minimum: list, add, edit, delete, fetch, health. Advanced features are Phase 5. |
| P2-R6 | **Test passes for the wrong reason.** Phase 1 found two such cases during verification. | Medium | Add an explicit verification pass in Week 5. For each exit criterion, confirm the test actually exercises the property, not a proxy. |

---

## Weekly Budget

| Week | Focus | Deliverable |
| :--- | :--- | :--- |
| **1** | Obscura integration spike: sidecar vs. embedded evaluation, `BrowserBackend` implementation, lifecycle | Obscura fetches a JS-heavy fixture and returns rendered HTML; integration model chosen and documented |
| **2** | Tier 1a ATS adapters, Tier 1b HTML extraction | ATS JSON and static HTML extraction work |
| **3** | Tier routing, deduplication, `fetch_tier` recording, failure classification | End-to-end: portal → route → fetch → store |
| **4** | Portals Configuration UI, manual entry, integration, crash recovery | Portal CRUD, health display, manual entry, backend restart |
| **5** | Verification pass, fallback confirmation, retrospective | Each exit criterion checked against the code; fallback build confirmed; retrospective memo written |

**Total:** 3–5 weeks. The lower end assumes Obscura integrates cleanly in Week 1 and the verification pass is uneventful. The upper end accounts for integration friction, a fallback switch, or verification findings.

---

## Exit Criteria

Phase 2 is complete when **all** of the following are true:

1. The Obscura `BrowserBackend` implementation fetches JS-heavy pages and returns post-JS DOM.
2. The integration model (sidecar or embedded) is chosen and documented with evidence.
3. The browser backend starts, is reused across fetches, and recovers after a crash or panic.
4. `ObscuraBrowserBackend::print_to_pdf` returns `BackendUnsupported`, confirming the two-backend architecture holds and no Phase 2 code depends on PDF rendering.
5. At least four ATS adapters fetch and normalize real job listings.
6. HTML extraction handles JSON-LD and CSS-selector-based career pages.
7. JS-required pages are routed to Tier 2, not surfaced as errors.
8. Tier routing follows `render_mode` with unconditional escalation.
9. `fetch_tier` is recorded per listing; `last_verified_at` is updated on success.
10. Duplicate listings are deduplicated via `job_descriptions::upsert`.
11. Portals UI supports CRUD, fetch progress, health drill-down, and browser status.
12. Manual job entry works and deduplicates against portal-fetched entries.
13. A fallback build using `rust-headless-chrome` or `chromiumoxide` passes the same scraping tests, proving the trait abstraction.
14. CI is green on all platforms.
15. **Verification pass complete.** Each exit criterion above has been checked against the code, with a note on how it is verified and whether the verification actually exercises the property. The retrospective records any criterion that passed for the wrong reason and how it was fixed.
16. A retrospective memo records Obscura integration findings, whether the fallback was needed, and what the verification pass found.

---

## What Phase 2 Is Not

Phase 2 acquires job postings. It does not evaluate them, score them, generate cover letters, or produce PDFs. Specifically:

- There is no AI evaluation, no `rig` integration, no match scoring.
- There is no cover letter generation.
- There is no PDF rendering. `print_to_pdf` returns `BackendUnsupported` for the Obscura backend; PDF rendering is Phase 4.
- There is no prompt seeding or management.

The browser backend being required does not change the phase boundary. It only changes the acquisition pipeline's internal dependency graph.

---

The single most important thing to take from this plan: **the Obscura integration spike in Week 1 is the gate.** The browser is now required, so if Obscura cannot be integrated or cannot scrape the target portals, the entire acquisition strategy needs a fallback. That fallback is cheap because of the `BrowserBackend` trait, but it is not free — it changes the bundle size, the process model, and possibly the rendering fidelity for Phase 4. Discover that in Week 1, not Week 5.

The second most important thing: **do not implement `print_to_pdf` on the Obscura backend.** It cannot render PDFs, the trait default correctly expresses that, and a test pins the behaviour. Overriding it with a stub that pretends to work would be worse than leaving the default in place.