# Phase 3: AI Integration — Detailed Execution Plan

**Parent plan:** Draft Execution Plan: Career-Ops Rust Desktop Application (v0.4), §12
**Status:** Draft
**Last updated:** 2026-10-12
**Duration:** 3–4 weeks
**Depends on:** Phase 2 completion (see Entry Conditions)

---

## Purpose

Phase 3 makes the application **intelligent**. It replaces the original project's external AI CLI calls with an embedded LLM integration via `rig`, and it implements the two core value-producing flows: job evaluation and cover letter generation. It also delivers the prompt management system that makes the AI logic editable, versioned, and reproducible.

The phase has three defining characteristics:

1. **Embedded AI, not delegated.** The original tool shells out to an AI coding CLI. Phase 3 embeds the LLM calls directly, using `rig` as the provider abstraction. This removes an external dependency and gives the application control over streaming, error handling, and structured output.
2. **Prompts are data, not code.** System prompts are seeded from embedded resources into SQLite on first launch and are read-only. User prompts are editable and versioned. Every evaluation report records the prompt version that produced it, so historical reports remain reproducible even after prompts change.
3. **Untrusted input is treated as untrusted.** Job descriptions are fetched from the open web and fed to an LLM. This is a prompt injection surface, and Phase 3 must defend against it explicitly (see Workstream 6).

---

## Entry Conditions

Phase 3 begins after Phase 2 exit criteria are met. The following prior outputs are consumed:

| Prior Output | How Phase 3 Uses It |
| :--- | :--- |
| `job_descriptions` table and repository | Source of job text for evaluation |
| `cv_entries` and `profile` repositories | Source of candidate context for prompts |
| `prompts` table and repository | Destination for seeded and user prompts |
| `evaluation_reports` table | Destination for generated reports |
| `cover_letters` table | Destination for generated cover letters |
| `settings` table | Stores provider/model preferences (keys stay in OS keychain) |
| GPUI shell with navigation | Host for Prompt Manager and Evaluation UI |
| Keystore (§7 of main plan) | Source of API keys |
| Unlock flow | Gates access to API keys and job data |
| Backend traits from Phase 1 | Not used directly; `PdfBackend` is Phase 4 |

**Phase 3 assumes job descriptions exist.** It does not acquire them — that is Phase 2. If Phase 2 produced a fixture corpus but no live portals, Phase 3 can still be developed and tested against fixtures.

---

## Scope

| In scope | Out of scope |
| :--- | :--- |
| `rig` integration with provider-agnostic client construction | PDF rendering of cover letters or CVs |
| API key retrieval from OS keychain | Acquisition logic (Phase 2) |
| Provider/model selection UI and persistence | File-based data migration |
| Prompt seeding from embedded `modes/*.md` resources | Full prompt templating language |
| Minimal variable substitution (`{{cv_summary}}`, `{{job_description}}`) | Prompt A/B testing framework |
| Prompt repository with versioning and content hashing | Prompt sharing / export / import |
| Evaluation pipeline with structured output | Browser-based evaluation |
| Streaming evaluation UI | Batch evaluation across hundreds of jobs |
| Match scoring with rationale | Fine-tuning or local model serving |
| Cover letter generation (text) | CV tailoring / rewriting |
| Prompt Manager UI (browse, edit, version) | Full prompt authoring wizard |
| Evaluation UI (trigger, stream, view report) | Visual regression testing |
| `prompt_version` provenance on reports | Multi-modal prompts (images, audio) |
| LLM evaluation tests via `rig::evals` | Continuous evaluation dashboards |
| Prompt injection defense | Adversarial red-teaming |

---

## Sequencing Rationale

The build order is driven by the dependency chain from prompt to report:

```
W1: Provider integration + key management + prompt seeding
    └── W2: Evaluation pipeline (structured output, streaming, storage)
        └── W3: Prompt Manager UI + Evaluation UI
            └── W4: Cover letter generation + LLM evaluation tests + prompt injection defense

W4a: Prompt injection defense (woven throughout, hardened in W4)
```

Provider integration comes first because everything else calls an LLM. Prompt seeding comes next because evaluation needs prompts to exist. The evaluation pipeline is the core deliverable; it must work headlessly before UI work begins. Cover letter generation reuses the same pipeline primitives and is therefore cheaper once evaluation works.

---

