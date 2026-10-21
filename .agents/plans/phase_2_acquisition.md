# Phase 2: Acquisition — Detailed Execution Plan

**Parent plan:** Draft Execution Plan: Career-Ops Rust Desktop Application (v0.5), §12
**Status:** Draft — Week 1 spike complete; sidecar integration confirmed; Obscura PDF capability verified
**Last updated:** 2026-10-20
**Duration:** 3–4 weeks (Week 1 complete; Weeks 2–4 remain)
**Depends on:** Phase 1 completion (confirmed 2026-10-19)

---

## Change Log

| Version | Change |
| :--- | :--- |
| v0.1 | Initial Phase 2 plan; browser backend feature-gated, `rust-headless-chrome` as default. |
| v0.2 | Browser backend is now a **required** dependency (not feature-gated). Obscura is the chosen backend. `rust-headless-chrome` retained as documented fallback. Tier 1 remains the default path where it works, but is no longer expected to cover the majority of target portals. |
| v0.3 | Revised against Phase 1 outcomes and current ecosystem. Removed the `print_to_pdf` deliverable — Obscura cannot render PDFs, and Phase 1's `BrowserBackend` trait provides a `BackendUnsupported` default. Added evaluation of Obscura's embedded Rust API alongside the sidecar model. Added `chromiumoxide` as a second fallback. Made test-support and mock-backend dependencies explicit. Added toolchain requirement to entry conditions. Added a verification-pass exit criterion, per the Phase 1 retrospective lesson. |
| v0.4 | **Sidecar integration confirmed.** The embedded model is deferred to a future enhancement. Rationale: crash isolation, GPUI event-loop safety, and portability across browser backend swaps (all three candidate backends are CDP-driven). Removed the sidecar-vs-embedded evaluation from the Week 1 spike; Week 1 now focuses on sidecar integration only. Reduced duration from 3–5 weeks to 3–4 weeks. |
| v0.5 | **Corrected the PDF claim.** Obscura v0.2.5, built with the `render` feature, **does** support `Page.printToPDF`; the Week 1 spike confirmed it produces a valid PDF (29305 bytes, `%PDF` magic) from `chromiumoxide::Page::pdf`. The v0.3 assertion that Obscura "cannot render PDFs" was based on the architecture doc and was wrong. Phase 2 **still does not implement `print_to_pdf`** on the Obscura backend — but now because PDF rendering is a Phase 4 concern, not because the capability is absent. Phase 4's first task is therefore to evaluate Obscura's PDF fidelity against the real resume templates, and only then decide between Obscura and `rust-headless-chrome` as the `PdfBackend`. Also folded the Week 1 spike results into Workstream 1, and clarified the final sentence of the Purpose section. |
| v0.6 | Added `portals.last_fetch_attempt_at` and `portals.last_failure_cause` (migration `0002`). The router now persists a classified failure cause on every failed fetch, and both columns are cleared/updated on success. `portals::record_fetch_attempt` supersedes `mark_verified` as the single entry point for recording fetch outcomes. Workstream 4's "Failure causes are classified and stored" criterion split into two: classification (unit-testable) and persistence (integration-testable). Workstream 5's health drill-down now has a schema field to display. |

---

## Purpose

Phase 2 makes the application **useful for the first time**. It implements the tiered job acquisition pipeline: Tier 1 (HTTP/JSON) handles portals that return complete content without JavaScript, and Tier 2 (headless browser via Obscura) handles the portals that require JS execution or are otherwise unreachable via plain HTTP.

**A material change from the v0.1 plan:** Spike B was not run, but its central question was answered directly by the project owner — **`reqwest` + `scraper` are not sufficient for the target portals.** JavaScript execution is required. This means:

1. The browser backend is **not** a feature-gated escalation. It is a **required dependency** for basic acquisition against the intended targets.
2. The "lean build" story from the main plan §9 does not apply to this deployment. The application ships with a browser backend.
3. Tier 1 is still valuable — it is faster, cheaper, and more robust for the portals it covers — but the expected coverage is lower than the v0.1 plan assumed.

