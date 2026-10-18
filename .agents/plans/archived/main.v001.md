# Draft Execution Plan: Career-Ops Rust Desktop Application

This document outlines the execution plan for porting the `career-ops` project from its current Node.js, file-based architecture to a Rust desktop application using GPUI, SQLite, and a pure-Rust technology stack. The plan incorporates your stated technical decisions and evaluates the key architectural considerations.

## 1. SQLite Schema Design

The most significant architectural change is inverting the original "files are canonical, databases are derived" doctrine. The new SQLite schema must become the primary, stateful source of truth for all user and system data.

### Proposed Schema

The schema should be designed to replace the file-based data contract. The following tables represent the core entities:

| Table | Replaces File(s) | Key Columns | Notes |
| :--- | :--- | :--- | :--- |
| `profile` | `config/profile.yml`, `modes/_profile.md` | `id`, `name`, `email`, `phone`, `location`, `summary`, `archetypes` (JSON), `preferences` (JSON) | Single-row table for candidate identity and preferences. |
| `cv_entries` | `cv.md` | `id`, `profile_id`, `type` (experience/education/skill), `title`, `organization`, `start_date`, `end_date`, `description`, `metadata` (JSON) | Normalized representation of the CV. |
| `job_descriptions` | `jds/*.md` | `id`, `title`, `company`, `source_url`, `raw_text`, `fetched_at` | Stores fetched job posting content. |
| `applications` | `data/applications.md`, `data/pipeline.md` | `id`, `job_description_id`, `status`, `applied_at`, `notes`, `match_score`, `evaluation_report_id` | The core application pipeline. Status enum: `discovered`, `evaluating`, `ready`, `applied`, `interviewing`, `rejected`, `offer`. |
| `evaluation_reports` | `reports/*.md` | `id`, `application_id`, `content` (markdown), `generated_at`, `model_used` | Stores AI-generated evaluation reports. |
| `cover_letters` | `output/cover_*.md` | `id`, `application_id`, `content` (markdown), `generated_at` | Stores generated cover letters. |
| `portals` | `portals.yml` | `id`, `company_name`, `url`, `scraper_config` (JSON) | Configuration for job board scraping. |
| `prompts` | `modes/*.md` | `id`, `name`, `category` (evaluate/cover/scan), `content`, `is_system`, `version`, `created_at`, `updated_at` | Replaces the Markdown prompt files (see §5). |
| `settings` | N/A | `key`, `value` (JSON) | Application-wide settings (API keys stored via OS keychain, not here). |

**Design Considerations:**
- Use `TEXT` for JSON columns and validate with `serde_json` on read.
- All `created_at` / `updated_at` columns should use `TEXT` in ISO 8601 format for `sqlx` compatibility.
- Foreign keys should be enforced with `PRAGMA foreign_keys = ON`.
- Indexes on `applications.status`, `applications.job_description_id`, and `prompts.category` will be necessary for UI responsiveness.

## 2. Business Logic Porting (Node.js → Rust)

The original project has approximately **70 `.mjs` scripts** at the repository root, each responsible for a single job. These need to be ported into Rust modules. The following table maps the major functional areas:

| Node.js Script Area | Rust Module | Responsibility |
| :--- | :--- | :--- |
| `scan.mjs`, `scan-all.mjs` | `core::scanning` | Fetch job postings from configured portals using `rust-headless-chrome` and `reqwest`. |
| `evaluate.mjs`, `evaluate-batch.mjs` | `core::evaluation` | Send job descriptions and CV to the LLM via `rig`; parse and store evaluation reports. |
| `cover-letter.mjs` | `core::cover_letter` | Generate tailored cover letters using LLM prompts. |
| `pdf.mjs` | `core::pdf` | Render CV and cover letters to PDF using `printpdf`. |
| `pipeline.mjs`, `dashboard.mjs` | `core::pipeline` | Manage application status transitions and pipeline analytics. |
| `update-system.mjs` | `core::updater` | Handle schema migrations and prompt seed updates. |
| `profile.mjs` | `core::profile` | CRUD operations for candidate profile and CV data. |
| Various utility scripts | `core::util` | Date parsing, text extraction, file I/O helpers, etc. |

