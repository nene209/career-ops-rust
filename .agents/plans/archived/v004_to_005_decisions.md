# Decisions Addendum: Career-Ops Rust Desktop Application

**Parent plan:** Draft Execution Plan: Career-Ops Rust Desktop Application (v0.4)
**Status:** Addendum — supersedes specific decisions in the parent plan
**Last updated:** 2026-10-17
**Purpose:** Record decisions made after v0.4 of the main plan, and specify exactly which sections of the main plan need updating. This addendum is read alongside the main plan, not instead of it. Where this addendum conflicts with the main plan, **this addendum wins.**

**Note** This document records the decisions that produced v0.5. It is a historical record, not an instruction set. Do not apply it to v0.5 or later." This preserves the rationale and eliminates the ambiguity

---

## How to Use This Document

1. Read the main plan (v0.4) first.
2. Read this addendum.
3. For any topic covered by both, follow the addendum.
4. Apply the "Required Main Plan Updates" section to produce v0.5 of the main plan.

The addendum is organized by decision, then by required plan updates.

---

## Decisions Recorded

### D-1: Phase 0 Is Skipped

**Decision:** Phase 0 (Validation Spikes) is not executed.

**Rationale:** The decisions that Phase 0 was designed to inform were made directly, based on targeted investigation rather than full spikes.

**Consequences:**
- No Spike A (printpdf) output exists. The `printpdf` medium-term path is assessed only by the earlier informal analysis, not by a formal spike.
- No Spike B (Tier 1 coverage) output exists. The coverage question was answered directly: Tier 1 is insufficient.
- No Spike C (browser backend comparison) output exists. The browser backend was chosen directly: Obscura.

**Impact on later phases:** Phase 1 is unaffected (it was designed to be independent of Phase 0 outcomes). Phase 2 is revised to reflect the direct decisions (see Phase 2 v0.2). Phases 3–7 are unaffected, except where the main plan needs updating (see below).

---

### D-2: `reqwest` + `scraper` Are Insufficient; Browser Is Required

**Decision:** Tier 1 acquisition (`reqwest` + `scraper`) is **not sufficient** for the target portals. JavaScript execution is required for acquisition. The browser backend is a **required dependency**, not a feature-gated escalation.

**Evidence:** Direct investigation of the target portals by the project owner. The portals require JS rendering or are otherwise unreachable via plain HTTP.

**Consequences:**
1. The `browser-backend` Cargo feature flag is **removed as an optional gate**. The browser backend is compiled in by default. (It may remain as a feature flag for build variants, but the default build includes it.)
2. The "lean build with no browser binary" story from the main plan §9 does **not** apply to the intended deployment. The primary artifact includes the browser backend.
3. The `BrowserBackend` trait remains valuable for backend swapping (Obscura ↔ `rust-headless-chrome`), but the trait is no longer about making the browser *optional*.
4. Tier 1 is still implemented and used where it works. It is an optimization, not the primary path.

**What this changes in the main plan:**
- §4 (Job Acquisition): the framing of Tier 2 as "feature-gated escalation" is wrong for this deployment. Tier 2 is required.
- §9 (Bundle Size): the "default build ships with no browser binary" claim is wrong. The default build includes the browser.
- §12 (Phases): Phase 2 is revised (see Phase 2 v0.2).

---

### D-3: Obscura Is the Initial Browser Backend

**Decision:** **Obscura** is the initial `BrowserBackend` implementation. `rust-headless-chrome` is the documented fallback if Obscura proves unsuitable.

**Rationale:**
- Obscura is a real, active Rust headless browser project.
- It supports JS execution (V8) and is described as a drop-in replacement for headless Chrome with Puppeteer/Playwright.
- It exposes CDP or a comparable interaction surface.
- Its footprint (~30 MB memory, ~70 MB binary) is materially smaller than a full Chrome bundle.
- Its repository activity is higher than the alternatives evaluated (`rust-headless-chrome`, `chromiumoxide`).

**Caveat recorded at decision time:** The search results describe Obscura's rendering engine as "an evolving independent engine" where "long-tail CSS, some Web APIs, media playback, compositor effects, and platform font rasterization may differ from Chromium." For **scraping**, this is a lower risk (JS execution and DOM access matter more than pixel fidelity). For **PDF generation**, it is a higher risk and must be verified in Phase 4. If Obscura's PDF fidelity is insufficient, the fallback for PDF rendering is `rust-headless-chrome`, while Obscura remains the scraping backend.

