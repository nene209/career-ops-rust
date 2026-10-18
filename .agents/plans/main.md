# Draft Execution Plan: Career-Ops Rust Desktop Application

**Status:** Draft — v0.5
**Last updated:** 2026-10-17

This document outlines the execution plan for porting the `career-ops` project from its current Node.js, file-based architecture to a Rust desktop application using GPUI, SQLite, and a primarily pure-Rust technology stack. The plan incorporates the stated technical decisions and evaluates the key architectural considerations.

### Change Log

| Version | Change |
| :--- | :--- |
| v0.1 | Initial draft. |
| v0.2 | Replaced single-browser scraping approach with tiered architecture (`reqwest` + `scraper` default, headless browser feature-gated). Removed `rust-headless-chrome` as a hard dependency. Added CDP-agnostic backend abstraction as a future extension. Added dependency-verification caveat. |
| v0.3 | Made PDF generation backend-agnostic via a `PdfBackend` trait, mirroring `BrowserBackend`. Selected `rust-headless-chrome` as the initial Tier 2 and PDF backend. Demoted `printpdf` from default PDF path to a medium-term alternative backend. Restructured §6. Lowered R-1 severity; raised the profile of the slimming path. |
| v0.4 | Encryption at rest is now mandatory with no unencrypted mode. Replaced direct keychain-stored DB key with envelope encryption (DEK wrapped by a keychain-stored KEK, plus a passphrase-derived recovery path). Added "never re-initialize on key loss" rule. Added `keyring` crate feature-flag hazard. Revised R-5; added R-10 and R-11. |
| v0.5 | Phase 0 skipped; decisions made directly. Tier 1 confirmed insufficient for target portals: browser is **required**, not feature-gated. Obscura chosen as initial `BrowserBackend` (scraping only). Obscura cannot render PDFs — no layout/paint engine — so `rust-headless-chrome` is the initial `PdfBackend`. Two-browser architecture documented; single-browser alternative flagged as an open question. `printpdf` confirmed as post-v1 (Track A). Bundle size expectations revised to ~190–250 MB. |

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
| `evaluation_reports` | `reports/*.md` | `id`, `application_id`, `content` (markdown), `generated_at`, `model_used`, `prompt_version` | Stores AI-generated evaluation reports. `prompt_version` supports reproducibility (see §5). |
| `cover_letters` | `output/cover_*.md` | `id`, `application_id`, `content` (markdown), `generated_at` | Stores generated cover letters. |
| `templates` | (new) | `id`, `name`, `kind` (cv/cover_letter), `content` (HTML), `version`, `content_hash`, `is_system`, `supersedes_id`, `created_at`, `updated_at` | HTML templates for PDF rendering (see §6). |
| `document_renders` | `output/*.pdf` | `id`, `application_id`, `kind` (cv/cover_letter), `backend_used`, `template_id`, `rendered_at` | Records which backend produced a given PDF. Useful for regression tracking across backend changes. |
| `portals` | `portals.yml` | `id`, `company_name`, `url`, `render_mode`, `scraper_config` (JSON), `last_verified_at` | Configuration for job acquisition. `render_mode` is `static` or `browser` (see §4). |
| `prompts` | `modes/*.md` | `id`, `name`, `category` (evaluate/cover/scan), `content`, `is_system`, `version`, `created_at`, `updated_at` | Replaces the Markdown prompt files (see §5). |
| `settings` | N/A | `key`, `value` (JSON) | Application-wide settings. Holds backend *preferences* among other config. API keys live in the OS keychain, not here. |

**Design Considerations:**
- Use `TEXT` for JSON columns and validate with `serde_json` on read.
- All `created_at` / `updated_at` columns should use `TEXT` in ISO 8601 format for `sqlx` compatibility.
- Foreign keys enforced with `PRAGMA foreign_keys = ON`.
- Indexes on `applications.status`, `applications.job_description_id`, and `prompts.category`.
- `portals.render_mode` is the routing key for §4. Per-portal acquisition strategy belongs in the database, not in code, so users can tune it without a rebuild.
- `settings` holds backend *preferences*, not backend *availability*. Availability is a compile-time property (see §4 and §6).
- **The wrapped DEK is not stored in this database.** Storing the key material that unlocks the database inside the database is a chicken-and-egg problem. It lives in a sibling file on disk. See §7.

**`fetch_tier` values (recorded per listing):**
- `tier1a:greenhouse` / `tier1a:lever` / `tier1a:ashby` / `tier1a:smartrecruiters` — ATS JSON API
- `tier1b:html` — HTML extraction (JSON-LD or CSS selectors)
- `tier2:obscura` — browser-based extraction (primary)
- `tier2:rust-headless-chrome` — browser-based extraction (fallback)
- `manual` — user-entered

---

## 2. Business Logic Porting (Node.js → Rust)

The original project has approximately **70 `.mjs` scripts** at the repository root, each responsible for a single job. These need to be ported into Rust modules.

| Node.js Script Area | Rust Module | Responsibility |
| :--- | :--- | :--- |
| `scan.mjs`, `scan-all.mjs` | `core::acquisition` | Acquire job postings via the tiered pipeline in §4. Tier selection, retry, and escalation logic live here. |
| `evaluate.mjs`, `evaluate-batch.mjs` | `core::evaluation` | Send job descriptions and CV to the LLM via `rig`; parse and store evaluation reports. |
| `cover-letter.mjs` | `core::cover_letter` | Generate tailored cover letters using LLM prompts. |
| `pdf.mjs` | `core::render` | Render CV and cover letters to PDF via the `PdfBackend` abstraction in §6. |
| `pipeline.mjs`, `dashboard.mjs` | `core::pipeline` | Manage application status transitions and pipeline analytics. |
| `update-system.mjs` | `core::updater` | Handle schema migrations and prompt seed updates. |
| `profile.mjs` | `core::profile` | CRUD operations for candidate profile and CV data. |
| Various utility scripts | `core::util` | Date parsing, text extraction, file I/O helpers, etc. |
| (new) | `core::keystore` | Envelope encryption, key derivation, keychain integration, unlock flows. See §7. |
| (new) | `core::llm` | Provider abstraction via `rig`. See Phase 3. |
| (new) | `core::template` | HTML template rendering for PDF. See §6. |

