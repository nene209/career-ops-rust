# Phase 3.1: LLM Usage Tracking — Detailed Execution Plan

**Parent plan:** Draft Execution Plan: Career-Ops Rust Desktop Application (v0.32), §12
**Status:** In progress — WS0–WS2 complete; WS3 (docs, manual §6e, retrospective) not started
**Last updated:** 2026-11-07
**Duration:** 3–4 days
**Depends on:** Phase 3 (`core::llm`, `rig` 0.46, `CompletionBackend`, evaluation and cover-letter pipelines, migration 0006)
**Blocks:** nothing. Phase 3.2 benefits: its triage and suggestion calls write to the same ledger with one new `task` value and no migration.

---

## Change Log

| Version | Change |
| :--- | :--- |
| v0.1 | Initial plan. Adds a per-call LLM usage ledger (migration **0007**), captures `rig`'s reported token counters instead of the character proxy, derives cost only from a known price, and adds a read-only **Usage** screen grouped by provider/model and by task. Also carries the default-model change to `deepseek-flash` as WS0, because `deepseek-chat` is absent from `rig`'s price catalog and therefore cannot be costed. Phase 3.2's migration moves to **0008**. |
| v0.2 | **WS0 and WS1 implemented.** WS0 switched the default to `deepseek-flash` and named it in `capabilities::deepseek`. WS1 lands the ledger: `CompletionBackend` returns `Completion { text, usage }` (with `Completion::text_only` for the seven fakes and cassettes), `core::llm::usage` owns `Usage`/`Task`/`Source`/`Outcome`/`Call`, and `Usage::from_rig` maps `rig`'s counters and prices them from `rig`'s built-in catalog when the provider sends no cost of its own (a unit test pins Flash at 0.15/0.6 per million; a provider-reported cost wins; an unlisted model stays unpriced). Migration **0007** and `repo::llm_usage` add `insert_many_tx`/`insert_many`/`record_best_effort` plus the three screen reads (`totals_by_provider_model`, `totals_by_task`, `recent`). The evaluation service now returns every attempt — repair included — and writes the rows in the report transaction, or best-effort before returning a failure; the cover letter uses `insert_tx` so the letter and its one row commit together, and an empty reply records the call but stores no letter. **Deviations:** `Call` omits the application/job ids that `NewLlmUsage::from_call` takes, so the ledger stays a repository concern; `tool_use_prompt_tokens` and `cache_creation_input_tokens` are not columns because DeepSeek reports neither; `RecentCall` carries `error_kind` for the screen; no per-job list (Q-49). Sixteen new tests (eight capture, eight repository/migration), full suite green. |
| v0.3 | **WS2 implemented.** `MainScreen::Usage` sits between Settings and Logs. `view_models::usage` owns the range (`7 days` / `30 days` / `All`, default 30), the header totals summed from the per-model groups, the per-row formatting, and the three derived lists; `screens::usage` renders the header, range chips, an error line with **Retry**, the empty state, and three `Grid`s (provider/model, task, recent calls) inside one `ScrollArea`. Costs render `est.` when every call in the group is priced, `≥` when any call is unpriced or partial, and `—` when none is — never `$0.00`. USD precision is adaptive so a single $0.0018 call is still legible. The load is lazy and **tagged with the range it was requested for**, so a result that arrives after the user picked another range is discarded and reloaded (the single-flight lesson from Prompts and Pipeline); the chips are disabled while a load runs. Twenty-three new tests: eleven view-model unit tests, eleven `egui_kittest` screen tests, and one app test that loads a real ledger and switches range. |

---

## Purpose

Phase 3 gives the app an LLM pipeline but no answer to "what did that cost?" Every stored record carries character counts (`input_chars`, `source_chars`), which were the honest proxy available before `rig` was wired in. `rig` 0.46 reports the real thing: `CompletionResponse.usage` carries input, output, total, cached-input, cache-write, and reasoning tokens, and DeepSeek's dialect (the one this build uses) asks for `stream_options.include_usage`, so both the unary and the streamed path report counters. We read only `response.choice` today and discard the rest.

This phase:

1. **Captures usage per provider call** in a new `llm_usage` table, one row per call, with the task it served and the job/application it concerned.
2. **Never invents a number.** A reported counter is stored as reported. Anything else is stored as an estimate and flagged as one. Cost is stored only when the provider reports it or `rig`'s model catalog prices the reported tokens; otherwise it is `NULL` and the UI shows tokens only.
3. **Adds a read-only Usage screen**: totals by provider/model, a per-task breakdown, a date range, and the recent calls.
4. **Changes the default DeepSeek model** from `deepseek-chat` to `deepseek-flash` (WS0, shippable on its own). The alias in use today is not in `rig`'s catalog, so it can neither be costed nor capability-checked from the catalog; `deepseek-flash` is a listed, active model with published prices.

This is an observability phase, not a billing phase. It records what the provider said and what the app can price; it does not reconcile against an account balance, enforce budgets, or trace spans. Those are named as out of scope and, where they belong to a later phase, recorded as open questions.

### Evidence

- `crates/core/src/llm/client.rs::response_text` reads `response.choice` and drops `response.usage`. `complete_text` returns `String`, and the streamed path discards the folded response's usage after assembling the text.
- `crates/core/src/llm/backend.rs::CompletionBackend::complete` returns `Result<String, LlmError>`, so a pipeline cannot see usage even if the client kept it.
- `crates/core/src/evaluation/service.rs` makes one or two calls (`ask` plus a repair retry, `complete_or_cancel`) and writes only the report.
- `crates/core/src/cover_letter/service.rs` makes one call and writes only the letter.
- `evaluation_reports` and `cover_letters` store `input_chars`, `source_chars`, `model_used`, `prompt_version`, `prompt_hash` — a character proxy, not tokens.
- `rig` 0.46's `Usage` (vendored: `rig-core-0.46.0/src/completion/request.rs`) has `input_tokens`, `output_tokens`, `total_tokens`, `cached_input_tokens`, `cache_creation_input_tokens`, `tool_use_prompt_tokens`, `reasoning_tokens`, `cost: Option<Cost>`, and `is_reported()`. `Cost` has input/output/cache parts, `total`, and `is_complete()`.
- `rig`'s built-in catalog (`rig::catalog::Catalog::builtin()`) lists `deepseek-flash` and `deepseek-v4-pro` with prices (input/output/cache-read, USD per million tokens) and a 1,048,576-token context. It does **not** list `deepseek-chat`.
- DeepSeek's dialect in `rig` inherits `stream_include_usage: true`, so a streamed evaluation returns usage in the final chunk.
- `crates/core/src/prompt_render.rs::estimated_tokens` (4 characters per token) is the existing fallback and stays the only estimator.
- The last applied migration is **0006**; `0007` is free. The **Usage** screen slots into `MainScreen` between Settings and Logs, which the sidebar already renders from `MainScreen::all()`.

---

## Scope

| In scope | Out of scope |
| :--- | :--- |
| `llm_usage` table, migration **0007** | Budgets, quotas, or a hard stop on spend |
| `Completion { text, usage }` through `CompletionBackend` | Per-turn/per-span tracing (Option C logging, Q-44) |
| One ledger row per provider call (including repair and failure/cancel) | Reconciling against the provider's balance endpoint |
| Reported vs estimated token provenance | A configurable price table or per-customer pricing |
| Cost from the provider or `rig`'s catalog, never invented | Charts or a charting crate |
| Read-only **Usage** screen (provider/model, task, range, recent) | Usage in the Pipeline detail (follow-up if wanted) |
| Tests, docs, manual script §6e, retrospective | Pruning or exporting usage (Phase 5 backup/export) |

---

## Design decisions

### 1. A ledger row per provider call, not per task

The unit is **one call to the provider**. That is the only unit with a usage record and a duration. An evaluation that needed a repair is two rows (`evaluation`, `evaluation_repair`); a cover letter is one; Phase 3.2's triage will be one.

This makes the arithmetic auditable: summing the ledger equals what was asked of the provider, and a failed first attempt is visible rather than folded into a successful retry.

### 2. Reported or estimated, never inferred silently

Each row carries `source`:

- **`reported`** — `rig` returned a non-empty usage block. The counters are stored verbatim.
- **`estimated`** — no usage was returned (or the call failed before one arrived). Input tokens are `estimated_tokens(prompt_chars)`; output tokens are `estimated_tokens(reply_chars)` when a reply exists and `NULL` when it does not.

The screen shows the split ("12 calls · 10 reported, 2 estimated") so a total is never presented as more precise than it is. There is no third "guessed" category: everything is one or the other.

### 3. Cost is derived, never invented

A row's `cost_usd` is set only from one of two sources:

1. **The provider's own figure** (`rig`'s `Usage.cost`, populated by gateways such as OpenRouter, Perplexity, and xAI). DeepSeek does not send one today.
2. **`rig`'s built-in catalog price** for the model, applied to the reported counters by `rig::catalog::Pricing::cost`. This is the standard-tier list price and excludes service tiers, long-context tiers, and hosted-tool fees.

If neither applies, `cost_usd` is `NULL`, and the UI says "tokens only, no price for this model" rather than `$0.00`. `cost_complete` records `Cost::is_complete()` so a partial price (for example, cache tokens with no cache rate) is rendered as a lower bound (`≥ $0.004`), not a total.

The pricing lookup lives inside `core::llm::usage`; no `rig` type leaves the module. Because `Catalog::get` needs a provider id, `LlmProvider` maps to it in one `match` beside the existing client construction.

### 4. The ledger is not a child of the job

`llm_usage.application_id` and `llm_usage.job_description_id` are plain nullable references, with no foreign key and no cascade. Deleting a job must not delete the record of the tokens it cost. The same reasoning as a bank statement: the charge survives the purchase.

Both ids are stored because they answer different questions and one does not always exist:

- `application_id` is set for evaluation and cover-letter calls (both call `applications::ensure` before the provider call).
- `job_description_id` is set for the same calls and is what the screen displays by default, because users think in postings, not application rows.

A future task that concerns no job (a hypothetical chat) leaves both `NULL`.

### 5. `CompletionBackend` returns usage with the text

```rust
pub struct Completion {
    pub text: String,
    pub usage: Usage,
}

#[async_trait]
pub trait CompletionBackend: Send + Sync {
    async fn complete(
        &self,
        prompt: &str,
        model: Option<&str>,
        json_mode: bool,
        deltas: Option<&DeltaSink>,
    ) -> std::result::Result<Completion, LlmError>;
}
```

A side channel (a cell the client writes into) would let a caller forget to read it and would make the fake backends lie by omission. Returning the pair makes usage impossible to ignore at the call site.

`Completion::text_only(text)` gives test fakes and cassettes a one-line constructor with `Usage::unknown()`, so each of the seven existing fakes changes by wrapping its return value, not by inventing counters.

`Usage` is the app's own type, not `rig`'s:

```rust
pub struct Usage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    pub cached_input_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
    pub cost_usd: Option<f64>,
    pub cost_complete: Option<bool>,
    pub reported: bool,
}
```

`Usage::unknown()` is all `None`, `reported: false`. `LlmClient` maps `rig`'s `Usage` into it and fills `cost_usd` from the catalog when the provider did not send one. Neither `tool_use_prompt_tokens` nor `cache_creation_input_tokens` gets a column in this phase: DeepSeek reports neither, and a column that is always `NULL` is noise. They stay in the mapped struct only if WS1 finds a provider that uses them; otherwise they are dropped.

### 6. Written in the report/letter transaction on success, best-effort on failure

- **Evaluation, success:** the report, the application update, the queue state, and the usage rows commit together. A report without its cost is a gap in the ledger; a cost without its report is impossible.
- **Evaluation, failure or cancellation:** `ask` still returns the calls it made, and `run_evaluation` inserts them in a small transaction of their own before returning the error. The app logs a warning if that insert fails; it never masks the original error.
- **Cover letter:** `repo::cover_letters` gains `insert_tx` so the letter and its one usage row commit together. Its current `insert(pool, ...)` stays as a thin wrapper for tests and any other caller.

`ask` therefore returns `(Result<EvaluationOutput>, Vec<Call>)` rather than an output that has lost the calls on the error path.