## Workstream 1 — LLM Provider Integration and Key Management

**Priority:** First. Everything else depends on a working LLM client.
**Estimated duration:** 1 week

### Deliverables

1. `core::llm` module wrapping `rig` client construction:

```rust
pub enum Provider {
    Anthropic,
    OpenAI,
    Gemini,
    // ... extensible
}

pub struct LlmConfig {
    pub provider: Provider,
    pub model: String,
    pub temperature: f32,
    pub max_tokens: Option<u32>,
}

impl LlmConfig {
    pub fn build_client(&self, api_key: &SecretString) -> Result<rig::client::Client>;
}
```

2. API key retrieval from OS keychain via `core::keystore`:

```rust
pub fn get_api_key(provider: Provider) -> Result<Option<SecretString>>;
pub fn set_api_key(provider: Provider, key: &str) -> Result<()>;
```

3. Provider/model selection stored in `settings` (not in `prompts`, not hardcoded)
4. Model capability registry: which models support structured output, streaming, tool calls, and their context window sizes
5. Error taxonomy mapping `rig` errors to application-level errors: `MissingApiKey`, `RateLimited`, `ContextLengthExceeded`, `ProviderError`, `InvalidResponse`
6. Retry policy with exponential backoff for transient failures (429, 503, network timeouts)

### Critical Implementation Notes

1. **`rig` is pre-1.0 and ships breaking changes.** The crates.io page explicitly warns: "future updates will contain breaking changes." Pin to a specific version in `Cargo.toml` and treat upgrades as deliberate work with a migration review. As of this writing, the current version is `0.44.0`.
2. **`rig` splits into `rig-core` and `rig-agent`.** The root `rig` facade re-exports both. Depend only on `rig` unless there is a specific reason to reach lower.
3. **API keys never touch the database.** They live in the OS keychain. The `settings` table stores only the *provider choice* and *model name*, never the key itself.
4. **Provider differences are real.** Not every provider supports structured output identically. The capability registry must be authoritative, and the UI must warn when a selected model lacks a required capability.
5. **Streaming requires a runtime bridge.** `rig`'s streaming traits return `Stream` types. GPUI uses a different async model. Use `gpui-tokio-bridge` or the equivalent to consume streams from the GPUI context without blocking the UI.
6. **`rig` supports OpenTelemetry GenAI semantic conventions.** Wire this up early if observability is desired; retrofitting it later means touching every call site.

### Acceptance Criteria

| Criterion | Verification |
| :--- | :--- |
| Client construction succeeds for at least two providers | Integration test (environment-gated) |
| API key round-trips through OS keychain | Integration test |
| Missing API key produces `MissingApiKey`, not a panic | Unit test |
| Invalid API key produces a typed provider error | Integration test |
| Model capability registry is queryable and accurate | Unit test against known capabilities |
| Retry policy handles 429 and 503 | Unit test with mock transport |
| No API key appears in logs or error messages | Log audit test |

---

## Workstream 2 — Prompt Seeding and Repository

**Priority:** After provider integration. Needed before evaluation.
**Estimated duration:** 3–5 days

### Deliverables

1. Embedded prompt resources from the original `modes/*.md` files:

```rust
// build.rs or include_str! at module level
const SYSTEM_PROMPTS: &[(&str, &str)] = &[
    ("evaluate", include_str!("../modes/evaluate.md")),
    ("cover_letter", include_str!("../modes/cover-letter.md")),
    ("scan", include_str!("../modes/scan.md")),
    // ... full set enumerated during implementation
];
```

2. First-launch seeding: if `prompts` is empty, insert all system prompts with `is_system = true`
3. Prompt versioning scheme:
   - `version` integer, incrementing on each edit
   - `content_hash` column (SHA-256 of content) for change detection
   - New version = new row, with `supersedes_id` pointing to the prior version
4. Minimal variable substitution supporting at least:
   - `{{cv_summary}}` — rendered from `profile` and `cv_entries`
   - `{{job_description}}` — raw text from `job_descriptions`
   - `{{company_name}}`, `{{job_title}}` — from `job_descriptions`
   - Strict mode: unknown variables fail rendering rather than producing empty strings
5. Prompt repository methods: `get_active(prompt_name)`, `list_versions(prompt_name)`, `create_version(...)`, `get_by_hash(hash)`

### Critical Implementation Notes