**Architectural Note:** The original tool is AI-agnostic and relies on external CLI tools for logic. The Rust port embeds this logic directly by calling LLM APIs through `rig`, which provides a unified interface across multiple providers.

---

## 3. GPUI UI Screens

The UI will be built with GPUI's hybrid immediate/retained mode architecture, using `Entity` for state management and `Render` traits for declarative views.

| Screen | Primary Data Sources | Key Components |
| :--- | :--- | :--- |
| **Unlock** | `core::keystore` | Passphrase entry when keychain is unavailable or the user opts into passphrase unlock. First-run key generation wizard. Recovery flow. |
| **Dashboard** | `applications`, `evaluation_reports` | Pipeline summary cards (counts by status), recent activity feed, match score distribution chart. |
| **Job Pipeline** | `applications`, `job_descriptions` | Virtualized table with status filter tabs, sort modes, inline status changes, lazy-loaded previews. |
| **Job Detail** | `applications`, `job_descriptions`, `evaluation_reports` | Full job posting text, evaluation report viewer, match score breakdown, action buttons (Evaluate, Generate Cover Letter, Mark as Applied). |
| **CV / Profile Editor** | `profile`, `cv_entries` | Form-based editor for personal details, experience entries, education, skills. Live markdown preview. PDF preview pane driven by the active `PdfBackend`. |
| **Portals Configuration** | `portals` | List of job boards with add/edit/delete. Per-portal `render_mode` selector and scraper configuration. Displays acquisition health and the tier that succeeded. |
| **Prompt Manager** | `prompts` | Browse, edit, and version prompt templates. System prompts are read-only; user prompts are editable. |
| **Settings** | `settings` | API key management (OS keychain), model provider selection, PDF template selection, key rotation, data export. |

**GPUI-Specific Considerations:**
- Use `gpui-kit` (or the current equivalent) as the primary dependency for production-ready UI primitives.
- The pipeline table requires virtualized rows for large application histories. Verify this capability early — it was a disqualifying gap for at least one comparable framework.
- Prefer GPUI's built-in test support for view-level tests rather than driving the UI externally.
- **Unlock must gate the app.** The database cannot be opened without a valid key (§7). The UI must handle "cannot unlock" as a first-class state with a recovery path, never as a crash.
- **Backend selection UI reflects the two-backend architecture.** The `BrowserBackend` and `PdfBackend` are separate selections. The UI should explain what each does (scraping vs rendering) rather than presenting a single "browser backend" option.

---

## 4. Job Acquisition: Tiered Scraping Architecture

The original project used Playwright for both job board scraping and HTML-to-PDF generation. This plan separates those two concerns entirely:

- **Job acquisition** uses a tiered architecture where Tier 1 is an optimization and Tier 2 is **required**.
- **PDF generation** uses a **different backend** from scraping (see §6).

### Tier 1 — Optimization: `reqwest` + `scraper` / ATS JSON APIs

Tier 1 remains valuable for speed and reliability where it works. It is **not** the primary path.

| Crate | Role |
| :--- | :--- |
| `reqwest` | Async HTTP client. |
| `scraper` | HTML parsing with CSS selectors. |

**Handles:** Server-rendered job boards and ATS platforms that return complete HTML or JSON (Greenhouse, Lever, Ashby, SmartRecruiters, and similar).

**Does not handle:** Client-side-rendered SPAs, or portals that block non-browser clients at the TLS/HTTP2 fingerprint level. **These are the target portals.** Direct investigation confirmed that Tier 1 is insufficient for the intended deployment.

**Bundle impact:** ~3–5 MB (`reqwest` + TLS + `scraper`).

### Tier 2 — Required: Headless Browser (Obscura)

Compiled in by default. The browser backend is **not feature-gated** for the intended deployment.

**Initial backend: Obscura.**

Obscura is a headless browser engine written in Rust. It runs JavaScript via V8, exposes the Chrome DevTools Protocol, and is designed as a drop-in replacement for headless Chrome with Puppeteer and Playwright.

**Reported characteristics** (to be verified before Phase 2 commitment):

| Metric | Obscura (reported) | Headless Chrome (typical) |
| :--- | :--- | :--- |
| Memory per instance | ~30 MB | 200+ MB |
| Binary size | ~70 MB | 300+ MB |
| Page load | ~85 ms | ~500 ms |
| CDP support | Yes | Yes |

**Critical limitation: Obscura has no layout or paint engine.**

Obscura parses HTML, builds a DOM, and runs JavaScript. It does not render pixels. Per current project documentation, `Page.printToPDF` and `Page.captureScreenshot` are intentionally unsupported, and `printToPDF` returns an explicit "not supported" error directing the caller to extract rendered HTML and render it elsewhere.

This makes Obscura suitable for **scraping** — which needs JS execution and DOM access, not pixel rendering — and **not suitable for PDF generation**. See §6.

> **⚠️ Verification required.** The claim that Obscura cannot render PDFs came from search results, not from direct source inspection. Confirm this against current Obscura documentation before Phase 2 commits. If Obscura has since gained a rendering engine, the §6 backend choice may be revisited. If the claim holds, the two-backend architecture in §6 is correct.

