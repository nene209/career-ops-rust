# Draft Execution Plan: Career-Ops Rust Desktop Application

**Status:** Draft — v0.4
**Last updated:** 2026-10-12

This document outlines the execution plan for porting the `career-ops` project from its current Node.js, file-based architecture to a Rust desktop application using GPUI, SQLite, and a primarily pure-Rust technology stack. The plan incorporates the stated technical decisions and evaluates the key architectural considerations.

### Change Log

| Version | Change |
| :--- | :--- |
| v0.1 | Initial draft. |
| v0.2 | Replaced single-browser scraping approach with tiered architecture (`reqwest` + `scraper` default, headless browser feature-gated). Removed `rust-headless-chrome` as a hard dependency. Added CDP-agnostic backend abstraction as a future extension. Added dependency-verification caveat. |
| v0.3 | Made PDF generation backend-agnostic via a `PdfBackend` trait, mirroring `BrowserBackend`. Selected `rust-headless-chrome` as the initial Tier 2 and PDF backend. Demoted `printpdf` from default PDF path to a medium-term alternative backend. Restructured §6. Lowered R-1 severity; raised the profile of the slimming path. |
| v0.4 | Encryption at rest is now mandatory with no unencrypted mode. Replaced direct keychain-stored DB key with envelope encryption (DEK wrapped by a keychain-stored KEK, plus a passphrase-derived recovery path). Added "never re-initialize on key loss" rule. Added `keyring` crate feature-flag hazard. Revised R-5; added R-10 and R-11. |

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
| `document_renders` | `output/*.pdf` | `id`, `application_id`, `kind` (cv/cover_letter), `backend_used`, `template_id`, `rendered_at` | Records which backend produced a given PDF. Useful for regression tracking across backend changes. |
| `portals` | `portals.yml` | `id`, `company_name`, `url`, `render_mode`, `scraper_config` (JSON), `last_verified_at` | Configuration for job acquisition. `render_mode` is `static` or `browser` (see §4). |
| `prompts` | `modes/*.md` | `id`, `name`, `category` (evaluate/cover/scan), `content`, `is_system`, `version`, `created_at`, `updated_at` | Replaces the Markdown prompt files (see §5). |
| `settings` | N/A | `key`, `value` (JSON) | Application-wide settings. Holds backend *preferences* (e.g. `pdf_backend = "browser"`) among other config. API keys live in the OS keychain, not here. |

**Design Considerations:**
- Use `TEXT` for JSON columns and validate with `serde_json` on read.
- All `created_at` / `updated_at` columns should use `TEXT` in ISO 8601 format for `sqlx` compatibility.
- Foreign keys enforced with `PRAGMA foreign_keys = ON`.
- Indexes on `applications.status`, `applications.job_description_id`, and `prompts.category`.
- `portals.render_mode` is the routing key for §4. Per-portal acquisition strategy belongs in the database, not in code, so users can tune it without a rebuild.
- `settings` holds backend *preferences*, not backend *availability*. Availability is a compile-time property (see §4 and §6).
- **The wrapped DEK is not stored in this database.** Storing the key material that unlocks the database inside the database is a chicken-and-egg problem. It lives in a sibling file on disk. See §7.

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
| **Portals Configuration** | `portals` | List of job boards with add/edit/delete. Per-portal `render_mode` selector and scraper configuration. Displays acquisition health. |
| **Prompt Manager** | `prompts` | Browse, edit, and version prompt templates. System prompts are read-only; user prompts are editable. |
| **Settings** | `settings` | API key management (OS keychain), model provider selection, PDF template selection, active backend selection (limited to compiled-in backends), key rotation, data export. |

**GPUI-Specific Considerations:**
- Use `gpui-kit` (or the current equivalent) as the primary dependency for production-ready UI primitives.
- The pipeline table requires virtualized rows for large application histories. Verify this capability early — it was a disqualifying gap for at least one comparable framework.
- Prefer GPUI's built-in test support for view-level tests rather than driving the UI externally.
- Backend selection UI must reflect what is actually compiled into the binary. A backend that is not feature-enabled should appear greyed out with an explanatory tooltip, not silently missing.
- **Unlock must gate the app.** The database cannot be opened without a valid key (§7). The UI must handle "cannot unlock" as a first-class state with a recovery path, never as a crash.

