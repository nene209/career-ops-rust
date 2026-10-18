# Phase 7: Post-v1 — Detailed Execution Plan

**Parent plan:** Draft Execution Plan: Career-Ops Rust Desktop Application (v0.4), §12
**Status:** Draft — living document
**Last updated:** 2026-10-12
**Duration:** Open-ended
**Depends on:** Phase 6 completion (v1.0.0 shipped)

---

## Purpose

Phase 7 is different from every prior phase. Phases 0–6 had a defined endpoint: a decision, a subsystem, a release. Phase 7 has no endpoint — it is the ongoing work of maintaining and extending the application after v1.0.0 ships.

This document exists to answer a single question: **given a backlog of plausible work, how do we decide what to build next?** It is not a roadmap with committed dates. It is a prioritization framework, a catalogue of known candidate work, and an explicit list of things that should probably never be built.

The most important thing to understand about this document: **nothing in it should be built without a signal from users.** The backlog's existence is not a reason to work on an item. Every track below has a "trigger" — a specific piece of evidence that would justify starting it. Without that evidence, the correct action is to work on bug fixes and wait.

---

## How to Read This Document

The prior phase documents describe **sequential work with gates**. This document describes **parallel candidate tracks with triggers**.

| Concept | Meaning |
| :--- | :--- |
| **Track** | A coherent body of work that can be built independently of the others |
| **Trigger** | The specific evidence that justifies starting the track |
| **Effort** | A range, not a commitment. Higher uncertainty than prior phases. |
| **Value** | What the user gains. Not every track is worth its effort. |
| **Prerequisite** | What must exist before the track can start |

Tracks are not ordered by priority. Priority is determined by which triggers fire first. The "Recommended Sequencing" section offers a default order in the absence of strong signals, but it is a fallback, not a plan.

---

## Prioritization Framework

When deciding whether to start a track, evaluate it against four questions, in order:

1. **Is there a signal?** A user-reported bug, a repeated feature request, a measured failure rate, or a security finding. Not "it would be nice."
2. **Does it address a stated goal?** The application's goals are: help users find jobs, evaluate fit, prepare materials, and never submit on their behalf. A track that does not serve these goals is off-mission.
3. **Is the effort proportionate to the value?** A four-week track that saves users ten seconds per use is not worth it. A two-day track that unblocks a common workflow is.
4. **Does it compromise a core principle?** Human-in-the-loop, mandatory encryption, and honest error reporting are non-negotiable. A track that weakens any of these is rejected regardless of other merits.

Tracks that pass all four questions are candidates. Tracks that fail any are deferred or rejected.

---

## Signals to Watch

The triggers for Phase 7 work come from these sources:

| Signal source | What it tells you | Cadence |
| :--- | :--- | :--- |
| **GitHub issues** | User-reported bugs and feature requests | Continuous |
| **Release feedback** | Reactions to release notes, discussions | Per release |
| **Migration reports** | What users could not migrate, what failed | After Phase 6 |
| **Fetch failure classification** | Which portals fail, and why | Aggregated locally; not transmitted unless the user opts in |
| **Crash reports** | Where the application fails | Requires a telemetry decision (see Track I) |
| **Direct user contact** | Qualitative feedback from active users | Continuous |

**There is no telemetry in v1.** This is deliberate. It means the signals available are limited to what users report. If this proves insufficient, Track I (telemetry) becomes a candidate — but the decision to add telemetry is itself a significant privacy question, not a routine feature.

---

## Track A — `printpdf` Manual-Ops `PdfBackend`

**Trigger:** Sustained user complaints about bundle size, or a strategic decision to eliminate the browser dependency from the default build.
**Effort:** 5–8 weeks (high uncertainty)
**Prerequisite:** A fresh feasibility spike, unless the Phase 0(a) and Phase 4/W5 spikes produced conclusive positive results
**Priority:** High strategic value, high risk

### Scope

