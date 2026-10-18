# Phase 4: Output Generation — Detailed Execution Plan

**Parent plan:** Draft Execution Plan: Career-Ops Rust Desktop Application (v0.4), §12
**Status:** Draft
**Last updated:** 2026-10-12
**Duration:** 3–4 weeks
**Depends on:** Phase 3 completion (see Entry Conditions)

---

## Purpose

Phase 4 turns text into documents. It implements the `PdfBackend` trait defined in Phase 1, renders CVs and cover letters to PDF, and delivers the ATS-safe output that is the original project's key differentiator. Critically, it does **not** introduce a new browser dependency: the browser-based `PdfBackend` delegates to the `BrowserBackend` implementation from Phase 2, which already exposes `print_to_pdf`.

The phase has three defining characteristics:

1. **Browser-based by default, with a pure-Rust slimming path.** The v1 `PdfBackend` uses the same CDP connection already established for Tier 2 acquisition. A `printpdf` manual-ops backend is assessed for the medium term, not shipped in v1.
2. **ATS safety is a verified contract, not a hope.** PDFs that look correct visually can extract as scrambled gibberish. Phase 4 treats extraction order as a contract, verifies it with `pdftotext` round-trips, and treats ATS-safe ordering as a testable property rather than a visual concern.
3. **Templates are data, not code.** Resume and cover letter templates are stored and versioned, so users can customize output without rebuilding the application. Template rendering is separated from PDF generation.

---

## Entry Conditions

Phase 4 begins after Phase 3 exit criteria are met. The following prior outputs are consumed:

| Prior Output | How Phase 4 Uses It |
| :--- | :--- |
| `PdfBackend` trait definition (Phase 1) | Implemented by the browser-based backend |
| `BrowserBackend` with `print_to_pdf` (Phase 2) | Delegated to for HTML-to-PDF rendering |
| `document_renders` table (Phase 1) | Records backend, template, and timestamp per render |
| `cv_entries` and `profile` repositories (Phase 1) | Source of data for CV rendering |
| `cover_letters` table with markdown (Phase 3) | Source of text for cover letter rendering |
| GPUI shell and CV editor (Phase 1, partial) | Host for the PDF preview pane |
| Test/CI infrastructure (Phase 1) | Extended with render golden-file and ATS tests |

**Phase 4 assumes cover letters and CV data exist as text.** If Phase 3 produced cover letters in markdown and Phase 1 populated `cv_entries`, Phase 4 can render. If not, Phase 4 can still be developed against fixtures.

---

## Scope

| In scope | Out of scope |
| :--- | :--- |
| Browser-based `PdfBackend` implementation | `printpdf` backend (medium term, assessed only) |
| HTML template system for CV and cover letter | Full template gallery |
| Template storage, versioning, and selection | WYSIWYG template editor |
| ATS-safe text ordering enforcement and verification | ATS parser simulation beyond `pdftotext` |
| PDF preview in the CV editor | Interactive PDF editing |
| Batch rendering of CV + cover letter as a bundle | Merging PDFs from multiple sources |
| Golden-file render tests | Visual regression dashboards |
| `document_renders` provenance recording | Render analytics |
| `pdftotext` round-trip verification harness | Automated ATS scoring |
| | `printpdf` manual-ops assessment spike (informational) |

---

## Sequencing Rationale

The build order is driven by the dependency chain from template to verified PDF:

```
W1: Template system + HTML rendering pipeline
    └── W2: Browser-based PdfBackend + integration with BrowserBackend
        └── W3: ATS-safe text ordering enforcement + pdftotext verification
            └── W4: PDF preview UI + golden-file tests + printpdf assessment

W4a: printpdf manual-ops spike (informational, parallel, does not gate)
```

The template system comes first because the browser backend renders HTML. The `PdfBackend` implementation is next because it is the core deliverable. ATS verification is third because it is what makes the output actually usable for job applications — a PDF that renders beautifully but extracts as noise fails at its purpose. The UI and tests come last, after the pipeline is proven headlessly.

---

## Workstream 1 — Template System

**Priority:** First. The browser backend renders HTML; templates produce HTML.
**Estimated duration:** 1 week

### Deliverables