---

## 4. Job Acquisition: Tiered Scraping Architecture

The original project used Playwright for both job board scraping and HTML-to-PDF generation. This plan separates those two concerns:

- **PDF generation** is handled by a `PdfBackend` abstraction (§6), which *may* share the browser dependency but is not coupled to it.
- **Job acquisition** uses a tiered architecture where a lightweight HTTP path is the default and a headless browser is an opt-in, feature-gated escalation.

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

Enabled via a Cargo feature flag (e.g. `browser-backend`). Used when Tier 1 fails or when a portal is explicitly configured with `render_mode = 'browser'`.

**Initial backend: `rust-headless-chrome`.**

Rationale for starting here:
- **Completeness.** It exposes a wide slice of the CDP surface, including navigation, evaluation, screenshots, and PDF printing. This matters because the same backend will serve §6.
- **Maturity.** It is the most established Rust CDP client, which reduces the risk of hitting undocumented gaps during the port.
- **Synchronous API.** Acceptable for both use cases. Scraping is low-concurrency in practice (a handful of portals), and PDF rendering is a discrete user-triggered action. Blocking calls can be wrapped in `spawn_blocking` without meaningful penalty.
- **Bundle cost is already paid.** If a browser binary is bundled for Tier 2 at all, the incremental cost of also using it for PDF generation is zero.

> **⚠️ Verification required before locking this in.** The Rust headless-browser ecosystem is young and fast-moving. Specific project names, licenses, benchmark figures, JS-engine choices, CDP command coverage, and maintenance status must be re-verified against current crates.io and GitHub state before the dependency is committed. Do not treat any named project here as fully confirmed. See §11, Risk R-2.

**Alternative under evaluation: `chromiumoxide`.**

An async-first CDP client that integrates more naturally with a `tokio`-based stack (`sqlx`, `rig`). Worth benchmarking against `rust-headless-chrome` on two axes before committing:
1. Completeness of `Page.printToPDF` parameter coverage.
2. Ergonomics of driving it from an async context without blocking the GPUI runtime.

**Other candidate backends (not chosen for v1):**

| Backend type | Rationale for deferral |
| :--- | :--- |
| Rust-native headless browser (V8 or pure-Rust JS engine) | Lower bundle and memory footprint, but compatibility and maturity risk. Revisit once the abstraction is proven. |
| System-installed Chrome over CDP | Zero bundled binary; requires the user to have Chrome. A strong candidate for a future `system-chrome` backend behind the same trait. |

**Selection criteria for future backends, in priority order:**
1. **JS execution fidelity** on real, modern SPAs. This tier exists *specifically* to handle sites Tier 1 can't.
2. **Bundle size delta** over the Tier 1 baseline.
3. **Process model.** Sidecar process preferred over linked-in, for binary leanness and crash isolation.
4. **Stealth / anti-detection** maturity, if target portals use bot management.
5. **Maintenance trajectory.** Commit recency, issue responsiveness, bus factor.

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

    /// Render an HTML string to PDF. See §6 for the PdfBackend trait,
    /// which may delegate to this.
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

- **Deferred commitment.** The browser engine can be swapped, or added as a user-selectable option, without touching acquisition logic.
- **User choice.** Power users could point the app at system Chrome, a bundled engine, or a remote browser service.
- **Testability.** A mock backend makes acquisition tests fast and hermetic, with no browser binary required in CI. This is the same technique recommended for Tier 1 fixtures in §8.
- **Reuse for PDF.** Because `print_to_pdf` is a CDP method, the same trait serves both §4 and §6. See §6 for how the traits relate.

This should be treated as a design constraint from the start, even if only one backend ships in v1. Retrofitting an abstraction over a concrete dependency is significantly more expensive than building it in.