**A second material change in v0.3:** Phase 1 established that `BrowserBackend::print_to_pdf` has a default implementation returning `BackendUnsupported`. Phase 2 therefore **does not implement `print_to_pdf`** on the Obscura backend. PDF rendering is a Phase 4 concern. What changed in v0.5 is the *reason*: as of Obscura v0.2.5 with the `render` feature, the backend **can** produce PDFs (verified during the Week 1 spike), but PDF rendering is out of scope for Phase 2. The trait default remains in place for Phase 2; Phase 4 will decide whether Obscura or `rust-headless-chrome` is the `PdfBackend`.

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
| `BrowserBackend` trait with `print_to_pdf` default | Implemented by the Obscura sidecar backend; `print_to_pdf` inherits the `BackendUnsupported` default in Phase 2 |
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
W1: Obscura sidecar integration (BrowserBackend implementation, CDP client, lifecycle)  ✅ COMPLETE
    └── W2: Tier 1a (ATS JSON) + Tier 1b (HTML extraction)
        └── W3: Tier routing, normalization, storage
            └── W4: Portals Configuration UI, manual entry, integration, verification pass
```

**Obscura integration comes first.** In the v0.1 plan, Tier 1 came first because it was the highest-leverage path. With the browser now required, the browser backend is the load-bearing dependency. If Obscura cannot be integrated, or its scraping fidelity is insufficient, the entire acquisition strategy needs a fallback — and that is better to discover in Week 1 than Week 4.

Tier 1 work follows because it is still valuable: ATS JSON APIs are faster and more reliable than browser-based scraping for the portals it covers, and they reduce the load on the browser process.

**The verification pass is folded into Week 4**, rather than being a separate week. The Phase 1 retrospective's central lesson was that a test passing is not the same as a test testing the right thing. Week 4 includes a check of each exit criterion against the code, rather than assuming the green suite implies completeness. If the verification pass surfaces findings, an additional half-week may be needed; that is budgeted in the duration range.

---

## Workstream 1 — Obscura Sidecar `BrowserBackend` Integration

**Priority:** First. Load-bearing dependency.
**Status:** Week 1 spike complete (2026-10-20). Integration hardening continues in Weeks 2–4.
**Estimated duration:** 1–1.5 weeks

### Week 1 Spike Results

The Week 1 gate has been passed. Findings recorded here so the rest of the plan can be read with the gate behind it rather than ahead of it.

| Question | Answer | Evidence |
| :--- | :--- | :--- |
| Sidecar spawns and binds on an allocated port | ✅ | `obscura serve --port <allocated>`; `TcpStream::connect` succeeds within ~100ms |
| `chromiumoxide` connects and drives a page | ✅ | `Browser::connect("ws://127.0.0.1:<port>")` succeeded; `new_page` and `goto` worked |
| JS-heavy page returns post-JS DOM | ✅ | `page.content()` returned HTML containing `<html`; the page's `<h1>` was present |
| Sidecar death is detected on next connect | ✅ | After `child.kill()`, `Browser::connect` returned `Err`, not a hang |
| `Page.printToPDF` succeeds | ✅ | `page.pdf()` returned 29305 bytes, magic `[37, 80, 68, 70]` (`%PDF`) |
| End-to-end latency per fetch | ~2.8s | Includes sidecar spawn (~100ms), CDP connect, navigation, DOM extraction |

**Data points worth preserving:**

- **Obscura version tested:** v0.2.5, built with the `render` feature.
- **Console noise is expected.** A JS-heavy page produced `VIDEOJS: ERROR` lines from the Obscura console logger. These are the host page's own console errors, surfaced by Obscura; they do not indicate test failure and are exactly the "long-tail Web API" gap the docs warn about. For scraping, they are irrelevant: the DOM is present, `content()` returns it, the test passes.
- **`printToPDF` changes Phase 4.** Obscura is not scraping-only. It can render PDFs. Whether its rendering fidelity is sufficient for ATS-friendly resume templates is a Phase 4 question, not a Phase 2 one, but the two-backend assumption in the main plan needs revisiting when Phase 4 begins.

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

    // print_to_pdf is deliberately NOT overridden in Phase 2.
    // The trait default returns BackendUnsupported. Phase 4 will
    // decide whether Obscura's printToPDF or rust-headless-chrome
    // provides the PdfBackend implementation.
}

fn name(&self) -> &'static str { "obscura-sidecar" }
```

