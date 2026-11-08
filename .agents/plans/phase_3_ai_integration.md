# Phase 3: AI Integration — Detailed Execution Plan

**Parent plan:** Draft Execution Plan: Career-Ops Rust Desktop Application (v0.30), §12
**Status:** In progress v0.15 — WS1–WS6 are complete (provider, keys, compact prompts, scoped versioning, strict rendering, evaluation with migration 0006, cover letters, the Prompt Manager, and the evaluation UI with injection defenses); WS7 can start
**Last updated:** 2026-11-07
**Duration:** 3–4 weeks
**Depends on:** Phase 2 completion, Phase 2.7 (CV/Profile editor), Phase 2.8 (fetch controls and detail opt-in), Phase 2.9 (queue state and preferences); see Entry Conditions

---

## Change Log

| Version | Change |
| :--- | :--- |
| v0.1 | Initial Phase 3 plan, written against parent plan v0.4. |
| v0.2 | Updated for parent plan v0.5. Removed Phase 0-derived "fixture corpus" language from Entry Conditions; Phase 2 now produces its own fixtures and live job descriptions. Added Open Questions section (Q-5). Noted that the two-backend architecture change does not affect Phase 3. `rig` version note flagged for verification. No material change to workstreams, estimates, or exit criteria. |
| v0.3 | Phase 2.3 closed, so the input shape is now known: every provider stores `job_descriptions.raw_text` as **HTML**, measured at 3.6–7.7 KB per posting live. Phase 3 no longer assumes prose. Workstream 3 gains a deterministic `core::text::plain_text` normalizer and an `input_chars` audit field; the plan states why stripping at acquisition was rejected; normalization gets fixtures, unit tests, an acceptance criterion, and an exit criterion. |
| v0.4 | Revised after Phases 2.5 and 2.6 closed. Records what the codebase already provides (seeded prompts with versioning, `core::text::plain_text`, `applications::ensure` and `Not a match`, the Pipeline search/detail/delete UI, `docs/LIVE_TESTS.md` gating). Corrects three stale assumptions: API keys get their own keychain namespace rather than using `core::keystore`; the original `modes/*.md` prompts cannot be sent verbatim (27–93 KB of tool-oriented instructions); and prompt versioning must be scoped by `is_system` so a user copy and a shipped system update cannot collide. Adds migration 0005 for report provenance, a job-id-based evaluation API that ensures the application, compact embedded prompts derived from the originals, and the current UI integration points. Open questions Q-24–Q-29 need owner decisions before WS1. |
| v0.5 | Owner decisions recorded. **Q-29:** Phase 2.7 (CV/Profile editor) precedes Phase 3 and provides the canonical `cv_summary`. **Q-25:** DeepSeek is the first provider, through its OpenAI-compatible API; the abstraction stays open for a second. **Q-24, Q-26, Q-27, Q-28, Q-5:** confirmed as recommended. WS1 is now DeepSeek-first; WS2 consumes the Phase 2.7 summary renderer; the exit criterion allows closing with DeepSeek if a second provider would delay the core flows. |
| v0.6 | Phase 2.7 closed, so the candidate-context dependency is a shipped function rather than an assumption: `repo::profile::cv_summary(&SqlitePool) -> Result<Option<String>>` for production and the pure `render_cv_summary(&Profile, &[CvEntry]) -> Option<String>` for tests, both in `crates/core/src/repo/profile.rs`. The format is fixed and documented in the renderer's doc comment; `None` for an empty profile is covered by `an_empty_profile_has_no_summary`. Phase 3 substitutes the string verbatim into `{{cv_summary}}` and must not re-format. |
| v0.7 | Records the Phase 2.8 and Phase 2.9 inputs and resolves the ordering. **2.9 was renumbered from 3.1 and now lands before Phase 3**, which removes the circular dependency: Phase 3's entry conditions had assumed the queue existed while 2.9/3.1 had assumed Phase 3 did. 2.9's own inputs were Phase 2.5 deliverables, so nothing from `rig` was ever required there. Consequences: the report write sets `queue_state = 'evaluated'` unconditionally in the same transaction (note 13 no longer needs a compile-time guard), and the migration is renumbered **0005 → 0006** because 2.9 takes 0005. WS3 item 11 (combined fetch-then-evaluate) and 6.1a (thin-row button text) respond to Phase 2.8's detail opt-in. |
| v0.8 | Checked against the shipped 2.7–2.9 code, with one correctness fix and four corrections. **The "same transaction" claim in note 13 was not expressible:** `job_descriptions::set_queue_states` opens and commits its own transaction, so WS3 adds a transaction-scoped variant (`set_queue_state_tx(&mut Transaction, ..)`) and calls it inside the report insert; otherwise a crash between the two writes leaves a report on a row still marked `evaluate`. Corrections: **Settings is a real screen** since 2.9 (acquisition preferences), so WS1 extends it with an AI section instead of replacing a placeholder; **the Pipeline detail already has a Queue row** (**Mark for details** / **Mark for evaluation** / **Dismiss**) and the header a bounded **Fetch descriptions (N)** batch, so WS6 composes with both and its thin-row path reuses `manual::refresh_job`; **`core::text::plain_text` matches the documented signature exactly** (`PlainText { text, truncated, source_chars }`), so WS3 item 3 is a call, not new code; and **`core::logging` + `docs/logging.md` already forbid logging prompts and completions**, so WS1 adds one count-and-outcome line per call rather than a second logging story. Also adds exit criterion 16 (the queue write), and **Q-43** — the plan never said what happens to `applications.status` during an evaluation, and `Discovered → Evaluating → Ready` is already permitted, so the owner has to confirm the intended behavior. |
| v0.9 | **Q-43 resolved by the owner: the evaluation moves the application through its lifecycle.** `discovered` and `ready` enter `evaluating` before the call, and success returns to `ready` — all three transitions are already permitted by `applications::is_valid_transition`. Any other status is left alone, a failure or cancellation returns to the status the row had, and a `NoMatch` report goes straight to `mark_not_a_match` instead of passing through `ready`. WS3 gains deliverable 12 and an acceptance criterion; exit criterion 17 pins it. **`rig` is pinned to `0.46.0`**, the current release as of this date; note 1 and P3-R1 now record the checked version instead of asking for one. Also corrects WS3 item 10: `mark_not_a_match` refuses a submitted application with an `Err` rather than acting as a silent no-op, so the caller treats that error as "leave the status alone" and still stores the report. |
| v0.10 | **WS1 implemented.** `core::llm` is the only module that names a `rig` type: `LlmProvider`/`LlmConfig` (stored in `settings`, keys `llm.*`), `capabilities()` (DeepSeek chat = JSON mode + tools + streaming + 64K; reasoner = no structured output and no tools; unknown models get provider defaults), `LlmError` (the planned taxonomy, with context-window detection from status 413 and body markers), `retry` (3 attempts, provider `Retry-After` wins over the computed backoff), `keys` (`com.career-ops.app` / `llm-api-key-{provider}-v1`, write-only, `keychain`-gated), and `status()` (the first unmet condition, in the order a user would fix it). The Settings screen gains an **AI evaluation** section with its own save: provider, model, a write-only key field, **Save key** / **Delete key**, **Test connection**, and the data-sharing disclosure. **Test connection is free:** `verify()` asks DeepSeek's `/user/balance`, so it spends no tokens and returns no completion. Only DeepSeek is constructible; OpenAI/Anthropic/Gemini are shown disabled rather than hidden, and they parse so a stored choice survives. **Deviation:** `rig` 0.46.0 requires rustc **1.97.1**, so the workspace MSRV moved from 1.83 to 1.97.1, and the tree now carries two `reqwest` versions (0.12 for acquisition, 0.13 for `rig-reqwest`) — recorded in the risk table rather than silently accepted. `LlmClient` exposes only `provider`, `model`, and `verify()` until WS3 adds completion and streaming. Live coverage: `deepseek_accepts_the_configured_key`, `#[ignore]`d and indexed in `docs/LIVE_TESTS.md`. |
| v0.11 | **WS2 implemented.** Two compact prompts replace the agent-oriented originals as call input: `evaluate.md` (5,990 chars ≈ 1.5k tokens) carries the five-dimension rubric, the global 1–5 bands mapped onto `match_score`/`recommendation`, the requirement-table rules (importance bands, evidence tiers, inferred capped below `critical`), red flags, and the JSON contract; `cover_letter.md` (2,558 chars ≈ 0.6k tokens) carries the letter rules, the banned-phrase list, and the shape. Both are seeded under the fixed names `evaluate` / `cover_letter` with matching categories; the originals stay seeded as reference. **Versioning is now scoped by `(name, is_system)`:** `create_version` and `supersedes_id` stay inside one owner's sequence, `active_for_use` prefers the user's copy and falls back to the system prompt, and `list_versions`, `list_categories`, and `get_by_hash` complete the repository. **`core::prompt_render`** substitutes the five known variables (`cv_summary`, `job_description`, `company_name`, `job_title`, `tone`) and fails on an unknown name or an unclosed placeholder rather than rendering an empty string; there is no logic beyond substitution. **Deviation:** the unique index on `(name, is_system, version)` stays in **0006**, because migrations are append-only and 0006 belongs to WS3 — the test that proves the seeded data and a user copy satisfy it ships now. |
| v0.12 | **WS3 implemented.** Migration **0006** adds `match_score`, `recommendation`, `input_chars`, `source_chars`, and `truncated` to `evaluation_reports`, `input_chars` to `cover_letters`, and the unique index on `prompts(name, is_system, version)`; the migration test builds a 0005 schema by hand, preserves a pre-existing report and letter, and pins the two-owner index rule. `core::evaluation` ships `EvaluationOutput` (schemars `JsonSchema`, `Recommendation` derived from the 1–5 bands), a strict `parse()` that rejects an out-of-range score, an empty report, or a missing field, a `CompletionBackend` trait with `LlmClient` as its only production implementation, and `run_evaluation`/`run_batch`: normalize with `text::plain_text`, resolve `active_for_use`, render strictly, check the model's context window, ensure the application, move `discovered`/`ready` through `evaluating`, ask once plus one repair, then write the report, the application link/score/status, and `queue_state = 'evaluated'` in **one transaction**. A failed or unusable reply restores the previous status and stores nothing; `NoMatch` calls `mark_not_a_match` after the commit and logs a refusal for a submitted application. **Deviations recorded:** the capability check is `schemars`-typed but DeepSeek's JSON mode still validates locally; the repair is capped at one retry; the CV budget is `MAX_CV_CHARS = 6_000`; the thin threshold is the new `llm.thin_description_chars` setting (default 400); the pre-flight context check estimates tokens at four characters per token because the client has no tokenizer; and the app reloads the page after a thin-row fetch rather than threading the fetched text through `EvaluationResult`. The app gains `PipelineAction::Evaluate` / `FetchDescriptionAndEvaluate` and `start_evaluation`/`poll_evaluation`; the two buttons stay disabled until WS6, and `with_evaluation_backend` is the offline test seam. Tests: 19 in `crates/core/tests/evaluation.rs`, one app round trip, and a delete-gate assertion through `user_touched_ids`. |
| v0.13 | **WS4 implemented.** `core::cover_letter` shares the evaluation's primitives rather than duplicating them: the `CompletionBackend` trait moves to **`core::llm`** (evaluation re-exports it, so no caller changed), and `prompt_render` now owns `MAX_CV_CHARS`, `estimated_tokens`, and `take_chars` for both flows. `run_cover_letter` normalizes the posting with `plain_text`, resolves `active_for_use`, renders the five variables with the **trimmed optional tone**, refuses a prompt over the model's context window, ensures the application, and stores free text (`json_mode = false`) with `model_used`, `prompt_version`, `prompt_hash`, and `input_chars`. `repo::cover_letters` adds insert/get/list/latest/delete. Regeneration is an insert, so every draft stays comparable; an empty reply is an error, not a stored blank; and a letter never changes the application's status or the job's queue. The app gains `PipelineAction::GenerateCoverLetter { job_id, tone }`, `start_cover_letter`/`poll_cover_letter`, and a `has_user_state` update; the model seam is now `with_completion_backend` and serves both pipelines. **Deviations:** letters are not streamed (the WS4 request shape has no `stream`; WS6 can add it), and `cover_letters.input_chars` records the **assembled prompt** per the 0006 column comment, not the job text that evaluation records. Tests: 11 in `crates/core/tests/cover_letter.rs` and two app tests (a stored letter plus the missing-key path). |
| v0.14 | **WS5 implemented.** `repo::prompts` gains `list_threads` (one row per `(name, is_system)` with latest version, count, and timestamp, ordered so both owners read together) and `usage(content_hash)` (evaluation reports and cover letters that recorded a version, joined to the posting). `core::prompt_render` gains `validate` (placeholder check without substitution), `variables_for_category` (only `evaluate` and `cover_letter` have the closed five; reference categories may use their own syntax — `pdf.md` genuinely uses `{{PHOTO}}`), and `Variable::description` for the reference panel. The **Prompts** screen replaces the placeholder: a left list grouped by category with **In use** markers, and a right pane with read-only system content, a user editor, a version list with hash prefixes and usage counts, a cached line diff against the current version, a "Used by" section, and the variable reference. **Duplicate as my copy**, **Save new version**, **ShowVersion**, and **Revert to this version** all write through `create_version`, so history is append-only; validation runs on save and refuses an unknown variable before any write. **A real bug surfaced and was fixed:** a detail load for a previously selected thread could overwrite the new selection because the task was single-flight — the app now records which selection a load is for and discards a stale result. Manual script gains **§6c**. Tests: 3 core integration tests, 2 renderer unit tests, 5 `egui_kittest` screen tests, and 2 app tests (the full duplicate/edit/revert flow and the validation rules). |
| v0.15 | **WS6 implemented.** The Pipeline detail gains three tabs — **Description**, **Evaluation**, **Cover letter** — and the two placeholder buttons become real: **Evaluate** (or **Fetch description and evaluate** when the stored text is thin) and **Generate cover letter**. The Evaluation tab shows prompt/model overrides, the report (score, recommendation, model, prompt version, narrative), a **History** list with every stored run, and an **Evaluate without description** link for a teaser; the cover-letter tab shows the letter, earlier generations, and a tone override. The row gains a compact score badge with the band in its hover, and the score, status, queue, delete gate, and history all follow a run without a full reload. **Streaming** text is displayed while the call runs, with **Stop**; `core::llm::CancelToken` races the provider future against the flag (`biased` so a pre-set Stop never sends the request), and cancellation takes the failure path: the status is restored and nothing is stored. **Injection defense:** `prompt_render::untrusted_block` fences the posting between explicit markers and neutralizes a forged marker; both compact prompts state the data/instruction rule; output validation and the absence of tools remain the strongest layers. `docs/prompt-injection.md` records the layers, the offline tests, and the residual risk. **Deviation:** `rig` 0.46 has a `documents` channel that would separate the posting structurally; it is not used because the prompt contract is a single templated `{{job_description}}` that user prompts rely on. **Deviations in UI:** evaluation history is a list, not a diff; the full report stays markdown (the structured strengths/gaps live in it). Manual script gains **§6d**. Tests: 21 evaluation (cancellation + a hostile posting), 12 cover-letter (cancellation), 47 `egui_kittest` Pipeline tests (tabs, thin/full, streaming, Stop, history, cover letter, score badge), and 169 app tests (score, re-evaluate with an override, and a mid-call Stop). |

