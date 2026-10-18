# Draft Execution Plan: Career-Ops Rust Desktop Application

**Status:** Draft — v0.2
**Last updated:** 2026-10-11

This document outlines the execution plan for porting the `career-ops` project from its current Node.js, file-based architecture to a Rust desktop application using GPUI, SQLite, and a primarily pure-Rust technology stack. The plan incorporates the stated technical decisions and evaluates the key architectural considerations.

### Change Log

| Version | Change |
| :--- | :--- |
| v0.1 | Initial draft. |
| v0.2 | Replaced single-browser scraping approach with tiered architecture (`reqwest` + `scraper` default, headless browser feature-gated). Removed `rust-headless-chrome` as a hard dependency. Added CDP-agnostic backend abstraction as a future extension. Added dependency-verification caveat. |

---

## 1. SQLite Schema Design

The most significant architectural change is inverting the original "files are canonical, databases are derived" doctrine. The new SQLite schema must become the primary, stateful source of truth for all user and system data.

### Proposed Schema

| Table | Replaces File(s) | Key Columns | Notes |
| :--- | :--- | :--- | :--- |
| `profile` | `config/profile.yml`, `modes/_profile.md` | `id`, `name`, `email`, `phone`, `location`, `summary`, `archetypes` (JSON), `preferences` (JSON) | Single-row table for candidate identity and preferences. |
| `cv_entries` | `cv.md` | `id`, `profile_id`, `type` (experience/education/skill), `title`, `organization`, `start_date`, `end_date`, `description`, `metadata` (JSON) | Normalized representation of the CV. |
| `job_descriptions` | `jds/*.md` | `id`, `title`, `company`, `source_url`, `raw_text`, `fetched_at`, `fetch_tier` | Stores fetched job posting content. `fetch_tier` records which acquisition path produced the content (see §4). |
| `applications` | `data/applications.md`, `data/pipeline.md` | `id`, `job_description_id`, `status`, `applied_at`, `notes`, `match_score`, `evaluation_report_id` | The core application pipeline. Status enum: `discovered`, `evaluating`, `ready`, `applied`, `interviewing`, `rejected`, `offer`. |
| `evaluation_reports` | `reports/*.md` | `id`, `application_id`, `content` (markdown), `generated_at`, `model_used` | Stores AI-generated evaluation reports. |
| `cover_letters` | `output/cover_*.md` | `id`, `application_id`, `content` (markdown), `generated_at` | Stores generated cover letters. |
| `portals` | `portals.yml` | `id`, `company_name`, `url`, `render_mode`, `scraper_config` (JSON), `last_verified_at` | Configuration for job acquisition. `render_mode` is `static` or `browser` (see §4). |
| `prompts` | `modes/*.md` | `id`, `name`, `category` (evaluate/cover/scan), `content`, `is_system`, `version`, `created_at`, `updated_at` | Replaces the Markdown prompt files (see §5). |
| `settings` | N/A | `key`, `value` (JSON) | Application-wide settings (API keys stored via OS keychain, not here). |

**Design Considerations:**
- Use `TEXT` for JSON columns and validate with `serde_json` on read.
- All `created_at` / `updated_at` columns should use `TEXT` in ISO 8601 format for `sqlx` compatibility.
- Foreign keys enforced with `PRAGMA foreign_keys = ON`.
- Indexes on `applications.status`, `applications.job_description_id`, and `prompts.category`.
- `portals.render_mode` is the routing key for §4. Per-portal acquisition strategy belongs in the database, not in code, so users can tune it without a rebuild.

---

## 2. Business Logic Porting (Node.js → Rust)

The original project has approximately **70 `.mjs` scripts** at the repository root, each responsible for a single job. These need to be ported into Rust modules.

