# Phase 0: Validation Spikes — Detailed Execution Plan

**Parent plan:** Draft Execution Plan: Career-Ops Rust Desktop Application (v0.4), §12
**Status:** Draft
**Last updated:** 2026-10-12
**Duration:** 1–2 weeks
**Nature:** Throwaway. Nothing built in Phase 0 ships. The deliverable is a decision, not code.

---

## Purpose

Phase 0 exists to **retire the plan's three highest-impact unknowns before any production architecture is committed**. Each spike is a bounded, time-boxed investigation with a pass/fail criterion. A failed spike is a success — it is cheaper to discover an invalid assumption in one week than in month three.

The spikes are deliberately sequenced so that **Spike B runs first**. If Tier 1 acquisition proves broadly viable, the Tier 2 browser feature becomes genuinely optional, and Spike C's urgency drops. Conversely, if Tier 1 fails on most target portals, Tier 2 moves into the critical path and Spike C becomes a hard gate. This ordering prevents wasted effort on a browser backend that may not be needed.

| Spike | Question | Gate? |
| :--- | :--- | :--- |
| **A** — `printpdf` viability | Can a pure-Rust backend produce ATS-parseable PDFs without a browser? | No (informational, informs medium-term path) |
| **B** — Tier 1 acquisition | Does `reqwest` + `scraper` / ATS JSON APIs handle the majority of target portals? | **Yes** — shapes Tier 2 necessity |
| **C** — Browser backend comparison | Which CDP client is the better fit for `BrowserBackend`? | No (informational, informs Phase 2) |

---

## Ground Rules

1. **Time-boxed.** Each spike has a hard stop. If the time budget is exhausted without a conclusion, the spike is a fail and the plan proceeds with the fallback assumption.
2. **Throwaway code.** Spike code lives in `spikes/` as separate binaries or examples. It does not become production code. Expect to delete it.
3. **No refactoring.** Spikes answer a question; they do not build infrastructure.
4. **Written conclusions required.** Each spike produces a short Markdown memo (see Deliverables) recording what was tested, what was observed, and the recommendation.
5. **Real data only.** No synthetic fixtures. Spikes must run against real resume content and real target portals.

---

## Spike A — `printpdf` Viability for ATS-Safe Resumes

### Question

Can a pure-Rust PDF backend produce a resume that (a) renders acceptably and (b) extracts in correct reading order under standard ATS text extraction? If yes, this unlocks a browser-free default build and retires R-1.

### Background

`printpdf`'s HTML-to-PDF feature (`PdfDocument::from_html`) is explicitly documented as experimental and WIP, recommended only for "basic layouts" with manual element positioning preferred for pixel-exact control. This means the HTML templates cannot be reused, and the realistic path is `printpdf`'s **manual ops API** — explicit text runs at explicit coordinates.

The critical unknown is not visual fidelity. It is **text extraction order**. A PDF can render beautifully while its embedded text layer extracts as scrambled gibberish, because ATS parsers read the text layer, not the pixels. The `resumark` project exists specifically to solve this: it treats extraction order as a contract, assigns every text run an explicit `order` index in declared reading order, and emits runs into the PDF content stream in that order, independent of visual position. Its `--verify` mode round-trips output through `pdftotext` in both `-raw` and default modes and asserts the strings appear in declared order. This is the verification technique the spike should adopt.

### Scope

| In scope | Out of scope |
| :--- | :--- |
| One realistic resume, single-column, no graphics | Multi-template system |
| Text layout via `glyph_brush_layout` + `printpdf` ops | Full HTML/CSS rendering |
| Extraction-order verification via `pdftotext` | Visual regression testing |
| Assessment of `glyph_brush_layout` integration friction | Font embedding beyond one or two faces |

### Method

1. **Extract a real resume.** Take one non-trivial existing CV — multiple roles, bullets, skills section, education. Not a toy example. The point is to hit real layout complexity.

2. **Build the layout path.** `printpdf` is a low-level drawing library operating in millimetres as `f64`; `glyph_brush_layout` works in pixels as `f32`. Integration requires converting glyph positions to `printpdf` coordinates, including manual vertical offsets — a known point of friction documented in community examples. Implement the minimal bridge for a single-column flow: text wrapping, vertical advance, section spacing.