1. **Template storage.** New `templates` table:

```sql
CREATE TABLE templates (
    id INTEGER PRIMARY KEY,
    name TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('cv', 'cover_letter')),
    content TEXT NOT NULL,           -- HTML with placeholders
    version INTEGER NOT NULL DEFAULT 1,
    content_hash TEXT NOT NULL,
    is_system INTEGER NOT NULL DEFAULT 0,
    supersedes_id INTEGER REFERENCES templates(id),
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL
);
```

2. **System templates seeded on first launch**, embedded as Rust resources:

```rust
const CV_TEMPLATE_DEFAULT: &str = include_str!("../templates/cv_default.html");
const COVER_LETTER_TEMPLATE_DEFAULT: &str = include_str!("../templates/cover_letter_default.html");
```

3. **Template rendering.** A minimal variable substitution engine:

```rust
pub struct TemplateContext {
    pub profile: Profile,
    pub cv_entries: Vec<CvEntry>,
    pub job: Option<JobDescription>,
    pub cover_letter_text: Option<String>,
    pub rendered_at: DateTime<Utc>,
}

pub fn render_template(template: &str, ctx: &TemplateContext) -> Result<String>;
```

Supported placeholders:
- `{{profile.name}}`, `{{profile.email}}`, `{{profile.phone}}`, `{{profile.location}}`, `{{profile.summary}}`
- `{{#each cv_entries}}...{{/each}}` with `{{title}}`, `{{organization}}`, `{{start_date}}`, `{{end_date}}`, `{{description}}`
- `{{cover_letter_text}}` (markdown converted to HTML)
- `{{job.title}}`, `{{job.company}}` (when rendering a tailored cover letter)
- `{{rendered_at}}`

4. **Template selection** stored in `settings` (`cv_template_id`, `cover_letter_template_id`)
5. **Template versioning** follows the same pattern as prompts (§5 of the main plan): new version = new row, content hash for change detection, system templates read-only

### Critical Implementation Notes

1. **Templates are HTML, not markdown.** The browser backend renders HTML. Markdown-to-HTML conversion happens before template rendering, not during it. This keeps the template engine simple and the rendering predictable.
2. **The template engine is deliberately minimal.** No conditionals beyond `#each`, no partials, no filters. Phase 4 ships flat substitution and iteration. A full templating language is deferred indefinitely.
3. **System templates are read-only but versioned.** When the application ships an updated template in a future release, it inserts a new version rather than overwriting. Users who duplicated a system template keep their copy.
4. **Template content is not encrypted.** Templates are not sensitive. They live in the encrypted database because that is where all state lives, but there is no special protection applied to them.
5. **`rendered_at` is injected for reproducibility.** A rendered document records when it was generated, which is useful when templates or data change.

### Acceptance Criteria

| Criterion | Verification |
| :--- | :--- |
| System templates seed on first launch | Integration test |
| Second launch does not duplicate seeds | Integration test |
| Template rendering substitutes all supported placeholders | Unit test |
| `#each` iterates over `cv_entries` correctly | Unit test |
| Unknown placeholder fails rendering in strict mode | Unit test |
| Template versioning creates new rows on edit | Unit test |
| Template selection persists across restarts | Integration test |

---

## Workstream 2 — Browser-Based `PdfBackend` Implementation

**Priority:** Core deliverable. After templates exist.
**Estimated duration:** 1–1.5 weeks

### Deliverables

1. `PdfBackend` implementation delegating to `BrowserBackend`:

```rust
pub struct BrowserPdfBackend {
    backend: Arc<dyn BrowserBackend>,
}

#[async_trait]
impl PdfBackend for BrowserPdfBackend {
    async fn render_pdf(
        &self,
        html: &str,
        opts: PdfOptions,
    ) -> Result<Vec<u8>> {
        self.backend.print_to_pdf(html, opts).await
    }

    fn name(&self) -> &'static str {
        "browser"
    }

    fn requires_browser(&self) -> bool {
        true
    }
}
```