---

## Purpose

Phase 3 makes the application **intelligent**. It replaces the original project's external AI CLI calls with an embedded LLM integration via `rig`, and it implements the two core value-producing flows: job evaluation and cover letter generation. It also delivers the prompt management system that makes the AI logic editable, versioned, and reproducible.

The phase has five defining characteristics:

1. **Embedded AI, not delegated.** The original tool shells out to an AI coding CLI. Phase 3 embeds the LLM calls directly, using `rig` as the provider abstraction. This removes an external dependency and gives the application control over streaming, error handling, and structured output.
2. **Prompts are data, not code.** System prompts are seeded from embedded resources into SQLite on first launch and are read-only. User prompts are editable and versioned. Every evaluation report records the prompt version that produced it, so historical reports remain reproducible even after prompts change.
3. **Untrusted input is treated as untrusted.** Job descriptions are fetched from the open web and fed to an LLM. This is a prompt injection surface, and Phase 3 must defend against it explicitly (see Workstream 6).
4. **Job text is markup, not prose.** Every provider stores what the source
   served: JSON-LD `description`, Eightfold's `jobDescription`, and
   SmartRecruiters' composed sections are all HTML, and the live Phase 2.3/2.6
   runs measured 2–11 KB per posting (Waymo 3.6 KB, NVIDIA 7.7 KB). Phase 3
   therefore normalizes job text to plain text in one deterministic place before
   prompt assembly. Stripping at acquisition was rejected: the stored row is the
   record of what the site said, the markup may be wanted later, and a re-fetch
   is not guaranteed. Telling the model to ignore tags was rejected too: it is
   unverifiable offline, wastes tokens on markup, and varies by model.
5. **Single-shot calls, not an agent.** The original `career-ops` prompts are
   written for a CLI agent with file access, Playwright, and a data root; they
   are 27 KB (`_shared`) to 93 KB (`oferta`) each. Phase 3 sends one request and
   receives one structured response, so it ships **compact prompts derived from**
   those files rather than sending them verbatim (Q-24).

**The two-backend acquisition architecture does not affect Phase 3.** It consumes `job_descriptions` rows, which have the same shape regardless of whether they were acquired by Tier 1 (HTTP/JSON) or Tier 2 (Obscura). It does not call a browser, and it does not depend on which backend produced the text.

---

## Entry Conditions

Phase 3 begins after Phase 2 exit criteria are met and Phase 2.7 is complete.
Phases 2.1–2.6 are complete; Phase 2.7 provides the candidate data. The following
prior outputs are consumed:

| Prior Output | How Phase 3 Uses It |
| :--- | :--- |
| `job_descriptions` table and repository | Source of job text for evaluation. `raw_text` is **HTML** for every provider; `core::text::plain_text` is implemented and tested, and the Pipeline's `JobRow` already carries the normalized `plain_text`. |
| `cv_entries` and `profile` repositories | Source of candidate context for prompts; Phase 2.7 adds the editor and the canonical `cv_summary` renderer |
| `prompts` table, `repo::prompts`, and `prompts_seed` | Already seeded on every unlock: 14 system prompts, idempotent, with `create_version`, `active`, and `active_system`. Phase 3 adds the compact prompts, lookup helpers, and `is_system`-scoped versioning. |
| `applications` table and repository | `ensure`, `mark_not_a_match`, `transition`, `attach_report`, and `set_match_score` exist. `Not a match` is a real status with transitions and a filter (Phase 2.5 WS3). Evaluation ensures the row before writing a report. |
| `evaluation_reports` and `cover_letters` tables | Present from 0001; **no repository or write path exists yet**. Phase 3 adds both, plus migration 0006 for report provenance (`input_chars` and structured fields). |
| `settings` table and `repo::settings` | `get_json`/`set_json`/`get_or` already exist for provider and model preferences. Keys never go here. |
| egui shell with navigation | Pipeline detail renders **Evaluate** and **Generate cover letter** disabled with an "Available in a later phase" tooltip, plus Phase 2.9's **Queue** row (**Mark for details** / **Mark for evaluation** / **Dismiss**) and the header's **Fetch descriptions (N)** batch. **Settings is a real screen** (acquisition preferences, Phase 2.9); **Prompts is still a placeholder**. Phase 3 extends the first and replaces the second. |
| Pipeline list/detail state | Search, filters, delete (with a typed gate), the raw-text toggle, and status badges exist. Evaluation UI must compose with them; deleting a job cascades its reports. |
| Keystore (§7 of main plan) | Provides the unlock flow and the envelope key. It is **not** a general secret store, so Phase 3 adds a separate keychain namespace for API keys (WS1). |
| Unlock flow | Gates access to API keys and job data |
| Backend traits from Phase 1 | Not used in Phase 3; `PdfBackend` rendering arrives in Phase 4 |
| `docs/LIVE_TESTS.md` conventions | Phase 3's live provider and LLM tests are indexed there, `#[ignore]`d or env-gated, and never run in the default offline suite. |
| Phase 2.8 output | **Refresh** / **Continue** / **Fetch all pages** are the portal fetch actions (the header keeps **Reload list** and **Fetch all portals**); detail enrichment is per-portal opt-in (default off for new portals, retained for Waymo and Ubisoft). Phase 3 therefore sees rows whose `raw_text` is often a card teaser until the user fetches descriptions. |
| Phase 2.9 output | `job_descriptions.queue_state` exists (`new` / `details_needed` / `evaluate` / `dismissed` / `evaluated`) with `set_queue_state(pool, id, state, reason)` and the batch `set_queue_states(pool, ids, state, reason) -> usize` (one transaction, reports rows changed). `profile.preferences` is populated; `manual::fetch_descriptions_batch(pool, ids, source, cancel, progress)` is available for a bounded refresh of flagged rows, capped at `DESCRIPTION_BATCH_LIMIT = 20`. |
| `core::logging` and `docs/logging.md` | One `tracing` seam with a rotated file and an in-memory ring, already rendering in the **Logs** screen and drawer. Its never-log list already forbids prompts, completions, CV text, and key material, so Phase 3 logs counts, models, and outcomes only. |

**Phase 3 assumes job descriptions exist in the database.** Phase 2 supplies them
via the tiered acquisition pipeline, and Phase 2 is complete (six known sites and
live-verified ATS boards). Phase 3 can be developed and tested against real rows;
fixtures captured during Phase 2 remain for offline tests. Evaluation and cover
letter generation do not need live network access to the target portals.

Phase 3 owns the LLM integration end-to-end. It does not acquire job
descriptions — that is Phase 2.

**Phase 3 may assume the text contains markup, and must not assume it is prose.**
A row is whatever the provider returned: Eightfold sends a full HTML description,
JSON-LD sends HTML, SmartRecruiters sends composed HTML sections, and a manual
paste may be plain text. The pipeline therefore treats normalization as a step
with fixtures and tests, not as an instruction to the model. The Phase 2.3
retrospective records the same conclusion under "What to carry into Phase 3".

**Candidate data comes from Phase 2.7 (complete).** That phase ships the CV /
Profile editor and the canonical `cv_summary` renderer in
`crates/core/src/repo/profile.rs`:

- `profile::cv_summary(pool) -> Result<Option<String>>` — loads the singleton
  (`ensure_singleton`) and its entries, then renders. This is what Phase 3 calls
  at evaluation time.
- `profile::render_cv_summary(&Profile, &[CvEntry]) -> Option<String>` — the pure
  function both `cv_summary` and the screen's preview use. Phase 3 tests can call
  it with fixtures.

Phase 3 must not format `profile` / `cv_entries` itself: it substitutes the
returned string verbatim into `{{cv_summary}}`. The format (header, contact line,
then `Summary:`, `Experience:`, `Education:`, `Skills:` in that order) is
documented on the renderer. An empty profile returns `None`, which Phase 3 turns
into an actionable "add your CV" error before assembling a prompt.

---

## Scope

| In scope | Out of scope |
| :--- | :--- |
| `rig` integration with provider-agnostic client construction | PDF rendering of cover letters or CVs (Phase 4) |
| API key retrieval from a dedicated OS-keychain namespace | Acquisition logic (Phase 2) |
| Provider/model selection UI and persistence | File-based data migration (Phase 6) |
| Compact prompts derived from the original `modes/*.md` files, seeded through the existing `prompts_seed` | Sending the original agent-oriented prompts verbatim (27–93 KB, file and tool instructions) |
| Minimal variable substitution (`{{cv_summary}}`, `{{job_description}}`) | Full prompt templating language |
| Prompt repository with versioning scoped by `is_system` | Prompt sharing / export / import |
| Evaluation pipeline with structured output | Browser-based evaluation |
| Ensuring an application row before evaluating a Pipeline job | Tool-using evaluation (browsing, file writes, multi-step agents) |
| Streaming evaluation UI | Batch evaluation across hundreds of jobs |
| Match scoring with rationale | Fine-tuning or local model serving |
| Cover letter generation (text) | CV tailoring / rewriting |
| Prompt Manager UI (browse, edit, version) | Full prompt authoring wizard |
| Evaluation UI (trigger, stream, view report) | Visual regression testing |
| `prompt_version`, `prompt_hash`, `model_used`, and `input_chars` provenance | Multi-modal prompts (images, audio) |
| Auto-marking `Not a match` from a NoMatch report, without overriding a submitted application (Q-20) | Continuous evaluation dashboards |
| Moving the application `discovered`/`ready` → `evaluating` → `ready` while an evaluation runs, and restoring it on failure (Q-43) | Any other status change initiated by the app (the user still owns `applied`, `interviewing`, `rejected`, `offer`) |
| LLM evaluation tests via `rig::evals`, gated like the existing live tests | Adversarial red-teaming |
| Prompt injection defense | Recording description-versus-teaser provenance on the job row (a Phase 2 follow-up) |
| Deterministic HTML-to-text normalization before prompting (the function already exists; Phase 3 calls it) | Sending markup to the model, or stripping it at acquisition |
| Migration 0006 for report provenance and structured fields | A prompt marketplace or remote sync |
| `{{cv_summary}}` rendered from the profile and CV repositories | A full CV/Profile editor (recommended as Phase 2.7; Q-29) |

---

## Sequencing Rationale

The build order is driven by the dependency chain from prompt to report:

```
WS1: Provider integration + key management + capability registry
    └── WS2: Compact prompts, repository helpers, variable rendering
        └── WS3: Evaluation pipeline (structured output, streaming, storage)
            ├── WS4: Cover letter generation
            ├── WS5: Prompt Manager UI
            └── WS6: Evaluation UI + prompt injection defense
                └── WS7: LLM evaluation tests (cassettes, live gates)
```

Provider integration comes first because everything else calls an LLM. Prompt
seeding and versioning already exist, so WS2 is smaller than originally planned:
the compact prompts, `is_system`-scoped versioning, and variable rendering.
The evaluation pipeline is the core deliverable; it must work headlessly before
UI work begins. Cover letter generation reuses the same pipeline primitives and
is therefore cheaper once evaluation works.

---

## Workstream 1 — LLM Provider Integration and Key Management

**Priority:** First. Everything else depends on a working LLM client.
**Estimated duration:** 1 week

### Deliverables

1. `core::llm` module wrapping `rig` client construction:

