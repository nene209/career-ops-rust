# Phase 2: Acquisition — Detailed Execution Plan

**Parent plan:** Draft Execution Plan: Career-Ops Rust Desktop Application (v0.5), §12
**Status:** Draft — sidecar integration confirmed; embedded model deferred
**Last updated:** 2026-10-19
**Duration:** 3–4 weeks
**Depends on:** Phase 1 completion (confirmed 2026-10-19)

---

## Change Log

| Version | Change |
| :--- | :--- |
| v0.1 | Initial Phase 2 plan; browser backend feature-gated, `rust-headless-chrome` as default. |
| v0.2 | Browser backend is now a **required** dependency (not feature-gated). Obscura is the chosen backend. `rust-headless-chrome` retained as documented fallback. Tier 1 remains the default path where it works, but is no longer expected to cover the majority of target portals. |
| v0.3 | Revised against Phase 1 outcomes and current ecosystem. Removed the `print_to_pdf` deliverable — Obscura cannot render PDFs, and Phase 1's `BrowserBackend` trait provides a `BackendUnsupported` default. Added evaluation of Obscura's embedded Rust API alongside the sidecar model. Added `chromiumoxide` as a second fallback. Made test-support and mock-backend dependencies explicit. Added toolchain requirement to entry conditions. Added a verification-pass exit criterion, per the Phase 1 retrospective lesson. |
| v0.4 | **Sidecar integration confirmed.** The embedded model is deferred to a future enhancement. Rationale: crash isolation, GPUI event-loop safety, and portability across browser backend swaps (all three candidate backends are CDP-driven). Removed the sidecar-vs-embedded evaluation from the Week 1 spike; Week 1 now focuses on sidecar integration only. Reduced duration from 3–5 weeks to 3–4 weeks. |

---

## Purpose

Phase 2 makes the application **useful for the first time**. It implements the tiered job acquisition pipeline: Tier 1 (HTTP/JSON) handles portals that return complete content without JavaScript, and Tier 2 (headless browser via Obscura) handles the portals that require JS execution or are otherwise unreachable via plain HTTP.

**A material change from the v0.1 plan:** Spike B was not run, but its central question was answered directly by the project owner — **`reqwest` + `scraper` are not sufficient for the target portals.** JavaScript execution is required. This means:

1. The browser backend is **not** a feature-gated escalation. It is a **required dependency** for basic acquisition against the intended targets.
2. The "lean build" story from the main plan §9 does not apply to this deployment. The application ships with a browser backend.
3. Tier 1 is still valuable — it is faster, cheaper, and more robust for the portals it covers — but the expected coverage is lower than the v0.1 plan assumed.

**A second material change in v0.3:** Phase 1 established that `BrowserBackend::print_to_pdf` has a default implementation returning `BackendUnsupported`, and the Phase 1 backend tests pin that property. Obscura has no layout or paint engine, so it cannot render PDFs. Phase 2 therefore **does not implement `print_to_pdf`** on the Obscura backend. PDF rendering is a Phase 4 concern, using a separate `PdfBackend` implementation based on `rust-headless-chrome`.

**A third material change in v0.4:** Obscura provides both a CDP server (sidecar model) and an embeddable Rust crate. **Sidecar is the confirmed integration model.** The rationale is threefold:

1. **Crash isolation.** A V8 panic, out-of-memory condition, or browser hang kills the sidecar process, not the host application. In the embedded model, V8 runs inside the app process and a crash is an application crash. For a consumer desktop app where users do not expect to lose their session because a job board served hostile JavaScript, this is decisive.
2. **GPUI event-loop safety.** CDP calls are I/O-bound and can be driven from a tokio task without touching the GPUI main thread. The embedded model's V8 execution would need to be verified to run off the main thread, and that verification is not guaranteed to succeed.
3. **Backend portability.** All three candidate backends — Obscura, `rust-headless-chrome`, and `chromiumoxide` — spawn a browser subprocess and speak CDP. The embedded model is unique to Obscura. Choosing it would optimize for the backend the project has explicitly named as a fallback, at the cost of portability to the other two.

The embedded model is documented as a **deferred future enhancement** (see Workstream 1). It may become attractive if the sidecar's process-management complexity proves burdensome and the GPUI thread-placement question is answered favorably.

---

## Entry Conditions