1. **System prompts are read-only in the UI but versioned in the database.** When the application ships an updated system prompt in a future release, it inserts a new version rather than overwriting. Historical reports referencing the old version remain reproducible.
2. **The `modes/*.md` file set must be enumerated explicitly.** The original project's prompt files need to be inventoried before seeding. Do not assume the set; read the repository and list them.
3. **Variable substitution is strict by default.** An unknown `{{variable}}` is a rendering error, not a silent empty string. This prevents prompts from degrading silently when a field is renamed.
4. **Content hashing enables reproducibility without storing full prompt text on every report.** The report stores the hash; the prompt text is retrieved from the `prompts` table by hash. If the prompt row is ever deleted, the report's provenance is broken — so prompt rows are never hard-deleted, only superseded.
5. **The full templating language is deferred.** Phase 3 supports flat variable substitution. Conditionals, loops, and partials are a future enhancement. Do not build a templating engine in Phase 3.

### Acceptance Criteria

| Criterion | Verification |
| :--- | :--- |
| First launch seeds all system prompts | Integration test |
| Second launch does not duplicate seeds | Integration test |
| System prompts are marked `is_system = true` | Unit test |
| Editing a user prompt creates a new version, not an overwrite | Unit test |
| Content hash is stable for identical content and changes for different content | Unit test |
| Unknown variable in strict mode fails rendering | Unit test |
| All required variables render correctly from fixture data | Unit test |

---

## Workstream 3 — Evaluation Pipeline

**Priority:** Core deliverable. After prompts exist.
**Estimated duration:** 1–1.5 weeks

### Deliverables

1. `core::evaluation` module with the evaluation flow:

```rust
pub struct EvaluationRequest {
    pub application_id: i64,
    pub prompt_name: String,       // defaults to "evaluate"
    pub model_override: Option<String>,
    pub stream: bool,
}

pub struct EvaluationResult {
    pub report_id: i64,
    pub match_score: f32,          // 0.0..=1.0
    pub recommendation: Recommendation,
    pub content: String,           // full markdown report
    pub prompt_version: i64,
    pub prompt_hash: String,
    pub model_used: String,
}

pub enum Recommendation {
    StrongMatch,
    Match,
    WeakMatch,
    NoMatch,
}
```

2. Structured output schema for the evaluation:

```rust
#[derive(Serialize, Deserialize, JsonSchema)]
pub struct EvaluationOutput {
    pub match_score: f32,
    pub recommendation: Recommendation,
    pub strengths: Vec<String>,
    pub gaps: Vec<String>,
    pub rationale: String,
    pub full_report_markdown: String,
}
```

3. Prompt assembly: system prompt + variable substitution with CV summary and job description
4. Two execution modes:
   - **Structured mode** — uses `rig`'s `Extractor` or `TypedPrompt` to get typed output. Preferred for the match score and recommendation.
   - **Streaming mode** — streams the full markdown report for UI display. Preferred for the narrative.
5. Report storage in `evaluation_reports` with `prompt_version`, `prompt_hash`, `model_used`, and `generated_at`
6. Match score storage on the `applications` row for pipeline sorting
7. Batch evaluation for multiple applications (sequential, with concurrency limit)

### Critical Implementation Notes

1. **Structured output and streaming are complementary, not alternatives.** The match score and recommendation should come from structured output for reliability. The narrative report should stream for UX. Run them as two calls, or use a single structured call and stream a separate narrative call. Do not try to parse a streamed response into a schema.
2. **`rig`'s `Extractor` uses a private "submit" tool.** The model calls the submit tool with data matching the schema. This is more reliable than asking the model to return JSON in prose. Use it for `EvaluationOutput`.
3. **Structured output support varies by provider.** The capability registry (Workstream 1) must be consulted. If the selected model does not support structured output, fall back to text parsing with a clear warning, or refuse with an actionable error.
4. **Token budget matters.** A long job description plus a full CV can exceed a small model's context window. The evaluation pipeline must estimate token count before the call and truncate or summarize when necessary. Prefer summarization over truncation — truncating a job description can drop the most relevant requirements.
5. **`prompt_version` and `prompt_hash` are both stored.** The version is a human-readable integer; the hash is the authoritative content identifier. If a prompt is edited without incrementing the version (a bug), the hash still detects the change.
6. **`model_used` is stored verbatim.** Provider model names change over time. Storing the exact string used at generation time is necessary for reproducibility audits.
7. **Errors are classified, not swallowed.** A failed evaluation must record why it failed. Do not store a partial report as if it succeeded.