| Node.js Script Area | Rust Module | Responsibility |
| :--- | :--- | :--- |
| `scan.mjs`, `scan-all.mjs` | `core::acquisition` | Acquire job postings via the tiered pipeline in §4. Tier selection, retry, and escalation logic live here. |
| `evaluate.mjs`, `evaluate-batch.mjs` | `core::evaluation` | Send job descriptions and CV to the LLM via `rig`; parse and store evaluation reports. |
| `cover-letter.mjs` | `core::cover_letter` | Generate tailored cover letters using LLM prompts. |
| `pdf.mjs` | `core::pdf` | Render CV and cover letters to PDF using `printpdf`. |
| `pipeline.mjs`, `dashboard.mjs` | `core::pipeline` | Manage application status transitions and pipeline analytics. |
| `update-system.mjs` | `core::updater` | Handle schema migrations and prompt seed updates. |
| `profile.mjs` | `core::profile` | CRUD operations for candidate profile and CV data. |
| Various utility scripts | `core::util` | Date parsing, text extraction, file I/O helpers, etc. |

**Architectural Note:** The original tool is AI-agnostic and relies on external CLI tools for logic. The Rust port embeds this logic directly by calling LLM APIs through `rig`, which provides a unified interface across multiple providers.

---

## 3. GPUI UI Screens

The UI will be built with GPUI's hybrid immediate/retained mode architecture, using `Entity` for state management and `Render` traits for declarative views.

| Screen | Primary Data Sources | Key Components |
| :--- | :--- | :--- |
| **Dashboard** | `applications`, `evaluation_reports` | Pipeline summary cards (counts by status), recent activity feed, match score distribution chart. |
| **Job Pipeline** | `applications`, `job_descriptions` | Virtualized table with status filter tabs, sort modes, inline status changes, lazy-loaded previews. |
| **Job Detail** | `applications`, `job_descriptions`, `evaluation_reports` | Full job posting text, evaluation report viewer, match score breakdown, action buttons (Evaluate, Generate Cover Letter, Mark as Applied). |
| **CV / Profile Editor** | `profile`, `cv_entries` | Form-based editor for personal details, experience entries, education, skills. Live markdown preview. |
| **Portals Configuration** | `portals` | List of job boards with add/edit/delete. Per-portal `render_mode` selector and scraper configuration. Displays acquisition health. |
| **Prompt Manager** | `prompts` | Browse, edit, and version prompt templates. System prompts are read-only; user prompts are editable. |
| **Settings** | `settings` | API key management (OS keychain), model provider selection, PDF template selection, data export. |

**GPUI-Specific Considerations:**
- Use `gpui-kit` (or the current equivalent) as the primary dependency for production-ready UI primitives.
- The pipeline table requires virtualized rows for large application histories. Verify this capability early — it was a disqualifying gap for at least one comparable framework.
- Prefer GPUI's built-in test support for view-level tests rather than driving the UI externally.

---

## 4. Job Acquisition: Tiered Scraping Architecture

The original project used Playwright for both job board scraping and HTML-to-PDF generation. This plan separates those two concerns entirely:

- **PDF generation** is handled by `printpdf` (§6) and has no browser dependency.
- **Job acquisition** uses a tiered architecture where a lightweight HTTP path is the default and a headless browser is an opt-in, feature-gated escalation.

This decoupling is the primary bundle-size win over the original project.

### Tier 1 — Default: `reqwest` + `scraper`

The default acquisition path. Always compiled in, zero browser dependency.

| Crate | Role |
| :--- | :--- |
| `reqwest` | Async HTTP client. |
| `scraper` | HTML parsing with CSS selectors. |

**Handles:** Server-rendered job boards and ATS platforms that return complete HTML (Greenhouse, Lever, Ashby, and similar). This is expected to cover the large majority of real-world portals.

**Does not handle:** Client-side-rendered SPAs, or portals that block non-browser clients at the TLS/HTTP2 fingerprint level.

**Bundle impact:** ~3–5 MB (`reqwest` + TLS + `scraper`).

### Tier 2 — Feature-gated: Headless Browser