**Architectural Note:** The original tool is AI-agnostic and relies on external CLI tools for logic. The Rust port will embed this logic directly by calling LLM APIs through `rig`, which provides a unified interface across 20+ providers.

## 3. GPUI UI Screens

The UI will be built with GPUI's hybrid immediate/retained mode architecture, using `Entity` for state management and `Render` traits for declarative views. The main screens should map directly to the business logic modules:

| Screen | Primary Data Sources | Key Components |
| :--- | :--- | :--- |
| **Dashboard** | `applications`, `evaluation_reports` | Pipeline summary cards (counts by status), recent activity feed, match score distribution chart. |
| **Job Pipeline** | `applications`, `job_descriptions` | Virtualized table with status filter tabs, sort modes, inline status changes, lazy-loaded previews. |
| **Job Detail** | `applications`, `job_descriptions`, `evaluation_reports` | Full job posting text, evaluation report viewer, match score breakdown, action buttons (Evaluate, Generate Cover Letter, Mark as Applied). |
| **CV / Profile Editor** | `profile`, `cv_entries` | Form-based editor for personal details, experience entries, education, skills. Live markdown preview. |
| **Portals Configuration** | `portals` | List of job boards with add/edit/delete. Per-portal scraper configuration (CSS selectors, etc.). |
| **Prompt Manager** | `prompts` | Browse, edit, and version prompt templates. System prompts are read-only; user prompts are editable. |
| **Settings** | `settings` | API key management (using OS keychain), model provider selection, PDF template selection, data export. |

**GPUI-Specific Considerations:**
- Use `gpui-kit` as the primary dependency to get a production-ready UI system with application-grade data, layout, and editing capabilities.
- The `gpui-component` layer provides buttons, inputs, dropdowns, tables, dialogs, and notifications out of the box.
- For the pipeline table, GPUI supports virtualized rows, which is essential for handling large application histories.

## 4. Browser Automation with `rust-headless-chrome`

The original project uses Playwright for generating ATS-friendly PDFs from HTML templates and for scraping job postings. The `rust-headless-chrome` crate is the Rust equivalent of Puppeteer, providing a high-level API to control headless Chrome over the DevTools Protocol.

**Primary Use Cases:**
1.  **Job Board Scraping:** Navigate to configured portals, extract job listings, and store them in the `job_descriptions` table.
2.  **PDF Rendering (Fallback):** If `printpdf` cannot achieve the required visual fidelity for ATS-friendly resumes, `rust-headless-chrome` can render HTML templates to PDF.

**Integration Notes:**
- By default, `rust-headless-chrome` downloads a compatible Chrome version. You can disable this behavior and point it to a system-installed Chrome.
- This is a heavy dependency (~100MB+ for the Chrome binary). For bundle size, consider making it an optional feature flag if PDF generation can be fully handled by `printpdf`.

## 5. AI Logic & Prompt Management

The original project stores all AI logic in Markdown prompt files under `modes/`. Your decision to store prompts in SQLite with embedded seed resources is sound. Here is a proposed approach:

**Seed Data Strategy:**
- Embed the original `modes/*.md` prompt files as Rust embedded resources using `include_str!` or a build script that generates a Rust module.
- On first startup, the application checks if the `prompts` table is empty. If so, it seeds it with the embedded system prompts.
- System prompts are marked with `is_system = true` and are **read-only** in the UI.

**User Prompt Management:**
- Users can create new prompts or edit existing non-system prompts.
- The `version` column enables a simple versioning scheme: each edit creates a new row, and the UI displays the latest version by default.
- Prompts are executed via `rig`, which supports agentic workflows with multi-turn streaming and prompting.