Phase 2 begins after Phase 1 exit criteria are met. Phase 1 is confirmed complete as of 2026-10-19. Consumed outputs:

| Phase 1 Output | How Phase 2 Uses It |
| :--- | :--- |
| `BrowserBackend` trait with `print_to_pdf` default | Implemented by the Obscura sidecar backend; `print_to_pdf` inherits the `BackendUnsupported` default |
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
| **Tier 2: Obscura sidecar-based `BrowserBackend`** (required, not feature-gated) | PDF rendering of CV or cover letters |
| Sidecar process lifecycle management | `rig` integration and LLM calls |
| Portal configuration UI (add, edit, delete, `render_mode` selector, health) | Prompt seeding and management |
| Tier routing with escalation and `fetch_tier` recording | File-based data migration |
| Acquisition tests using saved HTML/JSON fixtures | Full CV editor |
| `fetch_tier` and `last_verified_at` updates | Bulk import from external sources |
| Manual job entry (paste a URL, fetch it) | Browser stealth/anti-detection beyond Obscura defaults |
| Fallback path to `rust-headless-chrome` or `chromiumoxide` if Obscura proves unsuitable | Obscura embedded-crate integration (deferred) |

---

## Sequencing Rationale

```
W1: Obscura sidecar integration (BrowserBackend implementation, CDP client, lifecycle)
    └── W2: Tier 1a (ATS JSON) + Tier 1b (HTML extraction)
        └── W3: Tier routing, normalization, storage
            └── W4: Portals Configuration UI, manual entry, integration, verification pass
```

**Obscura integration comes first.** In the v0.1 plan, Tier 1 came first because it was the highest-leverage path. With the browser now required, the browser backend is the load-bearing dependency. If Obscura cannot be integrated, or its scraping fidelity is insufficient, the entire acquisition strategy needs a fallback — and that is better to discover in Week 1 than Week 4.

Tier 1 work follows because it is still valuable: ATS JSON APIs are faster and more reliable than browser-based scraping for the portals they cover, and they reduce the load on the browser process.

**The verification pass is folded into Week 4**, rather than being a separate week. The Phase 1 retrospective's central lesson was that a test passing is not the same as a test testing the right thing. Week 4 includes a check of each exit criterion against the code, rather than assuming the green suite implies completeness. If the verification pass surfaces findings, an additional half-week may be needed; that is budgeted in the duration range.

---

## Workstream 1 — Obscura Sidecar `BrowserBackend` Integration

**Priority:** First. Load-bearing dependency.
**Estimated duration:** 1–1.5 weeks (includes integration spike)

### Deliverables

1. `ObscuraBrowserBackend` implementing `BrowserBackend`:

```rust
pub struct ObscuraBrowserBackend {
    config: ObscuraConfig,
    client: Option<CdpClient>,  // lazily spawned
    child: Option<Child>,       // sidecar process handle
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

fn name(&self) -> &'static str { "obscura-sidecar" }
```

2. **Sidecar lifecycle management.** This is the load-bearing complexity of the workstream.

   - **Spawn on first request.** The sidecar is not started at application launch; it starts on the first acquisition request that needs Tier 2. This keeps startup fast and avoids a browser process running for users who only use Tier 1.
   - **Port allocation.** Spawn `obscura serve` on a free port, not a fixed port. Read the port from stdout, or pass it via a `--port 0` flag if supported, and parse the assigned port. A fixed port causes collisions when two copies of the app run, or when a stale process is still bound.
   - **Reuse within a session.** Subsequent fetches use the same sidecar. Do not spawn per-fetch.
   - **Health check.** Before each fetch, ping the CDP endpoint. If the ping fails, the sidecar has died; restart it and retry the fetch once. A second failure surfaces the error to the caller.
   - **Graceful shutdown.** On `AppState` drop or application exit, send SIGTERM to the child and wait for it to exit with a timeout. On timeout, SIGKILL. Orphaned sidecars are a resource leak, and a stale browser process holding a locked profile directory will block the next launch.
   - **Stale process detection on startup.** On first spawn, check for a lock file or port reservation from a previous run. If found, attempt a graceful shutdown before spawning. This handles the "app crashed with a sidecar running" case.