### Compile-Time vs. Runtime Backend Selection

Backend availability is a **compile-time** property. Cargo feature flags control what is linked into the binary; a backend that is not feature-enabled cannot be added after installation. Runtime configuration (via the `settings` table) can only select among backends that were compiled in.

For v1, this is the right trade-off. Dynamic loading of backends as separate shared libraries is a plausible future extension if post-install backend swapping becomes a requirement, but it requires a stable C ABI, version negotiation, and plugin distribution infrastructure — substantially more complexity than the feature-gate approach. Defer.

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

PDF generation is treated as a **backend-agnostic rendering concern**, mirroring §4a. The reasoning is straightforward: PDF generation via a headless browser is not a separate capability from scraping — it is the same CDP connection calling `Page.printToPDF`. If a browser backend is already available for Tier 2, PDF generation comes nearly free.

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

### Initial Backend: Browser-Based (`rust-headless-chrome`)

The v1 `PdfBackend` implementation delegates to a `BrowserBackend` and calls `print_to_pdf`. This reuses the Tier 2 dependency entirely.

**Why this is the right starting point:**
- **Zero new dependencies.** The browser binary is already bundled (or already absent, if the feature is disabled).
- **Feature completeness.** CDP `printToPDF` supports page size, margins, header/footer templates, background printing, CSS page-size preference, and page ranges. This covers everything the original Playwright pipeline relied on.
- **Fidelity.** The original tool's HTML/CSS templates can be reused nearly verbatim, because the rendering engine is Chromium — the same engine Playwright drove.
- **Isolation of risk.** The PDF pipeline becomes a known-good path rather than the plan's largest unknown.

### Relationship Between `PdfBackend` and `BrowserBackend`

Three possible coupling strategies:

| Approach | Pros | Cons | Verdict |
| :--- | :--- | :--- | :--- |
| **Shared CDP session** | One browser process for both scraping and PDF. Minimal memory. | Couples PDF generation to the scraping lifecycle. A scraping crash takes down PDF. | Reject for v1. |
| **Independent browser instance** | Isolation. PDF failures don't affect scraping. | Two browser processes if both active. Higher peak memory. | Acceptable. |
| **Backend-agnostic** — `PdfBackend` may or may not use a browser internally | Maximum flexibility. A future `printpdf` backend needs no browser at all. | Slightly more implementation work up front. | **Recommended.** |

The third option is the design goal. `PdfBackend` is a standalone trait; the browser-based implementation internally creates its own browser instance (isolation), while a future `printpdf` implementation simply does not require one. `requires_browser()` lets the UI communicate this difference.

### The Slimming Path (Deferred)

The long-term goal of removing the browser dependency from PDF generation remains valid, but it is **not** a near-term deliverable. The realistic path is:

1. **Now**: browser-based `PdfBackend`, reusing Tier 2. Ship working PDFs.
2. **Medium term**: evaluate `printpdf`'s **manual ops API** as a second backend. Resume templates expressed as explicit text blocks, lines, and embedded fonts — not HTML. This backend ships with `requires_browser() == false`, which is the actual slimming win.
3. **Long term**: revisit `printpdf`'s HTML feature, or a Rust-native browser's PDF renderer, as they mature.

> **Important caveat on `printpdf`.** Its HTML-to-PDF feature is experimental and geared toward basic layouts. It is not currently a drop-in replacement for Chromium-based rendering, and pixel-exact ATS-friendly resume output is unlikely to be achievable through it today. The slimming path runs through the **manual ops API**, which is a distinct implementation effort — not a configuration change. Verify the current state of both APIs before committing engineering time to either.

### Decision Gate

The Phase 0(a) spike is now **lower risk** than in v0.2, because the browser backend is the default and its fidelity is not in question. The spike should instead answer:

- Can the existing HTML/CSS templates be rendered acceptably through `rust-headless-chrome` with minimal modification?
- What is the actual `PdfOptions` coverage needed, and does the chosen backend expose it?
- Is a blocking `print_to_pdf` call acceptable in the GPUI event loop when wrapped in `spawn_blocking`?