3. **Assign explicit emission order.** Every text run gets an integer `order` index in declared reading order. Emit ops into the content stream sorted by `order`, not by visual position. This is the `resumark` contract, and it is the crux of ATS safety.

4. **Verify extraction.** Run the output through `pdftotext` in both `-raw` (stream order) and default (position-based reading order) modes. Assert that the extracted strings appear in the declared reading order in both. `resumark`'s `--verify` is the reference implementation of this check.

5. **Inspect visually.** Open the PDF. Confirm it is not embarrassing. The spike is not about pixel-perfection, but a PDF that extracts correctly and looks broken is not a viable backend.

### Success Criteria

| Criterion | Threshold |
| :--- | :--- |
| Text extracts in correct reading order (`pdftotext -raw`) | Required |
| Text extracts in correct reading order (`pdftotext` default) | Required |
| Visual output is acceptable for a single-column resume | Required |
| Layout bridge implemented in ≤ 3 days | Target |
| Assessment of effort to generalize to a second template | Deliverable |

### Failure Response

If extraction order cannot be made reliable, or if the layout bridge proves significantly more expensive than the 5–8 week post-v1 estimate, record that finding and **do not pursue `printpdf` as a near-term backend**. R-1 stays as-is. The `PdfBackend` trait remains the design constraint, and the browser backend remains the only v1 implementation.

### Deliverable

`spikes/a-printpdf-findings.md` containing:
- The test resume source and generated PDF.
- Raw `pdftotext` output in both modes, annotated.
- A time log of where the effort actually went.
- A recommendation: viable / not viable / viable with caveats, plus an effort estimate to generalize.

---

## Spike B — Tier 1 Acquisition Against Real Target Portals

### Question

Does the Tier 1 path (`reqwest` + `scraper`, with ATS JSON APIs where available) successfully acquire job postings from a representative sample of real target portals? This spike runs **first** because its outcome determines whether Spike C is a hard gate or an informational exercise.

### Background

Search results reveal a material fact that changes the Tier 1 design: **the major ATS platforms publish free, public, unauthenticated JSON APIs**. Greenhouse exposes `https://boards-api.greenhouse.io/v1/boards/{slug}/jobs?content=true`; Lever exposes `https://api.lever.co/v0/postings/{slug}?mode=json`; Ashby exposes `https://api.ashbyhq.com/posting-api/job-board/{slug}`; SmartRecruiters and Recruitee publish similar endpoints. This means that for a large fraction of target portals, Tier 1 is not HTML scraping at all — it is **JSON API consumption**, which is dramatically more robust, faster, and less brittle than DOM parsing.

The spike must therefore test **two Tier 1 sub-paths**:
- **Tier 1a — ATS JSON APIs** for known ATS platforms.
- **Tier 1b — `reqwest` + `scraper` HTML parsing** for company career pages not hosted on a recognized ATS.

### Scope