Enabled via a Cargo feature flag (e.g. `browser-fetch`). Used only when Tier 1 fails or when a portal is explicitly configured with `render_mode = 'browser'`.

**Candidate backends (not yet decided):**

| Backend | Engine | Rationale |
| :--- | :--- | :--- |
| Chrome-based (e.g. `rust-headless-chrome`, or a Chrome-over-CDP client) | V8 | Maximum web-platform compatibility. Highest bundle cost. |
| Rust-native headless browser (e.g. Obscura, OxiBrowser) | V8 or pure-Rust JS engine | Lower bundle and memory footprint; compatibility risk. |
| System-installed Chrome over CDP | V8 | Zero bundled binary; requires the user to have Chrome installed. |

> **⚠️ Verification required before locking this in.** The Rust headless-browser ecosystem is young and fast-moving. The specific project names, licenses, benchmark figures, JS-engine choices, and CDP completeness claims must be re-verified against current crates.io and GitHub state before any dependency is chosen. Do not treat any named project in this table as a confirmed decision. See §11, Risk R-2.

**Selection criteria, in priority order:**
1. **JS execution fidelity** on real, modern SPAs. This tier exists *specifically* to handle sites Tier 1 can't. If the fallback fails on those sites, it has no reason to exist.
2. **Bundle size delta** over the Tier 1 baseline.
3. **Process model.** Can it run as a sidecar process, or must it be linked into the binary? A sidecar is preferable — it keeps the main binary lean and isolates crashes.
4. **Stealth / anti-detection** maturity, if target portals use bot management.
5. **Maintenance trajectory.** Commit recency, issue responsiveness, bus factor.

**Bundle impact:** TBD, pending backend selection. This is the single largest variable in the final bundle size. See §9.

### Escalation and Routing Logic

Routing is driven by `portals.render_mode`, with a runtime fallback:

1. Read `render_mode` from the `portals` row.
2. If `static`, run Tier 1. On empty/failed extraction, escalate to Tier 2 if compiled in; otherwise surface a clear, actionable error.
3. If `browser`, go directly to Tier 2.
4. Record the tier that actually succeeded in `job_descriptions.fetch_tier`, and update `portals.last_verified_at`.
5. Optionally suggest a `render_mode` change to the user when escalation consistently succeeds (Tier 1 keeps failing) or consistently fails (Tier 2 unnecessary).

This keeps routing data-driven and lets the app self-tune its own configuration over time.

### Deferred: HTTP-Level Stealth Layer

An intermediate tier using TLS/HTTP2 browser impersonation (e.g. `impit`, `crawlingo`, or equivalent) would handle portals that block plain `reqwest` but don't require JS execution. **Not in scope for v1.** Worth revisiting only if Tier 1 failures are dominated by fingerprint-based blocking rather than JS rendering. Adding a fourth path before measuring is premature complexity.

### Future Extension: CDP-Agnostic Browser Backend

Design the Tier 2 layer against the **Chrome DevTools Protocol** rather than against any specific Rust crate's API. A thin internal trait — something like:

```rust
trait BrowserBackend {
    async fn fetch_rendered(&self, url: &Url, opts: RenderOpts) -> Result<RenderedPage>;
}
```

with CDP-based and library-native implementations behind it, gives several benefits:

- **Deferred commitment.** The browser engine can be swapped, or added as a user-selectable option, without touching acquisition logic.
- **User choice.** Power users could point the app at system Chrome, a bundled engine, or a remote browser service.
- **Testability.** A mock backend makes acquisition tests fast and hermetic, with no browser binary required in CI.

This should be treated as a design constraint on the Tier 2 layer *from the start*, even if only one backend ships in v1. Retrofitting an abstraction over a concrete dependency is significantly more expensive than building it in.

---

## 5. AI Logic & Prompt Management