Implement a `PdfBackend` that renders CVs and cover letters using `printpdf`'s manual ops API. No browser required. Templates are expressed as structured layout operations rather than HTML.

```
printpdf manual ops:
  - Text runs at explicit coordinates
  - Font embedding (one or two faces)
  - Lines and rules for section separators
  - Explicit reading order assigned to every text run
  - Extraction-order verification via pdftotext
```

### Why This Track Matters

This is the flagship post-v1 item because it retires R-1 from the main plan. In v1, PDF output requires the browser feature. A `printpdf` backend would restore browser-free PDF generation, making the lean build viable for users who do not need browser-based acquisition.

### Critical Constraints

1. **The slimming path runs through the manual ops API, not `printpdf`'s HTML feature.** The HTML feature is experimental and geared toward basic layouts. It is not a drop-in replacement for Chromium rendering.
2. **ATS extraction order becomes the backend's responsibility.** With a browser renderer, HTML document flow determines extraction order. With `printpdf`, the content stream is written directly, so reading order must be assigned and enforced explicitly. The `resumark` project's approach — assigning an explicit `order` index to every placed text run — is the reference implementation.
3. **Template format changes.** HTML templates cannot be reused. A new template format is required, likely a Rust data structure or a JSON representation of layout operations. This is a significant user-facing change for anyone who customized a template.
4. **Effort estimates from spikes are lower bounds.** The manual layout path looks tractable in a spike. Generalizing to multiple templates, handling edge cases (long job titles, non-Latin characters, multi-page flow), and verifying ATS extraction across variants is where the effort lives.

### Definition of Done

- A `printpdf`-based `PdfBackend` exists and is selectable alongside the browser backend.
- It renders the default CV and cover letter templates with ATS-verified extraction order.
- The lean build can produce PDFs without the browser feature.
- Golden-file and extraction tests pass for the new backend.
- Documentation explains the trade-off between the two backends.
- R-1 in the main plan is downgraded.

### Risks

| Risk | Mitigation |
| :--- | :--- |
| Manual layout is more expensive than estimated | Fresh spike before commitment. If the spike exceeds 2 weeks, reassess. |
| ATS extraction order cannot be made reliable | The Phase 0(a) spike is the go/no-go. If it fails, do not pursue. |
| Template format change alienates users with custom templates | Provide a migration path. Document the change loudly. |
| Duplicate maintenance burden (two backends, two template formats) | Accept it as the cost of browser-free output, or decide to deprecate one. |

---

## Track B — HTTP-Level Stealth Layer (Tier 1.5)

**Trigger:** Aggregated fetch-failure data shows fingerprint-based blocking is a common cause of Tier 1 failure. Without this data, the track is speculative.
**Effort:** 1–2 weeks
**Prerequisite:** None
**Priority:** Low until triggered

### Scope

An intermediate acquisition tier using TLS/HTTP2 browser impersonation. Handles portals that block plain `reqwest` at the fingerprint level but do not require JavaScript execution.

Candidate crates: `impit`, `crawlingo`, or equivalent.

### Why This Might Matter

If a meaningful fraction of target portals reject plain `reqwest` requests due to TLS fingerprints, this tier would handle them without the browser binary. That would reduce the browser feature's scope further.

### Why This Might Not Matter

If most Tier 1 failures are JS-required rather than fingerprint-blocked, this tier adds a path that handles cases the browser backend already handles. The `fetch_tier` classification from Phase 2 is the evidence needed to decide.

### Definition of Done

- A Tier 1.5 path exists behind a feature flag.
- `fetch_tier` records `tier1.5:impersonated` for successful fetches.
- Fixture-based tests verify it against saved portal responses.
- Documentation explains when the tier is used.

---

## Track C — Additional ATS Adapters

**Trigger:** User requests for specific ATS platforms not covered by the initial four (Greenhouse, Lever, Ashby, SmartRecruiters).
**Effort:** 1–3 days per adapter
**Prerequisite:** The `AtsAdapter` trait from Phase 2
**Priority:** Ongoing, incremental, high value per unit effort