**Fallback backend for scraping: `rust-headless-chrome`.**

If Obscura proves unsuitable for scraping (compatibility, stealth, or coverage gaps), switch the `BrowserBackend` implementation to `rust-headless-chrome`. The trait makes this a contained change. Note that `rust-headless-chrome` is already required for PDF rendering (§6), so the fallback does not add a new binary.

### Open Question: One Browser or Two?

Since `rust-headless-chrome` (and its bundled Chrome binary) is required for PDF rendering regardless, there is a legitimate alternative: **use `rust-headless-chrome` for both scraping and rendering**, and drop Obscura entirely.

| Approach | Pros | Cons |
| :--- | :--- | :--- |
| **Two binaries** (Obscura + Chrome) | Lighter runtime for scraping; faster page loads; smaller memory per scraping instance | Two browser binaries to package and maintain; ~190–250 MB total bundle |
| **One binary** (Chrome only) | One browser binary; ~100–150 MB total bundle; simpler packaging and testing | Heavier runtime for scraping; slower per-instance; larger memory footprint per session |

For a desktop application scraping 10–20 portals sequentially, the runtime advantage of Obscura may not justify the second binary. The argument for two binaries is stronger if scraping is heavily parallelized or runs frequently.

**Recommendation:** Start with the two-binary architecture as planned, because the `BrowserBackend` trait makes the one-binary variant cheap to evaluate. Measure bundle size and scraping performance during Phase 2. If the two-binary setup proves unjustified, collapse to a single `rust-headless-chrome` backend and revisit §9.

**Selection criteria for future backends, in priority order:**
1. **JS execution fidelity** on real, modern SPAs.
2. **Scraping coverage** against target portals.
3. **Bundle size delta** over the current baseline.
4. **Process model.** Sidecar process preferred over linked-in.
5. **Maintenance trajectory.**

### Escalation and Routing Logic

Routing is driven by `portals.render_mode`, with a runtime fallback:

1. Read `render_mode` from the `portals` row.
2. If `static`, run Tier 1. On failure or `js_required` classification, escalate to Tier 2 (always available).
3. If `browser`, go directly to Tier 2.
4. Record the tier that actually succeeded in `job_descriptions.fetch_tier`, and update `portals.last_verified_at`.
5. Optionally suggest a `render_mode` change when escalation consistently succeeds or consistently fails.

Escalation is unconditional. There is no "Tier 2 not compiled in" case. Tier 1 is an optimization: when it works, it is faster and lighter; when it fails, the browser is the normal path, not a last resort.

### Deferred: HTTP-Level Stealth Layer

An intermediate tier using TLS/HTTP2 browser impersonation (e.g. `impit`, `crawlingo`, or equivalent) would handle portals that block plain `reqwest` but don't require JS execution. **Not in scope for v1.** Worth revisiting only if Tier 1 failures are dominated by fingerprint-based blocking rather than JS rendering. Adding a fourth path before measuring is premature complexity.

---

## 4a. The `BrowserBackend` Trait

Design the Tier 2 layer against the **Chrome DevTools Protocol** rather than against any specific Rust crate's API.

```rust
#[async_trait]
trait BrowserBackend: Send + Sync {
    /// Navigate to a URL, wait for the page to settle, return the
    /// post-JavaScript DOM as HTML.
    async fn fetch_rendered(
        &self,
        url: &Url,
        opts: RenderOpts,
    ) -> Result<RenderedPage>;

    /// Implementations that cannot render PDFs return PdfError::Unsupported.
    /// See §6 for the PdfBackend trait, which has its own implementations.
    async fn print_to_pdf(
        &self,
        html: &str,
        opts: PdfOptions,
    ) -> Result<Vec<u8>>;
}

struct RenderedPage {
    final_url: Url,
    html: String,
    status: u16,
}
```

**Why this matters:**

- **Backend swapping for scraping.** Obscura is the initial backend; `rust-headless-chrome` is the fallback. The trait makes the switch contained.
- **Testability.** A mock backend makes acquisition tests fast and hermetic, with no browser binary required in CI. This is the same technique recommended for Tier 1 fixtures in §8.
- **Honest capability signaling.** `print_to_pdf` is part of the trait for interface completeness, but implementations may return `Unsupported`. Obscura does. `rust-headless-chrome` does not. See §6.

**Design constraint, not feature-gate.** The trait is not about making the browser optional — the browser is required. It is about allowing the scraping backend to be swapped without touching acquisition logic.

### Compile-Time vs. Runtime Backend Selection

Backend availability is a **compile-time** property. Cargo feature flags control what is linked into the binary; a backend that is not feature-enabled cannot be added after installation. Runtime configuration (via the `settings` table) can only select among backends that were compiled in.

The default build includes Obscura (scraping) and `rust-headless-chrome` (rendering). Build variants without one or the other are possible but are not the primary artifact.

Dynamic loading of backends as separate shared libraries is a plausible future extension, but requires a stable C ABI, version negotiation, and plugin distribution infrastructure. Defer.

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

**Reproducibility requirement:** every `evaluation_report` records the `prompt_version` (or content hash) that produced it. Without this, editing a prompt silently invalidates the provenance of historical reports. This is a schema-level decision (§1) and must be made before Phase 3.

**Deferred — to be flushed out separately:**
- Prompt variable/templating system (e.g. `{{cv_summary}}`, `{{job_description}}`).
- Prompt chaining for multi-stage evaluation pipelines.
- A/B testing of prompt variants against historical application data.
- Prompt export/import, for portability and sharing.

---

## 6. PDF Generation: Backend-Agnostic Rendering

PDF generation is treated as a **backend-agnostic rendering concern**. It uses a **different backend** from scraping.