**Seed Data Strategy:**
- Embed the original `modes/*.md` prompt files as Rust embedded resources (`include_str!` or a build script).
- On first startup, if the `prompts` table is empty, seed it with the embedded system prompts.
- System prompts are marked `is_system = true` and are **read-only** in the UI.

**User Prompt Management:**
- Users can create new prompts or edit existing non-system prompts.
- The `version` column enables a simple versioning scheme: each edit creates a new row; the UI displays the latest by default.
- Prompts are executed via `rig`, which supports multi-turn streaming and agentic workflows.

**Deferred — to be flushed out separately:**
- Prompt variable/templating system (e.g. `{{cv_summary}}`, `{{job_description}}`).
- Prompt chaining for multi-stage evaluation pipelines.
- A/B testing of prompt variants against historical application data.
- Prompt export/import, for portability and sharing.

**Open question:** how prompt edits interact with historical evaluation reports. If a report was generated with prompt v1 and the user edits to v2, the stored report must remain reproducible. Storing the prompt version (or a content hash) alongside each `evaluation_report` is the likely answer, but this needs a decision before Phase 3.

---

## 6. PDF Generation with `printpdf`

`printpdf` is a pure-Rust library for creating, reading, writing, and rendering PDF documents. It supports pages, bookmarks, layers, graphics, images, embedded fonts, and Unicode.

**Critical Caveat:**

> HTML-to-PDF rendering in `printpdf` is experimental / WIP. Element positioning is manual.

This means the existing HTML templates **cannot be reused**. The PDF module must be written from scratch against `printpdf`'s imperative API. The PDF output is a key differentiator (ATS-friendly resumes), so this is a **high-risk, high-effort** component.

**Mitigation Strategy:**
- **Prototype this first**, before committing to the rest of the architecture. A throwaway spike that renders one realistic resume page will de-risk the highest-uncertainty item in the plan.
- If `printpdf` proves insufficient, the fallback is HTML templating plus a browser-based renderer — which would then pull the Tier 2 browser backend into the critical path and change the bundle-size calculus in §9. This is the main reason the Tier 2 abstraction (§4) matters more than it first appears.
- Evaluate `svg2pdf` (used internally by `printpdf`) for complex graphic elements.

**Decision gate:** Phase 4 must not begin until the Phase 1 spike has produced a satisfactory page. If it hasn't, revisit §4 and §9 together.

---

## 7. Security & Data Encryption

**OS Keychain for Secrets:**
- Use the `keyring` crate to store API keys and tokens in the OS-native secure store (Windows Credential Manager, macOS Keychain, libsecret on Linux).
- Alternative: `db-keystore`, a SQLite-backed credential store with optional encryption-at-rest, built on the `keyring-core` API.

**Encryption at Rest — Evaluation:**

| Aspect | Recommendation |
| :--- | :--- |
| **CV & Profile Data** | **Encrypt.** PII. Users expect career history to be protected. |
| **Job History & Applications** | **Encrypt.** Reveals career trajectory; sensitive. |
| **Prompt Templates** | **No encryption needed.** Non-sensitive system data. |
| **Settings** | **Selectively encrypt.** Secrets belong in the OS keychain, not the DB. |

**Implementation Approach:**
- Use SQLCipher (via `libsqlite3-sys` with the `sqlcipher` feature) or an equivalent SQLite encryption layer.
- Derive the encryption key from a user passphrase using Argon2id, or store it in the OS keychain and unlock automatically on launch.
- Consider `enigma-storage` for encrypted local storage with pluggable key providers.

**Open question:** whether to make encryption default-on or opt-in. Default-on is the safer posture but introduces a passphrase-recovery problem with real data-loss potential. This needs an explicit decision, including a documented answer to "what happens if the user forgets their passphrase?"

**Additional consideration:** if Tier 2 uses a sidecar browser process, ensure no credentials or session tokens are passed to it, and that any browser profile directory is created in a location the user can inspect and delete.

---

## 8. Testing & Validation

A **full suite of new tests** is required. No Node.js tests port over directly.