2. **Sidecar lifecycle management.** This is the load-bearing complexity of the workstream.

   - **Spawn on first request.** The sidecar is not started at application launch; it starts on the first acquisition request that needs Tier 2. This keeps startup fast and avoids a browser process running for users who only use Tier 1.
   - **Port allocation.** Spawn `obscura serve` on a free port, not a fixed port. Allocate in Rust via `TcpListener::bind(("127.0.0.1", 0))`, drop the listener, and pass the port to the child. A fixed port causes collisions when two copies of the app run, or when a stale process is still bound.
   - **Reuse within a session.** Subsequent fetches use the same sidecar. Do not spawn per-fetch.
   - **Health check.** Before each fetch, ping the CDP endpoint. If the ping fails, the sidecar has died; restart it and retry the fetch once. A second failure surfaces the error to the caller.
   - **Graceful shutdown.** On `AppState` drop or application exit, send SIGTERM to the child and wait for it to exit with a timeout. On timeout, SIGKILL. `kill_on_drop(true)` on the `tokio::process::Command` is the backstop for hard crashes. Orphaned sidecars are a resource leak, and a stale browser process holding a locked profile directory will block the next launch.
   - **Stale process detection on startup.** On first spawn, check for a lock file or port reservation from a previous run. If found, attempt a graceful shutdown before spawning. This handles the "app crashed with a sidecar running" case.

3. **CDP client integration.** Use `chromiumoxide` as the CDP client. It is more actively maintained than `rust-headless-chrome`, async-first, tokio-native, and its `Page::goto`, `Page::content`, and `Page::evaluate` map cleanly to the `fetch_rendered` semantics. The Week 1 spike confirmed it drives Obscura without modification.

   Note: the sidecar model means the CDP client choice is **independent** of the browser choice. If Obscura is replaced by `rust-headless-chrome`'s subprocess or `chromiumoxide`'s own Chrome spawn, the client stays the same. This is a second portability benefit beyond the browser swap.

4. **No `print_to_pdf` implementation in Phase 2.** Deliberate:

   - The `BrowserBackend` trait's default `print_to_pdf` returns `CoreError::BackendUnsupported`.
   - `ObscuraBrowserBackend` inherits that default and must not override it **in Phase 2**.
   - Obscura's `printToPDF` works (verified), but PDF rendering is Phase 4's concern.
   - Phase 4 will decide the `PdfBackend` implementation: either wrap `ObscuraBrowserBackend::print_to_pdf` (which requires overriding the default then), or use `rust-headless-chrome` as a separate `PdfBackend`.
   - The Phase 2 test asserting `BackendUnsupported` is correct for Phase 2 and will need to change in Phase 4. That change is expected and documented.

5. **Fallback path.** If Obscura integration fails or its scraping fidelity proves insufficient against real target portals:

   - **First fallback:** `rust-headless-chrome`. Mature and complete, but less actively maintained than it once was.
   - **Second fallback:** `chromiumoxide` with its own Chrome spawn. More actively maintained.
   - Because the backend is a sidecar, the fallback is a trait swap plus a process-spawn change. The CDP client can stay the same in either case.
   - Document the reason for the switch and record the maintenance status of both crates as of the switch date.

### Deferred: Embedded Integration Model

Obscura also provides an embeddable Rust crate (`obscura`) that runs V8 in-process, without a sidecar process. This is **not** part of Phase 2. Recorded here so the option is not lost.

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

1. **Use `chromiumoxide` with `default-features = false` and `features = ["tokio-runtime"]`.** The default is `async-std`; leaving it on produces a hang where every command silently never returns. The Week 1 spike confirmed the tokio-runtime variant works with Obscura.
2. **Do not rely on `wait_for_navigation()`.** Obscura's default lifecycle for CDP navigation fires at `domcontentloaded`, which is before many portals populate their listings. Poll a selector with `page.evaluate()` instead. This is documented behaviour, not a bug.
3. **Wrap all browser calls in `spawn_blocking` or a tokio task.** CDP is async I/O; the sidecar's stdout/stderr and process-wait must not be polled on the GPUI main thread. Use `gpui-tokio-bridge` for the async client, and keep the process handle in a dedicated worker or tokio task.
4. **Distribution.** The `obscura` binary must be bundled with the application or downloaded on first use. Bundling is simpler but grows the artifact; download-on-first-use requires a trust story for the download source. The Week 1 spike used a vendored binary; the Phase 6 packaging plan should record which distribution model was chosen.
5. **The `canAccessOpener` CDP bug was fixed in Obscura PR #165.** If `Browser::connect` panics with `"Created target not present"`, the installed binary predates the fix. Upgrade the binary; do not change the Rust code.
6. **Fallback is not a failure.** Choosing `rust-headless-chrome` or `chromiumoxide` after evaluating Obscura is a legitimate outcome. The trait exists precisely to make that switch cheap.

### Acceptance Criteria