### The Hard Constraint

Obscura has no layout or paint engine. It runs JavaScript and builds a DOM, but it does not render pixels. Per current project documentation, `Page.printToPDF` returns an explicit "not supported" error.

This is not a missing feature or a bug. It is a design choice, and it will not change in the timeframe relevant to this plan. **Obscura cannot be the PDF backend.**

See §4 for the verification caveat on this claim.

### The `PdfBackend` Trait

```rust
#[async_trait]
trait PdfBackend: Send + Sync {
    async fn render_pdf(
        &self,
        html: &str,
        opts: PdfOptions,
    ) -> Result<Vec<u8>>;

    /// Human-readable identifier, recorded in document_renders.backend_used.
    fn name(&self) -> &'static str;

    /// Whether this backend requires a browser binary at runtime.
    /// Used by the UI to explain availability to the user.
    fn requires_browser(&self) -> bool;
}

struct PdfOptions {
    page_size: PageSize,
    margins: Margins,
    print_background: bool,
    prefer_css_page_size: bool,
    header_template: Option<String>,
    footer_template: Option<String>,
    // ...
}
```

### Initial Backend: `rust-headless-chrome`

The v1 `PdfBackend` implementation uses **`rust-headless-chrome`**.

**Why this is the right starting point:**
- **Feature completeness.** CDP `printToPDF` supports page size, margins, header/footer templates, background printing, CSS page-size preference, and page ranges. This covers everything the original Playwright pipeline relied on.
- **Fidelity.** The original tool's HTML/CSS templates can be reused nearly verbatim, because the rendering engine is Chromium — the same engine Playwright drove.
- **Isolation of risk.** The PDF pipeline becomes a known-good path rather than the plan's largest unknown.
- **Obscura cannot serve this role.** No layout engine, no PDF output.

**Why not share a session with Obscura:** Obscura cannot render, so there is no shared session to have. The two backends serve different purposes.

### Relationship Between `PdfBackend` and `BrowserBackend`

`PdfBackend` and `BrowserBackend` are **separate traits** with separate implementations:

| Trait | v1 Implementation | Purpose |
| :--- | :--- | :--- |
| `BrowserBackend` | Obscura (fallback: `rust-headless-chrome`) | Scraping: fetch JS-rendered DOM |
| `PdfBackend` | `rust-headless-chrome` | Rendering: HTML → PDF bytes |

They do not share a process or a session. This is intentional. Obscura is optimized for scraping (small, fast, JS-only). `rust-headless-chrome` is optimized for rendering (full Chromium engine). Coupling them would defeat the purpose of choosing Obscura.

If the Open Question in §4 is resolved in favor of a single browser, both traits would be implemented by `rust-headless-chrome`, and the `PdfBackend` implementation is unchanged.

### The Slimming Path (Post-v1)

The long-term goal of removing the browser dependency from PDF generation remains valid but is **post-v1**. The `printpdf` manual-ops backend (Track A in Phase 7) would provide browser-free PDF rendering. It is not a v1 deliverable.

> **Important caveat on `printpdf`.** Its HTML-to-PDF feature is experimental and geared toward basic layouts. It is not currently a drop-in replacement for Chromium-based rendering, and pixel-exact ATS-friendly resume output is unlikely to be achievable through it today. The slimming path runs through the **manual ops API**, which is a distinct implementation effort — not a configuration change. Verify the current state of both APIs before committing engineering time to either.

### Decision Gate

Phase 4 must verify that `rust-headless-chrome` renders the existing HTML templates acceptably. This is lower risk than the original Obscura-based plan because `rust-headless-chrome` uses full Chromium rendering, not Obscura's incomplete engine.

Specific questions the Phase 4 work must answer:
- Do the existing HTML/CSS templates render acceptably with minimal modification?
- Is the `PdfOptions` coverage sufficient for ATS-friendly output?
- Is a blocking `print_to_pdf` call acceptable in the GPUI event loop when wrapped in `spawn_blocking`?

---

## 7. Security & Data Encryption

### Policy: Encryption at Rest Is Mandatory

There is **no unencrypted mode**. The database is always encrypted at rest. This is not a user-facing toggle, not a build-time feature, and not a configuration setting. Removing the unencrypted path eliminates an entire class of accidental-data-exposure bugs and removes the need to test two storage configurations.

The consequence is that key management becomes a **correctness-critical subsystem**, not an optional hardening layer. A key-management bug can render a user's entire application history unreadable. §7 is therefore written as a design specification, not a recommendation.

### Key Management: Envelope Encryption

The database key is **not** stored directly in the OS keychain. Instead, the design uses envelope encryption, which decouples the database key from the keychain entry and provides a recovery path when the keychain is unavailable.

**Key hierarchy:**

| Layer | Key | Size | Stored where | Purpose |
| :--- | :--- | :--- | :--- | :--- |
| **DEK** (Data Encryption Key) | Random, CSPRNG-generated | 32 bytes | Never stored in plaintext. Only wrapped copies exist on disk. | Passed to SQLCipher via `PRAGMA key`. |
| **KEK-keychain** (Key Encryption Key) | Random, CSPRNG-generated | 32 bytes | OS keychain entry | Wraps the DEK. Primary unlock path. |
| **KEK-passphrase** | Derived from user passphrase via Argon2id | 32 bytes | Derived at unlock time; never stored | Wraps a second copy of the DEK. Recovery path. |

**Why two wrapping paths:** the DEK is wrapped independently by each KEK, producing two wrapped blobs. Either can unwrap the DEK. If the OS keychain becomes unavailable — due to an OS update, a code-signing identity change on macOS, a locked keychain on headless Linux, or a corrupted keychain entry — the user can still unlock their database with their passphrase. Conversely, if the user forgets their passphrase, the keychain path still works. Both must fail for data to be lost.