```rust
// Named to avoid colliding with `acquisition::config::Provider`.
pub enum LlmProvider {
    DeepSeek,      // first provider (Q-25)
    OpenAI,
    Anthropic,
    Gemini,
    // ... extensible
}

pub struct LlmConfig {
    pub provider: LlmProvider,
    pub model: String,
    pub temperature: f32,
    pub max_tokens: Option<u32>,
    /// Character budget for the job text (WS3's proxy for tokens).
    pub max_job_chars: usize,
}

/// The only `rig`-typed value in the crate; the app never names one.
pub struct LlmClient { /* provider, model, rig::providers::openai::OpenAI */ }

impl LlmConfig {
    pub fn build_client(&self, api_key: &Zeroizing<String>) -> Result<LlmClient, LlmError>;
}

impl LlmClient {
    pub fn provider(&self) -> LlmProvider;
    pub fn model(&self) -> &str;
    /// The credential check. DeepSeek answers `/user/balance`, so it is free.
    pub async fn verify(&self) -> Result<(), LlmError>;
}
```

   `rig` 0.46 constructs a DeepSeek client as
   `OpenAIConfig::with_key(&wire::DEEPSEEK, key).client()`, and the same type
   serves OpenAI; that is why `build_client` returns a wrapper rather than a
   `rig` type. Completion and streaming are added to the wrapper in WS3.

2. API key storage in a **dedicated keychain namespace**, not in `core::keystore`:

```rust
// core::llm::keys, gated by the existing `keychain` feature.
pub const SERVICE: &str = "com.career-ops.app";

pub fn get_api_key(provider: LlmProvider) -> Result<Option<Zeroizing<String>>>;
pub fn set_api_key(provider: LlmProvider, key: &str) -> Result<()>;
pub fn delete_api_key(provider: LlmProvider) -> Result<()>;
```

   `Zeroizing` comes from the crate the keystore already uses; no `secrecy`
   dependency is needed.

   Use one account per provider (`llm-api-key-deepseek-v1` first, then
   `llm-api-key-openai-v1`, …) under the same service name the keystore already
   uses. `core::keystore` owns the database envelope key and must not grow a
   general secret API; sharing the service name keeps the OS keychain entry
   grouping coherent without coupling the two concerns. The functions are absent
   (returning `KeychainUnavailable`) when the feature is off, matching the
   keystore's pattern.

3. Provider/model selection stored in `settings` (not in `prompts`, not hardcoded).
   The existing `repo::settings` JSON store is the destination; keys are
   `llm.provider`, `llm.model`, `llm.temperature`, `llm.max_tokens`, and
   `llm.max_job_chars`. The first-run default is `deepseek` + `deepseek-chat`.
   An absent value means "not configured yet", which the UI renders as a call to
   Settings rather than an error.
4. Model capability registry: which models support structured output, streaming, tool calls, and their context window sizes
5. Error taxonomy mapping `rig` errors to application-level errors: `MissingApiKey`, `RateLimited`, `ContextLengthExceeded`, `ProviderError`, `InvalidResponse`
6. Retry policy with exponential backoff for transient failures (429, 503, network timeouts)
7. **Settings UI.** The screen exists since Phase 2.9 and owns the
   acquisition preferences; Phase 3 adds an **AI** section beside them rather
   than replacing the screen: provider selector (DeepSeek selected by default),
   model field, API key entry (write-only; saved to the keychain and never shown
   again), a **Test connection** action, and the data-sharing disclosure (the CV
   summary and job text are sent to the selected provider; name the provider and
   its jurisdiction). Missing capability and missing-key states are explained
   here rather than surfaced as errors from the Pipeline. Saving the AI section
   is a separate action from **Save preferences**: one writes `profile`, the
   other `settings` and the keychain, and a failed keychain write must not leave
   the preferences form dirty.
8. A `llm.status()` helper the Pipeline uses to decide whether the Evaluate and
   Cover letter buttons are enabled, with a reason when they are not.

### Critical Implementation Notes

1. **`rig` is pre-1.0 and ships breaking changes.** The crates.io page explicitly warns: "future updates will contain breaking changes." Pin to a specific version in `Cargo.toml` and treat upgrades as deliberate work with a migration review. **Pin `0.46.0`**, which is the current release as of 2026-11-06 (this plan previously said `0.44.0`). Re-check before the pin lands in `Cargo.toml`; if it has moved, treat the bump as a deliberate step with a changelog review rather than a caret range.
2. **`rig` splits into `rig-core` and `rig-agent`.** The root `rig` facade re-exports both. Depend only on `rig` unless there is a specific reason to reach lower.
3. **DeepSeek first (Q-25).** DeepSeek exposes an OpenAI-compatible API at
   `https://api.deepseek.com` (`deepseek-chat`, `deepseek-reasoner`). Use `rig`'s
   OpenAI-compatible client with that base URL if the pinned version has no native
   DeepSeek module, and verify that path in Week 1. Prefer **`deepseek-chat`** for
   the structured evaluation; reasoning models may not expose tool or JSON output.
   **Structured output is the gate:** confirm tool/JSON-schema support before
   building the evaluation pipeline. If only
   `response_format: {"type":"json_object"}` is available, use it and validate
   the parsed object against the schema with one retry; do not parse free prose.
   The provider abstraction still supports a second provider (OpenAI or
   Anthropic) after DeepSeek is verified.
4. **API keys never touch the database.** They live in the OS keychain. The `settings` table stores only the *provider choice* and *model name*, never the key itself.
5. **Provider differences are real.** Not every provider supports structured output identically. The capability registry must be authoritative, and the UI must warn when a selected model lacks a required capability.
6. **Streaming requires a runtime bridge.** `rig`'s streaming traits return `Stream` types. egui renders on the UI thread while Portals work runs on a `tokio` runtime. Use the app's existing `async_bridge` pattern (Phase 2.1): poll the task from the UI, never block it.
7. **`rig` supports OpenTelemetry GenAI semantic conventions.** Wire this up early if observability is desired; retrofitting it later means touching every call site.
8. **DeepSeek is a PRC-hosted provider.** The Settings disclosure must say that the CV summary and job text leave the machine and are sent to DeepSeek. Do not log prompts or responses at info level.
9. **Logging already has a policy.** `core::logging` is the one `tracing` seam
   and `docs/logging.md` is its never-log list: prompts, completions, CV text,
   and key material are on it. Phase 3 adds one `info!` line per call — provider,
   model, `input_chars`, `elapsed_ms`, outcome, and the classified error on
   failure — and nothing that identifies the posting's text. The Logs screen
   then shows an evaluation as one line per call rather than a wall of prompt.

### Acceptance Criteria

| Criterion | Verification |
| :--- | :--- |
| Client construction succeeds for DeepSeek | Integration test (environment-gated) |
| DeepSeek returns a schema-valid `EvaluationOutput` (tool or JSON mode) | Integration test (environment-gated) — asserted with WS3's pipeline, once completions exist |
| API key round-trips through OS keychain | Integration test |
| Missing API key produces `MissingApiKey`, not a panic | Unit test |
| Invalid API key produces a typed provider error | Integration test |
| Model capability registry is queryable and accurate | Unit test against known capabilities |
| Retry policy handles 429 and 503 | Unit test with mock transport |
| Settings saves the provider/model and a key, and the key is never rendered after save | UI test |
| Test connection reports success and a typed failure | Integration test (environment-gated) |
| An unconfigured provider disables the Pipeline actions with a reason | `egui_kittest` — WS1 ships `llm.status()` and the Settings line; WS6 replaces the placeholder buttons and reads it |
| No API key appears in logs or error messages | Log audit test |

---

## Workstream 2 — Prompt Seeding and Repository

**Priority:** After provider integration. Needed before evaluation.
**Estimated duration:** 2–3 days

### Deliverables

1. **Compact Phase-3 prompts (Q-24).** The original files cannot be sent as
   single-shot prompts: `_shared.md` is 26.8 KB, `oferta.md` 93.4 KB, and
   `cover.md` 16.1 KB, and they instruct an agent to read files, drive
   Playwright, and write reports. Author two compact prompts instead:
   - `crates/core/prompts/evaluate.md` (~1–2k tokens): the scoring rubric, the
     red-flag rules, the untrusted-data rule, and the exact
     `EvaluationOutput` contract.
   - `crates/core/prompts/cover_letter.md` (~1k tokens): the letter rules,
     no-fabrication rule, tone variable, and length guidance.

   Derive the rubric and red flags from `oferta.md`; do not invent a new scoring
   philosophy. Keep the originals in the repo as reference, and keep them seeded
   (they are already there) but **do not use them for evaluation or cover
   letters**. The compact pair is seeded under the fixed names `evaluate` and
   `cover_letter` with their matching categories.

   `_shared.md` is not sent wholesale either. Each compact prompt embeds the
   short rules it needs (untrusted input, never fabricate claims, use only the
   supplied CV).

2. Seeding (already implemented): `prompts_seed::seed_or_update_system_prompts`
   runs on every unlock, is idempotent, and versions changed seed content.
   Extend `SYSTEM_PROMPTS` with the two compact prompts and keep the existing
   rows for reference.