2. **`PdfOptions` defaults tuned for ATS-friendly resumes:**
   - `page_size`: A4 or Letter (user-selectable)
   - `margins`: 1cm all sides (matching CDP default; sufficient for most resumes)
   - `print_background`: `false` by default (ATS parsers prefer plain text on white)
   - `prefer_css_page_size`: `false` (template controls layout, not CSS)
   - `header_template` / `footer_template`: `None` by default (headers/footers can confuse ATS parsers)

3. **`print_to_pdf` coverage verification.** The CDP `Page.printToPDF` command supports `landscape`, `displayHeaderFooter`, `printBackground`, `scale`, `paperWidth`, `paperHeight`, margins, `pageRanges`, `headerTemplate`, `footerTemplate`, and `preferCSSPageSize`. Confirm the chosen `BrowserBackend` exposes all parameters the templates require.

4. **Rendering pipeline:**

```
CV data → TemplateContext → render_template → HTML
    → BrowserPdfBackend.render_pdf → PDF bytes
    → store PDF → record in document_renders
```

5. **PDF storage.** Rendered PDFs are stored on disk in a user-inspectable directory, with metadata in `document_renders`:

```sql
CREATE TABLE document_renders (
    id INTEGER PRIMARY KEY,
    application_id INTEGER REFERENCES applications(id),
    kind TEXT NOT NULL CHECK (kind IN ('cv', 'cover_letter')),
    backend_used TEXT NOT NULL,
    template_id INTEGER REFERENCES templates(id),
    file_path TEXT NOT NULL,
    rendered_at TEXT NOT NULL
);
```

### Critical Implementation Notes

1. **The browser backend already has `print_to_pdf`.** Phase 2 implemented it on `BrowserBackend` specifically so Phase 4 does not introduce a new browser dependency. The `PdfBackend` wrapper is thin by design.
2. **Independent browser instance.** Per §6 of the main plan, the browser-based `PdfBackend` creates its own browser instance rather than sharing the acquisition session. This isolates PDF rendering from scraping failures. The browser is spawned on demand and torn down after rendering, unless the user is actively previewing.
3. **Blocking API handling.** `rust-headless-chrome` is synchronous; `chromiumoxide` is async. Either way, the GPUI event loop must not block. Use `gpui-tokio-bridge` or a dedicated worker thread. Add a UI responsiveness test.
4. **HTML is not persisted by default.** The rendered HTML is a transient artifact. Only the PDF is stored. If template debugging is needed, an opt-in setting can dump the HTML to disk.
5. **`displayHeaderFooter` defaults to `false`.** Headers and footers often confuse ATS parsers by injecting page numbers or dates into the extracted text. Leave them off unless a template explicitly requires them.
6. **`printBackground` defaults to `false`.** Colored backgrounds can interfere with text extraction and are unnecessary for most resumes. Templates should not rely on background colors.
7. **PDF metadata.** Set document title and author from `profile.name` and the document kind. This is a small quality-of-life touch that some ATS parsers use.

### Acceptance Criteria

| Criterion | Verification |
| :--- | :--- |
| `BrowserPdfBackend` renders HTML to PDF | Integration test |
| `PdfOptions` parameters are passed through to CDP | Unit test with mock browser |
| Rendered PDF is stored and recorded in `document_renders` | Integration test |
| Browser instance is independent of acquisition session | Architecture test |
| UI remains responsive during rendering | UI responsiveness test |
| `print_to_pdf` parameter coverage is verified against needs | Documentation check |
| PDF metadata is set correctly | Unit test |

---

## Workstream 3 — ATS-Safe Text Ordering

**Priority:** Core deliverable. Makes the output usable.
**Estimated duration:** 1 week

### Deliverables

1. **ATS-safe HTML template conventions.** Templates must use a single-column layout, semantic HTML elements (`<h1>`, `<h2>`, `<ul>`, `<li>`), and no floating text boxes, multi-column CSS, or absolute positioning. The default system templates demonstrate these conventions.
2. **Reading-order assertion in templates.** Every text block has a natural document-flow position. The template engine does not reorder content. This is the browser equivalent of `resumark`'s explicit `order` index: the HTML document flow *is* the declared reading order.
3. **`pdftotext` round-trip verification harness:**

```rust
pub fn verify_extraction_order(pdf: &[u8], expected_order: &[&str]) -> Result<()>;
```