**On-disk layout:**

```
<app-data-dir>/
  career-ops.db          # SQLCipher-encrypted database
  dek.wrapped.json       # Both wrapped copies of the DEK, plus KDF params
```

`dek.wrapped.json` is a versioned structure. It is **not** stored in the database — storing the material that unlocks the database inside the database is circular. It contains no plaintext key material.

```json
{
  "version": 1,
  "wraps": [
    {
      "method": "keychain",
      "created_at": "2026-10-12T...",
      "nonce": "<base64>",
      "ciphertext": "<base64>"
    },
    {
      "method": "passphrase",
      "created_at": "2026-10-12T...",
      "kdf": { "algo": "argon2id", "salt": "<base64>", "params": { ... } },
      "nonce": "<base64>",
      "ciphertext": "<base64>"
    }
  ]
}
```

Wrapping uses an authenticated cipher (e.g. AES-256-GCM or XChaCha20-Poly1305). The AEAD tag is what makes a corrupted wrap detectable rather than silently producing a wrong key.

### First-Launch Flow

1. Generate a random DEK.
2. Generate a random KEK-keychain. Write it to the OS keychain under a namespaced service/account.
3. Wrap the DEK with KEK-keychain. Record the wrap.
4. Prompt the user for a passphrase. Derive KEK-passphrase via Argon2id with a random salt. Wrap the DEK with it. Record the wrap.
5. Write `dek.wrapped.json`.
6. Create the SQLCipher database using the DEK, run migrations, seed prompts.

The passphrase step is not optional, because it is the only recovery path. The UI must explain *why* it is being asked for, and must not present it as an advanced or skippable setting. A user who declines a passphrase has a database that is one keychain failure away from being unreadable.

### Subsequent-Launch Flow

1. Read `dek.wrapped.json`.
2. **Try keychain path:** fetch KEK-keychain, attempt to unwrap. On success, proceed.
3. **Fall back to passphrase:** if the keychain is unavailable or unwrapping fails, prompt for the passphrase and attempt that wrap.
4. Open the database with the DEK. Immediately run a consistency check (`SELECT 1;`, or better, read a known row from `settings`) to confirm the key is correct before any write occurs.
5. If both unlock paths fail, enter the recovery state described below. **Do not proceed to database initialization.**

### The "Never Re-Initialize" Rule

This is the single most important safety constraint in §7:

> **If a `dek.wrapped.json` exists on disk, the application must never create a new DEK and initialize a fresh database without explicit, informed user confirmation.**

The destructive failure mode is a startup sequence that cannot unlock the database, concludes "there is no database," and re-initializes — silently orphaning the encrypted file. This is how key-management bugs turn into permanent data loss. The rule forbids it:

- If the database file exists but `dek.wrapped.json` is missing → refuse to start, explain, offer to locate or restore the key file.
- If `dek.wrapped.json` exists but no wrap can be opened → refuse to start, offer recovery options, never create a new DEK.
- Only if *neither* file exists is fresh initialization permitted.

"Refuse to start" means a clear UI state with actionable options, not a crash and not a silent fallback.

### Keychain Failure Handling

The `keyring` crate (v3.x) ships with **no default platform backends**. If `apple-native`, `windows-native`, or `linux-native` features are not explicitly enabled in `Cargo.toml`, every read silently returns `NoEntry`. The application will appear to work, but the keychain path will never succeed — and worse, a naive implementation could interpret `NoEntry` as "no key exists" and trigger re-initialization. The "never re-initialize" rule is the backstop against this.

**Mitigations:**
- Explicitly enable all three platform features in `Cargo.toml`.
- Add a startup self-test that writes and reads a sentinel value from the keychain, asserting round-trip persistence. Fail loudly at build or test time if the features are missing.
- Treat `NoEntry` differently from an unwrap failure. `NoEntry` on a first launch is expected; `NoEntry` when `dek.wrapped.json` already contains a keychain wrap is an error state, not an invitation to start over.

### Recovery Path

Recovery is a distinct UI state, not an error dialog. It should offer:

1. **Passphrase unlock** — the primary recovery mechanism.
2. **Retry keychain** — for transient keychain failures.
3. **Locate key file** — for users who moved or restored `dek.wrapped.json` manually.
4. **Export encrypted database** — allow the user to copy the `.db` and `dek.wrapped.json` somewhere safe before attempting anything destructive.
5. **Last resort: start fresh** — available only after the user has been shown the consequences and has confirmed. This must require typing a confirmation phrase, not clicking a button.

### Key Rotation

SQLCipher supports `PRAGMA rekey`, which atomically re-encrypts the database under a new key. Rotation should be offered in Settings for:
- Passphrase change (re-wrap the DEK with a new KEK-passphrase; the DEK itself need not change).
- Full DEK rotation (generate a new DEK, `PRAGMA rekey`, re-wrap).

Passphrase change is the common case and is cheap. Full DEK rotation is expensive on large databases and should be clearly labeled as such.

### Implementation Checklist