A secondary, non-blocking spike can assess `printpdf`'s manual ops API for a single representative resume template, to inform the medium-term slimming path. This should not gate Phase 1.

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

The browser backend (§4) introduces a separate sensitive surface. Ensure that:
- No credentials, session tokens, or DEK material are ever passed to the browser process.
- Any browser profile directory is created in a user-inspectable, user-deletable location.
- Rendered HTML (which may contain CV content) is not persisted to the browser's cache in a way the user cannot clear.

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
| **Acquisition Tests** | Tier 1 parsing against saved HTML fixtures; tier routing logic; escalation behavior | Saved fixtures + mock `BrowserBackend` |
| **Render Tests** | `PdfBackend` implementations produce byte-identical output for a fixed input and options | Mock `PdfBackend` for logic; golden PDFs for real backends |
| **Integration Tests** | End-to-end flows (unlock → fetch → evaluate → cover letter → PDF) | `assert_cmd` or custom harness |
| **LLM Evaluation Tests** | Prompt output consistency | `rig` evaluation facilities, if suitable |
| **PDF Validation** | Layout correctness and text extractability | Text extraction + visual regression |

**Key testing principles:**
- Tier 1 acquisition tests run against **saved HTML fixtures**, never live network calls. Every real-world portal failure becomes a regression fixture.
- The `BrowserBackend` and `PdfBackend` traits enable the same hermetic approach for Tier 2 and rendering: tests use mocks by default, with a small number of opt-in, environment-gated tests that exercise real backends.
- **The "never re-initialize" rule needs a dedicated test.** Construct a state where `dek.wrapped.json` exists and cannot be unwrapped, run the startup sequence, and assert that no new DEK is generated and the existing database file is untouched.
- **The keychain feature-flag hazard needs a test.** A test that writes and reads a sentinel value through the `keyring` crate will fail loudly if platform features are missing from `Cargo.toml`.
- Golden-file tests for `PdfBackend` output will need careful maintenance across backend changes. Record `backend_used` in `document_renders` (§1) so regressions can be attributed to a specific backend.

---

## 9. Performance & Bundle Size Assessment

**Performance:**
- **GPUI** is GPU-accelerated and benchmarked favorably against WebView-based alternatives on CPU and memory. Verify with your own benchmarks rather than relying on third-party numbers.
- **`sqlx` + SQLite** will not be a bottleneck at desktop data volumes, despite SQLite's synchronous core. The async interface exists to keep the UI responsive, not to increase throughput.
- **SQLCipher** adds measurable but modest overhead versus plain SQLite. For desktop-scale data volumes this is unlikely to be perceptible, but it should be benchmarked once against a realistic dataset rather than assumed.
- **Argon2id** derivation is intentionally slow (tens to hundreds of milliseconds). It runs once per unlock, not per query. Tune the work factor for a ~250–500 ms target on typical hardware.
- **`rig`** performance is dominated by network latency, not client overhead.
- **Browser operations** are the heaviest runtime cost. Both scraping and PDF rendering should run off the UI thread via `spawn_blocking` or a dedicated worker.

**Bundle Size:**

| Component | Estimated Impact |
| :--- | :--- |
| GPUI application base | ~10–15 MB |
| SQLite + SQLCipher (linked) | ~2–3 MB |
| `rig` + provider clients | ~2–5 MB |
| `reqwest` + TLS + `scraper` (Tier 1) | ~3–5 MB |
| Argon2 + crypto crates | ~1 MB |
| **Browser backend + binary (feature-gated)** | **TBD — 0 MB if excluded at build time; potentially 100–150 MB if bundled with a Chrome binary** |
| **Default build total (no browser feature)** | **~20–35 MB** |
| **Build with browser feature** | **Dominant cost is the browser binary** |

**The key property of this design:** the default build ships with **no browser binary at all**. Users who never encounter a JS-heavy portal and are content with Tier 1 acquisition never pay the cost.