### Acceptance Criteria

| Criterion | Verification |
| :--- | :--- |
| Evaluation produces a valid `EvaluationOutput` for a fixture job + CV | Integration test (environment-gated) |
| Match score is in `0.0..=1.0` | Unit test |
| Recommendation is a valid enum variant | Unit test |
| Report is stored with correct `prompt_version`, `prompt_hash`, `model_used` | Integration test |
| Match score is written to `applications.match_score` | Integration test |
| Streaming mode produces incremental text | Integration test |
| Missing API key fails with `MissingApiKey`, not a partial report | Unit test |
| Context length exceeded is detected before the call | Unit test with mock token counter |
| Batch evaluation respects concurrency limit | Unit test |

---

## Workstream 4 — Cover Letter Generation

**Priority:** After evaluation pipeline works. Reuses the same primitives.
**Estimated duration:** 3–5 days

### Deliverables

1. `core::cover_letter` module:

```rust
pub struct CoverLetterRequest {
    pub application_id: i64,
    pub prompt_name: String,       // defaults to "cover_letter"
    pub model_override: Option<String>,
    pub tone: Option<String>,      // optional user-selected tone
}

pub struct CoverLetterResult {
    pub cover_letter_id: i64,
    pub content: String,           // markdown
    pub prompt_version: i64,
    pub prompt_hash: String,
    pub model_used: String,
}
```

2. Prompt assembly: system prompt + variable substitution with CV summary, job description, and optional tone
3. Storage in `cover_letters` table
4. Regeneration: a new generation creates a new row rather than overwriting, so the user can compare versions

### Critical Implementation Notes

1. **Cover letters are generated as markdown text, not PDF.** PDF rendering is Phase 4. The separation matters: the generation pipeline should not depend on the rendering backend.
2. **Regeneration creates a new row.** Users will iterate on cover letters. Preserving every generation is cheap (text is small) and enables comparison. Do not overwrite.
3. **Tone is a soft parameter.** It is passed to the prompt as a variable, not enforced. If the model ignores it, that is a prompt quality issue, not a pipeline bug.
4. **No personal data leaves the local machine except to the LLM provider.** The CV and job description are sent to the configured provider. This must be disclosed to the user during provider setup. There is no local model option in v1.

### Acceptance Criteria

| Criterion | Verification |
| :--- | :--- |
| Cover letter generates for a fixture job + CV | Integration test (environment-gated) |
| Output is stored in `cover_letters` with provenance | Integration test |
| Regeneration creates a new row | Integration test |
| Tone parameter is passed to the prompt | Unit test |
| Missing API key fails cleanly | Unit test |

---

## Workstream 5 — Prompt Manager UI

**Priority:** After prompt repository is seeded and queryable. Can overlap with evaluation pipeline.
**Estimated duration:** 1 week

### Deliverables

1. **Prompt list screen** showing:
   - System prompts (read-only, visually distinguished)
   - User prompts (editable)
   - Category grouping (`evaluate`, `cover_letter`, `scan`)
   - Version count per prompt
   - Last modified timestamp
2. **Prompt detail view**:
   - Content editor (plain text or markdown)
   - Variable reference panel showing available variables for this prompt category
   - Version history list with timestamps and content hashes
   - Diff view between versions (text diff)
3. **Actions**:
   - Edit user prompt (creates new version)
   - Duplicate system prompt as a user prompt (creates an editable copy)
   - Revert to a prior version (creates a new version from old content)
   - View prompt usage (which reports reference this prompt version)
4. **Variable validation**: on save, verify that all `{{variables}}` in the prompt are in the known variable set for that category

### Critical Design Rules

1. **System prompts cannot be edited.** The only way to modify one is to duplicate it as a user prompt and edit the copy. This preserves the ability to ship prompt updates in future releases without conflicting with user changes.
2. **Version history is immutable.** Old versions are never deleted or modified. Reverting creates a new version with the old content, so the history is a complete record.
3. **Variable validation runs on save, not on use.** A prompt with an unknown variable is rejected at save time with a clear error naming the variable.
4. **Prompt usage is traceable.** From a prompt version, the user can see which evaluation reports and cover letters reference it.