### 7. Duration is measured by the pipeline, per call

`duration_ms` is `Instant::now()` around the `backend.complete` future, not inside the client. That keeps it per attempt (repair included), includes streaming time, and is available even when the call fails or is cancelled. It is wall time as the user experienced it, not provider processing time.

### 8. The screen is a read-only report, not a dashboard

`MainScreen::Usage`, between **Settings** and the separator before **Logs**. Layout:

```
Usage                                              [ 7 days ] [ 30 days ] [ All ]

12 calls · 118,204 input · 9,431 output · 127,635 total · est. $0.42
10 reported, 2 estimated · 1 call has no price

By provider and model
DeepSeek · deepseek-flash     12  118,204   9,431  127,635   $0.42
...

By task
evaluation                    9  ...
evaluation_repair             2  ...
cover_letter                  1  ...

Recent calls
Today 14:02  deepseek-flash  evaluation   Engineer · Acme    8,120 / 940   $0.0018
...
```

Range is a chip row defaulting to **30 days**. Model breakdown is a `Grid`; task breakdown is a short list; recent calls are the last 50, newest first. Everything is a plain `ScrollArea`, as elsewhere in the app — no `egui_extras`, no chart crate.

Empty state: "No LLM calls recorded yet. Evaluation and cover-letter calls appear here."

The screen is loaded lazily when navigated to, like Portals and Pipeline, with the usual single-flight guard and a `usage_loaded` flag. It does not refresh itself after an evaluation; leaving and returning reloads it. (A live-refreshing ledger view is a dashboard, which this is not.)

### 9. Migration 0007

```sql
CREATE TABLE llm_usage (
    id                     INTEGER PRIMARY KEY AUTOINCREMENT,
    provider               TEXT    NOT NULL,
    model                  TEXT    NOT NULL,
    task                   TEXT    NOT NULL,
    application_id         INTEGER,
    job_description_id     INTEGER,
    input_tokens           INTEGER,
    output_tokens          INTEGER,
    total_tokens           INTEGER,
    cached_input_tokens    INTEGER,
    reasoning_tokens       INTEGER,
    source                 TEXT    NOT NULL,
    cost_usd               REAL,
    cost_complete          INTEGER,
    outcome                TEXT    NOT NULL,
    error_kind             TEXT,
    duration_ms            INTEGER NOT NULL,
    created_at             TEXT    NOT NULL
);

CREATE INDEX idx_llm_usage_created_at ON llm_usage(created_at);
CREATE INDEX idx_llm_usage_provider_model ON llm_usage(provider, model);
CREATE INDEX idx_llm_usage_task ON llm_usage(task);
```

`task`, `source`, and `outcome` are free text with a Rust enum in front. Values this build does not know read as their neutral variant (`source = 'estimated'`, `outcome = 'error'`, task = its raw string), so a newer build's `triage` rows cannot break an older one — the same rule as `queue_state`.

`outcome` is `ok` | `cancelled` | `error`, with `error_kind` holding the `LlmError` discriminant for the last two. It is stored because a cancelled stream still bills for what it generated, and the honest record is "we asked, it was cancelled, we do not know how many tokens it produced".

No foreign keys, no cascades, no backfill: the table starts empty and only describes calls made after this phase.

---

## Workstream 0 — Default model (`deepseek-flash`)

**Estimated duration:** 0.5 day. Shippable on its own; land it before WS1.

### Deliverables