| Criterion | Verification |
| :--- | :--- |
| Sidecar starts on first request and accepts a fetch | Integration test |
| `fetch_rendered` returns post-JS DOM for a JS-heavy fixture | Integration test |
| `print_to_pdf` returns `BackendUnsupported` **in Phase 2** | Unit test |
| Sidecar uses an allocated port, not a fixed one | Unit test with two concurrent instances |
| Sidecar is reused across multiple fetches in a session | Integration test (process count assertion) |
| Sidecar death is detected and recovered on the next request | Fault injection test (kill child, retry) |
| Stale sidecar from a previous run is detected and cleaned up | Fault injection test (leave a fake lock file) |
| Sidecar is shut down cleanly on application exit | Integration test (assert child exit) |
| GPUI event loop is not blocked during fetch | UI responsiveness test |
| Fallback to `rust-headless-chrome` or `chromiumoxide` compiles and passes the same scraping tests | Compile + test with alternate backend |

---

## Workstream 2 — Tier 1a: ATS JSON API Adapters

**Priority:** After the browser backend is functional. Week 1 spike confirmed it is.
**Estimated duration:** 1 week

### Deliverables

1. `core::acquisition::ats` module with the `AtsAdapter` trait
2. Concrete adapters for Greenhouse, Lever, Ashby, SmartRecruiters
3. Auto-detection from portal URL
4. Normalization into `NormalizedJob`
5. Retry on transient failure, implemented as a shared helper in
   `acquisition/http.rs` (`get_with_retry` / `send_with_retry`). Wired
   into the adapters in Week 3, when the router owns the fetch loop.

   **Rate limiting is dropped.** A desktop application fetches one
   portal at a time on user action and never runs a background crawl.
   There is no scenario where a client-side token bucket or configurable
   inter-request delay does anything useful. The retry helper provides
   the only property that matters: a transient 429/503/timeout does not
   fail the whole fetch.

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
3. **Failure recording**, classified and persisted. `AtsError` and `FailureCause` provide the classification; `portals.last_fetch_attempt_at` and `portals.last_failure_cause` provide the storage. Every fetch attempt — success or failure — updates both columns. `portals.last_verified_at` continues to record only successful fetches.
4. **`fetch_tier` recording** per job listing, using the existing `FetchTier` enum values: `tier1a:ats`, `tier1b:html`, `tier2:obscura`, `manual`
5. **`last_verified_at` update** per portal on success
6. **`render_mode` suggestion logic** (optional): if Tier 1 fails with an escalatable cause and Tier 2 succeeds in the same fetch, suggest changing `render_mode` to `browser`. Persistent-failure suggestions require fetch history, which is not stored; see the change log note on `last_fetch_attempt_at`

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
| Failure causes are classified | Unit tests on `AtsError::cause` |
| Failure causes are persisted to `portals.last_failure_cause` | Integration test (`failed_fetch_records_classified_cause`) |
| `last_verified_at` is untouched on failure | Integration test (same test) |

---

## Workstream 5 — Portals Configuration UI

**Priority:** After routing works.
**Estimated duration:** 1 week

### Deliverables

1. **Portal list** showing health status, `render_mode`, and the tier that succeeded or failed last time
2. **Add/edit form** with `render_mode` selector
3. **Fetch progress** per portal
4. **Health drill-down** with classified failure causes and suggested actions
   The drill-down displays `last_fetch_attempt_at` (relative time), `last_verified_at` (if different), and `last_failure_cause` when present. Suggested actions map to causes: js_required → "switch to browser mode or enable the browser backend"; not_found → "check the board slug"; blocked → "the site may be blocking automated requests"; timeout → "retry, or check network connectivity".
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
- A small number of environment-gated tests exercise the real Obscura sidecar. The `obscura_spike` test (Week 1) is the model: it uses `OBSCURA_BIN` for the binary path, falls back to `vendor/obscura` resolved via `env!("CARGO_MANIFEST_DIR")`, and passes both lifecycle and DOM extraction assertions.
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
8. Confirm `ObscuraBrowserBackend::print_to_pdf` returns `BackendUnsupported` **in Phase 2**, so no Phase 2 code path accidentally depends on it.

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
| P2-R1 | **Obscura integration proves harder than expected.** Its CDP surface is incomplete, or its scraping fidelity is insufficient. | High | **Week 1 spike confirmed CDP compatibility** — this risk is downgraded from High to Low. If scraping fidelity fails on real portals, first fallback is `rust-headless-chrome`; second is `chromiumoxide` with its own Chrome spawn. |
| P2-R2 | **Obscura cannot render PDFs.** | Resolved | **False.** The Week 1 spike confirmed `Page.printToPDF` works (29305 bytes, valid `%PDF`). Phase 2 still does not implement `print_to_pdf` because PDF rendering is Phase 4's concern. Phase 4 will evaluate Obscura's fidelity against the real resume templates and decide between Obscura and `rust-headless-chrome` as the `PdfBackend`. |
| P2-R3 | **Sidecar process management introduces complexity.** Port collisions, orphaned processes, stale lock files, resource leaks. | Medium | Allocate ports dynamically; detect stale sidecars on startup; SIGTERM-then-SIGKILL shutdown with timeout; `kill_on_drop(true)` as backstop; integration test the crash and stale-process paths. |
| P2-R4 | **Tier 1 is less valuable than expected.** If ATS JSON APIs do not cover many targets, the browser does most of the work. | Low | Accepted. Tier 1 is an optimization, not the primary path. |
| P2-R5 | **Portal configuration UI scope creep.** | Medium | Ship the minimum: list, add, edit, delete, fetch, health. Advanced features are Phase 5. |
| P2-R6 | **Test passes for the wrong reason.** Phase 1 found two such cases during verification. | Medium | Fold a verification pass into Week 4. For each exit criterion, confirm the test actually exercises the property, not a proxy. |
| P2-R7 | **A future backend swap invalidates the sidecar assumptions.** | Low | All three candidate backends are CDP-driven subprocesses. If a future backend is embedded-only, the embedded model (already documented as deferred) becomes relevant again. |
| P2-R8 | **Console noise from target pages obscures real errors.** Week 1 spike saw `VIDEOJS` errors from the host page's own JavaScript. | Low | Filter console output by level in production; log only warnings and errors emitted by the acquisition layer itself. The host page's console errors are not failures. |