### Acceptance Criteria

| Criterion | Verification |
| :--- | :--- |
| System prompts are read-only in the UI | UI test |
| Duplicating a system prompt creates an editable user copy | UI integration test |
| Editing a user prompt creates a new version | UI integration test |
| Version history displays all versions with hashes | UI test |
| Reverting creates a new version with old content | UI integration test |
| Unknown variable is rejected on save | UI test |
| Prompt usage lists referencing reports | Integration test |

---

## Workstream 6 — Evaluation UI and Prompt Injection Defense

**Priority:** Evaluation UI after pipeline works. Injection defense woven throughout, hardened here.
**Estimated duration:** 1 week

### Deliverables

#### Evaluation UI

1. **"Evaluate" action** on the Job Detail screen
2. **Streaming progress display**: the report text appears incrementally as the model generates it
3. **Report viewer**: rendered markdown report with match score, recommendation, strengths, gaps, and narrative
4. **Match score badge** on the Pipeline table, sortable
5. **Re-evaluate action** with a prompt/model override selector
6. **Evaluation history**: all reports for an application, with prompt version and model used

#### Prompt Injection Defense

Job descriptions are untrusted input. A malicious job posting could contain instructions designed to manipulate the evaluation. The defense has three layers:

1. **Structural separation.** The job description is passed as data in a clearly delimited context block, not concatenated into the instruction portion of the prompt. `rig`'s `context()` method supports this; use it rather than string concatenation.
2. **Instruction hierarchy.** The system prompt explicitly states that content within the job description block is untrusted data, not instructions. This is imperfect but raises the bar.
3. **Output validation.** The structured output schema constrains what the model can produce. A match score outside `0.0..=1.0` is rejected. A recommendation outside the enum is rejected. The narrative is displayed but never executed or interpreted as instructions.
4. **No tool access from evaluation.** The evaluation agent has no tools other than the submit tool. It cannot browse, fetch, or take actions. This limits the blast radius of a successful injection.

### Critical Implementation Notes

1. **Streaming UI must be cancellable.** A long evaluation should be interruptible by the user. Use `rig`'s `PauseControl` or an equivalent cancellation mechanism.
2. **Streaming and GPUI integration is the same problem as the browser backend.** Use `gpui-tokio-bridge`. Add a UI responsiveness test.
3. **Injection defense is not a solved problem.** No defense is complete. The goal is to make the common cases fail safely, not to guarantee immunity. Document this limitation.
4. **The structured output validation is the strongest defense.** If the model produces a valid score and recommendation, the injection did not succeed in changing the structured result, even if it changed the narrative. Prioritize structured output reliability.
5. **Evaluation is per-application, not per-job.** The same job can be evaluated by multiple applications (unlikely but possible) with different CVs. The report is linked to the application, not the job.

### Acceptance Criteria

| Criterion | Verification |
| :--- | :--- |
| Evaluate action triggers evaluation and streams to the UI | UI integration test |
| Streaming can be cancelled | UI test |
| Report viewer renders all report fields | UI test |
| Match score badge appears on the Pipeline table | UI test |
| Re-evaluate creates a new report, preserving the old | Integration test |
| Evaluation history shows all reports for an application | UI test |
| Job description passed as context, not concatenated | Code review |
| System prompt states untrusted-data rule | Code review |
| Out-of-range score is rejected | Unit test |
| Evaluation agent has no tools beyond submit | Code review |
| UI remains responsive during streaming | UI responsiveness test |

---

## Workstream 7 — LLM Evaluation Testing

**Priority:** After evaluation pipeline works. Can run in parallel with UI work.
**Estimated duration:** 3–5 days

### Deliverables

1. `rig::evals` integration for prompt quality testing:

```rust
use rig::evals::{LlmJudgeMetric, LlmScoreMetric, SemanticSimilarityMetric};

// Example: assert that the evaluation prompt produces a plausible score
// for a known-good fixture job + CV pair.
let metric = LlmScoreMetric::builder(model)
    .preamble("Score whether the evaluation correctly identified the key requirements.")
    .threshold(0.7)
    .build()?;
let outcome = metric.eval(fixture_input, actual_output).await?;
assert!(outcome.is_pass());
```

2. A fixture set of known job descriptions and CVs with expected evaluation characteristics:
   - A strong match (score expected above 0.8)
   - A weak match (score expected below 0.3)
   - A borderline case (score expected between 0.4 and 0.6)