1. `LlmProvider::default_model()` returns `Some("deepseek-flash")` for DeepSeek. The doc comment stops calling the alias the current chat model and records why: `deepseek-flash` is a listed, active model in `rig`'s catalog (1,048,576-token context, published input/output/cache prices), while `deepseek-chat` is an alias the catalog does not list.
2. `capabilities::deepseek` names `deepseek-flash` in its documentation and gains a unit test asserting its abilities (JSON mode, streaming, tools, 65,536-token local budget). It deliberately keeps the unknown-model fallback for future renames rather than duplicating an identical arm. The conservative window is deliberate: the enforced limit is the 12,000-character job budget, and understating the window only refuses a prompt the app never builds. `deepseek-chat` keeps the same treatment for stored configurations.
3. The Settings hint becomes "for example deepseek-flash".
4. Every test that derives its expectation from `LlmConfig::default()` is updated: `config.rs` (`the_default_is_deepseek_chat` → `the_default_is_deepseek_flash`, `sanitizing_fills_and_clamps`), `client.rs` (`deepseek_builds_a_client_without_touching_the_network`), `crates/core/tests/llm_config.rs` (the round-trip comment and `saving_normalizes_an_empty_model`), `crates/app/src/app.rs` (the settings round trip), and the three pipeline tests that assert `model_used` from the default config (`tests/evaluation.rs`, `tests/cover_letter.rs`, `tests/evaluation_fixtures.rs`). Tests that pass `"deepseek-chat"` explicitly as a fixture string are left alone.

### Acceptance criteria

| Criterion | Verification |
| :--- | :--- |
| A fresh vault defaults to `deepseek-flash` | `config.rs` unit test |
| A stored `deepseek-chat` is kept, not rewritten | Existing round-trip test |
| `deepseek-flash` has an explicit capability test | `capabilities.rs` unit test |
| Settings names the new example | `settings_screen.rs` / manual check |

---

## Workstream 1 — Usage capture and the ledger

**Estimated duration:** 1.5 days.

### Deliverables

1. `core::llm::usage`:
   - `Usage`, `Usage::unknown()`, `Usage::is_reported()`, and the `rig` mapping.
   - `Task` (`Evaluation`, `EvaluationRepair`, `CoverLetter`, `Triage`, `Other(String)`), `Source` (`Reported`, `Estimated`), `Outcome` (`Ok`, `Cancelled`, `Error`).
   - `Call { provider, model, task, usage, outcome, error_kind, duration_ms }`.
   - `price(provider, model, &rig_usage) -> Option<Cost>` using `Catalog::builtin()`, inside this module only.
2. `LlmClient::complete_text` returns `Completion`; the unary path maps `response.usage`, and the streamed path maps the folded `finish()` response's usage (the fragment buffer stays the text fallback).
3. `CompletionBackend::complete` returns `Result<Completion, LlmError>` and `Completion::text_only` exists. All seven fakes and the live recorder are updated mechanically.
4. `core::evaluation::service`:
   - `ask` returns `(Result<EvaluationOutput>, Vec<Call>)`; the repair call is a second entry with `Task::EvaluationRepair`.
   - `run_evaluation` inserts the calls with the report inside the existing transaction, and best-effort on the error path.
   - `Call` timings are measured around each `complete_or_cancel` call.
5. `core::cover_letter::service` returns and records one call; `repo::cover_letters::insert_tx` is added and `insert` delegates to it.
6. `migrations/0007_llm_usage.sql` and `repo::llm_usage`:
   - `insert_many_tx(&mut Transaction, &[NewLlmUsage])` and `insert_many(pool, ...)`.
   - `totals_by_provider_model(pool, since)`, `totals_by_task(pool, since)`, `recent(pool, since, limit)`, each returning a `FromRow` aggregate type.
   - `since: Option<DateTime<Utc>>`; `None` means all time. `created_at` is RFC 3339 UTC, so the range filter is a string comparison, as elsewhere.
   - Cost aggregation keeps two facts: `SUM(cost_usd)` and the number of rows with `cost_usd IS NULL` or `cost_complete = 0`, so the screen can render "est. $0.42 (1 call unpriced)".
7. `model.rs` gains `LlmUsage` only if the recent-calls read needs it; the aggregate structs live in `repo::llm_usage`, beside the queries that produce them, as `PromptUsageEntry` does.
8. Tests (see Test Strategy).

### Acceptance criteria

| Criterion | Verification |
| :--- | :--- |
| An evaluation writes one `reported` row with the counters the provider sent | Core integration test |
| A repair writes a second row with task `evaluation_repair` | Core integration test |
| A fake with no usage writes an `estimated` row | Core integration test |
| A cancelled call writes `outcome = 'cancelled'` and does not lose the original error | Core test |
| A cover letter and its usage row commit together | Core test against the transaction |
| Usage rows survive deleting the job | Core test |
| Migration 0007 applies to a real 0006 database and preserves rows | Migration test |
| Costs are `NULL` when no price is known, never zero | Unit test |