3. **CDP client integration.** Use `chromiumoxide` as the CDP client. It is more actively maintained than `rust-headless-chrome`, async-first, tokio-native, and its `Page::goto`, `Page::content`, and `Page::wait_for_selector` map cleanly to the `fetch_rendered` semantics. If `chromiumoxide`'s CDP surface proves incompatible with Obscura's CDP server, `rust-headless-chrome` is the alternative client (it is synchronous, wrapped in `spawn_blocking`).

   Note: the sidecar model means the CDP client choice is **independent** of the browser choice. If Obscura is replaced by `rust-headless-chrome`'s subprocess or `chromiumoxide`'s own Chrome spawn, the client stays the same. This is a second portability benefit beyond the browser swap.

4. **No `print_to_pdf` implementation.** This is deliberate and load-bearing:

   - Obscura cannot render PDFs. It has no layout or paint engine.
   - The `BrowserBackend` trait's default `print_to_pdf` returns `CoreError::BackendUnsupported`.
   - `ObscuraBrowserBackend` inherits that default and must not override it.
   - PDF rendering is a Phase 4 concern, via `rust-headless-chrome` as a separate `PdfBackend`.
   - A test asserts that `ObscuraBrowserBackend::print_to_pdf` returns `BackendUnsupported`.

5. **Fallback path.** If Obscura integration fails or its scraping fidelity proves insufficient against real target portals:

   - **First fallback:** `rust-headless-chrome`. It is mature and complete, but less actively maintained than it once was.
   - **Second fallback:** `chromiumoxide` with its own Chrome spawn. More actively maintained.
   - Because the backend is a sidecar, the fallback is a trait swap plus a process-spawn change. The CDP client can stay the same in either case.
   - Document the reason for the switch and record the maintenance status of both crates as of the spike date.

### Deferred: Embedded Integration Model

Obscura also provides an embeddable Rust crate (`obscura`) that runs V8 in-process, without a sidecar process. This is **not** part of Phase 2. It is recorded here so the option is not lost.

**Why deferred:**

- **Crash isolation.** A V8 panic or out-of-memory condition in the embedded model takes down the host application. The sidecar model survives this.
- **GPUI event-loop risk.** Whether the embedded `Browser` can be driven entirely from a tokio worker thread while the GPUI main thread remains responsive is unverified. The sidecar model does not raise this question.
- **Portability.** The embedded model is unique to Obscura. The sidecar model is shared across all three candidate backends.

**When it might become attractive:**

- If sidecar process-management complexity (port allocation, stale process detection, graceful shutdown) proves more costly than anticipated.
- If distribution simplicity (one binary instead of two) becomes a higher priority in Phase 6 than crash isolation.
- If the GPUI thread-placement question is answered favorably and the browser-crash risk is judged acceptable.

The `BrowserBackend` trait makes this a contained change if it is ever pursued. It is a Phase 7+ candidate at the earliest.

### Critical Implementation Notes

1. **Verify Obscura's CDP surface on day one.** Do not assume full CDP compatibility. Check which CDP domains and methods the sidecar server exposes. `Page.navigate`, `Page.content`, and `Runtime.evaluate` are the minimum. `Page.waitForSelector` or equivalent is needed for reliable `fetch_rendered` semantics. If any of these is missing, the fallback path becomes more attractive.
2. **Scraping needs JS execution and DOM access, not pixel fidelity.** Obscura's rendering engine is an evolving independent engine; long-tail CSS and some Web APIs may differ from Chromium. For scraping, this is acceptable. For PDF generation it would not be, which is why PDF is not this backend's job.
3. **GPUI event loop must not block.** CDP calls are async I/O, but the sidecar's stdout/stderr and process-wait must not be polled on the GPUI main thread. Use `gpui-tokio-bridge` for the async client, and keep the process handle in a dedicated worker or tokio task.
4. **Distribution.** The `obscura` binary must be bundled with the application or downloaded on first use. Bundling is simpler but grows the artifact; download-on-first-use requires a trust story for the download source. Document the choice in the Phase 6 packaging plan.
5. **Fallback is not a failure.** Choosing `rust-headless-chrome` or `chromiumoxide` after evaluating Obscura is a legitimate outcome. The trait exists precisely to make that switch cheap, and the sidecar model makes the fallback a smaller change than the embedded model would.

### Acceptance Criteria