**Fallback path:** If Obscura fails during Phase 2 integration, switch the `BrowserBackend` implementation to `rust-headless-chrome`. The trait makes this a contained change.

**What this changes in the main plan:**
- §4 (Job Acquisition): replace "`rust-headless-chrome` (or Spike C choice)" with "Obscura (primary), `rust-headless-chrome` (fallback)".
- §11 (Risks): update R-2 to reflect that Obscura is chosen and verified as a real project, but its rendering fidelity for PDF generation remains unverified.

---

### D-4: `printpdf` Is Deferred

**Decision:** The `printpdf` manual-ops `PdfBackend` is deferred to post-v1 (Track A in Phase 7). It is not part of v1.

**Rationale:** With the browser now required for acquisition (D-2), the browser is already in the build. The primary strategic value of `printpdf` was enabling a browser-free build. That value no longer applies to the intended deployment. `printpdf` remains a plausible post-v1 track for users who want browser-free PDF generation, but it is not a v1 priority.

**What this changes in the main plan:**
- §6 (PDF Generation): the "medium-term slimming path" framing is downgraded. `printpdf` is a post-v1 track, not a near-term deliverable.
- §12 (Phases): Phase 4 does not assess `printpdf` as a v1 candidate.

---

### D-5: Phase 2 Sequencing Changes

**Decision:** Phase 2 begins with the Obscura `BrowserBackend` integration, not with Tier 1 adapters.

**Rationale:** The browser is now the load-bearing dependency. If Obscura cannot be integrated or cannot scrape the target portals, the entire acquisition strategy needs the fallback. Discovering this in Week 1 is cheaper than in Week 4.

**What this changes in the main plan:**
- §12 (Phases): the Phase 2 description should note that browser integration comes first.

---

## Required Main Plan Updates (v0.4 → v0.5)

The following changes are needed to bring the main plan into alignment with the decisions above. They are listed by section.

### §4 — Job Acquisition: Tiered Scraping Architecture

| Current text | Required change |
| :--- | :--- |
| "Tier 2 — Feature-gated: Headless Browser" | "Tier 2 — Required: Headless Browser (Obscura)" |
| "Enabled via a Cargo feature flag" | "Compiled in by default. May remain as a build-variant flag for users who explicitly want a leaner build, but the default and intended deployment includes it." |
| "Initial backend: `rust-headless-chrome`" | "Initial backend: **Obscura**. Fallback: `rust-headless-chrome`." |
| "Other candidate backends (not chosen for v1)" table | Update: Obscura is chosen; `rust-headless-chrome` is fallback; `chromiumoxide` is a secondary fallback; system Chrome over CDP remains a post-v1 candidate. |
| "Escalation and Routing Logic" step 2: "escalate to Tier 2 if compiled in; otherwise surface a clear, actionable error" | "escalate to Tier 2 (always available)" |
| "Deferred: HTTP-Level Stealth Layer" | Unchanged. Still deferred. |

### §4a — The `BrowserBackend` Trait

| Current text | Required change |
| :--- | :--- |
| "This should be treated as a design constraint from the start, even if only one backend ships in v1." | Keep, but add: "The trait now serves two purposes: (a) enabling the Obscura → `rust-headless-chrome` fallback if Obscura proves unsuitable; (b) keeping the door open for future backends." |
| "Compile-Time vs. Runtime Backend Selection" | Keep, but add: "The default build includes the browser backend. Build variants without it are possible but not the intended deployment." |

### §6 — PDF Generation: Backend-Agnostic Rendering

| Current text | Required change |
| :--- | :--- |
| "Initial Backend: Browser-Based (`rust-headless-chrome`)" | "Initial Backend: Browser-Based (**Obscura** for scraping; **`rust-headless-chrome`** for PDF rendering if Obscura's PDF fidelity is insufficient)" |
| "The Slimming Path (Deferred)" | Downgrade: `printpdf` is a **post-v1 track** (Phase 7 Track A), not a near-term deliverable. The reasoning that motivated it (browser-free build) no longer applies to the intended deployment. |
| "Decision Gate" | Update: Phase 4 must verify Obscura's PDF fidelity. If insufficient, use `rust-headless-chrome` for `print_to_pdf` while Obscura remains the scraping backend. |

