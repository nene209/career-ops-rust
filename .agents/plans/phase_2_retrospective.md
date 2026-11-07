# Phase 2 Retrospective

**Phase:** Acquisition
**Parent plan:** Draft Execution Plan: Career-Ops Rust Desktop Application (v0.5), §12
**Phase plan:** `docs/phase-2-acquisition.md`
**Period:** 2026-10-19 → 2026-10-23
**Estimate:** 3–4 weeks
**Actual:** 4–5 days of focused work, plus one substantive verification pass
**Status:** Complete against all seventeen exit criteria, with one item explicitly deferred (real-page HTML extraction, criterion 7's real-world half) and one feature-gated rather than default (the chromiumoxide fallback).

---

## Summary

Phase 2 is complete. The application can now acquire job postings from four ATS platforms and from arbitrary career pages, with a headless browser for the pages neither path can reach. The service layer that a Portals view will eventually call is implemented and tested. The `ObscuraBrowserBackend` — the production backend that drives the sidecar — exists.

The phase did **not** go according to the plan's shape. Two things happened that are worth examining:

1. **The Week 1 spike passed but did not deliver the production backend.** The spike proved the technology worked — CDP connection, JS execution, DOM extraction, even PDF rendering, which the plan did not expect. But it was a *spike*: a test file that spawned `obscura serve` directly with `tokio::process::Command`. The `ObscuraBrowserBackend` that the router needs — with lifecycle management, stale-lock cleanup, and graceful shutdown — was not written. The verification pass caught this. If it had not, Phase 2 would have shipped with a router that never escalates, because `browser: Option<&dyn BrowserBackend>` would always be `None`.

2. **The plan was revised eight times (v0.1 → v0.8).** Some of these were exploratory (the sidecar-vs-embedded decision, the printToPDF correction, the schema addition for failure persistence). Some were clarifying scope (the service/view split, the `BatchEntry` widening). Some were removing assumptions the implementation disproved. Eight revisions in one phase is a lot, and it deserves an honest accounting.

The phase finished **faster than estimated in wall-clock time**, but the estimate of 3–4 weeks assumed a shape — sequential workstreams with a verification pass at the end — that did not match what happened. The eight revisions are the actual cost driver, not the implementation.

---

## Exit criteria status

| # | Criterion | Status | Evidence |
| :--- | :--- | :--- | :--- |
| 1 | Obscura sidecar fetches JS-heavy pages, returns post-JS DOM | ✅ Complete | `obscura_returns_post_js_dom` — asserts marker injected via `evaluate` appears in `content()` |
| 2 | Sidecar starts lazily, allocates port, recovers after crash | ✅ Complete | `ObscuraBrowserBackend` with `ensure_started` and health check via `child.try_wait()` |
| 3 | Stale sidecars detected and cleaned up | ✅ Complete | `cleanup_stale_lock` + `obscura_backend_cleans_stale_lock` |
| 4 | Sidecar shut down cleanly on application exit | ✅ Complete | `graceful_shutdown` (SIGTERM → wait → SIGKILL) + `obscura_backend_graceful_shutdown` |
| 5 | `print_to_pdf` returns `BackendUnsupported` in Phase 2 | ✅ Complete | `obscura_backend_print_to_pdf_is_unsupported` |
| 6 | Four ATS adapters fetch and normalize real job listings | ✅ Complete | Live tests: Figma 141, Spotify 81, Linear 27, Ubisoft2 329 |
| 7 | HTML extraction handles JSON-LD and CSS-selector pages | ⚠️ Partial | Fixture-level verified; real-page verification not performed |
| 8 | JS-required pages routed to Tier 2, not surfaced as errors | ✅ Complete | `js_required_escalates_to_tier2` |
| 9 | Tier routing follows `render_mode` with unconditional escalation | ✅ Complete | Five tests covering each route decision |
| 10 | `fetch_tier` recorded; `last_verified_at` on success; `last_failure_cause` on failure | ✅ Complete | `static_portal_with_ats_url_records_tier1a`, `failed_fetch_records_classified_cause` |
| 11 | Duplicates deduplicated via `upsert` | ✅ Complete | `fetching_same_portal_twice_dedups`, `url_fetch_dedups_against_prior_portal_fetch` |
| 12 | Portals service layer: list, fetch, batch, suggested actions | ✅ Complete | 7 `health::tests` + 6 `portals_service` tests |
| 13 | Manual entry: URL fetch, paste, dedup both paths | ✅ Complete | 9 `manual_entry` tests |
| 14 | Fallback backend (chromiumoxide) passes same scraping tests | ✅ Complete, live env-gated | `ChromiumoxideBrowserBackend` behind `--features chromium-fallback`; compile-check in CI; integrated lifecycle test covers reuse, pdf-then-fetch, and shutdown-release. Live tests run when `RUN_CHROMIUM_FALLBACK_TESTS=1`. |
| 15 | CI green, including `portals-service` job | ✅ Complete | New `portals-service` and `browser-fallback` jobs |
| 16 | Verification pass complete | ✅ Complete | This document's per-criterion table is the record |
| 17 | Retrospective memo | ✅ Complete | This document |

Criterion 14 is marked "complete" rather than "partial" because the trait abstraction *is* proven: the fallback backend implements `BrowserBackend`, compiles under a feature flag, and CI runs a compile-check. The live test that drives an actual Chrome instance is environment-gated and runs only when `RUN_CHROMIUM_FALLBACK_TESTS=1`. That is the same arrangement as the live ATS tests, and it is honest about what is verified in CI versus what is verified locally.

---

## What went well

### The service/view split saved the phase

The single most consequential decision in Phase 2 was the Week 4 restructure (v0.7): deliver the service layer, defer the view layer to Phase 5. It came from an honest assessment — GPUI is pre-1.0, none of the view files exist, and building views against an unstable API means paying twice.

The consequence: Workstreams 5 and 6 delivered code that a test can exercise without a UI framework, which means their behaviour is verified rather than asserted. The `PortalHealth::from_portal` function is the clearest example — it is a pure function over a `Portal` row, so every health state can be exercised with a constructed row and nothing else. When Phase 5 builds the Portals screen, it will call a function that already works.

Without the split, Workstream 5 would have been a partially-finished view with the same tests skipped behind a `#[ignore]` because GPUI would not compile in CI.

### Real-world data caught real bugs

Three findings during Week 2 came from exercising the ATS adapters against real boards, and all three would have been missed by fixtures written from the documentation:

- **Ubisoft's SmartRecruiters slug is `Ubisoft2`, not `Ubisoft`.** The live test failed with "zero jobs", which is what SmartRecruiters returns for a slug it does not recognise. Any fixture named after the company would have used the wrong slug and passed.
- **The CSS-selector URL fallback did not handle the common shape.** The fixture in the first draft had `<h2 class="title"><a href="...">Title</a></h2>` — the title element is an `<h2>`, and the anchor is inside it. `select_attr(card, title_selector, "href")` looked at the `<h2>`, which has no `href`. Three tests failed, correctly, and the fix was to add a third resolution path.
- **Greenhouse `?content=true` returns double-encoded HTML** — `&lt;p&gt;We are hiring&lt;/p&gt;` rather than `<p>We are hiring</p>`. Without the unescape step, downstream extraction sees entity soup.

In each case, the fixture was written to match what the real API actually returns, which required exercising the real API. That is the value of the live tests.

### The `AtsError` / `CoreError` distinction earned its keep

The decision to introduce `AtsError` in Week 3 rather than continue with `CoreError::Internal(&'static str)` for adapter failures forced a semantic clarification. `Internal` was being used for two different things:

- Invariant violations ("the transition table has a row that should not exist")
- Dynamic failures ("the HTTP request timed out with message X")

Splitting them produced `AtsError` for the second category, and later `CoreError::Message(String)` for the same reason when manual entry hit the same problem. Both splits were mechanical once the distinction was articulated, and both removed a class of clippy warnings (`.into()` on a `&str` parameter that was already `&str`).

The downstream consequence is that `FailureCause` — the coarse classification stored in the database — maps cleanly from `AtsError`, because `AtsError` is designed to be classified. If adapter failures had stayed as `CoreError`, `FailureCause::from_error` would have had to pattern-match on a dozen stringified messages.

### `FetchSource` made the router testable

The router depends on `FetchSource`, not `reqwest::Client`. This was deliberate — the routing decisions (Tier 1a vs 1b vs 2, escalation, failure classification) are the parts worth testing, and they have nothing to do with HTTP. The `MockFetchSource` in `tests/common/mod.rs` lets `fetch_portal` be exercised in 30 milliseconds with scripted responses.

Without that abstraction, every router test would have needed a mock HTTP server or a live endpoint, and the tests would have been slower and flakier. The `sequence_ats` and `sequence_html` methods on the mock — which popped results in call order — made multi-attempt tests (batch fetch, escalation after a failure) straightforward.

---

## What the verification pass found

This is the section that matters most, because it is where Phase 2 diverged from its plan.

### F1 — The production backend did not exist (high severity)

The plan's Week 1 deliverable was "`BrowserBackend` implementation, sidecar lifecycle." The spike delivered neither. It delivered a test that spawned the sidecar directly and connected via `chromiumoxide::Browser::connect`. That proved:

- Sidecar can be spawned on an allocated port
- CDP connection works
- JS executes and `content()` returns post-JS DOM
- `printToPDF` works (surprisingly)

What it did not prove, and did not deliver:

- `ObscuraBrowserBackend` — the type the router takes as `Option<&dyn BrowserBackend>`
- Lazy spawn on first request
- Reuse across fetches
- Health check via `child.try_wait()`
- Stale-lock cleanup
- Graceful shutdown

The router tests all passed because they used `MockBrowserBackend`. The service layer tests all passed because they used `MockFetchSource` and passed `None` for `browser`. Every test that was actually run against the real product was a test where the browser was a mock.

The consequence, if the verification pass had not caught it: the shipped application would have had `browser: None` at every call site, because no code would have constructed a real backend. Every Tier 1 failure would have been returned as-is; escalation would never happen; the application would have been able to scrape only the portals the ATS adapters and static HTML extraction could handle, which is precisely the subset the plan said was insufficient.

The fix took one day: `ObscuraBrowserBackend` with lifecycle management, followed by the three F3 tests (`print_to_pdf` returns `Unsupported`, stale lock cleanup, graceful shutdown). The underlying technology worked — the spike proved that. The production structure around it did not exist.

### F2 — The DOM test did not prove JavaScript executed (medium severity)

The original `obscura_returns_post_js_dom` navigated to a URL and asserted `page.content()` contained `<html`. That assertion would pass against a page whose HTML was served statically, no JavaScript required. It proved the CDP connection returned *something*, not that JS ran and mutated the DOM.

The fix was to inject an attribute via `page.evaluate()` and then read it back via `content()`:

```rust
page.evaluate("document.body.setAttribute('data-obscura-marker', '...')").await?;
let html = page.content().await?;
assert!(html.contains(&marker));
```

That exercises the full round-trip: V8 executes, the DOM mutates, `content()` reflects the mutation. A page whose HTML happens to contain `<html` would fail this test, correctly.

This is the same failure mode as the Phase 1 retrospective's "test that passed for the wrong reason." It is worth recording that it happened again, in a different form, in a different phase. The lesson is not "be more careful" — it is that *assertions should be as specific as the property they claim to verify*, and a general assertion is often a check of the harness rather than the behaviour.

### F4 — The fallback backend was not implemented (medium severity)

Criterion 14 said "a fallback build using `rust-headless-chrome` or
`chromiumoxide` passes the same scraping tests, proving the trait
abstraction and sidecar-model portability." No such build existed. The
trait was designed to make the fallback cheap, but nothing demonstrated
that it *was* cheap.

The fix was `ChromiumoxideBrowserBackend` behind
`--features chromium-fallback`, with a compile-check job in CI. The
integration was, in fact, cheap: a new `BrowserBackend` impl, a
`ChromiumConfig`, and a re-export. The router and service layer were
untouched. That is the trait abstraction working as designed — but the
point is that it had not been *tested* until the fallback was written,
and the cost of the fallback was an estimate, not a measurement.

The fix cycle found two issues that would have made the fallback
unreliable in production:

1. **Profile singleton lock collision.** The first version of the
   fallback tests ran `chromium_backend_fetches_rendered_dom` and
   `chromium_backend_produces_pdf` in parallel threads. Both used
   `chromiumoxide`'s default profile directory, and Chrome aborts with
   `RESULT_CODE_PROFILE_IN_USE` when two instances try to share one
   profile. The fix was to add `user_data_dir: Option<PathBuf>` to
   `ChromiumConfig` and give each test its own `tempfile::TempDir`. The
   failure was not a bug in the backend; it was a footgun in the config
   default that would have bitten anyone instantiating more than one
   backend in the same process.

2. **Missing re-exports.** `ChromiumoxideBrowserBackend` and
   `ChromiumConfig` are defined in `backend::chromium` but needed to be
   re-exported through `backend` for tests and downstream code to reach
   them. Two compile failures in a row, one per type. This is the
   pattern lesson 5 names — the `dyn Trait` abstraction is fine, but the
   concrete types around it are easy to under-export because tests that
   mock the trait do not exercise the concrete path.

The final state is more thorough than the criterion required. Three
tests cover the pieces (`implements_trait`, `fetches_rendered_dom`,
`produces_pdf`), and a fourth — `chromium_backend_integrated_lifecycle`
— covers the properties the per-operation tests do not:

- **State reuse within one backend.** A `fetch_rendered` →
  `print_to_pdf` → `fetch_rendered` sequence on a single backend goes
  through the same `ensure_started` guard. If reuse were broken, the
  second operation would attempt to launch a second Chrome on the same
  profile and abort with the profile-lock error the fix cycle had just
  fixed.
- **PDF render does not corrupt subsequent fetches.** The second fetch
  must return non-empty HTML, not just succeed.
- **Shutdown releases the profile.** A new backend constructed on the
  same profile directory after `shutdown()` must be able to launch.
- **The trait object is the production shape.** The test holds the
  backend as `Box<dyn BrowserBackend>`, which is what the router would
  use.

The integrated test has a retry in its post-shutdown phase. That is
deliberate and honest: `Browser.close()` sends the CDP command and
awaits the response, but does not wait for the Chrome process to exit,
and the profile lock is released slightly after the process exits. The
retry asserts "the profile *becomes* reusable", not "the profile *is*
reusable at the exact instant `shutdown()` returned." The 15-second
timeout is well above any realistic cleanup window; a failure after that
timeout is a genuine shutdown bug, not a race.

What remains environment-gated is live execution, not coverage. The
`browser-fallback` CI job runs `cargo check --features
chromium-fallback` unconditionally, which proves the trait abstraction
compiles under the feature flag. The live tests run only when
`RUN_CHROMIUM_FALLBACK_TESTS=1` is set, because they require a Chrome
installation on the runner. That is the same arrangement as the live
ATS tests, and it is honest about what is verified in CI versus what is
verified locally. The criterion's "passes the same scraping tests" is
true locally and in any CI that opts into the live tests; it is not true
of the default CI job, and the plan should say so explicitly rather than
reading as if the live tests run everywhere.

### F5 and F6 — Test and CI gaps (low severity)

`fetching_same_portal_twice_dedups` was missing despite being in the plan's integration test list. `portals-service` was a named CI job in the plan but not in `ci.yml`. Both were fixed. Neither changed behaviour; both would have been small annoyances the next time someone tried to debug acquisition.

### F7 — Real-page HTML extraction not tested (informational)

Criterion 7's fixture tests prove the extraction chain works on curated HTML. They do not prove it works on any real career page. The live ATS tests do not cover this, because they go through the ATS JSON path, not the HTML path. A live HTML test would require picking a stable non-ATS career page and betting on its markup staying similar — which is exactly the kind of test that gets flaky and then gets `#[ignore]`d. Fixture-level verification is the right level for this parser, but the plan should say so explicitly rather than leave the criterion reading as if it covers real pages.

---

## What was harder than expected

### The `shutdown` deadlock

`ObscuraBrowserBackend::shutdown` initially hung. The sequence was:

1. `graceful_shutdown(&mut child)` sends SIGTERM, waits, and returns when the child exits.
2. `state.browser.close().await` sends a `Browser.close` CDP command over a WebSocket whose peer is now dead, and awaits a response that never arrives.

The fix was two-fold:

- Abort the handler task **before** killing the child. The handler is the task that dispatches CDP commands; aborting it removes the source of the deadlock.
- `drop(state.browser)` rather than `state.browser.close().await`. Dropping is synchronous and correct; closing sends a command to a dead peer.

This is not an obvious pattern, and it is worth recording because anyone editing `shutdown` later will be tempted to add `.close().await` back "for correctness." The comment in the code that explains why not is the most important part of the fix.

### Platform-specific paths

The Windows CI failure (`os error 3`, "system cannot find the path specified") was because `$(pwd)` in Git Bash returns a POSIX path (`/d/a/repo/repo`), and `tokio::process::Command` on Windows cannot use POSIX paths. The fix was `cygpath -w` before exporting `OBSCURA_BIN`.

This is a class of bug that only appears when a value crosses from bash to a Rust process. It is worth noting because the same pattern will recur whenever a test or build script reads a path from an environment variable on Windows.

### The eight plan revisions

Each revision was defensible. The printToPDF correction (v0.5) was based on evidence the spike produced. The sidecar decision (v0.4) was based on the portability argument. The failure persistence addition (v0.6) was because the plan's "classify failure causes" deliverable had no storage for the classification. The service/view split (v0.7) was because the GPUI constraint made the original Week 4 deliverable unrealisable in the phase's time budget.

But eight revisions in one phase is a signal. The plan's initial shape assumed more certainty about what would be discovered than was warranted. A reader could reasonably ask: if the plan was going to change this much, what was it for?

The honest answer is that the plan provided the checklist — the criteria that the verification pass ultimately walked — and the revisions were the cost of keeping the checklist accurate. That is the useful function of the document. But the length of the change log is itself a finding, and Phase 3's plan should be written with the expectation that it will be revised at least three times, and should state which decisions are "settled by this plan" versus "to be settled during the phase."

---

## Deviations from the plan

| Planned | Actual | Impact |
| :--- | :--- | :--- |
| Week 1: `BrowserBackend` implementation | Week 1: spike only; production backend in verification pass | F1 — the single largest finding |
| `print_to_pdf` verified as absent | `Page.printToPDF` verified as working | Phase 4's scope changed; `printpdf` deferred indefinitely |
| Sidecar-vs-embedded evaluation in Week 1 | Decision made before Week 1 based on portability argument | Correct; the argument holds |
| Failure causes classified and logged | Failure causes classified, logged, **and persisted** to new columns | Schema addition `0002` |
| Workstreams 5 and 6 deliver UIs | Workstreams 5 and 6 deliver service layers | Correct; view layer deferred to Phase 5 |
| `BatchEntry.result` is `Result<usize, FailureCause>` | Widened to `Result<usize, FetchError>` | Detail available for drill-down without second fetch |
| `portals-service` CI job | Added during verification pass | Small delay; no functional impact |
| Eight plan versions (v0.1–v0.8) | — | Change log is long; the revisions are honest but they add up |

---

## Open items carried into Phase 3

1. **Real-page HTML extraction (F7).** The fixture tests prove the parser is correct on its inputs. Whether it works on a given real career page depends on that page's markup, and that is not tested. This is noted in the plan as informational; it is not a Phase 2 blocker, and it should not become one.

2. **Chromiumoxide fallback (F4).** The backend exists and compiles under a feature flag, and CI does a compile-check. The live test that drives Chrome is environment-gated. Phase 3 does not depend on this backend; if Obscura proves unsuitable during Phase 3, this is the swap target, and the migration path is documented.

3. **`ObscuraConfig::default()` uses `PathBuf::from("obscura")`.** In a shipped application, the binary should be bundled or looked up from a known location. The current default works for development (with `OBSCURA_BIN` set) and fails clearly otherwise. Phase 6 (packaging) is where this should be resolved.

4. **The `shutdown` pattern.** The `drop(browser)` rather than `close().await` sequence is correct but non-obvious. If a future maintainer changes it, the deadlock returns. The comment in the code is the only safeguard; a test for it would be more robust but is harder to write (`shutdown` succeeding is a weak assertion). Worth considering whether the comment is sufficient.

---

## Lessons for Phase 3

1. **A spike is not a production artifact.** Phase 2's Week 1 gate passed, and the phase proceeded — but the gate was a proof of technology, not a delivery of the thing the plan said would be delivered. The distinction between "verified" and "delivered" needs to be explicit in the plan, because "the spike passed" reads like "this workstream is done" and it is not.

2. **Ask what a test proves, not whether it passes.** F2 was a test that asserted a page contained `<html`. It passed on any page. The question "what would this assertion fail on?" would have caught it immediately. Phase 3 should apply that question to every new test, especially ones that verify "an API call succeeded" or "an error was returned."

3. **The verification pass is where self-report fails.** F1 was an exit criterion marked complete at the spike level. F4 was an exit criterion marked complete at the trait level. F5 and F6 were plan items with no corresponding code. None of these would have been caught by "the test suite is green." The verification pass is the moment where the checklist is checked against the code, and Phase 3 should schedule it explicitly — not as a ceremonial step, but as the phase's most important output.

4. **Fixture content is a design decision.** The CSS-selector URL bug existed because the fixture's HTML shape did not match a common real shape. The fix required changing the fixture to reflect reality — and that required knowing what reality looks like. In Phase 3, evaluation will involve prompt fixtures, job descriptions, CV entries, and LLM responses. Each of those has a "what does the real thing look like" question, and each should be answered before the fixture is written.

5. **The `Option<&dyn Trait>` pattern is a footgun if nothing constructs a real implementation.** The router accepted `browser: Option<&dyn BrowserBackend>` and was tested with mocks. It was "testable" and "correct." It also could not be used in production because no code path produced a real `ObscuraBrowserBackend`. Testing with mocks that nothing constructs is a class of test that proves the interface works without proving the implementation works. In Phase 3, `rig` integration should have at least one test that constructs a real client and makes a real call, gated behind an env var, even if the bulk of tests use cassettes.

6. **Two error variants (invariant vs. dynamic) is worth the small extra code.** `CoreError::Internal` and `CoreError::Message`, and `CoreError` vs. `AtsError`, all distinguish "a developer broke something" from "the caller supplied bad data." The distinction made clippy fixes mechanical and made `FailureCause` mapping trivial. Phase 3 will encounter the same decision with LLM errors — `MissingApiKey`, `RateLimited`, `ContextLengthExceeded`, `ProviderError` — and the shape of those variants determines how testable the evaluation pipeline is.

---

## Estimate accuracy

The estimate was 3–4 weeks. Actual was 4–5 days of focused implementation plus one verification pass.

The gap is not a normal estimation error. It is the consequence of a phase that was not performed in the shape the estimate assumed:

- **The estimate assumed sequential workstreams.** In practice, the Week 1 spike and Week 2–3 implementation were interleaved with the plan revisions, and the Week 4 service layer was executed after the verification pass identified what was missing. The "3–4 weeks" assumed a linear path that did not exist.
- **The estimate did not budget for the verification pass.** The Phase 1 retrospective recommended budgeting for it, and Phase 2 did not. The verification pass caught F1, which was not a small fix — it required writing the production backend.
- **The estimate was not validated against real complexity.** Eight plan revisions is a strong signal that the estimate assumed less unknown than the phase had. If the plan had been written with the expectation of eight revisions, the estimate might have been closer to six weeks.

The honest framing: the phase was completed faster than estimated in calendar time because it was compressed into fewer days of more intense work. The estimate would have been more accurate if it had accounted for the verification pass and the plan churn. For Phase 3, budget for:
- The implementation work at the plan's estimate
- The verification pass at 20–30% of the implementation estimate
- One revision of the plan per significant discovery

Phase 3's plan should say what the expected number of revisions is, so the estimate is not measured against a number that assumes zero revisions.

---

## Closing note

Phase 2 delivered more verified work than Phase 1, and it also contained the most consequential verification-pass finding of the project so far. The two facts are related: the more code there is, the more places there are for an exit criterion to be marked complete at the wrong level. The `ObscuraBrowserBackend` gap existed for the same reason the assertion in F2 was weak — the test suite was green, the plan said the work was done, and nothing in either document asked "but does a real production object exist?"

The verification pass asked that question, and F1 was the answer. The phase is now genuinely complete because the pass happened. Without it, Phase 3 would have started with a router that never escalates, an application that cannot scrape its target portals, and no clear record of why. The evaluation pipeline Phase 3 builds would have produced reports on jobs the acquisition layer could not actually reach.

The lesson for Phase 3 is not "verify more." It is: **the exit criteria are not the deliverable; the deliverable is what the exit criteria describe, and the description is only as good as the check.** An exit criterion that reads "the backend exists and works" can be passed by a spike if nothing asks the criterion what "the backend" means. The verification pass is where the criterion is interpreted, and it needs to be scheduled as a workstream, not as an afterthought.