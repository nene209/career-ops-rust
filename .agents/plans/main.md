# Draft Execution Plan: Career-Ops Rust Desktop Application

**Status:** Draft — v0.39
**Last updated:** 2026-11-08

This document outlines the execution plan for porting the `career-ops` project from its current Node.js, file-based architecture to a Rust desktop application using egui, SQLite, and a primarily pure-Rust technology stack. The plan incorporates the stated technical decisions and evaluates the key architectural considerations.

### Change Log

| Version | Change |
| :--- | :--- |
| v0.1 | Initial draft. |
| v0.2 | Replaced single-browser scraping approach with tiered architecture (`reqwest` + `scraper` default, headless browser feature-gated). Removed `rust-headless-chrome` as a hard dependency. Added CDP-agnostic backend abstraction as a future extension. Added dependency-verification caveat. |
| v0.3 | Made PDF generation backend-agnostic via a `PdfBackend` trait, mirroring `BrowserBackend`. Selected `rust-headless-chrome` as the initial Tier 2 and PDF backend. Demoted `printpdf` from default PDF path to a medium-term alternative backend. Restructured §6. Lowered R-1 severity; raised the profile of the slimming path. |
| v0.4 | Encryption at rest is now mandatory with no unencrypted mode. Replaced direct keychain-stored DB key with envelope encryption (DEK wrapped by a keychain-stored KEK, plus a passphrase-derived recovery path). Added "never re-initialize on key loss" rule. Added `keyring` crate feature-flag hazard. Revised R-5; added R-10 and R-11. |
| v0.5 | Phase 0 skipped; decisions made directly. Tier 1 confirmed insufficient for target portals: browser is **required**, not feature-gated. Obscura chosen as initial `BrowserBackend` (scraping only). Obscura cannot render PDFs — no layout/paint engine — so `rust-headless-chrome` is the initial `PdfBackend`. Two-browser architecture documented; single-browser alternative flagged as an open question. `printpdf` confirmed as post-v1 (Track A). Bundle size expectations revised to ~190–250 MB. |
| v0.6 | UI framework changed from GPUI to **egui/eframe** after Phase 2.01. §3 rewritten against egui. §9 bundle table updated (egui ~5 MB base vs GPUI ~12 MB). §12 amended with Phase 2.01 and Phase 2.1. Decisions log records GPUI as evaluated and rejected, and egui as the chosen framework. Appendix B updated: Q-2 resolved, Q-7 added for the accessibility gap. Toolchain requirement lowered from 1.95.0 (GPUI's `cold_path`) to the workspace's actual MSRV. |
| v0.7 | Phase 2.2 inserted between the UI shell and AI integration. The Phase 2.1 verification run against a real selector-configured careers page showed that fetched postings are not yet evaluable: Tier 1b stops at page one, stores the card teaser instead of the description, and the SmartRecruiters adapter stores neither a per-posting URL nor any text. Phase 2.2 is the smallest work that closes that gap before Phase 3 builds evaluations on top of it. Phase 2.1 marked complete. |
| v0.8 | Phase 2.3 added after probing `https://jobs.nvidia.com/careers`, which is an Eightfold site: its postings come from `/api/pcsx/*` JSON endpoints, not from HTML. Neither extraction tier can fetch it, with or without a selector config. Phase 2.3 therefore introduces site providers (selectors, eightfold) and an embedded known-site registry that offers a config when the app knows the host, while keeping manual configuration as the escape hatch. |
| v0.9 | Phase 2.4 added after Phase 2.3 closed. The registry and prefill work, but the only way to reach them is to type a URL the app already knows, so the embedded profiles are effectively hidden. Phase 2.4 adds a chooser at the start of the add flow, a "based on" attribution with a modified marker, and a visible restore action. It is UI over the existing constants: no storage, no migration, no new provider. |
| v0.10 | Phase 2.5 added after the first real manual test session. Eight findings, but only some are what they first appeared: the duplicate NVIDIA rows are an identity/dedup bug (manual add bypasses the provider, and dedup skips its title/company fallback when an `external_id` exists), the empty and truncated descriptions are coverage and clobbering, and the HTML in the detail panel is a display bug, not a storage one. Phase 2.5 closes those before Phase 3 builds on the data. |
| v0.11 | Phase 2.6 added after Phase 2.5 closed. The registry exposes only the two hand-curated sites while four ATS boards are verified by live tests and invisible in the UI. Phase 2.6 models an ATS board as a site whose engine is selected by URL (an `Engine` enum, not a new provider), adds Figma/Greenhouse, Spotify/Lever, Linear/Ashby, and Ubisoft/SmartRecruiters as known sites, and adds the chooser filter that Phase 2.4 deferred. No storage change, no migration, no new dependency. |
| v0.12 | Phase 2.7 inserted between 2.6 and Phase 3 after the Phase 3 review found that the CV / Profile screen is still a placeholder. Phase 3's `{{cv_summary}}` needs identity, a summary, and CV entries; the repositories exist but there is no editor and no canonical summary renderer. Phase 2.7 adds both, with no schema change, so Phase 3 can stay AI-focused. Phase 3's plan moves to v0.5: DeepSeek is the first provider through its OpenAI-compatible API, the compact prompts are confirmed, report provenance is a migration, and auto `Not a match` uses the existing writer. |
| v0.13 | Phase 2.7 complete: the CV / Profile screen replaces the placeholder, and `repo::profile::render_cv_summary` / `cv_summary` is the single candidate-summary renderer. Phase 3's `{{cv_summary}}` dependency is now a shipped function, not an assumption; its plan moves to v0.6 with the exact call, the `None` semantics, and the tests named. |
| v0.14 | Phases 2.8, 2.9, and 3.2 added and the ordering made explicit: **2.8 → 2.9 → Phase 3 → 3.2**. Phase 3 and the queue phase had come to depend on each other (Phase 3's entry conditions assumed the queue existed; the queue plan listed Phase 3 as a dependency), and the queue plan's real inputs — `applications::ensure`, `Not a match`, the raw/plain toggle, `refresh_job` — are all Phase 2.5 deliverables. It therefore moves ahead of Phase 3 as **2.9**. Migrations renumber with it: 2.9 takes **0005**, Phase 3 takes **0006**, Phase 3.2 keeps **0007**. Phase 2.8 settles its naming (**Refresh** / **Continue** / **Fetch all pages**, with the header becoming **Reload list** / **Fetch all portals**), fixes Refresh rewinding the continuation cursor, and makes detail enrichment a per-portal opt-in. §3's screen table reflects the new Pipeline filters and Portals actions. |
| v0.15 | Phase 2.8 complete. The fetch row is **Refresh** / **Continue** / **Fetch all pages** with a cumulative progress line and Stop, the header reads **Reload list** / **Fetch all portals**, the cursor is monotonic per run kind, and `detail.enabled` gates enrichment (legacy blocks and Eightfold keep their behavior). Manual testing found three problems, all fixed: a chunk that ended at a refused page was misreported as "no progress" (now `Stopped`, naming the page); a page that kept refusing left no way past it (the walk now records the pages it read, and the screen offers **Skip page N and continue**); and the page that started all of it was a parse bug — NVIDIA sends `postedTs: 0.0`, one `Option<i64>` field failed the whole page, and the parser now reads that field defensively and skips a posting it cannot read instead of the page. A failed single fetch also shows its classified reason in the header. §4.3e and §4.4a–c are verified by the owner; §4.4d is test-covered, and stays in the script for a genuine refusal. |
| v0.16 | **Phase 2.8.1 (Logging and Diagnostics)** inserted between 2.8 and 2.9, so the order is **2.8 → 2.8.1 → 2.9 → Phase 3 → 3.2**. The phase adds nothing user-facing except the ability to see what happened: today `main.rs` logs to stderr with a level that only `RUST_LOG` sets, so a windowed build is silent, and the ~25 call sites are failure-only — which is why the Phase 2.8 stall took three round trips (F8, F9, F10) to diagnose. It installs one seam with two sinks (`<data_dir>/logs/`, rotated, plus a bounded in-process ring buffer), a **Logs** screen (chips, search, pause, copy, open folder, delete), an optional bottom drawer for watching a fetch live, per-page and per-chunk `info!` events, sidecar stderr capture, and a panic hook. Release logs at `info` with no environment variable; `RUST_LOG` still overrides. No migration, no new external crate, no change to fetch behavior. **Option C** (JSON logs, per-task spans, diagnostic bundle) is recorded as deferred in the phase plan. Phase 2.9's content is unchanged; only its ordering dependency on this phase is recorded. |
| v0.17 | Phase 2.8.1 complete. `career_ops_core::logging` owns one `tracing` seam with two sinks: a rotated file under `<data_dir>/logs/` (5 MB × 5 archives, flushed per write) and a 2,000-line in-process ring, both structured, plus a panic hook that writes through its own handle. The app renders the ring in a **Logs** screen (level chips, search over message/target/fields, pause with a captured-line counter, follow, copy, open folder, two-step delete) and in a bottom drawer toggled from the sidebar or `Cmd/Ctrl+L`, which spans the window because it is shown before the shell panels. Fetches now log one line per page (`page`, `bytes`, `jobs`, `added`), one per fetch call (`tier`, `pages`, `stop`, `elapsed_ms`), and one per walk chunk (`first_page`, `pages`, `deepest`, `stop`); the Eightfold parser names the posting index and the page size it lost, and the Obscura sidecar's stderr reaches the log instead of `Stdio::null()`. **View logs** sits beside a Portals error and a stalled walk. `docs/logging.md` records the sinks, the level policy, and the never-log list. No migration; `Cargo.lock` gained two edges on `career-ops-core` and lost the app's `tracing-subscriber` edge; no fetch behavior changed. Plan: `.agents/plans/phase_2.8.1_logging_and_diagnostics.md`; retrospective: `.agents/plans/phase_2.8.1_retrospective.md`. |
| v0.18 | Phase 2.8.1's manual pass (§9) found two bugs, both fixed. **Follow did not return to the tail:** `stick_to_bottom` only keeps a view that is already at the end, and the frame after the click still held the old offset, so the screen's own scroll check undid the click. The view now owes a per-mount jump, applied as a finite scroll offset (a first attempt with `f32::MAX` overflowed egui's scroll math into a NaN rect and panicked — the kittest suite caught it). **A failed fetch wrote nothing to the log:** every `?` in the router returns before its `fetch finished` line, so a configuration error the UI displayed had no record. `portals::service` now logs `fetch failed` with `portal`, `cause`, and `detail` for single fetches, batch entries, and walk chunks — the one place every path passes. Eleven of thirteen §9 steps pass; §9.4 and §9.10 are the two to re-run. |
| v0.19 | Phase 2.8.1's manual gate is closed: §9.4 (Follow returning to the tail) and §9.10 (a failed fetch leaving a log line) were re-verified after v0.18's fixes, so all thirteen steps of manual script §9 pass. The final suite then found one latent defect in the buffer itself: sequence numbers were assigned outside the buffer lock, so concurrent writers could store a line behind a newer one — and because the UI refreshes on `latest_seq`, that line could be missed. The counter and the drop count moved inside the mutex, which makes sequence order and storage order identical. |
| v0.20 | Phase 2.9 WS3 complete, so the queue exists end to end. Migration 0005 adds `queue_state` / `queue_reason` / `queued_at` to `job_descriptions` (default `new`, no backfill, indexed) and the Pipeline detail gains **Mark for details**, **Mark for evaluation**, and **Dismiss**, with a queue badge beside the status badge and a `Dismissed` filter — dismissing hides a row from every other view rather than deleting it. Unknown state values read as `new`, so a newer build's state cannot break an older one, and a queue write on a vanished row is an error the app answers with a reload. The migration test builds a real 0004 schema and applies 0005 by hand, because `create_and_migrate` can only produce the newest schema. WS4 (batch **Fetch descriptions**) and WS5 remain. |
| v0.21 | Phase 2.9 WS4 complete. `manual::fetch_descriptions_batch` runs the per-job repair path over the flagged rows, bounded at **20 per click**, cancellable between postings, and advances each row that produced text from `details_needed` to `evaluate` in one transaction — a failure leaves the row in the queue. The Pipeline header offers **Fetch descriptions (N)** with a count from the whole page, shows `Descriptions: 4 of 12…` with a failure count and **Stop** while it runs, and puts the first cause in the error line when something fails. Each entry carries the stored text rather than a byte count so the detail view updates without a reload; `PipelineViewModel::set_description` is now the single derivation of plain text plus the markup flag, and the per-job **Refresh description** uses it too (it previously left the plain view stale). WS5 (manual script §6b, docs, retrospective) remains. |
| v0.22 | **Phase 2.9 complete.** The last workstream wrote manual script **§6b** (preferences → badges → queue actions → batch → Stop → Dismiss → a failure) and extended §6.2/§6.3, recorded Phase 3.2's entry conditions, and closed the phase with `.agents/plans/phase_2.9_retrospective.md`. The retrospective's findings are the substance: a preference field with no column behind it (locations match the posting text, a badge so a false positive is cheap), two hiding decisions needing two escape hatches (a row both excluded and dismissed was invisible in `Dismissed`), a migration test that has to build the 0004 schema by hand, and a stale-plain-text bug the batch exposed in per-job **Refresh description** (both now go through `PipelineViewModel::set_description`). §6b is verified by the owner for steps 1–7; step 8 (a failure that keeps the row in the queue) stays in the script for a genuine refusal and is covered by the offline tests. Phase 3 is next and its entry conditions are met. |
| v0.23 | **Phase 3 checked against the shipped 2.7–2.9 code; plan v0.8.** One real correctness fix: Phase 3's plan said the queue update happens "in the same transaction" as the report, but `job_descriptions::set_queue_states` opens and commits its own transaction, so WS3 must add a transaction-taking helper and write both inside one transaction. Four corrections: Settings is a real screen since 2.9, so WS1 extends it rather than replacing a placeholder; the Pipeline detail already carries Phase 2.9's queue row and the header its description batch, so WS6 composes with both and its thin-row path reuses `manual::refresh_job`; `core::text::plain_text` matches the documented signature exactly; and `docs/logging.md` already forbids logging prompts, so WS1 adds a count-and-outcome line, not a second logging story. New exit criterion 16 (the queue write) and **Q-43**, which needs a decision before WS6: does an evaluation move `applications.status` through `evaluating` → `ready`, or leave it alone? Also corrects Q-45's premise (Settings is no longer a placeholder). |
| v0.24 | **Phase 3 is ready to start (plan v0.9).** Q-43 resolved by the owner: an evaluation moves `discovered`/`ready` → `evaluating` → `ready`, restores the previous status on failure or cancellation, leaves every other status alone, and sends a `NoMatch` report straight to `mark_not_a_match`; all three transitions already exist, so it is two calls in WS3 and no schema change. `rig` is pinned to **0.46.0** (checked 2026-11-06) instead of asking for a version at WS1. Exit criteria 16 (queue write) and 17 (application lifecycle) now pin both behaviors. |
| v0.25 | **Phase 3 WS1 complete.** `core::llm` wraps `rig` 0.46 so no other module names it: provider/config in `settings` (`llm.*`), API keys in the OS keychain (`com.career-ops.app`, one account per provider, write-only), a capability table (DeepSeek chat = JSON mode, tools, 64K; reasoner = no structured output), the planned error taxonomy, a retry policy where the server's `Retry-After` wins, and `status()` naming the first unmet condition. Settings gains an **AI evaluation** section with its own save, a write-only key field, **Save key** / **Delete key**, and **Test connection** — which asks DeepSeek for the account balance and therefore spends no tokens. Only DeepSeek is constructible; the other providers show as disabled rather than hidden. **Two dependency consequences:** `rig` 0.46 requires rustc 1.97.1, so the workspace MSRV moved from 1.83, and `rig-reqwest` adds `reqwest` 0.13 beside acquisition's 0.12 (plan P3-R16). One ignored live test (`deepseek_accepts_the_configured_key`) is indexed in `docs/LIVE_TESTS.md`. WS2 (compact prompts, scoped versioning) is next. |
| v0.26 | **Phase 3 WS2 complete.** Two compact prompts replace the 27–93 KB originals as call input: `evaluate.md` (5,990 chars) carries the five-dimension rubric, the 1–5 global bands mapped onto the report's `match_score`/`recommendation`, the requirement-table rules (evidence tiers, `inferred` capped below `critical`), red flags, legitimacy, and the JSON contract; `cover_letter.md` (2,558 chars) carries the letter rules and the banned-phrase list. Both are seeded under `evaluate` / `cover_letter`; the originals remain as reference. Versioning is scoped by `(name, is_system)` so a user copy and a later shipped system prompt cannot claim the same number, `active_for_use` resolves user-first with a system fallback, and `core::prompt_render` substitutes the five known variables strictly (an unknown name or an unclosed placeholder is an error, not an empty string). The unique index on `(name, is_system, version)` stays in migration 0006 with WS3's columns; the test proving the data satisfies it ships now. WS3 (evaluation pipeline, migration 0006) is next. |
| v0.27 | **Phase 3 WS3 complete.** Migration **0006** adds the report's structured provenance columns and the prompt-version index; `crates/core/tests/evaluation.rs` builds a real 0005 schema to prove it is additive. `core::evaluation` normalizes with the existing `text::plain_text`, resolves the prompt through `active_for_use`, renders strictly, refuses a model without structured output, refuses a prompt over the model's context window (a four-characters-per-token estimate), ensures the application, moves `discovered`/`ready` through `evaluating`, asks once with one repair retry, and writes the report, the application link/score/status, and `queue_state = 'evaluated'` in **one transaction**. A failure restores the previous status and stores nothing; a `NoMatch` report calls `mark_not_a_match` after the commit and logs a refusal for a submitted application. Streaming fragments are display-only; the stored report is the assembled reply. The app gains `PipelineAction::Evaluate` / `FetchDescriptionAndEvaluate` and a polled evaluation task, with `with_evaluation_backend` as the offline seam; the buttons stay disabled until WS6. **Deviations:** `schemars` is a new dependency; the thin threshold is the new `llm.thin_description_chars` setting (default 400); the CV budget is 6,000 chars; the context check is an estimate because there is no tokenizer; and a thin-row fetch triggers a page reload instead of carrying the text in `EvaluationResult`. Nineteen core tests plus one app round trip cover the pipeline. WS4 (cover letters) is next. |
| v0.28 | **Phase 3 WS4 complete.** `core::cover_letter` reuses the evaluation's primitives: the `CompletionBackend` trait moves to `core::llm` (evaluation re-exports it), and `prompt_render` owns the CV budget, the token estimate, and the CV truncation helper for both flows. `run_cover_letter` normalizes the posting, resolves `active_for_use`, renders the five variables with the trimmed optional tone, refuses a prompt over the model's context window, ensures the application, and stores free text with `model_used`, `prompt_version`, `prompt_hash`, and `input_chars`. Regeneration is an insert so every draft stays comparable; an empty reply is an error; and a letter changes neither the application status nor the queue. The app gains `PipelineAction::GenerateCoverLetter { job_id, tone }` and a polled task, and the offline model seam is now `with_completion_backend`, shared by both pipelines. **Deviations:** letters are not streamed yet, and `cover_letters.input_chars` records the assembled prompt rather than the job text. Eleven core tests plus two app tests cover the flow. WS5 (Prompt Manager) is next. |
| v0.29 | **Phase 3 WS5 complete.** The **Prompts** screen replaces the placeholder: a left list of `(name, owner)` threads grouped by category with **In use** markers and version counts, and a right pane with read-only system content, a user editor, a version list (hash prefix, timestamp, usage count), a cached line diff against the current version, a "Used by" section, and a variable reference. **Duplicate as my copy**, **Save new version**, and **Revert** all write through `create_version`, so history is append-only; a user copy is what `active_for_use` sends, and the system row stays as reference. Validation runs on save: `evaluate` and `cover_letter` are checked against the closed five-variable set and reject an unknown placeholder before writing, while reference categories such as `render` keep their own syntax (`pdf.md` uses `{{PHOTO}}`). `repo::prompts` gains `list_threads` and `usage(content_hash)` (reports and letters, joined to the posting). **A real bug surfaced:** a single-flight detail load could overwrite a newly selected thread; the app now records which selection a load is for and discards a stale result. Manual script gains **§6c**. Three core tests, two renderer unit tests, five `egui_kittest` screen tests, and two app tests cover it. WS6 (evaluation UI and injection defense) is next. |
| v0.30 | **Phase 3 WS6 complete.** The Pipeline detail has three tabs — **Description**, **Evaluation**, **Cover letter** — and the placeholder buttons are live: **Evaluate** (or **Fetch description and evaluate** for a thin row), **Evaluate without description**, and **Generate cover letter**. The Evaluation tab shows prompt/model overrides, score, recommendation, provenance, the narrative, and the full run history; the letter tab shows the latest letter, earlier generations, and a tone override. A compact score badge sits on each evaluated row with its band in the hover, and a run updates the score, status (`NoMatch` → not a match), queue, and delete gate in place. Streaming text is displayed while the call runs, with **Stop**: `core::llm::CancelToken` races the provider future (`biased`, so a pre-set Stop never sends the request) and cancellation restores the status and stores nothing. **Injection defense:** the posting is fenced between explicit markers (`prompt_render::untrusted_block`), a forged marker inside it is neutralized, both compact prompts state the data/instruction rule, output validation rejects a malformed or out-of-range report before any write, and the clients have no tools. `docs/prompt-injection.md` records the layers and the residual risk. **Deviation:** `rig` 0.46's `documents` channel is not used because the prompt contract is a single templated `{{job_description}}` that user prompts rely on. Manual script gains **§6d**. Twenty-one evaluation tests, twelve cover-letter tests, 47 Pipeline screen tests, and 169 app tests cover it. WS7 (cassette-backed LLM tests and the retrospective) is next. |
| v0.31 | **Phase 3 complete (offline gates green).** WS7 substitutes for an unavailable API: `rig` **0.46 has no `evals` module and no `experimental` feature**, so the planned `LlmScoreMetric`/`SemanticSimilarityMetric` tests cannot exist at the pinned version. The replacement is a curated fixture set and a cassette layer inside the app's own backend seam: three postings (strong/weak/borderline) with documented bands, recorded replies replayed by `evaluation_fixtures.rs`, a schema-stability test, and a letter-rules check in the default suite; `llm_eval_live.rs` holds the `#[ignore]`d band and similarity checks and doubles as the recorder (`CAREER_OPS_RECORD_CASSETTES=1`). The shipped cassettes are marked **synthetic** until re-recorded with a key; `docs/llm-testing.md` explains the workflow, and `docs/LIVE_TESTS.md` indexes the live tests. A short retrospective (`phase_3_retrospective.md`) records the decisions, friction (`rig` MSRV and `reqwest` duplication, no evals API, hand-authored cassettes), and follow-ups (record the cassettes, run the live checks, a red-team corpus, a cost line). Manual §6c/§6d and the live checks await the owner. Phase 3 is complete: 169 app lib tests, 47 pipeline-screen tests, 21 evaluation, 12 cover-letter, 4 fixture, 5 prompts-screen, and 3 prompt-manager tests pass offline, with fmt and clippy clean. |
| v0.32 | **Phase 3.1 (LLM Usage Tracking) added between Phase 3 and Phase 3.2.** Phase 3 stores character counts, not tokens: `rig` 0.46 reports real counters on both the unary and the streamed path, and the client discards them. The phase adds a per-call `llm_usage` ledger (migration **0007**), reported-vs-estimated provenance, cost only from the provider or `rig`'s catalog (never invented), and a read-only **Usage** screen. It also carries the `deepseek-flash` default (WS0) because `deepseek-chat` is absent from `rig`'s price catalog. Phase 3.2's migration moves 0007 → **0008** and its manual-script section 6c → 6f. Plan: `.agents/plans/phase_3.1_llm_usage_tracking.md`. |
| v0.33 | **Phase 3.1 WS0–WS1 complete.** The default is `deepseek-flash`. `CompletionBackend` returns `Completion { text, usage }`, `core::llm::usage` maps `rig`'s reported counters and prices them from `rig`'s built-in catalog when the provider sends no cost, and migration **0007** with `repo::llm_usage` records one row per provider call — evaluation (repair included), cover letter, failure, and cancellation. Success rows share the report/letter transaction; failure rows are best-effort. Cost is `NULL` when no price is known, never `$0.00`. WS2 (the **Usage** screen) and WS3 (docs, manual §6e, retrospective) remain. |
| v0.34 | **Phase 3.1 WS2 complete.** The **Usage** screen sits between Settings and Logs: a header summary with the range chips (7 days / 30 days / All, default 30), a reported-vs-estimated line, and three grids (provider/model, task, recent calls). Costs render `est.` when priced, `≥` when any call is unpriced or partial, and `—` when none is, never `$0.00`. The load is lazy and tagged with the range it answered, so a stale result is discarded and reloaded; the chips are disabled while it runs. Eleven view-model unit tests, eleven `egui_kittest` screen tests, and one app load test cover it. WS3 (docs, manual §6e, retrospective) is next. |
| v0.35 | **Phase 3.1 complete (offline gates green; manual §6e awaits the owner).** WS3 adds manual script **§6e**, `docs/usage.md` (what a row is, reported vs estimated, what the cost means, what is deliberately absent), the `docs/LIVE_TESTS.md` note that a live call also writes the ledger, and `.agents/plans/phase_3.1_retrospective.md`. The retrospective's substance: the ledger is a statement rather than a child of the job, usage travels with the text through `CompletionBackend`, the provider's cost beats the catalog, success rows share the report/letter transaction while failure rows are best-effort, and the Usage load is the third single-flight read to need an entity tag (Prompts, Pipeline detail, Usage). Phase 3.2 is next and writes `task = 'triage'` through the same seam with no migration. |
| v0.36 | **Phase 3.2 WS1 complete.** The triage contract and prompt exist: `crates/core/prompts/triage_batch.md` (2,515 chars, about 629 tokens; the imported `triage.md` stays as reference) states the four decisions, the batch rules, and the untrusted-data rule, seeded as `triage`; `core::triage` adds `TriageChoice`/`TriageDecision`/`TriageOutput` (schemars) and a parser that refuses an id outside the batch, a duplicate, a missing id, an empty reason, or an unknown choice. `prompt_render` grows two names (`{{preferences}}`, `{{jobs}}`), per-pipeline variable sets, and `validate_for_category`, so the Prompt Manager now rejects a variable from another pipeline. The batch line is `id | title | company | brief` because no location column exists. WS2 (the service and migration 0008) is next. |
| v0.37 | **Phase 3.2 WS2 complete.** Migration **0008** adds the `triage_*` provenance columns and `JobDescription` exposes them. `repo::job_descriptions::apply_triage_tx` writes the queue state, the reason, and the provenance for the whole batch in one transaction, aborting if a row vanished. `core::triage::{estimate, run_triage}` share one preparation step (postings, CV, preferences, prompt, rendering, context check); the run records its `llm_usage` row with `task = 'triage'` before parsing, validates the reply against the loaded ids, and applies the decisions. The batch is 50 by default and 100 at most; the service refuses rather than truncating. Costs are `Option<f64>`: an unpriced model is not `$0.00`. Triage creates no application, fetches no description, and evaluates nothing. Eleven integration tests plus a mapping unit test cover it; WS3 (the review UI) is next. |
| v0.38 | **Phase 3.2 WS3 complete.** The Pipeline header gains **Triage (N)**, gated on new, non-excluded rows and on no model call or description batch in flight. The confirmation modal shows the batch size, model, token estimate, and catalog price; **Run triage** waits for `triage::estimate`, and **Cancel** spends nothing. A run shows **Triaging N jobs…** with **Stop** (the service's cancel token, so nothing partial is applied) and reports through `pipeline_message` with the four per-choice counts, the actual-or-estimated cost, and the elapsed time. `JobRow` carries `queue_reason` and `triage`; the detail shows the choice badge (keeping `Unsure` distinct from `Evaluate` although both map to `evaluate`) and the stored reason. A **Triaged** filter lists the rows a batch covered (a skip lives under **Dismissed**), and a dismissible banner states that every decision can be overridden. The completion line reuses the existing message instead of the planned one-frame flash, and the button counts all new rows while one call covers at most 50, matching **Fetch descriptions**. Twelve `egui_kittest` tests and one app round trip with a scripted backend cover it. WS4 (portal suggestions) and WS5 (docs, retrospective) remain. |
| v0.39 | **Phase 3.2 WS4 complete.** `core::suggest` makes one compact call that proposes 5–10 companies whose careers page uses an embedded engine. The model returns a company, an engine, a board slug, an optional role count, and one sentence; it never returns a URL. The URL is derived from the engine and the slug, and any slug that is not one safe path segment is refused, so a reply cannot point the app at an arbitrary host. The validator also refuses an empty list, more than ten proposals, an empty rationale, a duplicate board, and an unknown engine. A known board keeps its hand-written config via `known_sites::lookup`; a non-known Eightfold board stores its domain config; an ATS board stores none. The prompt seeds as `suggest` with a new `{{known_sites}}` variable, and `prompt_render` now has eight names and a `suggest` set. The Portals screen gains **Suggest portals**, a Stop, and a review modal with **Approve and add**, **Skip**, and **Fetch after adding**; nothing is written until a proposal is approved, and an approval adds exactly one portal. The call records `task = 'suggest_portals'` in the usage ledger. `suggest_portals` takes a cancel token and returns a report so the modal can show the cost; the modal shows the cost after the call rather than a pre-call confirmation. WS5 (docs, manual §6f, retrospective) remains. |

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
| `portals` | `portals.yml` | `id`, `company_name`, `url`, `render_mode`, `scraper_config` (JSON), `last_verified_at`, `last_fetch_attempt_at`, `last_failure_cause` | Configuration for job acquisition. `render_mode` is `static` or `browser` (see §4). |
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
- `portals.last_fetch_attempt_at` and `portals.last_failure_cause` were added in migration `0002_portal_fetch_status.sql` during Phase 2. They are distinct from `last_verified_at` by design: `last_verified_at` records the last successful fetch, `last_fetch_attempt_at` records the last attempted fetch of either outcome, and `last_failure_cause` is NULL on success and a FailureCause::as_str() value on failure. The Portals UI health drill-down reads all three.

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

## 3. egui UI Screens

The UI is built with **egui 0.37** on **eframe 0.37**, chosen after the Phase 2.01 evaluation (see `docs/ui-framework-spike.md`). egui is immediate-mode: the `CareerOpsApp::update` method runs on every frame and renders the current state. Async work is spawned on tokio and polled via `request_repaint_after` (see Phase 2.1's async bridge).

| Screen | Primary Data Sources | Key Components |
| :--- | :--- | :--- |
| **Unlock** | `core::keystore` via `AppState` | `labelled_input` for passphrase, two buttons, error area |
| **First-launch wizard** | `AppState::complete_first_launch` | Three-step `egui::Window`: welcome, passphrase + acknowledgement, backup guidance |
| **Recovery** | `AppState::current() == Recovery` | Full-screen flow with five option cards |
| **Dashboard** | `applications`, `evaluation_reports` | Summary cards, activity feed, match-score histogram |
| **Job Pipeline** | `applications`, `job_descriptions`, `profile.preferences` | `egui_extras::TableBuilder` with virtualized rows; status, queue, and preference filters (Phase 2.9); row selection opens detail; triage review (Phase 3.2) |
| **Job Detail** | `applications`, `job_descriptions`, `evaluation_reports` | Raw description, evaluation viewer, disabled Phase 3 action buttons |
| **CV / Profile Editor** | `profile`, `cv_entries` | Form-based editor; PDF preview pane arrives in Phase 4 |
| **Portals Configuration** | `portals`, `portals::health`, `portals::service` | List with health badges; add/edit/delete modals; per-portal **Refresh** / **Continue** / **Fetch all pages** with progress and Stop (Phase 2.8); batch **Fetch all portals**; sidecar status |
| **Prompt Manager** | `prompts` | Phase 3 |
| **Settings** | `settings`, `profile.preferences` | Phase 2.9 adds the acquisition preferences section; the rest arrives in Phase 5 |
| **Logs** | `core::logging` (ring buffer), the log file | Last in the sidebar after a separator: level chips, search, pause with a new-line count, copy, open folder, delete, dropped-line marker (Phase 2.8.1). The same rows also mount as a resizable bottom drawer, gated by a sidebar toggle and `Cmd/Ctrl+L`, so a fetch can be watched while it runs |

**egui-specific considerations:**

- **Async work requires `request_repaint_after`.** egui's update loop sleeps when there is nothing to repaint. The async bridge schedules the next frame while a task is pending, so results are picked up promptly without a busy loop.
- **`labelled_input` for every text input.** egui's `TextEdit` does not expose `hint_text` as an accessible label. The helper in `crates/app/src/screens/common.rs` is the single place this is handled.
- **`egui_extras::TableBuilder`** is the right primitive for the Pipeline table. For Portals, a plain `ScrollArea` is sufficient at the expected portal count. `crates/app/docs/patterns.md` records the swap point.
- **Unlock gates the app.** The `AppState` state machine is the source of truth. Screens render `AppState::current()`; they do not maintain navigation state of their own.

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
| egui + eframe application base | ~5 MB (smallest of the four evaluated frameworks) |
| SQLite + SQLCipher (linked) | ~2–3 MB |
| `rig` + provider clients | ~2–5 MB |
| `reqwest` + TLS + `scraper` (Tier 1) | ~3–5 MB |
| Argon2 + crypto crates | ~1 MB |
| **Obscura binary (scraping)** | **~70 MB** |
| **Chrome binary (PDF rendering, via `rust-headless-chrome`)** | **~100–150 MB** |
| **Total (default build)** | **~185–245 MB** |

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
| **Phase 1: Foundation** | ✅ Complete — Keystore subsystem (§7) first — envelope encryption, keychain integration, unlock and recovery flows. Then SQLite schema, `sqlx` integration, profile/CV CRUD, basic GPUI shell, `BrowserBackend` and `PdfBackend` trait definitions. | 4–5 weeks |
| **Phase 2: Acquisition** | ✅ Complete — **Obscura integration first** (`BrowserBackend` implementation, sidecar lifecycle). Then Tier 1 (ATS JSON, HTML extraction), tier routing, `portals` config UI, acquisition tests with fixtures. Verify Obscura's PDF limitation against current docs in Week 1. | 3–4 weeks |
| **Phase 2.01: UI Framework Spike** | ✅ Complete — egui selected, GPUI evaluated and rejected | 1 week |
| **Phase 2.1: UI Shell** | ✅ Complete — egui/eframe shell; unlock, first-launch, recovery; Portals and Pipeline screens against the Phase 2 service layer. Retrospective: `.agents/plans/phase_2.1_retrospective.md`. | 1.5–2 weeks |
| **Phase 2.2: Acquisition Completeness** | ✅ Complete — selector pagination, per-host request pacing, detail-page enrichment, the SmartRecruiters job-URL/description fix, and the "Test config" preview are implemented and verified live. Criterion 8 is partially verified (Waymo's WAF challenged the detail route); caps confirmed, two findings recorded for follow-up. Plan: `.agents/plans/phase_2.2_acquisition_completeness.md`; retrospective: `.agents/plans/phase_2.2_retrospective.md`. | 4–6 days |
| **Phase 2.3: Site Providers and Known Sites** | ✅ Complete — site providers (`selectors`, `eightfold`), the Eightfold engine, the typed known-site registry with form prefill, and the curation workflow are implemented. The first full registry run verified both entries live: Waymo 90 jobs / 3 pages / 2 of 2 descriptions, NVIDIA 50 jobs / 5 pages / 40 of 40 descriptions. Plan: `.agents/plans/phase_2.3_site_providers.md`; registry doc: `docs/known-sites.md`; retrospective: `.agents/plans/phase_2.3_retrospective.md`. | 4–6 days |
| **Phase 2.4: Known-Site Catalog** | ✅ Complete — **Add portal** opens the embedded registry (sorted, with provider and verification date), choosing an entry runs the same prefill path as typing the URL, and the form names its source, marks a divergent config, and offers **Restore known config**. No storage change, no migration, no new provider. Plan: `.agents/plans/phase_2.4_known_site_catalog.md`; retrospective: `.agents/plans/phase_2.4_retrospective.md`. | 1–2 days |
| **Phase 2.5: Manual-Testing Fixes** | ✅ Complete — WS1 (canonical identity, dedup, never-clobber, provider-aware manual add), WS2 (page cursor with **Fetch more**, per-job **Refresh description**, Waymo covering every fetched posting), WS3 (`Not a match` status with its first writer and filter), WS4 (delete with a typed gate for touched rows, Pipeline search with a result count), and WS5 (`core::text::plain_text` and the Pipeline's **Raw text** toggle) implemented; Enter unlocks from the password field and the list restores its scroll position. Manual script §6 verified 2026-11-05. Plan: `.agents/plans/phase_2.5_manual_testing_fixes.md`; retrospective: `.agents/plans/phase_2.5_retrospective.md`; live-test index: `docs/LIVE_TESTS.md`. | 4–6 days |
| **Phase 2.6: Known ATS Boards and a Filtered Chooser** | ✅ Complete — `SiteEngine` distinguishes a config-selected provider from a URL-selected ATS adapter, and Figma/Greenhouse, Spotify/Lever, Linear/Ashby, and Ubisoft/SmartRecruiters are verified known sites (registry run 2026-11-05: Figma 143 jobs, Spotify 86, Linear 31, Ubisoft 207/25 enriched). The chooser filters by name, host, or engine, with Clear and a no-match state. Greenhouse's board-host migration is handled. Plan: `.agents/plans/phase_2.6_known_ats_sites.md`; retrospective: `.agents/plans/phase_2.6_retrospective.md`. | 1–1.5 days |
| **Phase 2.7: CV / Profile Editor** | ✅ Complete — replaces the CV / Profile placeholder with an identity + summary editor and experience/education/skill entries, and adds one canonical `cv_summary` renderer in core (`repo::profile::render_cv_summary` / `cv_summary`) used by the screen's preview and by Phase 3. No migration, no new dependency. Plan: `.agents/plans/phase_2.7_cv_profile_editor.md`. Retrospective: `.agents/plans/phase_2.7_retrospective.md`. | 3–5 days |
| **Phase 2.8: Portal Fetch Controls** | ✅ Complete — per-portal **Refresh** / **Continue** / **Fetch all pages** (bounded at 50 pages per click, cancellable, cumulative progress), header **Reload list** / **Fetch all portals**, a monotonic continuation cursor (Refresh raises it to the deepest page read and never lowers it; only Continue clears it at the end), and a per-portal **Fetch descriptions during fetch** toggle stored as `detail.enabled` — legacy `detail` blocks and Eightfold boards keep enriching, and the toggle defers to a hand-edited config. No migration, no new dependency. Plan: `.agents/plans/phase_2.8_portal_fetch_controls.md`. Retrospective: `.agents/plans/phase_2.8_retrospective.md`. | 2–3 days |
| **Phase 2.8.1: Logging and Diagnostics** | ✅ Complete — one `tracing` seam with two structured sinks: a rotated log file under `<data_dir>/logs/` (5 MB × 5) and a bounded in-process ring, so the app's own history is readable in release with no environment variable. A **Logs** screen (level chips, search, pause, copy, open folder, delete) plus a bottom drawer (`Cmd/Ctrl+L`) that spans the window, per-page and per-chunk `info!` events, sidecar stderr capture, and a panic hook. Sequenced before 2.9 because the description batch and Phase 3's LLM calls are the next things that need it. No migration, no new external package, no change to fetch behavior; `docs/logging.md` records the policy and the never-log list. Manual script §9 is fully verified by the owner (thirteen of thirteen steps), after the two findings it produced (§9.4 Follow, §9.10 a failure missing from the log) were fixed. Option C (JSON logs, per-task spans, diagnostic bundle) is deferred in the plan. Plan: `.agents/plans/phase_2.8.1_logging_and_diagnostics.md`; retrospective: `.agents/plans/phase_2.8.1_retrospective.md`. | 2–3 days |
| **Phase 2.9: Acquisition Preferences and Job Queue** | ✅ Complete — `core::preferences::Preferences` (preferred companies/locations/keywords, exclusions, seniority) in the existing `profile.preferences` column, a real **Settings** screen with the five list fields, a preference-aware Pipeline (`Preferred`/`Excluded` badges, `Preferred`/`Excluded` filters, `All` hiding exclusions, a header summary), the pre-evaluation queue (migration 0005; **Mark for details** / **Mark for evaluation** / **Dismiss** in the detail view, a queue badge, and a `Dismissed` filter that is the only view showing a dismissed row), and the bounded **Fetch descriptions (N)** batch (20 per click, cancellable, `details_needed` → `evaluate` on success, failures left in the queue). Location preferences match the posting text because no location column exists yet. Manual script §6b is verified by the owner except step 8 (a failing fetch), which is covered by the offline tests. Plan: `.agents/plans/phase_2.9_acquisition_preferences_and_job_queue.md`; retrospective: `.agents/plans/phase_2.9_retrospective.md`. | 3–4 days |
| **Phase 3: AI Integration** | ✅ Complete (offline gates green; manual §6c/§6d and live checks await the owner) — `core::llm` wraps `rig` 0.46 (DeepSeek), keys live in a dedicated keychain namespace, Settings has an **AI evaluation** section with **Test connection**, the compact prompts (`evaluate`, `cover_letter`) are seeded and versioned by owner with `active_for_use`, `core::prompt_render` substitutes variables strictly, evaluation stores a structured, provenance-carrying report in one transaction (migration 0006) with a visible lifecycle and cooperative cancellation, cover letters keep every draft, the **Prompts** screen manages copies with append-only history, a diff, usage, and save-time validation, the **Pipeline** detail evaluates, streams, cancels, shows history, writes letters, and fences untrusted postings (`docs/prompt-injection.md`), and a fixture/cassette suite replaces the unavailable `rig::evals` API (`docs/llm-testing.md`). `rig`'s MSRV raised the workspace to rustc 1.97.1 and adds `reqwest` 0.13 beside acquisition's 0.12 (plan P3-R16). Retrospective: `.agents/plans/phase_3_retrospective.md`. | 3–4 weeks |
| **Phase 3.1: LLM Usage Tracking** | ✅ Complete (offline gates green; manual §6e awaits the owner) — a per-provider-call `llm_usage` ledger (migration **0007**) records `rig`'s reported input/output/total/cached/reasoning tokens, the task served (`evaluation`, `evaluation_repair`, `cover_letter`, later `triage`), the job/application, duration, and outcome. Character estimates are used only when a provider reports nothing and are flagged `estimated`; cost is provider-reported or catalog-priced from `rig`, otherwise `NULL` (never `$0.00`). The read-only **Usage** screen (between Settings and Logs) shows totals by provider/model and by task, a range selector, and the last 50 calls. The default model is now `deepseek-flash`, which the catalog prices. Success rows share the report/letter transaction; failure and cancellation rows are best-effort. No new dependency. Plan: `.agents/plans/phase_3.1_llm_usage_tracking.md`; retrospective: `.agents/plans/phase_3.1_retrospective.md`; usage doc: `docs/usage.md`. | 3–4 days |
| **Phase 3.2: Assisted Triage** | In progress — WS1 (prompt and contract), WS2 (the batch service, migration 0008, the usage row), WS3 (the review UI: **Triage (N)**, a cost-estimate confirmation, progress/Stop, the **Triaged** filter, decision badges with reasons, and an override banner), and WS4 (portal suggestions: one compact call, derived URLs, a review modal with **Approve and add** / **Skip** / **Fetch after adding**, and the embedded registry's configs) complete; WS5 remains. An advisory LLM triage batch over fresh postings: per-job `details_needed` / `evaluate` / `skip` with a recorded rationale, cost and time shown before and after, and suggest-and-approve portal additions. Decisions are never applied automatically, and no portal is added without approval; the user accepts or overrides them in the queue Phase 2.9 creates. Triage calls write to Phase 3.1's usage ledger with `task = 'triage'`; suggestions write `task = 'suggest_portals'`. Migration **0008**. Plan: `.agents/plans/phase_3.2_assisted_triage.md`. | 5–7 days |
| **Phase 4: Rendering** | `rust-headless-chrome`-based `PdfBackend`, template system, ATS-safe extraction verification, PDF preview, golden-file tests. | 3–4 weeks |
| **Phase 5: Polish & Hardening** | Complete Settings, key rotation UI, backend selection, onboarding, error handling audit, panic audit, accessibility, platform testing, performance tuning, backup/export, documentation, visual polish. | 3–4 weeks |
| **Phase 6: Migration & Release** | Migration tool, packaging, code signing, notarization, release candidate testing, auto-update check, release process, documentation finalization. | 2–4 weeks |
| **Phase 7: Post-v1** | Ongoing maintenance and candidate tracks. `printpdf` manual-ops backend (Track A) is the primary post-v1 item. Single-browser collapse (see §4 Open Question) should be evaluated alongside it. | Open-ended |

**Total Estimate:** ~5–7 months for a functional MVP, assuming one experienced Rust developer. The keystore subsystem is sequenced first in Phase 1 because every other subsystem depends on it. Obscura integration is sequenced first in Phase 2 because the browser is the load-bearing acquisition dependency. Phases 2.8 and 2.9 add about a week; Phase 2.8.1 adds two to three days; Phase 3.1 adds three to four days and Phase 3.2 another five to seven.

---

## Appendix A: Decisions Log

| # | Decision | Status |
| :--- | :--- | :--- |
| 1 | SQLite as primary state store (inverting the original file-canonical doctrine) | Confirmed |
| 2 | Port ~70 Node.js scripts into `core::*` Rust modules | Confirmed |
| 3 | **egui/eframe** as UI framework (was GPUI) | **Superseded by Phase 2.01** — egui chosen after four-framework evaluation |
| 4 | `rig` for LLM integration | Confirmed |
| 5 | `sqlx` (async-first) for SQLite | Confirmed |
| 6 | Backend-agnostic PDF generation via a `PdfBackend` trait | Confirmed (v0.3) |
| 7 | `reqwest` for HTTP | Confirmed |
| 8 | `reqwest` + `scraper` for Tier 1 acquisition | Confirmed — **as optimization, not default path** (v0.5) |
| 9 | Headless browser is **required**, not feature-gated | Confirmed (v0.5) |
| 10 | Browser backend: Obscura sidecar | Confirmed |
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
| 27 | **GPUI evaluated and rejected** — pre-1.0, tightly coupled test harness, no semantic query in base crate, testing guide gaps | Confirmed (Phase 2.01) |
| 28 | **egui's accessibility gap** — text inputs need `labelled_input` helper; `hint_text` is visual only | Confirmed (Phase 2.01) — Phase 5 audit item |

---

## Appendix B: Open Questions

These are tracked here so they are not lost between phases.

| # | Question | Phase to resolve |
| :--- | :--- | :--- |
| Q-1 | Can Obscura's PDF limitation be verified against current project documentation? | Phase 2, Week 1 |
| Q-3 | What is the target Argon2id work factor for representative hardware? | Phase 1 |
| Q-4 | Do the HTML/CSS templates render acceptably through `rust-headless-chrome`? | Phase 4 |
| Q-5 | How should prompt edits interact with historical evaluation reports beyond storing `prompt_version`? | Phase 3 |
| Q-6 | Should encryption be default-on with a passphrase, or should onboarding require it? (Currently: required.) | Phase 5 |
| Q-7 | **Does egui's `TextEdit` support `labelled_by` at 0.37, or does the accessibility gap persist to Phase 5?** | Phase 2.1 — answer with `labelled_input` implementation; audit in Phase 5 if not resolvable |
| Q-44 | **Should logs become structured JSON with per-task spans and a diagnostic-bundle export (Option C)?** | Phase 5 or Phase 3 — Phase 2.8.1 deliberately ships a transcript viewer instead; spans become worthwhile when Phase 3 needs per-call cost and latency records. The deferral and its triggers are recorded in `.agents/plans/phase_2.8.1_logging_and_diagnostics.md`. |
| Q-45 | **Should the log level be settable from Settings instead of `RUST_LOG`?** | Phase 5 — Settings exists since Phase 2.9 but owns acquisition preferences only; the log level belongs with the operational settings that arrive in Phase 5. Until then the Logs screen states its capture level and points at `RUST_LOG`. |

---

This plan is a living document. Open items requiring resolution are tracked in Appendix B. The Obscura PDF limitation (Q-1) and the two-browser architecture question (Q-2) are the most consequential near-term items.