**Future Enhancement (to be flushed out later):**
- Prompt variables/templating system (e.g., `{{cv_summary}}`, `{{job_description}}`).
- Prompt chaining for complex evaluation pipelines.
- A/B testing of prompt variants against historical application data.

## 6. PDF Generation with `printpdf`

`printpdf` is a pure-Rust library for creating, reading, writing, and rendering PDF documents. It supports pages, bookmarks, layers, graphics (lines, shapes, bezier curves, SVG), images, embedded fonts, and Unicode.

**Critical Caveat:**
> **Important:** HTML-to-PDF rendering in `printpdf` is still experimental (a stub API) and Work-in-Progress. You need to position PDF elements manually for now.

This means you **cannot** reuse the existing HTML templates from the original project. The PDF generation module will need to be rewritten from scratch using `printpdf`'s imperative API. The original tool's PDF output is a key differentiator (ATS-friendly resumes), so this is a **high-risk, high-effort** component.

**Mitigation Strategy:**
- Prototype the PDF layout early to validate that `printpdf` can achieve the required visual fidelity.
- If the pure-Rust approach proves insufficient, `rust-headless-chrome` can be used as a fallback for HTML-to-PDF rendering.
- Consider using `svg2pdf` (which `printpdf` uses internally) for complex graphic elements.

## 7. Security & Data Encryption

**OS Keychain for Secrets:**
- Use the `keyring` crate to store API keys, tokens, and other credentials in the OS-native secure store (Windows Credential Manager, macOS Keychain, libsecret on Linux).
- The `db-keystore` crate provides a SQLite-backed credential store with optional encryption-at-rest and is built on the `keyring-core` API.

**Encryption at Rest — Evaluation:**
Your instinct to evaluate encrypting the SQLite database is correct. Here is a breakdown:

| Aspect | Recommendation |
| :--- | :--- |
| **CV & Profile Data** | **Encrypt.** This is personally identifiable information (PII). Users would expect their career history to be protected. |
| **Job History & Applications** | **Encrypt.** Application data reveals career trajectory and can be sensitive. |
| **Prompt Templates** | **No encryption needed.** These are non-sensitive system data. |
| **Settings** | **Selectively encrypt.** Only values that are secrets (e.g., API keys) need protection; these should live in the OS keychain, not the DB. |

**Implementation Approach:**
- Use SQLCipher (via `libsqlite3-sys` with the `sqlcipher` feature) or `turso`'s encryption capabilities (as used by `db-keystore`) to encrypt the entire database file.
- Derive the encryption key from a user passphrase using Argon2id, or store it in the OS keychain and unlock it automatically on application launch.
- The `enigma-storage` crate provides encrypted local storage with mandatory at-rest protection and pluggable key providers.

## 8. Testing & Validation

The original project has a test suite in `tests/` with `{module}.test.mjs` naming. The Rust port will require a **full suite of new tests**. The following test categories are recommended:

| Test Type | Scope | Tools |
| :--- | :--- | :--- |
| **Unit Tests** | Business logic modules (evaluation, pipeline, PDF generation) | `#[test]`, `rstest`, `mockall` for mocking `rig` and `reqwest` |
| **Database Tests** | Schema migrations, CRUD operations, query correctness | `sqlx::test` with an in-memory SQLite database |
| **UI Tests** | GPUI views and state transitions | GPUI's built-in test support (`gpui = { features = ["test-support"] }`) |
| **Integration Tests** | End-to-end flows (fetch job → evaluate → generate cover letter → PDF) | `assert_cmd` or custom test harness |
| **LLM Evaluation Tests** | Prompt quality and output consistency | `rig`'s experimental evaluation framework (`rig::evals`) |
| **PDF Validation** | Correctness of generated PDF layout and text extraction | `printpdf`'s text extraction + visual regression testing |

## 9. Performance & Bundle Size Assessment