### Scope

Add adapters for additional ATS platforms. Candidates:

- Workable
- Recruitee
- BambooHR
- JazzHR
- Personio
- Teamtailor
- Jobvite

### Why This Is the Best Ongoing Investment

Each adapter is small, self-contained, and immediately useful. The trait from Phase 2 was designed for exactly this. The cost is low and the value is direct: users on a new platform can fetch jobs.

### Definition of Done

- The adapter implements `AtsAdapter`.
- Auto-detection recognizes the platform's URL pattern.
- Fixture-based tests cover a real response from the platform.
- A live test (environment-gated) verifies the endpoint.
- Documentation lists the supported platform.

---

## Track D — Additional Browser Backends

**Trigger:** Bundle size complaints, browser compatibility issues, or availability of a materially better backend.
**Effort:** 1–3 weeks per backend
**Prerequisite:** The `BrowserBackend` trait from Phase 1
**Priority:** Medium, depends on the specific backend

### Sub-tracks

**D1 — System Chrome over CDP.** Zero bundled binary. Users who already have Chrome install it themselves. The application points at the system browser via a configurable path.

- Effort: 1–2 weeks
- Value: Eliminates the bundle size cost for users who already have Chrome
- Risk: Depends on the user having Chrome installed; version compatibility issues

**D2 — `chromiumoxide` swap.** If Phase 0 Spike C recommended `chromiumoxide` but `rust-headless-chrome` was chosen for maturity, this track makes the swap. The `BrowserBackend` trait makes it a contained change.

- Effort: 1–2 weeks
- Value: Async-first integration; potentially cleaner GPUI interaction
- Risk: Low, because the trait already isolates the choice

**D3 — Rust-native headless browser.** Obscura, OxiBrowser, or whatever has matured by the time this track is considered. Lower memory and bundle footprint than Chrome.

- Effort: 2–3 weeks
- Value: Smaller bundle, faster startup
- Risk: Compatibility with modern SPAs; must be verified against real target portals before commitment

### Definition of Done

Per backend: implemented behind the trait, tested against a fixture and a live environment-gated test, and selectable in Settings.

---

## Track E — Dynamic Backend Loading

**Trigger:** Strong, repeated user demand for post-install backend selection. This is speculative.
**Effort:** 4–6 weeks
**Prerequisite:** A stable C ABI for the backend traits, version negotiation, plugin distribution infrastructure
**Priority:** Low — possibly never

### Why This Is Probably Not Worth It

Dynamic loading was explicitly deferred in the main plan (§4a). The reasons still hold:

- Requires a stable C ABI, which Rust does not natively provide
- Requires version negotiation between host and plugin
- Requires a distribution mechanism for the plugins themselves
- Adds a significant security surface (loading arbitrary shared libraries)

The benefit — a user switching backends without reinstalling — is achievable in practice by downloading the appropriate build. The application already ships lean and full variants. A third or fourth variant is a smaller lift than a plugin system.

### Recommendation

Do not build this unless a specific, compelling use case emerges that cannot be solved by shipping additional build variants.

---

## Track F — Prompt Templating and A/B Testing

**Trigger:** Power users repeatedly requesting conditionals, loops, or partials in prompts; or a measured benefit from prompt variants.
**Effort:** 2–4 weeks
**Prerequisite:** The prompt versioning and variable substitution from Phase 3
**Priority:** Medium, if there is demand

### Sub-tracks

**F1 — Prompt templating language.** Conditionals (`{{#if}}`), loops beyond `#each`, partials (`{{> partial}}`), and simple filters.

- Effort: 2–3 weeks
- Value: More expressive prompts for advanced users
- Risk: Templating languages grow unboundedly. Scope creep is a real danger.

**F2 — A/B testing of prompt variants.** Run two prompt versions against the same input and compare outputs.