---

## Weekly Budget

| Week | Focus | Deliverable |
| :--- | :--- | :--- |
| **1** | Obscura sidecar spike: CDP surface verification, `BrowserBackend` implementation, process lifecycle | ✅ **Complete.** Sidecar fetches a JS-heavy fixture and returns rendered HTML; lifecycle assertions pass; `printToPDF` verified. |
| **2** | Tier 1a ATS adapters, Tier 1b HTML extraction | ATS JSON and static HTML extraction work |
| **3** | Tier routing, deduplication, `fetch_tier` recording, failure classification | End-to-end: portal → route → fetch → store |
| **4** | Portals Configuration UI, manual entry, integration, verification pass, retrospective | Portal CRUD, health display, manual entry, sidecar recovery verified, each exit criterion checked against the code |

**Total:** 3–4 weeks for Weeks 2–4, plus the completed Week 1. The lower end assumes Tier 1a and 1b integrate cleanly and the verification pass is uneventful. The upper end accounts for a fallback switch or verification findings.

---

## Exit Criteria

Phase 2 is complete when **all** of the following are true:

1. ✅ **Complete (Week 1).** The Obscura sidecar fetches JS-heavy pages and returns post-JS DOM.
2. ✅ **Complete (Week 1).** The sidecar starts lazily on first request, uses an allocated port, and recovers after a crash.
3. Stale sidecars from previous runs are detected and cleaned up on startup.
4. The sidecar is shut down cleanly on application exit.
5. `ObscuraBrowserBackend::print_to_pdf` returns `BackendUnsupported` **in Phase 2**, confirming no Phase 2 code path depends on PDF rendering. Note: Obscura *can* render PDFs (verified); the `BackendUnsupported` default is a Phase 2 scoping decision, not a capability limitation. Phase 4 revisits this.
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
- There is no PDF rendering in the application, even though Obscura's `printToPDF` works. `print_to_pdf` returns `BackendUnsupported` in Phase 2; PDF rendering is Phase 4.
- There is no prompt seeding or management.
- There is no Obscura embedded-crate integration. The sidecar model is confirmed; the embedded model is a deferred future enhancement.

The browser backend being required does not change the phase boundary. It only changes the acquisition pipeline's internal dependency graph.

---

The single most important thing to take from this plan: **the Week 1 spike is complete and the gate is open.** The Obscura sidecar drives cleanly over CDP from `chromiumoxide`, returns post-JS DOM, detects sidecar death, and — unexpectedly — produces valid PDFs. That last finding changes Phase 4's scope but not Phase 2's: PDF rendering stays out of Phase 2, and Phase 4 becomes an evaluation of which backend produces the better resume output rather than an implementation of the only one that can.

The second most important thing: **do not implement `print_to_pdf` on the Obscura backend in Phase 2.** It works, and that is exactly why it is tempting. But adding it to Phase 2 would pull Phase 4's design decisions forward without the fidelity testing that justifies them. The `BackendUnsupported` default holds until Phase 4 has rendered a real resume template through both candidate backends and compared the results.