The harness:
- Extracts text via `pdftotext` in both `-raw` (stream order) and default (position-based reading order) modes.
- Asserts that expected strings appear in declared order.
- Uses the `resumark` project's `--verify` approach as the reference implementation.

4. **Extraction-order tests per template.** Every system template has a corresponding fixture test that renders a known CV and asserts extraction order.
5. **Layout constraints documented.** A short `templates/README.md` explaining what makes a template ATS-safe: single column, no floats, no absolute positioning, semantic headings, text-based lists, no images of text.

### Critical Implementation Notes

1. **ATS parsers read the text layer, not the pixels.** This is the central fact. A PDF can look perfect and extract as scrambled column soup. Visual inspection is necessary but not sufficient.
2. **Reading order is a document-flow property, not a renderer property.** With a browser renderer, the HTML document flow determines extraction order. If the template is single-column and semantic, extraction order follows visual order. If the template uses floats or absolute positioning, extraction order may not match visual order. The constraint is on the template, not the renderer.
3. **`pdftotext` is the reference tool.** It is the same tool `resumark` uses for verification. Run it in both modes: `-raw` (stream order) and default (position-based). Both must produce the declared reading order for a template to pass.
4. **The verification harness is a test, not a runtime check.** It runs in CI against fixtures. It does not run on every render, because `pdftotext` may not be available on all user systems.
5. **Existing HTML templates from the original project may need modification.** The original tool's templates were built for Playwright, which is also Chromium. They should render identically, but they may use CSS patterns that scramble extraction order. The Phase 0(a) spike assessed this; if modifications are needed, they are part of this workstream.

### Acceptance Criteria

| Criterion | Verification |
| :--- | :--- |
| Default CV template renders in single-column document flow | Code review |
| Extraction order matches declared order in `-raw` mode | Fixture test |
| Extraction order matches declared order in default mode | Fixture test |
| Extraction order is stable across two renders of the same data | Fixture test (idempotency) |
| Template README documents ATS-safe conventions | Documentation check |
| A deliberately multi-column test template fails extraction verification | Negative test |

---

## Workstream 4 — PDF Preview and Rendering UI

**Priority:** After the rendering pipeline works headlessly.
**Estimated duration:** 1 week

### Deliverables

1. **PDF preview pane** in the CV editor:
   - Renders the current CV through `BrowserPdfBackend`
   - Displays the PDF inline (using a GPUI PDF viewer or a rasterized preview)
   - Refreshes on data change (debounced)
2. **"Render PDF" action** on the Job Detail and CV editor screens
3. **Render options** (collapsible, advanced):
   - Page size (A4 / Letter)
   - Template selection
   - `print_background` toggle (off by default)
4. **Render history** in `document_renders`: list of past renders with backend, template, and timestamp; option to download or open
5. **Batch render** for a job application: CV + cover letter rendered together
6. **Error handling** for render failures: classified cause (browser unavailable, template error, timeout) with an actionable message

### Critical Design Rules

1. **Preview is not a separate rendering path.** The preview uses the same `PdfBackend` as the final render. A preview that differs from the output is a bug.
2. **Rendering is cancellable.** A long render should be interruptible. Use the same cancellation mechanism as evaluation streaming (Phase 3).
3. **Browser availability is surfaced.** If `browser-backend` is not compiled in, the render action is unavailable with an explanatory message. There is no fallback in v1.
4. **Render history is provenance.** Every render is recorded. Users can see which template and backend produced a given PDF, which is useful when comparing outputs or debugging extraction issues.

### Acceptance Criteria

| Criterion | Verification |
| :--- | :--- |
| Preview renders the current CV | UI integration test |
| Preview refreshes on data change | UI test (debounced) |
| Render action produces a stored PDF | UI integration test |
| Render history lists past renders with metadata | UI test |
| Render failure shows a classified cause | UI test |
| Rendering is cancellable | UI test |
| Render action is unavailable when browser backend is disabled | UI test with feature disabled |

---

## Workstream 5 — `printpdf` Assessment Spike

**Priority:** Informational, parallel, does not gate Phase 4.
**Estimated duration:** 3–5 days

### Deliverables