3. Semantic similarity tests: the generated cover letter should be semantically similar to a reference cover letter for the same fixture
4. Regression tests: evaluation output schema is stable across model versions (where possible)
5. Cassette-backed tests for provider calls, replaying offline by default

### Critical Implementation Notes

1. **`rig::evals` requires the `experimental` feature flag.** As of v0.31.0, it is experimental. Enable `rig = { features = ["experimental"] }` and expect API changes.
2. **LLM evaluation is non-deterministic.** A single run is not a test. Run each evaluation multiple times and assert on the distribution, not a single outcome. Or use a lower threshold with a known variance budget.
3. **Cassette-backed tests are the default.** Live provider calls are expensive and slow. Record cassettes for fixture interactions and replay them in CI. Live tests run nightly or on demand.
4. **Prompt quality is subjective.** The fixture set defines what "good" means. Keep it small and curated. A large fixture set with ambiguous expectations is worse than a small one with clear ones.
5. **Model version changes invalidate cassettes.** When a provider updates a model, re-record cassettes and re-review the fixture expectations. This is deliberate work, not automatic.

### Acceptance Criteria

| Criterion | Verification |
| :--- | :--- |
| Cassette-backed evaluation tests pass in CI | CI green |
| Strong-match fixture scores above 0.8 | Evaluation test |
| Weak-match fixture scores below 0.3 | Evaluation test |
| Borderline fixture scores in expected range | Evaluation test |
| Cover letter semantic similarity passes threshold | Evaluation test |
| Evaluation output schema is stable | Schema assertion test |
| Live tests are environment-gated and nightly | CI configuration |

---

## Test Strategy

### Fixture-Based Testing

| Fixture type | Source | Used by |
| :--- | :--- | :--- |
| Strong-match job + CV | Curated | Evaluation pipeline, LLM eval tests |
| Weak-match job + CV | Curated | Evaluation pipeline, LLM eval tests |
| Borderline job + CV | Curated | Evaluation pipeline |
| Original `modes/*.md` prompts | Repository | Prompt seeding tests |
| Malicious job description with injection payload | Hand-crafted | Injection defense tests |
| Long job description exceeding context window | Hand-crafted | Token budget tests |

### Cassette-Backed Testing

All LLM calls in tests use recorded cassettes by default. Live calls are environment-gated and run nightly. Cassettes are stored in `tests/cassettes/` and are treated as fixtures — changes require review.

### Integration Tests

End-to-end flows:

1. Seed prompts → select provider → configure API key → evaluate a fixture application → verify report stored with provenance.
2. Generate a cover letter → verify stored with provenance → regenerate → verify new row.
3. Edit a user prompt → evaluate → verify new report references the new prompt version.
4. Attempt to edit a system prompt → verify rejection.
5. Submit a malicious job description → verify injection does not produce an out-of-range score or invalid recommendation.
6. Evaluate with a model lacking structured output support → verify fallback or clear error.

### CI Jobs

| Job | Purpose |
| :--- | :--- |
| `llm-cassette` | All LLM tests against recorded cassettes (always runs) |
| `llm-live` | LLM tests against real providers (environment-gated, nightly) |
| `llm-evals` | `rig::evals` prompt quality tests (cassette-backed, always runs) |
| `injection-defense` | Malicious input tests (always runs) |
| `ui-evaluation` | Evaluation UI tests with GPUI harness |

---

## Phase 3 Risks