| Test Type | Scope | Tools |
| :--- | :--- | :--- |
| **Unit Tests** | Business logic (evaluation, pipeline, PDF layout, tier routing) | `#[test]`, `rstest`, `mockall` |
| **Database Tests** | Migrations, CRUD, query correctness | `sqlx::test` with in-memory SQLite |
| **UI Tests** | GPUI views and state transitions | GPUI test support |
| **Acquisition Tests** | Tier 1 parsing against saved HTML fixtures; tier routing logic; escalation behavior | Saved fixtures + mock `BrowserBackend` |
| **Integration Tests** | End-to-end flows (fetch → evaluate → cover letter → PDF) | `assert_cmd` or custom harness |
| **LLM Evaluation Tests** | Prompt output consistency | `rig` evaluation facilities, if suitable |
| **PDF Validation** | Layout correctness and text extractability | Text extraction + visual regression |

**Key testing principle:** Tier 1 acquisition tests should run against **saved HTML fixtures**, not live network calls. This keeps CI hermetic and fast, and turns every real-world portal failure into a regression fixture. The `BrowserBackend` trait (§4) enables the same approach for Tier 2.

---

## 9. Performance & Bundle Size Assessment

**Performance:**
- **GPUI** is GPU-accelerated and benchmarked favorably against WebView-based alternatives on CPU and memory. Verify with your own benchmarks rather than relying on third-party numbers.
- **`sqlx` + SQLite** will not be a bottleneck at desktop data volumes, despite SQLite's synchronous core.
- **`rig`** performance is dominated by network latency, not client overhead.

**Bundle Size — Revised:**

| Component | Estimated Impact |
| :--- | :--- |
| GPUI application base | ~10–15 MB |
| SQLite (linked) | ~1–2 MB |
| `rig` + provider clients | ~2–5 MB |
| `printpdf` | ~1–2 MB |
| `reqwest` + TLS + `scraper` (Tier 1) | ~3–5 MB |
| **Tier 2 browser backend (feature-gated)** | **TBD — 0 MB if excluded at build time; potentially 40–150 MB if bundled** |
| **Default build total** | **~20–30 MB** |

The headline improvement over v0.1: by making the browser backend feature-gated *and* decoupling PDF generation from it, the **default build ships with no browser binary at all**. Users who never hit a JS-heavy portal never pay the cost. This is a strict improvement over the original project, which bundles Playwright unconditionally.

If a user *does* enable the browser feature, the Tier 2 backend will dominate their bundle regardless of which engine is chosen. The choice of backend therefore affects the size of an opt-in build, not the default.

---

## 10. Human-in-the-Loop Principle

The original tool **never submits applications on the user's behalf**. This principle is preserved.

**Implementation:**
- All state-modifying actions (e.g. marking as "Applied") require explicit confirmation via a dialog.
- No job board credentials are stored or used for automated submission.
- A future "Submit" action may be added, but it must require manual confirmation and never auto-submit.

This principle should be asserted in tests, not just documented — e.g. a test that fails if any code path transitions an application to `applied` without an explicit user action.

---

## 11. Risks & Mitigations

| ID | Risk | Impact | Mitigation |
| :--- | :--- | :--- | :--- |
| R-1 | `printpdf` HTML-to-PDF is experimental; manual layout may not reach required fidelity | **High** | Prototype in Phase 1 before committing. Fallback pulls in a browser renderer, which cascades into §4 and §9. |
| R-2 | **Unverified browser backend landscape.** Named Rust headless-browser projects, their capabilities, licenses, and metrics have not been verified. | **High** | Verify against crates.io/GitHub before Phase 2. Build the Tier 2 layer against the `BrowserBackend` trait so the choice stays reversible. Consider system-Chrome-over-CDP as a zero-bundle fallback. |
| R-3 | GPUI is pre-1.0; breaking changes tied to Zed's roadmap | Medium | Pin versions; monitor release notes; isolate GPUI behind app-level view types. |
| R-4 | Tier 1 fails on a large fraction of real target portals, making Tier 2 effectively mandatory | Medium | Measure before building. Collect a sample of real target portals and test Tier 1 against them during Phase 1. |
| R-5 | Encryption passphrase loss causes unrecoverable data loss | Medium | Explicit UX decision required (§7). Document recovery story before shipping. |
| R-6 | LLM API surface changes | Low | `rig` abstracts across providers, reducing lock-in. |
| R-7 | Scope creep into prompt tooling before core pipeline works | Medium | Prompt authoring (§5) is explicitly deferred until after Phase 3. |