| Criterion | Verification |
| :--- | :--- |
| Sidecar starts on first request and accepts a fetch | Integration test |
| `fetch_rendered` returns post-JS DOM for a JS-heavy fixture | Integration test |
| `print_to_pdf` returns `BackendUnsupported` | Unit test |
| Sidecar uses an allocated port, not a fixed one | Unit test with two concurrent instances |
| Sidecar is reused across multiple fetches in a session | Integration test (process count assertion) |
| Sidecar death is detected and recovered on the next request | Fault injection test (kill child, retry) |
| Stale sidecar from a previous run is detected and cleaned up | Fault injection test (leave a fake lock file) |
| Sidecar is shut down cleanly on application exit | Integration test (assert child exit) |
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
5. **Sidecar status indicator** showing whether the Obscura sidecar is running, its PID, and its port

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
| Sidecar status indicator reflects process state | UI test |

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
| JS-heavy SPA shell HTML | Curated | Obscura sidecar integration, routing |
| ATS JSON responses | Curated | Tier 1a adapters |
| Static career page HTML | Curated | Tier 1b extraction |
| Malformed JSON / HTML | Hand-crafted | Error handling |

### Mock-Based Testing

- Routing logic tests use `MockBrowserBackend` from `core::backend` (Phase 1).
- A small number of environment-gated tests exercise the real Obscura sidecar.
- Keychain-dependent integration tests use `career-ops-test-support::install()`, not `keyring::mock`. The persistent test backend is the only keychain backend in use across the workspace.

### Integration Tests

End-to-end flows:

1. Add a Greenhouse portal → fetch → verify `fetch_tier = 'tier1a:ats'`.
2. Add a static career page → fetch → verify `fetch_tier = 'tier1b:html'`.
3. Add an SPA portal → fetch → verify Tier 1 fails or is classified `js_required` → escalate to Tier 2 → `fetch_tier = 'tier2:obscura'`.
4. Fetch the same portal twice → verify no duplicates via `upsert`.
5. Manual URL entry → verify it appears alongside portal-fetched entries.
6. Kill the sidecar mid-batch → verify the next fetch recovers and succeeds.
7. Leave a stale sidecar lock file → verify startup cleans it up.
8. Confirm `ObscuraBrowserBackend::print_to_pdf` returns `BackendUnsupported`, so no code path accidentally depends on it.

### CI Jobs

| Job | Purpose |
| :--- | :--- |
| `acquisition-fixtures` | Tier 1a and 1b tests against saved fixtures |
| `browser-obscura` | Obscura sidecar integration tests (environment-gated) |
| `browser-fallback` | Same scraping tests with `rust-headless-chrome` or `chromiumoxide`, verifying the trait abstraction |
| `routing` | Tier routing and escalation tests with mock backends |
| `ui-portals` | Portals UI tests |

---

## Phase 2 Risks

| ID | Risk | Impact | Mitigation |
| :--- | :--- | :--- | :--- |
| P2-R1 | **Obscura integration proves harder than expected.** Its CDP surface is incomplete, or its scraping fidelity is insufficient. | High | Spike CDP compatibility in Week 1. First fallback: `rust-headless-chrome`. Second fallback: `chromiumoxide` with its own Chrome spawn. Record maintenance status of both as of the spike date. The sidecar model makes each swap contained. |
| P2-R2 | **Obscura cannot render PDFs.** Confirmed by its architecture; not a risk to Phase 2. | Resolved | PDF rendering is Phase 4, via `rust-headless-chrome` as `PdfBackend`. Phase 2 must not implement `print_to_pdf`; a test pins the `BackendUnsupported` default. |
| P2-R3 | **Sidecar process management introduces complexity.** Port collisions, orphaned processes, stale lock files, resource leaks. | Medium | Allocate ports dynamically; detect stale sidecars on startup; SIGTERM-then-SIGKILL shutdown with timeout; integration test the crash and stale-process paths. |
| P2-R4 | **Tier 1 is less valuable than expected.** If ATS JSON APIs do not cover many targets, the browser does most of the work. | Low | Accepted. Tier 1 is an optimization, not the primary path. |
| P2-R5 | **Portal configuration UI scope creep.** | Medium | Ship the minimum: list, add, edit, delete, fetch, health. Advanced features are Phase 5. |
| P2-R6 | **Test passes for the wrong reason.** Phase 1 found two such cases during verification. | Medium | Fold a verification pass into Week 4. For each exit criterion, confirm the test actually exercises the property, not a proxy. |
| P2-R7 | **A future backend swap invalidates the sidecar assumptions.** If the fallback backend cannot run as a sidecar. | Low | All three candidate backends are CDP-driven subprocesses. If a future backend is embedded-only, the embedded model (already documented as deferred) becomes relevant again. The trait makes the swap contained. |