| ID | Risk | Impact | Mitigation |
| :--- | :--- | :--- | :--- |
| P3-R1 | **`rig` pre-1.0 breaking changes.** The crate explicitly warns of breaking changes. | High | Pin to a specific version. Treat upgrades as deliberate work with migration review. Isolate `rig` behind `core::llm` so churn is absorbed in one place. |
| P3-R2 | **Provider API differences.** Structured output, streaming, and tool support vary by provider. | Medium | Capability registry per provider/model. UI warns when a model lacks a required capability. Fallback or refusal is explicit. |
| P3-R3 | **Prompt injection from malicious job descriptions.** | High | Structural separation, instruction hierarchy, structured output validation, no tool access. Documented as imperfect. See Workstream 6. |
| P3-R4 | **Streaming + GPUI integration causes UI stalls.** Same problem as the browser backend. | Medium | Use `gpui-tokio-bridge`. Add UI responsiveness test. Cancellation support. |
| P3-R5 | **Token costs and rate limits.** A large CV plus a long job description can be expensive. | Medium | Token budget estimation before calls. Prefer summarization over truncation. Batch evaluation with concurrency limits. Display estimated cost where possible. |
| P3-R6 | **Prompt quality is hard to test.** LLM output is non-deterministic. | Medium | Cassette-backed tests with curated fixtures. Run each evaluation multiple times. Assert on distributions, not single outcomes. |
| P3-R7 | **Model version changes invalidate reproducibility.** A provider updates a model and outputs change even with identical input. | Medium | Store `model_used` verbatim. Document that reproducibility is best-effort, not guaranteed. Cassettes are re-recorded on model changes. |
| P3-R8 | **Prompt versioning schema drift.** If the versioning scheme changes, historical reports lose provenance. | Medium | Content hash is the authoritative identifier. Prompt rows are never hard-deleted. Schema changes require a migration that preserves hashes. |
| P3-R9 | **Scope creep into prompt tooling.** Building a full templating language or A/B testing framework. | Medium | Phase 3 supports flat variable substitution only. Conditionals, loops, and partials are explicitly deferred. Resist. |

---

## Weekly Budget

| Week | Focus | Deliverable |
| :--- | :--- | :--- |
| **1** | W1 provider integration, key management, capability registry; W2 prompt seeding, variable substitution, versioning | `rig` client works for two providers; system prompts seeded; variables render correctly |
| **2** | W3 evaluation pipeline: structured output, streaming, storage, provenance | Evaluation produces and stores a report with correct provenance |
| **3** | W4 cover letter generation; W5 Prompt Manager UI | Cover letters generate and store; Prompt Manager browses, edits, versions |
| **4** | W6 Evaluation UI, streaming, injection defense; W7 LLM evaluation tests | End-to-end: evaluate → stream → view report → regenerate; injection tests pass |

**Total:** 3–4 weeks. The lower end (3 weeks) assumes `rig` integrates smoothly and structured output works across the chosen providers. The upper end (4 weeks) accounts for provider quirks and injection-defense iteration.

---

## Exit Criteria

Phase 3 is complete when **all** of the following are true:

1. At least two LLM providers are integrated and selectable in Settings.
2. API keys are stored in and retrieved from the OS keychain, never the database.
3. System prompts are seeded on first launch from embedded resources and are read-only.
4. User prompts can be created, edited, versioned, and reverted.
5. Every evaluation report records `prompt_version`, `prompt_hash`, and `model_used`.
6. Evaluation produces a structured `EvaluationOutput` with a valid match score and recommendation.
7. Evaluation reports stream to the UI incrementally and can be cancelled.
8. Cover letters generate as markdown and are stored with provenance.
9. Prompt injection defenses are implemented and tested against malicious fixtures.
10. `rig::evals` tests run in CI against cassettes and pass.
11. CI is green on Linux, macOS, and Windows, including cassette, injection-defense, and UI jobs.
12. A short Phase 3 retrospective memo exists, recording `rig` integration friction, provider quirks, and injection-defense effectiveness.

---

## What Phase 3 Is Not

Phase 3 makes the application intelligent. It does not render PDFs, and it does not acquire jobs. Specifically:

- There is no PDF rendering of cover letters or CVs. That is Phase 4.
- There is no browser backend work. That is Phase 2.
- There is no file-based data migration.
- There is no full prompt templating language (conditionals, loops, partials).
- There is no prompt A/B testing framework.
- There is no fine-tuning, no local model serving, no multi-modal input.
- There is no batch evaluation of hundreds of jobs in one action.

The temptation to "just render the cover letter to PDF" before Phase 3 is complete is the most common way AI phases overrun. The `PdfBackend` trait exists; the implementation is Phase 4. Resist.

---

The single most important thing to take from this plan: **structured output is the backbone of the entire AI integration.** The match score, the recommendation, and the injection defense all depend on the model returning a schema-conformant object rather than prose. `rig`'s `Extractor` and `TypedPrompt` make this reliable, but only if the chosen model supports structured output. Verify that capability for your target providers **in Week 1**, before building anything on top of it. If structured output is unreliable, the entire evaluation pipeline needs a different design, and discovering that in Week 3 is expensive.