---

## Workstream 2 — Usage screen

**Estimated duration:** 1 day.

### Deliverables

1. `MainScreen::Usage` in `navigation.rs`, labelled **Usage**, placed between Settings and Logs.
2. `UsageViewModel` in `view_models`:
   - `UsageTotals { calls, input, output, total, cost, unpriced, reported, estimated }` derived from the repository rows, so the arithmetic is unit-testable without egui.
   - Range selection (`7d`, `30d`, `All`, default `30d`) and a `since()` helper.
   - Formatting helpers: token counts with thousands separators, cost with `est.` and `≥` when incomplete, and a "no price" suffix.
3. `screens/usage.rs`: header summary, range chips, three `Grid`s (provider/model, task, recent), empty state, and an error line. Plain `ScrollArea`.
4. `CareerOpsApp` wiring: `ScreenAction::Usage`, `usage_task`, `usage_loaded`, `usage_error`, `start_usage_load`, `poll_usage_task`, and the lazy load on navigation, following the Portals pattern.
5. `egui_kittest` tests (see Test Strategy).

### Acceptance criteria

| Criterion | Verification |
| :--- | :--- |
| The sidebar lists **Usage** between Settings and Logs | `egui_kittest` |
| Totals, provider/model rows, task rows, and recent calls render | `egui_kittest` |
| The empty state renders for an empty ledger | `egui_kittest` |
| An unpriced or incomplete cost renders as `est.` / `≥`, not `$0.00` | View-model unit test + screen test |
| Navigating to the screen loads once and does not re-load on every frame | App test |

---

## Workstream 3 — Docs, verification, retrospective

**Estimated duration:** 0.5 day.