3. Prompt versioning scoped by `is_system`:
   - `create_version` currently computes the next version from `active(name)`
     across both system and user rows. A user duplicate can therefore collide
     with a later system seed (both become version N+1). Version within
     `(name, is_system)` instead.
   - Add `active_for_use(name)`: the latest user version if one exists, else the
     latest system version. Evaluation and cover-letter assembly call this, not
     `active`.
   - Add the unique index on `(name, is_system, version)` to migration 0006
     (shared with WS3) once a test proves existing data cannot violate it.

4. Minimal variable substitution supporting at least:
   - `{{cv_summary}}` — rendered deterministically from `profile` and
     `cv_entries`; an empty profile is an actionable "add your CV" error, not an
     empty candidate section (Q-29)
   - `{{job_description}}` — `plain_text(raw_text).text`, never the markup
   - `{{company_name}}`, `{{job_title}}` — from `job_descriptions`
   - `{{tone}}` — cover letters only; empty when unset
   - Strict mode: unknown variables fail rendering rather than producing empty
     strings

5. Prompt repository methods: `active_for_use(prompt_name)`,
   `list_versions(name, is_system)`, `list_categories()`, `get_by_hash(hash)`,
   and the scoped `create_version(...)`. The existing `active_system` stays for
   seeding.

### Critical Implementation Notes

1. **System prompts are read-only in the UI but versioned in the database.** When the application ships an updated system prompt in a future release, it inserts a new version rather than overwriting. Historical reports referencing the old version remain reproducible.
2. **The prompt inventory is already done.** `prompts_seed` ships 14 system prompts from `crates/core/prompts/` (`_shared`, `oferta`, `scan`, `cover`, `pdf`, and the utility set) and seeds them on every unlock. The remaining Phase 3 work is the two compact prompts, not the seeding machinery.
3. **Size is a correctness constraint, not a style preference.** `_shared.md` + `oferta.md` is roughly 120 KB (~30k tokens) before the CV and job text. That exceeds many models' context windows and costs far more per call than the task needs. The compact prompts keep the evaluation input in the low thousands of tokens; a test asserts a ceiling on each compact prompt's size.
4. **Versioning must be scoped by `is_system`.** `active(name)` and `create_version` currently ignore the flag, so a user copy and a later shipped system update can both claim version N+1. Scope both to `(name, is_system)` and resolve the prompt for use with `active_for_use` (user version first, system fallback).
5. **Variable substitution is strict by default.** An unknown `{{variable}}` is a rendering error, not a silent empty string. This prevents prompts from degrading silently when a field is renamed.
6. **Content hashing enables reproducibility without storing full prompt text on every report.** The report stores the hash; the prompt text is retrieved from the `prompts` table by hash. If the prompt row is ever deleted, the report's provenance is broken — so prompt rows are never hard-deleted, only superseded.
7. **The full templating language is deferred.** Phase 3 supports flat variable substitution. Conditionals, loops, and partials are a future enhancement. Do not build a templating engine in Phase 3.

### Acceptance Criteria

| Criterion | Verification |
| :--- | :--- |
| Seeding inserts the compact prompts and is idempotent | Integration test (existing suite extended) |
| System prompts are marked `is_system = true` | Unit test |
| Editing a user prompt creates a new version, not an overwrite | Unit test |
| A user copy and a later system seed cannot share `(name, is_system, version)` | Unit test |
| `active_for_use` prefers the user version, then the system version | Unit test |
| Each compact prompt stays under its token/char ceiling | Unit test on the embedded text |
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
    /// The Pipeline row the user is evaluating. The core ensures an application
    /// row (a job may have none) and returns it in the result.
    pub job_description_id: i64,
    pub prompt_name: String,       // defaults to "evaluate"
    pub model_override: Option<String>,
    pub stream: bool,
}

pub struct EvaluationResult {
    pub report_id: i64,
    pub application_id: i64,
    pub match_score: f32,          // 0.0..=1.0
    pub recommendation: Recommendation,
    pub content: String,           // full markdown report
    pub prompt_version: i64,
    pub prompt_hash: String,
    pub model_used: String,
    /// Characters of job text actually sent, after normalization and truncation.
    pub input_chars: usize,
    /// The stored raw size, for the "thin input" diagnosis.
    pub source_chars: usize,
    pub truncated: bool,
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

3. Text normalization, in `core::text`:

```rust
/// Convert stored job markup to prompt text. Deterministic and idempotent:
/// plain text passes through unchanged, and calling it twice equals calling
/// it once.
pub fn plain_text(raw: &str, max_chars: usize) -> PlainText;

pub struct PlainText {
    pub text: String,
    pub truncated: bool,
    /// `raw` length, so a report can compare markup bytes with prompt chars.
    pub source_chars: usize,
}
```

   Rules: drop `script`/`style`/`noscript`/`head`; turn block boundaries
   (`p`, `br`, `div`, `li`, `tr`, `h1`–`h6`) into line breaks and `li` into
   `"- "`; decode entities (`&amp;`, `&#39;`, `&nbsp;`); collapse runs of spaces
   and three or more blank lines; truncate on a character boundary. Parse with
   the existing `scraper`/html5ever dependency rather than a regex, so malformed
   provider markup does not produce tag soup in the prompt.

   **Already implemented (Phase 2.5 WS5).** `core::text::plain_text` exists in
   `crates/core/src/text.rs` with this signature and its own unit tests over a
   real Waymo JSON-LD description. Phase 3 calls it; it must not add a second
   normalizer. The acceptance criteria below are the regression suite for it.

4. Prompt assembly: the compact `evaluate` prompt resolved through
   `active_for_use` (user copy first, system fallback), strict variable
   substitution with the CV summary, and the **normalized** job description.
   `{{job_description}}` receives `plain_text(raw_text).text`; no raw markup
   reaches a prompt.
5. Two execution modes:
   - **Structured mode** — uses `rig`'s `Extractor` or `TypedPrompt` to get typed output. Preferred for the match score and recommendation.
   - **Streaming mode** — streams the full markdown report for UI display. Preferred for the narrative.
6. Report storage in `evaluation_reports` with `prompt_version`, `prompt_hash`,
   `model_used`, `input_chars`, `source_chars`, `truncated`, `match_score`,
   `recommendation`, and `generated_at`. The table has the provenance columns
   but not the structured fields or `input_chars`; migration `0006` adds them
   (and `input_chars` to `cover_letters`) and updates both models. The full
   markdown stays in `content`; the structured fields make history and sorting
   possible without re-parsing it.
7. Match score storage on `applications` via the existing `set_match_score`, and
   the latest report linked with `attach_report`.
8. Application resolution: `applications::ensure` runs before the report is
   written, because a Pipeline row may have no application yet. The report is
   linked to the application, not the job (Workstream 6, note 5).
9. A bounded batch helper for a handful of selected applications (sequential,
   hard limit 10) is allowed for tests and future UI; there is no batch UI and
   no hundreds-of-jobs run in Phase 3.
10. Auto-mark `Not a match` when the report recommends `NoMatch` (Q-20), using
    `applications::mark_not_a_match`. For an application that is applied,
    interviewing, offered, or rejected the writer **refuses** the transition with
    an error, so the caller treats that error as "leave the status alone" and
    still stores the report. The status stays visible and editable.
11. **Combined fetch-then-evaluate.** `EvaluationRequest` gains
    `fetch_description_if_thin: bool` (default `false`). When `true` and the
    job's normalized text is below `llm.thin_description_chars` (default 400),
    the pipeline runs `manual::refresh_job` first, and only then assembles the
    prompt. The report records whether the fetch happened. With Phase 2.8's
    detail opt-in, most pipeline rows are thin on arrival, so the UI uses this
    flag by default when the user clicks **Evaluate** on a row without text.