| In scope | Out of scope |
| :--- | :--- |
| 10–15 real target portals, mixed ATS and custom | Full portal configuration system |
| ATS JSON endpoint discovery and parsing | Pagination beyond first page |
| HTML extraction for at least 3 non-ATS career pages | Rate limiting, retries, caching |
| Classification of failures by cause | Browser escalation (that is Spike C's concern) |

### Method

1. **Assemble the sample.** Select 10–15 real target portals. Deliberately mix:
   - Companies on Greenhouse, Lever, Ashby, and at least one other ATS.
   - At least 3 company career pages hosted on custom infrastructure (not a recognized ATS).
   - At least 1 known JS-heavy SPA, to establish the Tier 1 failure boundary empirically.

2. **Implement Tier 1a.** For each ATS-hosted portal, call the documented JSON endpoint. Parse the response into a common `job_descriptions` shape (`title`, `company`, `source_url`, `raw_text`). Note payload size, response time, and any schema quirks.

3. **Implement Tier 1b.** For the custom career pages, fetch with `reqwest` and extract with `scraper` CSS selectors. Do not over-invest in selector robustness — the goal is to determine whether a reasonable selector approach works, not to build a production scraper.

4. **Classify every result.** For each portal, record one of:
   - **Success (1a)** — ATS JSON returned complete data.
   - **Success (1b)** — HTML extraction returned usable data.
   - **Partial** — Data acquired but incomplete (e.g. missing description body).
   - **Fail — JS-required** — Content absent from initial HTML; requires browser.
   - **Fail — blocked** — Bot detection or fingerprint-based rejection.
   - **Fail — other** — Record the cause.

5. **Record the raw evidence.** Save the actual HTTP responses (or their headers plus a truncated body) for every portal. These become the fixture corpus referenced in §8 of the main plan — real-world portal failures become regression fixtures.

### Success Criteria

| Criterion | Threshold |
| :--- | :--- |
| Tier 1a success rate on recognized ATS portals | ≥ 90% |
| Tier 1b success rate on non-ATS career pages | ≥ 50% |
| Overall Tier 1 success rate across the mixed sample | ≥ 70% |
| Failure causes are classified, not merely counted | Required |
| Fixture corpus captured for regression use | Required |

The 70% overall threshold is the decision point. Above it, Tier 2 is genuinely an escalation for edge cases. Below it, Tier 2 is effectively mandatory for a usable product, and the plan's default-build story changes.

### Failure Response

If Tier 1 success is below 70%, **Tier 2 moves from "feature-gated escalation" to "required for basic function."** This does not invalidate the architecture — the `BrowserBackend` trait still applies — but it changes the product framing. A default build without the browser feature would not be able to acquire jobs from a meaningful fraction of portals. Record this explicitly and flag it for the main plan's §4 and §9.

### Deliverable

`spikes/b-tier1-findings.md` containing:
- The portal sample list with classification and evidence links.
- Success rate broken down by ATS vs. non-ATS, and by failure cause.
- A recommendation on whether Tier 2 is an escalation or a requirement.
- The fixture corpus, structured for reuse in §8 testing.

---

## Spike C — Browser Backend Comparison

### Question

Between `rust-headless-chrome` and `chromiumoxide`, which is the better initial `BrowserBackend` implementation for this application's access pattern?

### Background

The two candidates differ primarily in **execution model**:

- **`rust-headless-chrome`** exposes a **synchronous** API. `Tab::print_to_pdf(options)` returns the PDF bytes directly after decoding the base64 CDP result. `PrintToPdfOptions` covers the full parameter set the original Playwright pipeline relied on. This is the mature, widely-used option.
- **`chromiumoxide`** exposes an **async-first** API. `Page::pdf(params)` and `Page::save_pdf(params, path)` are both `async`. `PrintToPdfParams` is a direct mirror of the CDP protocol type, exposing `landscape`, `displayHeaderFooter`, `printBackground`, `paperWidth`, `paperHeight`, margins, `pageRanges`, `headerTemplate`, and `footerTemplate`. It integrates naturally with a `tokio`-based stack.

The question is not which is "better" in the abstract. It is which introduces less friction given that the rest of the stack (`sqlx`, `rig`) is async, and given that browser calls must not stall the GPUI event loop (R-9).

### Scope

| In scope | Out of scope |
| :--- | :--- |
| `printToPDF` parameter coverage comparison | Navigation and extraction comparison |
| Async ergonomics under a tokio runtime | Stealth / anti-detection features |
| Blocking-call handling from GPUI | Full scraping implementation |
| One real resume rendered through each | Performance benchmarking at scale |

### Method

1. **Render the same resume through both.** Take the Spike A test resume (or an equivalent HTML source) and produce a PDF via each crate. Compare parameter coverage needed vs. exposed.

2. **Test async integration.** For `chromiumoxide`, drive a `print_to_pdf` call inside a `tokio` runtime alongside a `sqlx` query, and confirm no runtime conflict. For `rust-headless-chrome`, wrap the blocking call in `spawn_blocking` and confirm the pattern is clean.

3. **Test GPUI interaction.** Confirm that neither approach blocks the GPUI event loop. This is the R-9 check. A minimal GPUI harness that triggers a render and asserts the UI remains responsive is sufficient.

4. **Check maintenance signals.** Commit recency, open issue responsiveness, release cadence, and whether either crate has known gaps in `printToPDF` coverage. Record dates and version numbers — the ecosystem moves.

### Success Criteria

| Criterion | Threshold |
| :--- | :--- |
| Both crates produce a usable PDF from the same source | Required |
| `printToPDF` parameter coverage sufficient for ATS resume needs | Required for the chosen crate |
| Async integration is clean (either natively or via `spawn_blocking`) | Required for the chosen crate |
| No GPUI event-loop stall | Required for the chosen crate |
| Maintenance signals documented with dates | Required |

### Deliverable

`spikes/c-browser-backend-findings.md` containing:
- Side-by-side PDF output from both crates.
- Parameter coverage table (`rust-headless-chrome` `PrintToPdfOptions` vs. `chromiumoxide` `PrintToPdfParams` vs. the needs list).
- Async integration notes with code snippets.
- GPUI responsiveness result.
- A recommendation with rationale, plus the verification date for R-2.

---

## Sequencing and Time Budget

```
Day 1–2    Spike B, setup + portal sample assembly
Day 3–5    Spike B, Tier 1a (ATS JSON) + Tier 1b (HTML)
Day 6      Spike B, classification + findings memo
           ── DECISION GATE ──
Day 7–9    Spike A, layout bridge + extraction-order verification
Day 10     Spike A, visual inspection + findings memo
Day 11–12  Spike C, both crates + comparison
Day 13     Spike C, GPUI responsiveness + findings memo
Day 14     Consolidation: update main plan §4, §6, §9, §11
```

**Hard stop:** 2 weeks. If any spike is incomplete at its budget, record the partial finding and move on. An inconclusive spike is a valid output; an overrun is not.

---

## Decision Gate Summary

| Gate | Trigger | Consequence |
| :--- | :--- | :--- |
| **B** | Tier 1 ≥ 70% overall | Tier 2 remains feature-gated escalation. Default build is viable without browser. |
| **B** | Tier 1 < 70% overall | Tier 2 becomes required for basic function. Rework §4 default-build framing and §9 bundle story. |
| **B** | ATS JSON APIs cover ≥ 80% of sample | Tier 1a becomes the primary acquisition path; HTML scraping becomes secondary. Adjust §4 wording. |
| **A** | Extraction order verified | `printpdf` manual-ops backend is viable as a medium-term deliverable. R-1 can be downgraded. |
| **A** | Extraction order unreliable | `printpdf` deferred indefinitely. R-1 stays. Browser backend remains sole v1 PDF path. |
| **C** | `chromiumoxide` async integrates cleanly | Prefer `chromiumoxide` for `BrowserBackend`. Update decision log #10. |
| **C** | `rust-headless-chrome` materially more complete | Keep `rust-headless-chrome`. Document the gap in `chromiumoxide` as the reason. |

---

## Outputs

At the end of Phase 0, the following exist:

1. `spikes/a-printpdf-findings.md`
2. `spikes/b-tier1-findings.md`
3. `spikes/c-browser-backend-findings.md`
4. A fixture corpus from Spike B, structured for reuse in §8 testing.
5. An updated main plan with revised §4 (acquisition framing), §6 (PDF backend status), §9 (bundle implications), and §11 (R-1, R-2, R-4 statuses).

All spike code is deleted or moved to an archival directory. Nothing from `spikes/` is promoted into the production crate.

---

## What Phase 0 Is Not

Phase 0 is **not** a prototype of the application. It does not touch GPUI screen layout, the SQLite schema, the keystore subsystem, or `rig` integration. Those are Phase 1 concerns. The temptation to "just start building the real thing" during a spike is the most common way Phase 0 overruns, and it defeats the purpose — which is to learn cheaply what the real thing should be.

---

The single most important thing to take from this plan: **run Spike B first, and let its result decide how much you care about Spike C.** If the ATS JSON APIs cover the majority of your target portals, the browser backend becomes a genuinely optional feature, and the entire bundle-size conversation in §9 softens considerably. That is worth knowing before writing a line of `BrowserBackend`.