| Concern | Action |
| :--- | :--- |
| **`keyring` crate features** | Explicitly enable `apple-native`, `windows-native`, `linux-native`. Add a build-time or test-time assertion that round-trips a sentinel value. |
| **Keychain unavailability** | Fall back to passphrase. Never silently proceed. Never re-initialize. |
| **Thread safety** | The `keyring` crate and underlying platform stores may not handle concurrent access reliably, particularly on Windows and Linux. Serialize all keychain access through a single dedicated thread or a mutex. |
| **Key derivation** | Argon2id with a high work factor for the passphrase KEK. SQLCipher supports PBKDF2, Argon2id, and scrypt — verify which is available in the specific build and prefer Argon2id. |
| **Key consistency check** | After `PRAGMA key`, execute a read against a known row before allowing any writes. Catching a wrong key at open time prevents corrupted writes. |
| **AEAD for wrapping** | Use an authenticated cipher so a corrupted or tampered wrap fails loudly rather than producing a wrong DEK. |
| **Zeroization** | Use `zeroize` on in-memory key material. The `keyring` crate has known limitations around zeroization — treat keys in memory as sensitive and minimize their lifetime. |
| **Namespace collision** | On Windows, Credential Manager entries may collide across applications. Use a fully-qualified, application-specific service name. |
| **Backup guidance** | `dek.wrapped.json` is as sensitive as the database. Document that users should back up both together, and that losing the key file plus both unlock paths means permanent loss. |

### Platform-Specific Considerations

| Platform | Consideration |
| :--- | :--- |
| **macOS** | The keychain may cache keys in memory to avoid re-prompting, exposing them to memory-dump attacks on a compromised machine. Consider configuring an idle-lock policy. A code-signing identity change can cause the keychain to treat the app as a different entity, denying access — the passphrase path must handle this gracefully. |
| **Windows** | Credential Manager does not support multiple keychains; uniqueness depends on the service name. Ensure the entry is namespaced to avoid collisions with other applications. |
| **Linux** | Requires `libsecret` (GNOME) or an equivalent Secret Service implementation. On headless, minimal, or containerized systems this may be absent. The passphrase path must be fully functional without any keychain backend. Add a CI job that runs the unlock flow with no Secret Service available. |

### Additional Consideration: Browser Backend Isolation

The browser backends (§4 and §6) introduce a separate sensitive surface. Ensure that:
- No credentials, session tokens, or DEK material are ever passed to a browser process.
- Any browser profile directory is created in a user-inspectable, user-deletable location.
- Rendered HTML (which may contain CV content) is not persisted to the browser's cache in a way the user cannot clear.
- Two browser backends means two profile directories to audit.

### Deferred

- Hardware-backed key storage (TPM, Secure Enclave) as an alternative or additional wrap method.
- Biometric unlock as a convenience layer over the keychain path.
- Encrypted export/import bundles for moving data between machines.

---

## 8. Testing & Validation

A **full suite of new tests** is required. No Node.js tests port over directly.

| Test Type | Scope | Tools |
| :--- | :--- | :--- |
| **Unit Tests** | Business logic (evaluation, pipeline, tier routing, PDF options construction) | `#[test]`, `rstest`, `mockall` |
| **Database Tests** | Migrations, CRUD, query correctness | `sqlx::test` with in-memory SQLite |
| **Keystore Tests** | DEK/KEK wrap and unwrap, Argon2id derivation, wrong-passphrase rejection, corrupted-wrap detection, keychain `NoEntry` handling, "never re-initialize" enforcement | In-memory DB + mocked keychain backend |
| **UI Tests** | GPUI views and state transitions, including unlock and recovery states | GPUI test support |
| **Acquisition Tests** | Tier 1 parsing against saved HTML/JSON fixtures; tier routing logic; escalation behavior | Saved fixtures + mock `BrowserBackend` |
| **Browser Backend Tests** | Obscura and `rust-headless-chrome` implementations against fixtures | Environment-gated integration tests |
| **Render Tests** | `PdfBackend` implementations produce byte-identical output for a fixed input and options | Mock `PdfBackend` for logic; golden PDFs for real backends |
| **ATS Extraction Tests** | `pdftotext` round-trip verification in `-raw` and default modes | `pdftotext` in CI image |
| **Integration Tests** | End-to-end flows (unlock → fetch → evaluate → cover letter → PDF) | `assert_cmd` or custom harness |
| **LLM Evaluation Tests** | Prompt output consistency | `rig` evaluation facilities, if suitable |

**Key testing principles:**
- Tier 1 acquisition tests run against **saved fixtures**, never live network calls. Every real-world portal failure becomes a regression fixture.
- The `BrowserBackend` and `PdfBackend` traits enable hermetic testing: mocks by default, with a small number of environment-gated tests that exercise real backends.
- **The "never re-initialize" rule needs a dedicated test.** Construct a state where `dek.wrapped.json` exists and cannot be unwrapped, run the startup sequence, and assert that no new DEK is generated and the existing database file is untouched.
- **The keychain feature-flag hazard needs a test.** A test that writes and reads a sentinel value through the `keyring` crate will fail loudly if platform features are missing from `Cargo.toml`.
- **The Obscura PDF limitation needs a test.** Assert that `ObscuraBrowserBackend::print_to_pdf` returns `Unsupported`, so a future Obscura change does not silently break the two-backend assumption.
- **Golden-file tests for `PdfBackend` output** need careful maintenance across backend changes. Record `backend_used` in `document_renders` (§1) so regressions can be attributed to a specific backend.
- **ATS extraction order tests** are required for every system template. `pdftotext` is the reference tool; extraction order is the contract.

---

## 9. Performance & Bundle Size Assessment

**Performance:**
- **GPUI** is GPU-accelerated and benchmarked favorably against WebView-based alternatives on CPU and memory. Verify with your own benchmarks rather than relying on third-party numbers.
- **`sqlx` + SQLite** will not be a bottleneck at desktop data volumes, despite SQLite's synchronous core. The async interface exists to keep the UI responsive, not to increase throughput.
- **SQLCipher** adds measurable but modest overhead versus plain SQLite. For desktop-scale data volumes this is unlikely to be perceptible, but it should be benchmarked once against a realistic dataset rather than assumed.
- **Argon2id** derivation is intentionally slow (tens to hundreds of milliseconds). It runs once per unlock, not per query. Tune the work factor for a ~250–500 ms target on typical hardware.
- **`rig`** performance is dominated by network latency, not client overhead.
- **Obscura** is lightweight (~30 MB memory per instance) and fast (~85 ms page load). Suitable for scraping.
- **`rust-headless-chrome`** is heavier (~200 MB memory per instance) and slower, but it is used only for PDF rendering, which is a discrete user-triggered action.
- **Both browser backends** run off the UI thread via `spawn_blocking` or a dedicated worker.