1. Manual script **§6e Usage**: evaluate a posting, open Usage, confirm the call appears with its reported tokens, check that a cover letter adds a second row under a different task, switch the range, and confirm the empty state on a fresh vault.
2. `docs/usage.md`: what is stored (counters, task, subject, outcome, duration, cost), what reported vs estimated means, what the price is (list-price estimate, not an invoice), and the privacy rule (no prompt or reply text, consistent with `docs/logging.md`).
3. `docs/LIVE_TESTS.md`: note that the existing live evaluation test now also produces ledger rows, so a live run is a way to verify `source = 'reported'` against the real API. No new ignored test is strictly required; add one only if the live run shows DeepSeek omitting usage.
4. Full validation: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --offline -- -D warnings`, `cargo test --workspace --features career-ops-app/keychain --offline -- --skip obscura`.
5. Parent plan: v0.32 changelog and a Phase 3.1 row; Phase 3.2's migration reference moves to 0008 and its manual-script section to §6f.
6. Short retrospective in `.agents/plans/phase_3.1_retrospective.md`.

---

## Test Strategy

| Layer | Covers |
| :--- | :--- |
| Core unit | `Usage` mapping (`is_reported`, totals); reported vs estimated; cost pricing (provider cost wins, catalog price, no price); `Task`/`Source`/`Outcome` parsing with an unknown value; token formatting |
| Core integration (fresh DB) | Evaluation success writes report + usage together; repair writes two rows; cover letter writes letter + usage together; failed/cancelled call writes its row and still returns the error; deleting a job keeps the usage rows |
| Core migration | Build a real 0006 schema from `include_str!`, insert a job/report/letter, apply 0007 by hand, assert the existing rows survive and `llm_usage` is empty |
| Repository aggregates | `totals_by_provider_model`, `totals_by_task`, `recent`; range boundaries; NULL cost counted as unpriced; `cost_complete = 0` counted as a lower bound |
| `egui_kittest` | Sidebar entry; summary; two breakdowns; recent list; empty state; `est.`/`≥` rendering |
| App | Navigation loads once, stores the view model, and survives a repository error |
| Live (ignored) | No new test; the existing live evaluation, when run, is the check that DeepSeek reports usage |

No network for any offline test. No new dependency. One migration.

---

## Risks

| ID | Risk | Impact | Mitigation |
| :--- | :--- | :--- | :--- |
| P3.1-R1 | **Usage is recorded but wrong.** A provider reports a counter in a different unit or omits one part. | Medium | `rig` normalizes to `Usage`; counters are stored verbatim with `source = 'reported'`; totals are sums, not derivations; the live test compares one real call. |
| P3.1-R2 | **A cancelled stream's cost disappears.** The future is dropped before `finish()`, so no counters arrive. | Low | The row is recorded with `outcome = 'cancelled'` and an estimated input; the UI counts unestimated output as unknown, not zero. |
| P3.1-R3 | **The price is mistaken for a bill.** Catalog prices are standard-tier list prices and can differ from the account's charges. | Medium | `docs/usage.md` and the screen label say "est."; `cost_complete` renders `≥`; the provider-reported cost always wins when present. |
| P3.1-R4 | **The ledger grows without bound.** One row per call. | Low | A row is under 200 bytes; a heavy month is tens of thousands of rows, which SQLite and the summary queries handle. Pruning/export is Phase 5. |
| P3.1-R5 | **The trait change ripples through every fake.** | Low | `Completion::text_only` makes each fake a one-line wrapper; the compiler finds every call site. |
| P3.1-R6 | **`deepseek-flash`'s catalog window (1M) exceeds the local capability table (64K).** | Low | The local table is deliberately conservative and the 12,000-character job budget is the enforced limit; a wrong window only changes when a pre-flight refusal happens. |
| P3.1-R7 | **A usage insert failure aborts a completed evaluation.** | Medium | Success rows share the report transaction (so a failure is atomic, not partial); failure-path rows are best-effort and logged, never masking the original error. |

---

## Exit Criteria

1. `llm_usage` exists (migration 0007) and every provider call the app makes writes exactly one row.
2. A reported usage block is stored verbatim with `source = 'reported'`; a missing one is estimated and flagged.
3. Cost is `NULL` when no price is known, never `0.00`, and a partial price renders as a lower bound.
4. An evaluation writes its usage in the report transaction; a failure or cancellation still records its calls and returns the original error.
5. A cover letter writes its letter and usage together.
6. Deleting a job leaves its usage rows.
7. The **Usage** screen is in the sidebar and renders totals by provider/model, totals by task, a range selector, recent calls, and an empty state.
8. A fresh vault defaults to `deepseek-flash`; a stored `deepseek-chat` is preserved.
9. Migration 0007 applies to a real 0006 database and preserves every existing row.
10. Offline suite, fmt, and clippy are clean.
11. Manual script §6e, `docs/usage.md`, parent plan, and a short retrospective are updated.

---

## Open Questions

| # | Question | Position |
| :--- | :--- | :--- |
| Q-46 | **Should the price table be user-editable** (custom endpoints, negotiated rates, a changed list price)? | Not now. The catalog is a list-price estimate and the screen says so. A user-maintained price table is a settings surface with no demand yet; revisit if a real invoice disagrees. |
| Q-47 | **Should usage be prunable or exportable?** | Not now. The ledger is small and is part of the data the Phase 5 backup/export path should include. Deleting history is a deliberate action, not a maintenance chore. |
| Q-48 | **Should every future LLM task share this ledger?** | Yes. `task` is text with an unknown fallback, and Phase 3.2 writes `triage` with no migration. A per-task column or a second table would fragment the totals. |
| Q-49 | **Should the Pipeline detail show a job's usage?** | Not in this phase. The recent-calls list already names the job. If it proves useful during manual testing, it is a small read plus a line in the detail panel, not a second screen. |

---

## What Phase 3.1 Is Not

- Not a budget or quota system: nothing stops a call, and no threshold is enforced.
- Not billing reconciliation: the provider's balance endpoint is not queried.
- Not per-span tracing: `task` + subject is the association. Per-task spans remain Option C of Phase 2.8.1 (Q-44).
- Not a dashboard: no charts, no auto-refresh, no per-day bars in this phase.
- Not a change to evaluation or cover-letter behavior: the prompts, the transactions' contents, and the outputs are unchanged; only the call's usage is kept.
- Not a second provider: only DeepSeek is constructible, so only DeepSeek is priced.