**Important consequence of the v0.3 change:** PDF generation now depends on the browser backend by default. This means a user who wants PDF output must enable the browser feature, which pulls in the browser binary. In v0.2, `printpdf` was expected to provide PDF output without any browser. That is a real regression in default-build capability, accepted deliberately in exchange for:
- A working, high-fidelity PDF pipeline from day one.
- Removal of the plan's largest technical unknown.
- A clear, tractable slimming path via a future `printpdf` manual-ops backend.

Once a non-browser `PdfBackend` exists, the default build can produce PDFs again, and the browser feature becomes purely an acquisition concern. This is the medium-term goal.

---

## 10. Human-in-the-Loop Principle

The original tool **never submits applications on the user's behalf**. This principle is preserved.

**Implementation:**
- All state-modifying actions (e.g. marking as "Applied") require explicit confirmation via a dialog.
- No job board credentials are stored or used for automated submission.
- A future "Submit" action may be added, but it must require manual confirmation and never auto-submit.

This principle should be asserted in tests, not just documented — e.g. a test that fails if any code path transitions an application to `applied` without an explicit user action.

**Browser-specific note:** the browser backend is capable of authenticated interaction with job boards. The application must never store or inject job board credentials into a browser session. If a future "assisted submission" feature is pursued, it should be a user-driven browsing session, not an automated one.

---

## 11. Risks & Mitigations

| ID | Risk | Impact | Mitigation |
| :--- | :--- | :--- | :--- |
| R-1 | No non-browser `PdfBackend` exists in v1, so PDF output requires the browser feature | **Medium** | Accepted trade-off. Default build is lean; PDF requires the browser feature. Medium-term `printpdf` manual-ops backend restores browser-free PDF generation. |
| R-2 | **Unverified browser backend landscape.** Named Rust CDP clients, their capabilities, licenses, CDP command coverage, and maintenance status have not been verified. | **High** | Verify against crates.io/GitHub before Phase 2. Build both tiers against the `BrowserBackend` / `PdfBackend` traits so the choice stays reversible. Benchmark `rust-headless-chrome` against `chromiumoxide` on `printToPDF` coverage and async ergonomics. |
| R-3 | GPUI is pre-1.0; breaking changes tied to Zed's roadmap | Medium | Pin versions; monitor release notes; isolate GPUI behind app-level view types. |
| R-4 | Tier 1 fails on a large fraction of real target portals, making Tier 2 effectively mandatory | Medium | Measure before building. Collect a sample of real target portals and test Tier 1 against them during Phase 0. |
| R-5 | **Catastrophic key loss.** Both the keychain entry and the passphrase are unavailable, making the database permanently unreadable. | **High** | Envelope encryption with two independent unwrap paths reduces probability substantially. Recovery UI encourages exporting the encrypted DB and key file. Backup guidance surfaced during onboarding. No technical mitigation exists for total loss of both paths — this is inherent to mandatory encryption. |
| R-6 | LLM API surface changes | Low | `rig` abstracts across providers, reducing lock-in. |
| R-7 | Scope creep into prompt tooling before core pipeline works | Medium | Prompt authoring (§5) is explicitly deferred until after Phase 3. |
| R-8 | HTML/CSS templates require significant modification to render acceptably through the chosen browser backend | Low–Medium | Phase 0(a) spike validates this early against real templates. |
| R-9 | Blocking browser API causes UI stalls in the GPUI event loop | Medium | Wrap all browser calls in `spawn_blocking` or a dedicated worker thread. Add a UI responsiveness test. |
| R-10 | **Silent keychain failure.** `keyring` crate platform features omitted from `Cargo.toml`, causing every read to return `NoEntry`. A naive startup sequence could interpret this as "no key" and re-initialize, orphaning the database. | **High** | Explicitly enable platform features. Add a sentinel round-trip test. Enforce the "never re-initialize" rule. Distinguish `NoEntry` on first launch from `NoEntry` when a keychain wrap already exists. |
| R-11 | Keychain unavailable on headless or minimal Linux, blocking startup | Medium | Passphrase path must be fully functional with no keychain backend. Add a CI job exercising this configuration. |