1. A throwaway spike assessing `printpdf`'s manual ops API for a single representative CV template.
2. Reuse the Phase 0(a) spike's findings if it was completed, or redo the assessment if the spike was inconclusive.
3. A memo with the following:
   - Can a single-column CV be expressed as `printpdf` ops (text blocks, lines, embedded fonts)?
   - What is the effort to integrate `glyph_brush_layout` for text wrapping and vertical positioning?
   - What is the effort to enforce and verify ATS extraction order?
   - What is the delta between the browser-based output and the `printpdf` output for the same CV?
   - Recommendation: viable / viable with caveats / not viable, plus an effort estimate to generalize.

### Critical Context

1. **`printpdf`'s HTML-to-PDF feature is not viable.** The official documentation states it is experimental and WIP, recommended only for "basic layouts" with manual element positioning preferred for pixel-exact control. The slimming path runs through the **manual ops API**, not the HTML feature.
2. **`printpdf` is a low-level drawing library.** It gives you `PdfDocument`, pages, layers, and ops for placing text and graphics at explicit coordinates. It does not provide automatic flow layout, text wrapping, or vertical positioning. Integration with `glyph_brush_layout` is required, including manual vertical offset conversions between `f32` pixels and `f64` millimetres.
3. **ATS extraction order becomes your responsibility.** With a browser renderer, HTML document flow determines extraction order. With `printpdf`, you control the content stream directly, so you must explicitly assign and enforce reading order for every text run. The `resumark` project exists specifically to solve this, treating extraction order as a contract and assigning an explicit `order` index to every placed text run.
4. **The spike is informational.** It informs a post-v1 decision. It does not gate Phase 4, and its outcome does not change the v1 architecture.

### Acceptance Criteria

| Criterion | Verification |
| :--- | :--- |
| A representative CV is rendered via `printpdf` manual ops | Spike output |
| `pdftotext` extraction order is verified | Spike output |
| Effort to generalize is estimated | Memo |
| Recommendation is recorded | Memo |

---

## Test Strategy

### Golden-File Render Tests

Every system template has a golden-file test:

- Render a fixture CV through the template and `BrowserPdfBackend`.
- Compare the extracted text (via `pdftotext`) against a golden text file.
- Assert extraction order matches the golden order.
- Optionally compare a rasterized hash for visual regression (lower priority than text order).

Golden files are updated deliberately, not automatically. A change to a golden file requires review.

### ATS Extraction Tests

| Test | Purpose |
| :--- | :--- |
| Single-column CV extracts in declared order | Baseline ATS safety |
| Two renders of the same data produce identical extraction | Idempotency |
| Multi-column test template fails extraction verification | Negative test |
| Floating text box test template fails extraction verification | Negative test |
| Template with images of text fails extraction (no text layer) | Negative test |

### Integration Tests

End-to-end flows:

1. Populate CV data → render → verify PDF exists and is recorded in `document_renders`.
2. Render a cover letter → verify PDF exists and is recorded.
3. Change template → render → verify new render references the new template version.
4. Render the same CV twice → verify two `document_renders` rows (not deduplicated; renders are events, not entities).
5. Batch render CV + cover letter → verify two PDFs and two records.
6. Render with browser backend disabled → verify action is unavailable.

### CI Jobs

| Job | Purpose |
| :--- | :--- |
| `pdf-render` | Render tests with `browser-backend` feature enabled (runs on PR when template or render files change) |
| `pdf-extraction` | ATS extraction-order tests via `pdftotext` (always runs; `pdftotext` installed in CI image) |
| `pdf-golden` | Golden-file comparison tests (always runs) |
| `ui-preview` | PDF preview UI tests with GPUI harness |

---

## Phase 4 Risks