Your assessment that these decisions will keep performance and bundle size reasonable is **mostly correct, with one significant caveat**.

**Performance:**
- **GPUI** is GPU-accelerated and benchmarked at **77% lower CPU** and **88% less memory** compared to Tauri for similar applications. This is a major strength.
- **sqlx** with SQLite is synchronous under the hood (via `libsqlite3`), but its async interface integrates well with GPUI's runtime. For a desktop app, database performance will not be a bottleneck.
- **`rig`** is designed for ergonomics and modularity with minimal boilerplate. LLM calls are network-bound, so performance is dominated by API latency, not the Rust client.

**Bundle Size:**
| Component | Estimated Size Impact |
| :--- | :--- |
| GPUI Application | ~12 MB base |
| SQLite (linked) | ~1–2 MB |
| `rig` + provider clients | ~2–5 MB |
| `printpdf` | ~1–2 MB |
| `reqwest` + TLS | ~3–5 MB |
| **`rust-headless-chrome` (Chrome binary)** | **~100–150 MB** |

**Caveat:** If `rust-headless-chrome` is used for scraping and PDF generation, it will **dominate the bundle size**. The original project bundles Playwright for the same purpose, so this is not a regression, but it's worth noting. If `printpdf` can fully replace HTML-to-PDF rendering, `rust-headless-chrome` could be made an optional feature or removed entirely.

## 10. Human-in-the-Loop Principle

The original tool's core principle is that it **never submits applications on the user's behalf**; it prepares and evaluates, and the human reviews and clicks. This principle must be preserved in the Rust port.

**Implementation:**
- All actions that modify application state (e.g., marking as "Applied") require explicit user confirmation via a dialog.
- No credentials for job boards are ever stored or used for automated submission.
- A future "Submit" button can be added as an enhancement, but it must trigger a manual confirmation flow and never auto-submit.

## 11. Risks & Mitigations

| Risk | Impact | Mitigation |
| :--- | :--- | :--- |
| `printpdf` HTML-to-PDF is experimental | High | Prototype early; fall back to `rust-headless-chrome` for HTML rendering. |
| GPUI pre-1.0 breaking changes | Medium | Pin to a specific GPUI version; monitor Zed's release notes; use `gpui-kit` which provides a stable facade. |
| `rust-headless-chrome` bundle size | Medium | Make it an optional feature; prefer `printpdf` if possible. |
| LLM API changes | Low | `rig` provides a unified interface across 20+ providers, reducing provider lock-in. |
| SQLite encryption performance | Low | SQLCipher adds minimal overhead for desktop-scale data volumes. Test with realistic data sizes. |

## 12. Suggested Development Phases

| Phase | Scope | Duration (Estimate) |
| :--- | :--- | :--- |
| **Phase 1: Foundation** | SQLite schema, `sqlx` integration, profile/CV CRUD, basic GPUI shell. | 3–4 weeks |
| **Phase 2: Core Pipeline** | Job fetching (`rust-headless-chrome` + `reqwest`), application tracking UI, status transitions. | 4–6 weeks |
| **Phase 3: AI Integration** | `rig` integration, prompt seeding/management, evaluation report generation. | 3–4 weeks |
| **Phase 4: Output Generation** | `printpdf` CV/cover letter rendering, template system. | 4–6 weeks (high uncertainty) |
| **Phase 5: Polish & Security** | Encryption at rest, OS keychain, settings screen, testing suite. | 3–4 weeks |
| **Phase 6: Migration & Release** | Data migration from files (if needed), packaging, documentation. | 2–3 weeks |

**Total Estimate:** 4–6 months for a functional MVP, assuming one experienced Rust developer.

---

This plan is a living document. The prompt management approach (item 11) is marked for a follow-up discussion, and the PDF generation strategy (item 7) is the highest-risk component that should be prototyped first. Would you like to drill deeper into any specific section before proceeding?