- Effort: 1–2 weeks
- Value: Data-driven prompt improvement
- Risk: Requires a metric for "better," which is subjective for evaluation prompts

### Recommendation

F1 is worth doing if there is demand. F2 is probably not — the value is unclear and the metric problem is hard.

---

## Track G — Local LLM Support

**Trigger:** User requests for offline operation or privacy-focused deployment.
**Effort:** 1–2 weeks
**Prerequisite:** The `core::llm` abstraction from Phase 3
**Priority:** High value per unit effort if there is demand

### Why This Is Cheaper Than It Looks

`rig` supports Ollama and other local providers natively. Adding a local provider is largely a matter of:

1. Adding the provider to the `Provider` enum
2. Adding Ollama client construction to `LlmConfig::build_client`
3. Documenting how to install and run Ollama
4. Handling the case where Ollama is not running

### Caveats

- Local models are typically weaker than hosted models. Users should understand that evaluation quality may degrade.
- Local models do not support structured output as reliably as hosted providers. The capability registry must reflect this.
- Ollama is a separate installation. The application does not bundle it.

### Definition of Done

- Ollama is selectable as a provider.
- The capability registry accurately reflects local model limitations.
- Documentation explains the trade-offs.
- The UI warns when a local model lacks structured output support.

---

## Track H — Full Auto-Update

**Trigger:** User frustration with the notify-only update flow, or a strong operational need.
**Effort:** 3–4 weeks
**Prerequisite:** The signed manifest infrastructure from Phase 6
**Priority:** Low — the security surface is significant

### Why the Notify-Only Model Is Better for v1

Full auto-update requires the application to download and execute an installer. This is a meaningful attack surface:

- The download must be over HTTPS and signature-verified
- The installer must be code-signed
- The update process must not be hijackable by a local attacker
- A failed update must not leave the application in a broken state

Each of these is a known source of real-world vulnerabilities in other applications. The notify-and-download model avoids all of it.

### Definition of Done (If Pursued)

- Update download is signature-verified against an embedded public key
- The install process is atomic and rollback-capable
- Failed updates leave the prior installation intact
- The mechanism is documented in the security page

---

## Track I — Telemetry and Crash Reporting

**Trigger:** A demonstrable need for operational data that cannot be obtained otherwise.
**Effort:** 2–3 weeks engineering, plus ongoing privacy and legal work
**Prerequisite:** A privacy policy that users have accepted
**Priority:** Contested — see below

### Why This Is Genuinely Contested

Telemetry would provide crash reports, usage patterns, and failure classifications that would sharpen every prioritization decision in Phase 7. It would also:

- Require a privacy policy
- Require user consent (opt-in, not opt-out)
- Require a data retention policy
- Require handling for users in jurisdictions with data protection laws
- Introduce a new attack surface (the telemetry endpoint)
- Change the application's privacy posture from "sends nothing identifying" to "sends anonymized data"

The v1 privacy page says the update check sends nothing identifying. Adding telemetry would complicate that statement.

### Recommendation

Do not build telemetry in the near term. The application can be maintained effectively using GitHub issues and direct user feedback. If the project grows to a size where operational data is essential, revisit the decision deliberately — but treat it as a privacy decision, not a feature decision.

### A Middle Path

**Local-only diagnostics.** Collect the same data (crash logs, fetch failure classifications, performance metrics) but store it locally and never transmit it. Provide a "Export diagnostics" action so users can send a bundle when reporting an issue. This gives maintainers the data they need without a telemetry endpoint.

This is worth doing even if full telemetry is never added. It is much cheaper than telemetry and has no privacy cost.

---

## Track J — Assisted Submission

**Trigger:** Repeated user requests for help with the submission step, and a clear policy position on what "assisted" means.
**Effort:** 4–8 weeks (plus significant policy work)
**Prerequisite:** A deliberate decision about what this feature is and is not
**Priority:** Policy question first, technical question second

### The Principle at Stake