**Bundle Size:**

| Component | Estimated Impact |
| :--- | :--- |
| GPUI application base | ~10–15 MB |
| SQLite + SQLCipher (linked) | ~2–3 MB |
| `rig` + provider clients | ~2–5 MB |
| `reqwest` + TLS + `scraper` (Tier 1) | ~3–5 MB |
| Argon2 + crypto crates | ~1 MB |
| **Obscura binary (scraping)** | **~70 MB** |
| **Chrome binary (PDF rendering, via `rust-headless-chrome`)** | **~100–150 MB** |
| **Total (default build)** | **~190–250 MB** |

**The key property of this design:** the default and intended deployment includes **two browser-related binaries** — Obscura for scraping and Chrome for PDF rendering. This is larger than a single-browser approach, but it reflects genuine functional separation: Obscura is fast and light for JS execution; Chrome is complete for rendering.

**Bundle size is a known cost of v1.** There are three ways to reduce it, all deferred or conditional:

1. **Post-v1 `printpdf` backend** removes the Chrome binary (~100–150 MB), leaving Obscura for scraping. Total drops to ~90–105 MB.
2. **Single-browser collapse** (see §4 Open Question) removes Obscura, leaving Chrome for both scraping and rendering. Total drops to ~120–170 MB.
3. **Build variants** could offer a scraping-only build (~90–105 MB) and a rendering-only build (~120–170 MB), but this multiplies packaging complexity and is not justified until demand is measured.

**The v0.1 "lean build" claim is gone.** In v0.1–v0.4, the plan assumed a default build with no browser binary at all. That is not viable for the intended deployment because Tier 1 is insufficient. The primary artifact includes both backends.

---

## 10. Human-in-the-Loop Principle

The original tool **never submits applications on the user's behalf**. This principle is preserved.

**Implementation:**
- All state-modifying actions (e.g. marking as "Applied") require explicit confirmation via a dialog.
- No job board credentials are stored or used for automated submission.
- A future "Submit" action may be added, but it must require manual confirmation and never auto-submit.

This principle should be asserted in tests, not just documented — e.g. a test that fails if any code path transitions an application to `applied` without an explicit user action.

**Browser-specific note:** both browser backends are capable of authenticated interaction with job boards. The application must never store or inject job board credentials into a browser session. If a future "assisted submission" feature is pursued, it should be a user-driven browsing session, not an automated one.

---

## 11. Risks & Mitigations

| ID | Risk | Impact | Mitigation |
| :--- | :--- | :--- | :--- |
| R-1 | **No non-browser `PdfBackend` exists in v1.** PDF output requires `rust-headless-chrome` (Chrome binary). | Medium | Accepted trade-off. `printpdf` manual-ops backend is post-v1 (Track A). |
| R-2 | **Obscura integration friction and scraping fidelity.** The project is young; its JS execution and stealth may not cover all target portals. | **High** | Spike Obscura integration in Phase 2 Week 1. Test against real target portals. Fallback to `rust-headless-chrome` is a trait swap (and Chrome is already bundled for rendering, so the fallback adds no new binary). |
| R-3 | **Obscura cannot render PDFs.** Confirmed via search results, not yet verified against source. | **Resolved in design** | `rust-headless-chrome` is the `PdfBackend`. Verify the limitation against current Obscura docs in Phase 2 Week 1. |
| R-4 | **Two browser binaries increase bundle size** to ~190–250 MB. | Medium | Accepted for v1. Documented in §9. Post-v1 `printpdf` or single-browser collapse reduces this. |
| R-5 | **Tier 1 is less valuable than originally assumed.** The browser does most of the work. | Low | Accepted. Tier 1 remains an optimization for the portals it covers. No change to the architecture. |
| R-6 | GPUI is pre-1.0; breaking changes tied to Zed's roadmap | Medium | Pin versions; isolate GPUI behind app-level view types. |
| R-7 | **Catastrophic key loss.** Both the keychain entry and the passphrase are unavailable, making the database permanently unreadable. | **High** | Envelope encryption with two independent unwrap paths. Recovery UI. Backup guidance. No technical mitigation exists for total loss of both paths. |
| R-8 | LLM API surface changes | Low | `rig` abstracts across providers. |
| R-9 | Scope creep into prompt tooling before core pipeline works | Medium | Prompt authoring (§5) is explicitly deferred until after Phase 3. |
| R-10 | **Silent keychain failure.** `keyring` platform features omitted, causing every read to return `NoEntry`. | **High** | Explicit features; sentinel round-trip test; "never re-initialize" rule. |
| R-11 | Keychain unavailable on headless or minimal Linux, blocking startup | Medium | Passphrase path must be fully functional with no keychain backend. Add a CI job. |
| R-12 | **Blocking browser calls stall the GPUI event loop.** | Medium | Wrap all browser calls in `spawn_blocking` or dedicated worker. UI responsiveness test. |
| R-13 | **Two-browser architecture is simpler than it needs to be.** A single-browser setup might suffice. | Low | Flagged as Open Question in §4. Trait design makes collapse cheap if evidence justifies it. |

---

## 12. Suggested Development Phases

**Phase 0 (Spikes) is skipped.** The decisions it was designed to inform were made directly. See the Decisions Addendum for the record.