    This is the single-row path; do not add a second fetch loop. A user who has
    many thin rows already has Phase 2.9's **Fetch descriptions (N)** batch
    (`fetch_descriptions_batch`), which fetches them and advances the queue.
12. **Application lifecycle (Q-43).** The evaluation is a visible step in the
    application's status. Before the model call, a `discovered` or `ready`
    application moves to `evaluating`; after the report is stored, `evaluating`
    returns to `ready`. All three transitions already exist in
    `applications::is_valid_transition`. Every other status is left alone (a
    report on an `applied` row must not move it backwards), a failed or cancelled
    call returns the row to the status it had, and a `NoMatch` report goes
    straight to `mark_not_a_match` instead of passing through `ready`.

### Critical Implementation Notes

1. **Structured output and streaming are complementary, not alternatives.** The match score and recommendation should come from structured output for reliability. The narrative report should stream for UX. Run them as two calls, or use a single structured call and stream a separate narrative call. Do not try to parse a streamed response into a schema.
2. **`rig`'s `Extractor` uses a private "submit" tool.** The model calls the submit tool with data matching the schema. This is more reliable than asking the model to return JSON in prose. Use it for `EvaluationOutput`.
3. **Structured output support varies by provider.** The capability registry (Workstream 1) must be consulted. If the selected model does not support structured output, fall back to text parsing with a clear warning, or refuse with an actionable error.
4. **Token budget is configured, not guessed at call time.** Use `llm.max_job_chars` (default 12,000) and a CV budget (default 6,000) as a character proxy for tokens (~4 chars/token). `plain_text(raw, max_job_chars)` does the job-text cut; the CV summary is rendered within its own budget. Record `truncated` and `input_chars` on the report. Prefer summarization over truncation in a later iteration; v1 must not silently truncate without the flag.
5. **`prompt_version` and `prompt_hash` are both stored.** The version is a human-readable integer; the hash is the authoritative content identifier. If a prompt is edited without incrementing the version (a bug), the hash still detects the change.
6. **`model_used` is stored verbatim.** Provider model names change over time. Storing the exact string used at generation time is necessary for reproducibility audits.
7. **Errors are classified, not swallowed.** A failed evaluation must record why it failed. Do not store a partial report as if it succeeded.
8. **`raw_text` is markup for every provider, and normalization happens before
   the call.** Do not ask the model to ignore tags, and do not strip at
   acquisition: the row is the record of what the site served, Phase 4 may want
   it, and a re-fetch is not guaranteed. One pure function with fixtures is
   testable offline and gives the same input to every provider; a prompt
   instruction gives neither. The API below is the whole contract:
   `plain_text(raw, max_chars)` is idempotent and leaves plain text unchanged,
   because manual pastes and some ATS payloads are already plain.
9. **Record `input_chars` after normalization.** A report that says the model saw
   ~400 characters is telling you the row only had a card teaser (Phase 2 enriches
   at most `detail.max_jobs`, 40 by default; Waymo uses 90 and Ubisoft 25), not
   that the model was weak. Store the post-normalization count, and keep
   `source_chars` in the same record if the markup size is useful for diagnosis.
10. **Truncation is a budget decision, not a detail.** When truncation happens,
    cut on a character boundary and set `truncated` so the report can say so.
    Never cut mid-entity: decode first, then truncate.
11. **Migration 0006 changes the report shape.** It adds `match_score`,
    `recommendation`, `input_chars`, `source_chars`, and `truncated` to
    `evaluation_reports`, adds `input_chars` to `cover_letters`, and a unique
    index on `prompts(name, is_system, version)` after a duplicate check. Update
    both models at the same time; the report list must not infer these from
    `content`.
12. **Auto-marking `Not a match` is a side effect with a guard.** Use the
    Phase 2.5 writer (`applications::mark_not_a_match`), which refuses submitted
    applications. After it succeeds, the Pipeline row's badge and `has_user_state`
    must be updated in place, exactly as the manual action does. The status stays
    editable, and the report is still stored when the transition is refused.
13. **Phase 2.9's queue state is written by evaluation, in the report's
    transaction.** `job_descriptions::set_queue_states` opens and commits its own
    transaction, so calling it after the report insert would leave a window where
    a report exists on a row still marked `evaluate`. The evaluation service
    therefore opens one `Transaction` and writes both: the report insert and the
    queue update (`queue_state = 'evaluated'`) through a transaction-taking
    helper. 2.9 landed first, so the column exists and there is no presence
    check, and the row is set to `evaluated` even if the user changed the queue
    in the meantime — the report is the fact. The loaded Pipeline row follows
    with `PipelineViewModel::set_queue(id, QueueState::Evaluated)`, so the badge
    changes without a reload.

### Acceptance Criteria

| Criterion | Verification |
| :--- | :--- |
| Evaluation produces a valid `EvaluationOutput` for a fixture job + CV | Integration test (environment-gated) |
| Match score is in `0.0..=1.0` | Unit test |
| Recommendation is a valid enum variant | Unit test |
| Report is stored with correct `prompt_version`, `prompt_hash`, `model_used` | Integration test |
| The report stores `match_score`, `recommendation`, `input_chars`, `source_chars`, and `truncated` | Integration test |
| Migration 0006 applies to a 0005 database and preserves existing rows | Migration test |
| Evaluating a Pipeline job with no application ensures one | Integration test |
| A `NoMatch` report marks the application `not_a_match`, but not a submitted one | Integration test |
| Match score is written to `applications.match_score` | Integration test |
| Streaming mode produces incremental text | Integration test |
| Missing API key fails with `MissingApiKey`, not a partial report | Unit test |
| Context length exceeded is detected before the call | Unit test with mock token counter |
| Batch evaluation respects concurrency limit | Unit test |
| Prompt resolution uses the user's copy when present, else the system prompt | Unit test |
| Job text is normalized before prompt assembly | Unit test: `plain_text` over real provider fixtures; integration test asserts no markup in the assembled prompt |
| Normalization is idempotent and leaves plain text unchanged | Unit test: `plain_text(plain_text(x)) == plain_text(x)`; manual-paste fixture |
| Entities and block boundaries survive normalization | Unit test: `&amp;`, `&#39;`, `&nbsp;`, `<li>`, `<p>`, `<br>` |
| Truncation never splits a character or an entity | Unit test at the boundary |
| The report records `input_chars` after normalization | Integration test |
| An evaluation moves a `discovered`/`ready` application to `evaluating`, then to `ready` (Q-43) | Integration test |
| A failed or cancelled evaluation restores the application's previous status | Integration test |
| A report on an `applied` application leaves its status alone | Integration test |

---

## Workstream 4 — Cover Letter Generation

**Priority:** After evaluation pipeline works. Reuses the same primitives.
**Estimated duration:** 3–5 days

### Deliverables

1. `core::cover_letter` module:

```rust
pub struct CoverLetterRequest {
    /// The Pipeline row; the core ensures an application (same as evaluation).
    pub job_description_id: i64,
    pub prompt_name: String,       // defaults to "cover_letter"
    pub model_override: Option<String>,
    pub tone: Option<String>,      // optional user-selected tone
}

pub struct CoverLetterResult {
    pub cover_letter_id: i64,
    pub application_id: i64,
    pub content: String,           // markdown
    pub prompt_version: i64,
    pub prompt_hash: String,
    pub model_used: String,
    pub input_chars: usize,
}
```

2. Prompt assembly: the compact `cover_letter` prompt resolved through
   `active_for_use`, variable substitution with the CV summary, the **normalized**
   job description, and the optional tone. It reuses the `{{job_description}}`
   substitution from Workstream 3, so `plain_text` runs here too; a cover letter
   must not see tags either.
3. Storage in the `cover_letters` table, including `input_chars` (migration 0006) alongside the existing provenance columns.
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
| The job description in the prompt is normalized, the same way as evaluation | Unit test: no markup in the assembled prompt |
| Missing API key fails cleanly | Unit test |

---

## Workstream 5 — Prompt Manager UI

**Priority:** After prompt repository is seeded and queryable. Can overlap with evaluation pipeline.
**Estimated duration:** 1 week

### Deliverables

1. **Prompt list screen** showing:
   - System prompts (read-only, visually distinguished), including the compact
     `evaluate` and `cover_letter` prompts the pipeline uses, plus the seeded
     reference set (`_shared`, `oferta`, `scan`, `cover`, `pdf`, utility)
   - User prompts (editable)
   - Category grouping (`shared`, `evaluate`, `scan`, `render`, `cover_letter`,
     `utility`)
   - Version count per prompt and last modified timestamp
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

1. **System prompts cannot be edited.** The only way to modify one is to duplicate it as a user prompt with the same name and `is_system = 0`, then edit the copy. `active_for_use` prefers the user copy; the system row stays visible for reference. This preserves the ability to ship prompt updates in future releases without conflicting with user changes.
2. **Versioning is scoped by `(name, is_system)`.** A user copy starts at its own version 1, so a later shipped system update cannot collide with it or silently become the active prompt.
3. **Version history is immutable.** Old versions are never deleted or modified. Reverting creates a new version with the old content, so the history is a complete record.
4. **Variable validation runs on save, not on use.** A prompt with an unknown variable is rejected at save time with a clear error naming the variable.
5. **Prompt usage is traceable.** From a prompt version, the user can see which evaluation reports and cover letters reference it (by hash).

### Acceptance Criteria

| Criterion | Verification |
| :--- | :--- |
| System prompts are read-only in the UI | UI test |
| Duplicating a system prompt creates an editable user copy with the same name and `is_system = 0` | UI integration test |
| A later system seed does not become the active prompt while a user copy exists | Unit test |
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

1. **Enable the existing disabled buttons.** The Pipeline detail renders
   **Evaluate** and **Generate cover letter** with an "Available in a later
   phase" tooltip, below a **Queue** row that Phase 2.9 added. Replace the two
   disabled actions with the real ones and leave the queue row where it is: the
   queue is what the user wants done, the buttons are doing it. When no provider
   or API key is configured, the enabled-state check fails with a clear message
   and a link to Settings, not a silent no-op.
1a. **Thin-row handling.** When the selected job's normalized text is under the
    thin threshold, the Evaluate button reads **Fetch description and evaluate**
    and passes `fetch_description_if_thin: true`. When the text is sufficient,
    it reads **Evaluate**. The fetch is `manual::refresh_job` — the same path as
    the row's **Refresh description** — so there is no second extractor. A
    failure to fetch keeps the row unevaluated with a message; the user can still
    evaluate the teaser by clicking again with the "Evaluate without description"
    link. A user with many thin rows should use the header's **Fetch
    descriptions (N)** batch instead of clicking each one.
2. **Streaming progress display**: the report text appears incrementally in the
   detail view while the model generates it.
3. **Report viewer**: the detail gains sections/tabs for **Description**,
   **Evaluation**, and **Cover letter**. The existing plain-text/raw toggle stays
   on the description tab. The evaluation tab shows the match score,
   recommendation, strengths, gaps, and narrative.
4. **Match score badge** in the Pipeline row, beside the queue badge. The row
   already carries a tier label, a status badge, a queue badge, and the
   **Preferred**/**Excluded** markers, so the score is one compact number (for
   example `0.82`) with the recommendation in its hover text rather than a
   second badge. The list is a `ScrollArea`, not a table, so Phase 3 adds the
   badge and an explicit sort control (score / fetched) rather than a sortable
   column header.
5. **Re-evaluate action** with a prompt/model override selector; each run stores
   a new report and updates the latest-report pointer on the application.
6. **Evaluation history**: all reports for an application, with prompt version,
   model used, score, and recommendation, read from the new report columns.
7. **Cover letter tab**: the latest generation with a **Regenerate** action. A new
   generation creates a row; earlier ones remain selectable.
8. **Auto `Not a match`**: a `NoMatch` report marks the application through the
   existing writer and updates the row badge and `has_user_state` in place, the
   same way the manual action does.
9. **Queue state after a report.** Writing a report sets `queue_state =
   'evaluated'` in the same transaction (WS3, note 13) and updates the loaded
   row with `PipelineViewModel::set_queue`. The detail's queue badge then reads
   **Evaluated** and the three manual queue actions still work: **Mark for
   evaluation** is a pre-evaluation hint, not a substitute for **Evaluate**.
   Re-evaluating leaves the row `evaluated`.

The existing delete gate already names evaluations as lost state, and deleting a
job cascades its reports and letters. The evaluation UI therefore does not need a
second deletion warning; it must keep the detail consistent after a delete.

#### Prompt Injection Defense

Job descriptions are untrusted input. A malicious job posting could contain instructions designed to manipulate the evaluation. The defense has three layers:

1. **Structural separation.** The job description is passed as data in a clearly delimited context block, not concatenated into the instruction portion of the prompt. `rig`'s `context()` method supports this; use it rather than string concatenation.
2. **Instruction hierarchy.** The system prompt explicitly states that content within the job description block is untrusted data, not instructions. This is imperfect but raises the bar.
3. **Output validation.** The structured output schema constrains what the model can produce. A match score outside `0.0..=1.0` is rejected. A recommendation outside the enum is rejected. The narrative is displayed but never executed or interpreted as instructions.
4. **No tool access from evaluation.** The evaluation agent has no tools other than the submit tool. It cannot browse, fetch, or take actions. This limits the blast radius of a successful injection.

### Critical Implementation Notes

1. **Streaming UI must be cancellable.** A long evaluation should be interruptible by the user. Use `rig`'s `PauseControl` or an equivalent cancellation mechanism.
2. **Streaming and egui integration is the same problem as the browser backends.** Use the `async_bridge` polling pattern. Add a UI responsiveness test.
3. **Injection defense is not a solved problem.** No defense is complete. The goal is to make the common cases fail safely, not to guarantee immunity. Document this limitation.
4. **The structured output validation is the strongest defense.** If the model produces a valid score and recommendation, the injection did not succeed in changing the structured result, even if it changed the narrative. Prioritize structured output reliability.
5. **Evaluation is per-application, not per-job.** Today `applications::ensure` keeps one application per job, so the Pipeline's row maps to one. The report is linked to the application anyway, because a future multi-CV feature can add a second application for the same posting without changing the report schema.

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
3. **Cassette-backed tests are the default.** Live provider calls are expensive and slow. Record cassettes for fixture interactions and replay them in the default suite. Live tests follow the repository's existing convention: `#[ignore = "hits the network"]` plus an environment variable for anything beyond the cheapest call, and they are indexed in `docs/LIVE_TESTS.md` with the other live tests. They never run in the offline suite (`-- --skip obscura` remains the standard command; `#[ignore]` excludes these too).
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
| Live tests are `#[ignore]`d and environment-gated, indexed in `docs/LIVE_TESTS.md` | Code review + docs |

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
| CV fixture (profile + cv_entries) | Hand-crafted from the seed profile | Evaluation pipeline, prompt rendering |
| Real job descriptions captured in Phase 2 | Phase 2 fixtures and live runs | Evaluation pipeline (integration) |
| Provider markup: Eightfold `jobDescription`, JSON-LD `description`, SmartRecruiters sections, and a plain-text manual paste | Phase 2.3 fixtures + hand-crafted | `plain_text` unit tests (idempotence, entities, block boundaries, truncation) |

Phase 3 uses its own fixtures. It does not depend on any Phase 0 artifact.

### Cassette-Backed Testing

All LLM calls in tests use recorded cassettes by default. Live calls are
environment-gated and run on demand. Cassettes live under the crate that owns
the call (`crates/core/tests/cassettes/` for the pipeline, `crates/app/tests/` for
UI flows if needed) and are treated as fixtures — changes require review.

Add an entry to `docs/LIVE_TESTS.md` for the provider smoke test and the cassette
recording command, alongside the existing acquisition live tests. Do not add a
second gating convention.

### Integration Tests

End-to-end flows:

1. Seed prompts → select provider → configure API key → evaluate a fixture application → verify report stored with provenance.
2. Generate a cover letter → verify stored with provenance → regenerate → verify new row.
3. Edit a user prompt → evaluate → verify new report references the new prompt version.
4. Attempt to edit a system prompt → verify rejection.
5. Submit a malicious job description → verify injection does not produce an out-of-range score or invalid recommendation.
6. Evaluate with a model lacking structured output support → verify fallback or clear error.
7. Evaluate an application whose `raw_text` is real provider markup → assert the assembled prompt contains no tags and records `input_chars`.
8. Evaluate a Pipeline job with no application → an application is ensured, the report is linked, and the row's badge/score update in place.
9. Delete the job after evaluating → the report cascades (existing Phase 2.5 behavior) and the detail falls back cleanly.

### CI Jobs

| Job | Purpose |
| :--- | :--- |
| `llm-cassette` | All LLM tests against recorded cassettes (always runs) |
| `llm-live` | Provider smoke test against real providers (environment-gated, on demand; indexed in `docs/LIVE_TESTS.md`) |
| `llm-evals` | `rig::evals` prompt quality tests (cassette-backed, always runs) |
| `injection-defense` | Malicious input tests (always runs) |
| `ui-evaluation` | Evaluation UI tests with the `egui_kittest` harness |

---

## Phase 3 Risks

| ID | Risk | Impact | Mitigation |
| :--- | :--- | :--- | :--- |
| P3-R1 | **`rig` pre-1.0 breaking changes.** The crate explicitly warns of breaking changes. | High | Pin `0.46.0` (checked 2026-11-06) with no caret range. Treat upgrades as deliberate work with migration review. Isolate `rig` behind `core::llm` so churn is absorbed in one place. |
| P3-R2 | **Provider API differences.** Structured output, streaming, and tool support vary by provider. | Medium | Capability registry per provider/model. UI warns when a model lacks a required capability. Fallback or refusal is explicit. |
| P3-R3 | **Prompt injection from malicious job descriptions.** | High | Structural separation, instruction hierarchy, structured output validation, no tool access. Documented as imperfect. See Workstream 6. |
| P3-R4 | **Streaming + egui integration causes UI stalls.** Same problem as the browser backends in Phase 2 and 4. | Medium | Use the `async_bridge` polling pattern (Phase 2.1). Add UI responsiveness test. Cancellation support. |
| P3-R5 | **Token costs and rate limits.** A large CV plus a long job description can be expensive. | Medium | A configured character budget before calls (WS3, note 4) and a visible `truncated` flag. Batch evaluation with concurrency limits. Display estimated cost where possible. |
| P3-R6 | **Prompt quality is hard to test.** LLM output is non-deterministic. | Medium | Cassette-backed tests with curated fixtures. Run each evaluation multiple times. Assert on distributions, not single outcomes. |
| P3-R7 | **Model version changes invalidate reproducibility.** A provider updates a model and outputs change even with identical input. | Medium | Store `model_used` verbatim. Document that reproducibility is best-effort, not guaranteed. Cassettes are re-recorded on model changes. |
| P3-R8 | **Prompt versioning schema drift.** If the versioning scheme changes, historical reports lose provenance. | Medium | Content hash is the authoritative identifier. Prompt rows are never hard-deleted. Schema changes require a migration that preserves hashes. |
| P3-R9 | **Scope creep into prompt tooling.** Building a full templating language or A/B testing framework. | Medium | Phase 3 supports flat variable substitution only. Conditionals, loops, and partials are explicitly deferred. Resist. |
| P3-R10 | **Markup reaches the model as prose.** A prompt built directly from `raw_text` is mostly tags and attributes; the model sees less content per token and behavior varies by provider. | Medium | `core::text::plain_text` is a pipeline step with fixtures and idempotence tests, applied to every provider's text before assembly. `input_chars` on the report makes a thin input visible. |
| P3-R11 | **The original prompts are sent verbatim.** They total ~120 KB, assume file and tool access, and ask for a 7-block report the schema cannot express. | High | Ship compact prompts derived from them (WS2, Q-24), with a size ceiling test. Keep the originals as reference, not as call input. |
| P3-R12 | **Prompt versions collide.** A user copy and a later system seed can both claim `(name, version)`. | Medium | Scope versioning by `is_system`, resolve with `active_for_use`, and add a unique index in migration 0006 after a duplicate check. |
| P3-R13 | **Auto `Not a match` overrides the user.** A model opinion changes a status the user set, or a submitted application. | Low | Use `applications::mark_not_a_match`, which refuses submitted states; the status remains visible and editable, and the report is stored even when the transition is refused. |
| P3-R14 | **DeepSeek's structured-output path is uncertain.** Tool-calling and strict JSON schemas may differ from OpenAI's. | High | Verify in Week 1 before building on it. Prefer tool/JSON-schema output; fall back to `response_format: json_object` plus schema validation and one retry. Never parse free prose. |
| P3-R15 | **Single-provider dependency and jurisdiction.** DeepSeek is the only provider at launch and is PRC-hosted. | Medium | Keep `core::llm` provider-agnostic and add a second provider behind the same boundary. Disclose the jurisdiction in Settings. Do not log prompts. |
| P3-R16 | **`rig` raises the toolchain and duplicates a dependency.** 0.46.0 requires rustc **1.97.1** (the workspace MSRV moved from 1.83), and `rig-reqwest` pulls `reqwest` **0.13** beside acquisition's 0.12. | Medium | MSRV is declared as 1.97.1 rather than left stale. The two `reqwest` versions coexist until acquisition can move; revisit at the next `rig` bump, and treat the pairing as a size cost, not a correctness one. |

---

## Weekly Budget

| Week | Focus | Deliverable |
| :--- | :--- | :--- |
| **1** | WS1 DeepSeek integration, key management, capability registry; WS2 compact prompts, scoped versioning, variable rendering | DeepSeek client returns a schema-valid result; the compact prompts seed and a report renders with no markup |
| **2** | WS3 evaluation pipeline: structured output, streaming, storage, provenance, migration 0006 | Evaluation produces and stores a report with correct provenance and normalized input |
| **3** | WS4 cover letter generation; WS5 Prompt Manager UI | Cover letters generate and store; Prompt Manager browses, edits, versions |
| **4** | WS6 Evaluation UI, streaming, injection defense; WS7 LLM evaluation tests | End-to-end: evaluate → stream → view report → regenerate; injection tests pass |

**Total:** 3–4 weeks. Phase 2 already supplies prompt seeding, versioning, and
`plain_text`, so the lower end is more likely than it was in v0.3. The upper end
accounts for `rig` quirks, structured-output differences, and injection-defense
iteration.

---

## Exit Criteria

Phase 3 is complete when **all** of the following are true:

1. DeepSeek is integrated and selectable in Settings, and returns a schema-valid structured result. The provider abstraction is ready for a second provider; adding one may slip without blocking the phase.
2. API keys are stored in and retrieved from a dedicated OS-keychain namespace, never the database or `settings`.
3. The compact `evaluate` and `cover_letter` prompts are seeded on unlock, are read-only, and stay under their size ceilings; the original prompt files remain reference material.
4. User prompts can be created, edited, versioned, and reverted; versioning is scoped by `is_system` and `active_for_use` prefers the user's copy.
5. Every evaluation report records `prompt_version`, `prompt_hash`, `model_used`, `input_chars`, `source_chars`, `truncated`, `match_score`, and `recommendation`.
6. Evaluation produces a structured `EvaluationOutput` with a valid match score and recommendation, and ensures the application row for a Pipeline job.
7. Job text is normalized deterministically before prompting: no markup reaches
   the model, `plain_text` is idempotent and leaves plain text unchanged, and
   each report records `input_chars`.
8. Evaluation reports stream to the UI incrementally and can be cancelled.
9. Cover letters generate as markdown and are stored with provenance.
10. Prompt injection defenses are implemented and tested against malicious fixtures.
11. A `NoMatch` report marks an eligible application `not_a_match`; a submitted application is left alone.
12. Migration 0006 applies to an existing 0005 database and preserves rows.
13. `rig::evals` tests run in the default suite against cassettes and pass.
14. CI is green on Linux, macOS, and Windows, including cassette, injection-defense, and UI jobs.
15. A short Phase 3 retrospective memo exists, recording `rig` integration friction, provider quirks, prompt-size decisions, and injection-defense effectiveness.
16. Writing a report sets `queue_state = 'evaluated'` in the same transaction, and the Pipeline row's badge updates without a reload (WS3, note 13; WS6, item 9).
17. An evaluation moves a `discovered`/`ready` application to `evaluating` and back to `ready`, restores the previous status on failure or cancellation, and never rewrites a submitted application's status (Q-43; WS3, deliverable 12).

---

## Open Questions (Phase 3 Owns)

The main plan (v0.30) tracks cross-phase questions in Appendix B, and Phases
2.5/2.6 record their own Q-18–Q-23 in their phase plans. Phase 3's questions
were resolved on 2026-11-05, and **Q-43 on 2026-11-06**.

| # | Question | Resolution |
| :--- | :--- | :--- |
| Q-5 | How should prompt edits interact with historical evaluation reports beyond storing `prompt_version`? | **Confirmed:** `prompt_version` + `prompt_hash`, never hard-deleting prompt rows, is sufficient. Comparing regenerated reports is post-v1. |
| Q-24 | Send the original `modes/*.md` prompts, or author compact embedded ones? | **Compact prompts derived from the originals**, with a size-ceiling test. The originals stay as reference. |
| Q-25 | Which providers ship in Phase 3? | **DeepSeek first**, through its OpenAI-compatible API. The abstraction is ready for a second provider; adding one may slip without blocking the core flows. |
| Q-26 | Where do report structured fields live? | **Columns in migration 0006**: `match_score`, `recommendation`, `input_chars`, `source_chars`, `truncated`; `applications.match_score` stays the latest score. |
| Q-27 | Is there a batch-evaluation UI? | **No for now.** A bounded headless helper may exist for tests; the UI evaluates one job at a time. Revisit later. |
| Q-28 | Should a `NoMatch` report change the application status automatically? | **Yes**, through `applications::mark_not_a_match`, which refuses submitted applications; the status stays editable. |
| Q-29 | Where does the candidate's CV/profile data come from? | **Phase 2.7** ships the CV/Profile editor and the canonical `cv_summary`; Phase 3 consumes it. |
| Q-42 | **Should Phase 3 auto-fetch descriptions for thin rows without asking?** | No. Fetching is a network call and a user cost; the Evaluate button makes the cost explicit ("Fetch description and evaluate") rather than hiding it. |
| Q-43 | **What happens to `applications.status` during an evaluation?** | **Resolved:** the evaluation moves the application through its lifecycle. `discovered` and `ready` enter `evaluating` before the call and return to `ready` on success; every other status is left alone, a failure or cancellation restores the previous status, and `NoMatch` goes straight to `mark_not_a_match`. All three transitions already exist in `applications::is_valid_transition`, so this needs no schema or repository change — just the two calls in WS3, deliverable 12. |

Q-29 is complete before WS1 starts. Q-24 and Q-25 are complete before WS2/WS1;
Q-26 and Q-5 before WS3 completes; Q-27, Q-28 and Q-43 before WS6 completes.

---

## What Phase 3 Is Not

Phase 3 makes the application intelligent. It does not render PDFs, and it does not acquire jobs. Specifically:

- There is no PDF rendering of cover letters or CVs. That is Phase 4.
- There is no browser backend work. That is Phase 2.
- There is no file-based data migration.
- There is no tool-using agent: the model does not browse, read the data root, or write reports. The app supplies the text and stores the structured result.
- There is no "send the original `modes/*.md` prompt" path; the compact prompts are the call input.
- There is no full prompt templating language (conditionals, loops, partials).
- There is no prompt A/B testing framework.
- There is no fine-tuning, no local model serving, no multi-modal input.
- There is no batch-evaluation UI and no run over hundreds of jobs.

The temptation to "just render the cover letter to PDF" before Phase 3 is complete is the most common way AI phases overrun. The `PdfBackend` trait exists; the implementation is Phase 4. Resist.

---

The single most important thing to take from this plan: **structured output is the backbone of the entire AI integration.** The match score, the recommendation, and the injection defense all depend on the model returning a schema-conformant object rather than prose. `rig`'s `Extractor` and `TypedPrompt` make this reliable when the provider supports tool or JSON-schema output. **Verify DeepSeek's path for this in Week 1**, before building anything on top of it. If DeepSeek offers only JSON mode, build the schema validation and retry then; if neither works, the evaluation pipeline needs a different design, and discovering that in Week 3 is expensive.