---

## Weekly Budget

| Week | Focus | Deliverable |
| :--- | :--- | :--- |
| **1** | Obscura sidecar spike: CDP surface verification, `BrowserBackend` implementation, process lifecycle | Obscura sidecar fetches a JS-heavy fixture and returns rendered HTML; lifecycle tests pass |
| **2** | Tier 1a ATS adapters, Tier 1b HTML extraction | ATS JSON and static HTML extraction work |
| **3** | Tier routing, deduplication, `fetch_tier` recording, failure classification | End-to-end: portal → route → fetch → store |
| **4** | Portals Configuration UI, manual entry, integration, verification pass, retrospective | Portal CRUD, health display, manual entry, sidecar recovery verified, each exit criterion checked against the code |

**Total:** 3–4 weeks. The lower end assumes Obscura's CDP surface is complete and integrates cleanly in Week 1. The upper end accounts for CDP-surface gaps or a fallback switch.

---

## Exit Criteria

Phase 2 is complete when **all** of the following are true:

1. The Obscura sidecar fetches JS-heavy pages and returns post-JS DOM.
2. The sidecar starts lazily on first request, uses an allocated port, is reused across fetches, and recovers after a crash.
3. Stale sidecars from previous runs are detected and cleaned up on startup.
4. The sidecar is shut down cleanly on application exit.
5. `ObscuraBrowserBackend::print_to_pdf` returns `BackendUnsupported`, confirming the two-backend architecture holds and no Phase 2 code depends on PDF rendering.
6. At least four ATS adapters fetch and normalize real job listings.
7. HTML extraction handles JSON-LD and CSS-selector-based career pages.
8. JS-required pages are routed to Tier 2, not surfaced as errors.
9. Tier routing follows `render_mode` with unconditional escalation.
10. `fetch_tier` is recorded per listing; `last_verified_at` is updated on success.
11. Duplicate listings are deduplicated via `job_descriptions::upsert`.
12. Portals UI supports CRUD, fetch progress, health drill-down, and sidecar status.
13. Manual job entry works and deduplicates against portal-fetched entries.
14. A fallback build using `rust-headless-chrome` or `chromiumoxide` passes the same scraping tests, proving the trait abstraction and the sidecar-model portability.
15. CI is green on all platforms.
16. **Verification pass complete.** Each exit criterion above has been checked against the code, with a note on how it is verified and whether the verification actually exercises the property. The retrospective records any criterion that passed for the wrong reason and how it was fixed.
17. A retrospective memo records Obscura sidecar integration findings, whether the fallback was needed, and what the verification pass found.

---

## What Phase 2 Is Not

Phase 2 acquires job postings. It does not evaluate them, score them, generate cover letters, or produce PDFs. Specifically:

- There is no AI evaluation, no `rig` integration, no match scoring.
- There is no cover letter generation.
- There is no PDF rendering. `print_to_pdf` returns `BackendUnsupported` for the Obscura backend; PDF rendering is Phase 4.
- There is no prompt seeding or management.
- There is no Obscura embedded-crate integration. The sidecar model is confirmed; the embedded model is a deferred future enhancement.

The browser backend being required does not change the phase boundary. It only changes the acquisition pipeline's internal dependency graph.

---

The single most important thing to take from this plan: **the Obscura sidecar spike in Week 1 is the gate.** The browser is now required, so if Obscura cannot be integrated or cannot scrape the target portals, the entire acquisition strategy needs a fallback. That fallback is cheap because of the `BrowserBackend` trait and the CDP-common sidecar model, but it is not free — it changes the bundle size, the process model, and possibly the rendering fidelity for Phase 4. Discover that in Week 1, not Week 4.

The second most important thing: **the sidecar model is chosen for isolation and portability, not performance.** The IPC cost is negligible at this scale. The value is that a browser crash does not crash the app, and that the same integration pattern works across all three candidate backends. If the sidecar's process-management complexity proves burdensome, the embedded model is documented as a future option — but only after the GPUI thread-placement question is answered, and only if the portability trade-off is judged acceptable.