| Phase | Scope | Duration (Estimate) |
| :--- | :--- | :--- |
| **Phase 1: Foundation** | Keystore subsystem (§7) first — envelope encryption, keychain integration, unlock and recovery flows. Then SQLite schema, `sqlx` integration, profile/CV CRUD, basic GPUI shell, `BrowserBackend` and `PdfBackend` trait definitions. | 4–5 weeks |
| **Phase 2: Acquisition** | **Obscura integration first** (`BrowserBackend` implementation, sidecar lifecycle). Then Tier 1 (ATS JSON, HTML extraction), tier routing, `portals` config UI, acquisition tests with fixtures. Verify Obscura's PDF limitation against current docs in Week 1. | 3–4 weeks |
| **Phase 3: AI Integration** | `rig` integration, prompt seeding and read-only system prompts, evaluation report generation with `prompt_version` provenance, streaming, injection defense. | 3–4 weeks |
| **Phase 4: Rendering** | `rust-headless-chrome`-based `PdfBackend`, template system, ATS-safe extraction verification, PDF preview, golden-file tests. | 3–4 weeks |
| **Phase 5: Polish & Hardening** | Complete Settings, key rotation UI, backend selection, onboarding, error handling audit, panic audit, accessibility, platform testing, performance tuning, backup/export, documentation, visual polish. | 3–4 weeks |
| **Phase 6: Migration & Release** | Migration tool, packaging, code signing, notarization, release candidate testing, auto-update check, release process, documentation finalization. | 2–4 weeks |
| **Phase 7: Post-v1** | Ongoing maintenance and candidate tracks. `printpdf` manual-ops backend (Track A) is the primary post-v1 item. Single-browser collapse (see §4 Open Question) should be evaluated alongside it. | Open-ended |

**Total Estimate:** 4–6 months for a functional MVP, assuming one experienced Rust developer. The keystore subsystem is sequenced first in Phase 1 because every other subsystem depends on it. Obscura integration is sequenced first in Phase 2 because the browser is the load-bearing acquisition dependency.

---

## Appendix A: Decisions Log

| # | Decision | Status |
| :--- | :--- | :--- |
| 1 | SQLite as primary state store (inverting the original file-canonical doctrine) | Confirmed |
| 2 | Port ~70 Node.js scripts into `core::*` Rust modules | Confirmed |
| 3 | GPUI as UI framework | Confirmed |
| 4 | `rig` for LLM integration | Confirmed |
| 5 | `sqlx` (async-first) for SQLite | Confirmed |
| 6 | Backend-agnostic PDF generation via a `PdfBackend` trait | Confirmed (v0.3) |
| 7 | `reqwest` for HTTP | Confirmed |
| 8 | `reqwest` + `scraper` for Tier 1 acquisition | Confirmed — **as optimization, not default path** (v0.5) |
| 9 | Headless browser is **required**, not feature-gated | Confirmed (v0.5) |
| 10 | **Obscura as the initial `BrowserBackend`** (scraping) | Confirmed (v0.5) |
| 11 | CDP-agnostic `BrowserBackend` trait | Confirmed |
| 12 | **`rust-headless-chrome` as the initial `PdfBackend`** (rendering) | Confirmed (v0.5) |
| 13 | Prompts seeded from embedded resources, stored in SQLite, user-editable | Confirmed; detail TBD |
| 14 | `prompt_version` recorded on evaluation reports for reproducibility | Confirmed (v0.3) |
| 15 | Data migration from existing files | Deferred to Phase 6 |
| 16 | Human-in-the-loop; no automated submission | Confirmed |
| 17 | OS keychain for secrets | Confirmed |
| 18 | **Encryption at rest is mandatory; no unencrypted mode** | Confirmed (v0.4) |
| 19 | **Envelope encryption: random DEK wrapped by a keychain-stored KEK and a passphrase-derived KEK** | Confirmed (v0.4) |
| 20 | **"Never re-initialize" rule: existing `dek.wrapped.json` blocks fresh initialization** | Confirmed (v0.4) |
| 21 | Full new test suite | Confirmed |
| 22 | `printpdf` as the default PDF backend | **Superseded** — deferred to post-v1 (Track A) (v0.5) |
| 23 | Compile-time backend selection (Cargo features) | Confirmed for v1; dynamic loading deferred |
| 24 | **Phase 0 skipped; decisions recorded directly** | Confirmed (v0.5) |
| 25 | **Two-browser architecture (Obscura for scraping, Chrome for rendering)** | Confirmed (v0.5); single-browser collapse is an open question in §4 |
| 26 | **`printpdf` deferred to post-v1 (Track A)** | Confirmed (v0.5) |

---

## Appendix B: Open Questions

These are tracked here so they are not lost between phases.

| # | Question | Phase to resolve |
| :--- | :--- | :--- |
| Q-1 | Can Obscura's PDF limitation be verified against current project documentation? | Phase 2, Week 1 |
| Q-2 | Is the two-browser architecture justified, or would a single `rust-headless-chrome` backend be simpler? | Phase 2, after scraping performance is measured |
| Q-3 | What is the target Argon2id work factor for representative hardware? | Phase 1 |
| Q-4 | Do the existing HTML/CSS templates render acceptably through `rust-headless-chrome`? | Phase 4 |
| Q-5 | How should prompt edits interact with historical evaluation reports beyond storing `prompt_version`? | Phase 3 |
| Q-6 | Should encryption be default-on with a passphrase, or should onboarding require it? (Currently: required.) | Phase 5 |

---

This plan is a living document. Open items requiring resolution are tracked in Appendix B. The Obscura PDF limitation (Q-1) and the two-browser architecture question (Q-2) are the most consequential near-term items.