---

## 12. Suggested Development Phases

| Phase | Scope | Duration (Estimate) |
| :--- | :--- | :--- |
| **Phase 0: Spikes** | Two throwaway spikes: (a) `printpdf` renders a realistic resume page; (b) Tier 1 `reqwest` + `scraper` against 5–10 real target portals. Both are decision gates. | 1–2 weeks |
| **Phase 1: Foundation** | SQLite schema, `sqlx` integration, profile/CV CRUD, basic GPUI shell. | 3–4 weeks |
| **Phase 2: Acquisition** | Tier 1 pipeline, `portals` config UI, `BrowserBackend` trait definition, tier routing, acquisition tests with fixtures. Tier 2 implementation only if Phase 0(b) showed it's needed. | 3–5 weeks |
| **Phase 3: AI Integration** | `rig` integration, prompt seeding and read-only system prompts, evaluation report generation. | 3–4 weeks |
| **Phase 4: Output Generation** | `printpdf` CV/cover letter rendering, template system. Gate on Phase 0(a). | 3–6 weeks (high uncertainty) |
| **Phase 5: Polish & Security** | Encryption at rest, OS keychain, settings screen, full test suite. | 3–4 weeks |
| **Phase 6: Migration & Release** | File-based data migration (deferred enhancement), packaging, documentation. | 2–3 weeks |

**Total Estimate:** 4–6 months for a functional MVP, assuming one experienced Rust developer. The two Phase 0 spikes are the cheapest way to reduce the largest uncertainties, and both can invalidate or reshape later phases.

---

## Appendix A: Decisions Log

| # | Decision | Status |
| :--- | :--- | :--- |
| 1 | SQLite as primary state store (inverting the original file-canonical doctrine) | Confirmed |
| 2 | Port ~70 Node.js scripts into `core::*` Rust modules | Confirmed |
| 3 | GPUI as UI framework | Confirmed |
| 4 | `rig` for LLM integration | Confirmed |
| 5 | `sqlx` (async-first) for SQLite | Confirmed |
| 6 | `printpdf` for PDF generation, pure-Rust preference | Confirmed, pending Phase 0(a) validation |
| 7 | `reqwest` for HTTP | Confirmed |
| 8 | `reqwest` + `scraper` as default acquisition path | Confirmed |
| 9 | Headless browser as feature-gated escalation only | Confirmed |
| 10 | Specific browser backend | **Undecided — verify before Phase 2 (R-2)** |
| 11 | CDP-agnostic `BrowserBackend` trait | Confirmed as design constraint |
| 12 | No `rust-headless-chrome` as a hard dependency | Confirmed (superseded by decision 9) |
| 13 | Prompts seeded from embedded resources, stored in SQLite, user-editable | Confirmed; detail TBD |
| 14 | Data migration from existing files | Deferred to future enhancement |
| 15 | Human-in-the-loop; no automated submission | Confirmed |
| 16 | OS keychain for secrets | Confirmed |
| 17 | Encryption at rest | Confirmed in principle; default-on vs. opt-in TBD |
| 18 | Full new test suite | Confirmed |

---

This plan is a living document. Open items requiring resolution: the browser backend choice (§4, R-2), the `printpdf` fidelity gate (§6), the encryption default and recovery story (§7), and prompt versioning semantics (§5).