The application's core principle is human-in-the-loop: it never submits applications on the user's behalf. This is not just a feature choice; it is the product's ethical position and a meaningful differentiator from automation tools.

Assisted submission is a spectrum:

| Position | What it means | Compatibility |
| :--- | :--- | :--- |
| **No assistance** (v1) | User submits manually in a browser | Current position |
| **Pre-fill assistance** | Application opens the job posting in a browser and pre-fills known fields | Borderline — the user still clicks submit |
| **Assisted submission with confirmation** | Application fills the form and requires an explicit "Submit" click | Weaker — the application is doing the submission work |
| **Automated submission** | Application submits without user action | Violates the principle |

The v1 plan's §10 says a future "Submit" action "must require manual confirmation and never auto-submit." That places the ceiling at the third row: assisted with confirmation. It does not authorize the fourth row.

### Why This Is Hard

Even "pre-fill assistance" requires:

- Storing job board credentials (currently forbidden)
- Handling CAPTCHAs, MFA, and anti-bot measures
- Handling per-board form variations
- Maintaining working integrations against sites that actively resist automation
- Taking on the reputational risk of a submission that goes wrong

The maintenance burden is ongoing and large. Every supported board can change its form at any time.

### Recommendation

Do not build this unless there is a clear, sustained user demand and a policy decision to cross the human-in-the-loop boundary carefully. The current position — user submits manually — is the right one for v1 and probably for v1.x.

If the boundary is crossed, the first step is pre-fill assistance (row 2), not assisted submission (row 3). That is a much smaller change and it keeps the user in control of the final action.

---

## Track K — Key Storage Enhancements

**Trigger:** User requests for biometric unlock or hardware-backed key storage.
**Effort:** 2–4 weeks
**Prerequisite:** The keystore from Phase 1
**Priority:** Low to medium

### Sub-tracks

**K1 — Biometric unlock.** Use Touch ID / Windows Hello / libfido2 as a convenience layer over the keychain path. The biometric gates access to the KEK; it does not replace it.

- Effort: 1–2 weeks per platform
- Value: Faster unlock, better UX for users who already have biometrics enabled
- Risk: Platform-specific APIs and permission models

**K2 — Hardware-backed key storage.** Use the TPM, Secure Enclave, or equivalent to store the KEK in hardware.

- Effort: 2–3 weeks
- Value: Higher assurance against software-based key extraction
- Risk: Platform-specific and may not be available on all systems; adds a new failure mode where hardware is unavailable

### Recommendation

K1 is worth doing if there is demand. K2 is a defense-in-depth enhancement that only matters for users with a specific threat model — probably not worth the complexity for most users.

---

## Track L — Additional Platforms and Architectures

**Trigger:** User requests for a platform or architecture not currently shipped.
**Effort:** 1–2 weeks per platform
**Prerequisite:** The packaging infrastructure from Phase 6
**Priority:** Depends on demand

### Candidates

- macOS x86_64 (Intel Macs)
- Linux aarch64 (ARM servers, Raspberry Pi, Asahi Linux)
- Windows aarch64 (Surface, Snapdragon laptops)
- BSDs (FreeBSD, OpenBSD)

### Recommendation

Add architectures for platforms already supported before adding new platforms. A Linux aarch64 build is straightforward; a FreeBSD build is a new platform with its own packaging, keychain backend, and testing requirements.

---

## Track M — Localization

**Trigger:** User requests in non-English languages, or a strategic decision to serve a specific market.
**Effort:** 4–8 weeks initial, ongoing
**Prerequisite:** A string externalization pass
**Priority:** Low until triggered

### Why This Is Expensive

Localization is not just translation. It requires:

- Externalizing every user-visible string
- Handling pluralization rules
- Handling date, number, and currency formatting
- Handling right-to-left layouts
- Testing in each locale
- Maintaining translations as the application changes

The initial cost is high, and the ongoing cost is higher. A single-locale application is fine for a long time.

### Recommendation