| ID | Risk | Impact | Mitigation |
| :--- | :--- | :--- | :--- |
| P4-R1 | **Existing HTML templates scramble extraction order.** The original tool's templates may use CSS patterns that produce correct visual output but incorrect text-layer order. | High | Phase 0(a) spike assessed this. If modifications are needed, single-column document flow is the fix. Extraction tests catch regressions. |
| P4-R2 | **Browser backend is a hard dependency for PDF output in v1.** Users who disable the `browser-backend` feature cannot produce PDFs. | Medium | Accepted trade-off (see main plan §9). `printpdf` manual-ops backend is the medium-term slimming path. UI clearly communicates availability. |
| P4-R3 | **`pdftotext` is not available on all user systems.** The verification harness requires it. | Low | The harness runs in CI and development, not at runtime. Users are not required to install `pdftotext`. |
| P4-R4 | **Rendering stalls the GPUI event loop.** Same problem as the browser backend and evaluation streaming. | Medium | Use `gpui-tokio-bridge`. Add UI responsiveness test. Cancellation support. |
| P4-R5 | **Template engine scope creep.** Building a full templating language with conditionals, filters, or partials. | Medium | Phase 4 supports flat substitution and `#each` only. Resist adding features. |
| P4-R6 | **`printpdf` spike produces an optimistic estimate.** The manual-ops path looks tractable in a spike but proves expensive to generalize. | Medium | The spike is informational. Treat its estimate as a lower bound. The post-v1 decision should include a second, more detailed assessment if the spike looks promising. |
| P4-R7 | **PDF preview UI is harder than expected.** Inline PDF rendering in GPUI may require a rasterization step or a third-party viewer. | Medium | Start with a simple rasterized preview or an "open in system viewer" fallback. Inline rendering is a polish item, not a gate. |

---

## Weekly Budget

| Week | Focus | Deliverable |
| :--- | :--- | :--- |
| **1** | W1 template system, system template seeding, rendering engine | Templates render HTML from CV data |
| **2** | W2 `BrowserPdfBackend`, integration with `BrowserBackend`, PDF storage | HTML renders to PDF and is stored with provenance |
| **3** | W3 ATS-safe template conventions, extraction-order verification harness, extraction tests | Default templates pass `pdftotext` extraction in both modes |
| **4** | W4 PDF preview UI, render history, batch render, golden-file tests; W5 `printpdf` spike | End-to-end: edit CV → preview → render → open; spike memo written |

**Total:** 3–4 weeks. The lower end (3 weeks) assumes the original templates are already ATS-safe and require no modification. The upper end (4 weeks) accounts for template fixes and preview UI iteration.

---

## Exit Criteria

Phase 4 is complete when **all** of the following are true:

1. System CV and cover letter templates are seeded on first launch and are read-only.
2. Templates render HTML from CV and cover letter data.
3. `BrowserPdfBackend` renders HTML to PDF via the `BrowserBackend` from Phase 2.
4. Rendered PDFs are stored and recorded in `document_renders` with backend, template, and timestamp.
5. Default templates pass `pdftotext` extraction in both `-raw` and default modes.
6. A deliberately multi-column test template fails extraction verification.
7. PDF preview works in the CV editor and refreshes on data change.
8. Render history is accessible and shows provenance.
9. Batch render produces both CV and cover letter PDFs.
10. The `printpdf` assessment memo exists with a recommendation and effort estimate.
11. CI is green on Linux, macOS, and Windows, including render, extraction, and golden-file jobs.
12. A short Phase 4 retrospective memo exists, recording template modification effort, extraction-order findings, and `printpdf` spike outcomes.

---

## What Phase 4 Is Not

Phase 4 renders documents. It does not acquire jobs, evaluate them, or generate text. Specifically:

- There is no new browser dependency. `PdfBackend` delegates to `BrowserBackend`.
- There is no `printpdf` backend in v1. The spike is informational.
- There is no file-based data migration.
- There is no full templating language (conditionals, filters, partials).
- There is no WYSIWYG template editor.
- There is no automated ATS scoring beyond extraction-order verification.
- There is no PDF merging from external sources.

The temptation to "just add a `printpdf` backend now that the spike looks promising" before Phase 4 is complete is the most common way rendering phases overrun. Resist it. The browser backend works. The slimming path is post-v1.

---

The single most important thing to take from this plan: **extraction order is the product.** A resume PDF that looks beautiful but extracts as scrambled columns fails at its only job. The browser backend handles rendering; the template conventions and the `pdftotext` verification harness handle correctness. Treat the extraction tests as the primary acceptance criterion, not the visual output. Visual inspection is necessary but not sufficient — `resumark`'s core insight is that the text layer is the contract, and Phase 4 should adopt that discipline from day one.