---

## 12. Suggested Development Phases

| Phase | Scope | Duration (Estimate) |
| :--- | :--- | :--- |
| **Phase 0: Spikes** | Three throwaway spikes: (a) existing HTML/CSS templates render acceptably via `rust-headless-chrome`; (b) Tier 1 `reqwest` + `scraper` against 5–10 real target portals; (c) `rust-headless-chrome` vs. `chromiumoxide` on `printToPDF` coverage and async ergonomics. (a) and (b) are decision gates; (c) is informational. | 1–2 weeks |
| **Phase 1: Foundation** | Keystore subsystem (§7) first — envelope encryption, keychain integration, unlock and recovery flows. Then SQLite schema, `sqlx` integration, profile/CV CRUD, basic GPUI shell, `BrowserBackend` and `PdfBackend` trait definitions. | 4–5 weeks |
| **Phase 2: Acquisition** | Tier 1 pipeline, `portals` config UI, tier routing, acquisition tests with fixtures. Tier 2 (`rust-headless-chrome`) behind the `browser-backend` feature. | 3–5 weeks |
| **Phase 3: AI Integration** | `rig` integration, prompt seeding and read-only system prompts, evaluation report generation with `prompt_version` provenance. | 3–4 weeks |
| **Phase 4: Rendering** | Browser-based `PdfBackend`, template integration, PDF preview in the CV editor, golden-file render tests. | 3–4 weeks |
| **Phase 5: Polish & Security** | Key rotation UI, OS keychain settings, backend selection, full test suite, platform-specific unlock testing. | 3–4 weeks |
| **Phase 6: Migration & Release** | File-based data migration (deferred enhancement), packaging, documentation, backup guidance. | 2–3 weeks |
| **Post-v1 (medium term)** | `printpdf` manual-ops `PdfBackend` implementation, restoring browser-free PDF generation. | Separate effort |

**Total Estimate:** 4–6 months for a functional MVP, assuming one experienced Rust developer. The keystore subsystem is sequenced first in Phase 1 because every other subsystem depends on it, and because a late key-management change would require re-encrypting all existing data.

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
| 8 | `reqwest` + `scraper` as default acquisition path | Confirmed |
| 9 | Headless browser as feature-gated escalation only | Confirmed |
| 10 | `rust-headless-chrome` as the initial browser backend | Confirmed for v1; benchmark against `chromiumoxide` in Phase 0(c); verify before Phase 2 (R-2) |
| 11 | CDP-agnostic `BrowserBackend` trait | Confirmed as design constraint |
| 12 | `rust-headless-chrome` is *not* a hard dependency | Confirmed — it is the initial feature-gated implementation, not a baseline requirement |
| 13 | Prompts seeded from embedded resources, stored in SQLite, user-editable | Confirmed; detail TBD |
| 14 | `prompt_version` recorded on evaluation reports for reproducibility | Confirmed (v0.3) |
| 15 | Data migration from existing files | Deferred to future enhancement |
| 16 | Human-in-the-loop; no automated submission | Confirmed |
| 17 | OS keychain for secrets | Confirmed |
| 18 | **Encryption at rest is mandatory; no unencrypted mode** | Confirmed (v0.4) |
| 19 | **Envelope encryption: random DEK wrapped by a keychain-stored KEK and a passphrase-derived KEK** | Confirmed (v0.4) |
| 20 | **"Never re-initialize" rule: existing `dek.wrapped.json` blocks fresh initialization** | Confirmed (v0.4) |
| 21 | Full new test suite | Confirmed |
| 22 | `printpdf` as the default PDF backend | **Superseded** — demoted to a medium-term alternative backend via `PdfBackend` |
| 23 | Compile-time backend selection (Cargo features) | Confirmed for v1; dynamic loading deferred |

---

This plan is a living document. Open items requiring resolution: the browser backend choice pending Phase 0(c) and R-2 verification, the `printpdf` manual-ops feasibility assessment, the Argon2id work-factor tuning target, and prompt versioning semantics (§5).