Do not build this unless a specific market justifies the ongoing investment. If built, start with one additional language and treat it as an experiment.

---

## Track N — File-Based Export

**Trigger:** User requests for interoperability with tools that consume markdown or YAML.
**Effort:** 1–2 weeks
**Prerequisite:** The export infrastructure from Phase 5
**Priority:** Low to medium

### Scope

Export the database contents back to the file-based format used by the original `career-ops`. This is the reverse of the Phase 6 migration.

### Why It Might Matter

The original tool's file-based format is portable, human-readable, and interoperable with other tools. Some users may want both: a structured database for the application and a file-based snapshot for other purposes.

### Why It Might Not

Most users will not need this. The database is the source of truth, and the export bundle from Phase 5 already covers backup and portability.

### Definition of Done

- An `export-files` command or UI action produces a directory in the original format
- Round-trip test: migrate → export → re-migrate produces equivalent data
- Documentation explains the use case

---

## Recommended Sequencing

In the absence of strong signals, this is the default order:

### Wave 1 — First 3 months post-v1

- **Patch releases** for bugs found in v1.0
- **Track C** — additional ATS adapters as requested
- **Track G** — local LLM support (cheap, high value)
- **Track I (middle path)** — local-only diagnostics export

### Wave 2 — 3–6 months post-v1

- **Track A** — `printpdf` manual-ops backend, gated on a fresh spike
- **Track B** — HTTP-level stealth layer, if fetch data justifies it
- **Track D1** — system Chrome over CDP, if bundle size is a complaint

### Wave 3 — 6–12 months post-v1

- **Track F1** — prompt templating, if there is demand
- **Track K1** — biometric unlock
- **Track L** — additional architectures

### Wave 4 — 12+ months post-v1 (speculative)

- **Track D3** — Rust-native browser backend, if one has matured
- **Track J** — assisted submission, only with a policy decision
- **Track M** — localization, only with a specific market target
- **Track E, H, I (full)** — probably never

---

## Explicit Won't-Do List

These items are plausible but should not be built unless the stated condition changes:

| Item | Reason |
| :--- | :--- |
| **Automated submission** | Violates the human-in-the-loop principle. This is the product's ethical position, not a technical choice. |
| **Telemetry without user consent** | Contradicts the privacy posture established in v1. |
| **Cloud sync** | Turns a local-first application into a service. Different product, different threat model. |
| **Web or mobile builds** | The desktop application is the product. A web build would require a server, which changes the trust model entirely. |
| **Dynamic backend loading** | High complexity, speculative value. Ship variants instead. |
| **Prompt marketplace / sharing** | Adds moderation burden and a distribution problem with unclear value. |
| **Fine-tuning** | Local fine-tuning requires GPU infrastructure and a training data pipeline. Out of scope for a desktop application. |

---

## How This Document Evolves

Phase 7 is a living document. It should be updated:

- **Quarterly**, to reassess priorities against current signals
- **After each release**, to record what was built and what was not
- **When a track's trigger fires**, to commit to a timeline
- **When a track is completed**, to move it to a "Done" section
- **When a track is rejected**, to add it to the won't-do list

There is no version tag on this document, because it does not have a completion state. It is a reference, not a plan with an endpoint.

---

## Reassessment Cadence

Every quarter, answer these questions:

1. Which tracks have fired their triggers?
2. Which tracks had no signal and should be deprioritized?
3. Are there new tracks that were not anticipated?
4. Is anything in the won't-do list now worth reconsidering?
5. Is the prioritization framework still producing good decisions?

The answers to these questions determine the next quarter's work. Do not follow the recommended sequencing blindly. It is a default for the case where no strong signal exists — not a commitment.

---

The single most important thing to take from this document: **the backlog's existence is not a reason to build it.** Every track has a trigger. Work on a track when the trigger fires. Otherwise, work on bugs, support users, and wait. A small, well-maintained application that does its job is worth more than a large one that accumulated features because they were on a list.