### §9 — Performance & Bundle Size Assessment

| Current text | Required change |
| :--- | :--- |
| "The key property of this design: the default build ships with **no browser binary at all**." | **Remove.** Replace with: "The default and intended deployment includes a browser backend (Obscura, ~70 MB binary). The lean build without a browser is possible as a build variant but is not the primary artifact and cannot scrape JS-heavy portals." |
| "Default build total (no browser feature): ~20–35 MB" | "Default build (with Obscura): ~90–105 MB" |
| "Build with browser feature: Dominant cost is the browser binary" | "Build with Obscura: browser binary is the dominant cost (~70 MB)" |
| "Important consequence of the v0.3 change: PDF generation now depends on the browser backend by default." | Update: "PDF generation depends on the browser backend. If Obscura's PDF fidelity is insufficient, `rust-headless-chrome` is used for `print_to_pdf` while Obscura remains the scraping backend. This adds the Chrome binary to the build in that case." |

### §11 — Risks & Mitigations

| Risk ID | Current | Required change |
| :--- | :--- | :--- |
| R-1 | "No non-browser `PdfBackend` exists in v1, so PDF output requires the browser feature" | Update: "No non-browser `PdfBackend` exists in v1. PDF output requires the browser backend. `printpdf` is deferred to post-v1 (Track A)." |
| R-2 | "Unverified browser backend landscape" | Update: "Obscura is chosen as the initial browser backend. It is a real, active project with JS execution and CDP compatibility. Its rendering fidelity for PDF generation remains unverified and must be tested in Phase 4. Fallback to `rust-headless-chrome` is available via the `BrowserBackend` trait." |
| R-4 | "Tier 1 fails on a large fraction of real target portals" | Update: "Tier 1 coverage is confirmed insufficient. The browser is required. This is no longer a risk; it is a settled decision. Remove R-4 or mark it as closed." |

### §12 — Suggested Development Phases

| Current | Required change |
| :--- | :--- |
| "Phase 0: Spikes" | Mark as **skipped**. Record the decisions in this addendum. |
| "Phase 2: Acquisition" | Update scope: browser backend (Obscura) comes first; Tier 1 follows. Feature-gating removed. |
| "Phase 4: Output Generation" | Add: verify Obscura's PDF fidelity; fallback to `rust-headless-chrome` for `print_to_pdf` if insufficient. |
| "Phase 7: Post-v1" | Track A (`printpdf`) is now the primary post-v1 candidate, not a near-term item. |

### Appendix A — Decisions Log

| # | Current | Required change |
| :--- | :--- | :--- |
| 10 | "`rust-headless-chrome` as the initial browser backend" | "**Obscura** as the initial browser backend. `rust-headless-chrome` as documented fallback." |
| 12 | "`rust-headless-chrome` is *not* a hard dependency" | Update: "Obscura is the initial backend. `rust-headless-chrome` is the fallback. Neither is optional in the default build — the browser backend is required." |
| 22 | "`printpdf` as the default PDF backend" | Already superseded. Add: "`printpdf` is deferred to post-v1 (Track A)." |
| New row | — | "Phase 0 is skipped. Direct decisions recorded in the Decisions Addendum." |

---

## Summary of What Changes for the Reader

If you are reading the main plan (v0.4) and this addendum together, the following mental adjustments apply:

1. **The browser is required.** Not feature-gated, not optional. The default build includes it.
2. **Obscura is the initial backend.** `rust-headless-chrome` is the fallback.
3. **The lean build story is gone.** The primary artifact is the full build.
4. **Tier 1 is an optimization, not the primary path.** The browser does the heavy lifting.
5. **`printpdf` is post-v1.** Not a near-term deliverable.
6. **Phase 0 is skipped.** Phase 2 starts with Obscura integration.

Everything else in the main plan (schema, business logic, UI structure, AI integration, security model, testing principles, human-in-the-loop) is unchanged.

---

## What This Addendum Does Not Do

- It does not rewrite the main plan. The main plan remains the architectural reference; this addendum adjusts specific decisions.
- It does not cover Phase 1 or Phases 3–7. Those phases are unchanged, except where the main plan updates above affect them.
- It does not reopen the Obscura decision. Ob