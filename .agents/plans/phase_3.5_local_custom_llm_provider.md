# Phase 3.5: Local / Custom LLM Provider — Detailed Execution Plan

**Parent plan:** Draft Execution Plan: Career-Ops Rust Desktop Application (v0.53), §12
**Status:** Planned — not started
**Last updated:** 2026-11-09
**Duration:** 3–4 days (including WS0)
**Depends on:** Phase 3 (`core::llm` facade, evaluation/letter calls), Phase 3.1 (usage ledger and Settings), Phase 3.2 (triage/suggest call the same facade)
**Blocks:** nothing. Phase 3.6 (PII redaction) follows it but does not depend on it.

---

## Change Log

| Version | Change |
| :--- | :--- |
| v0.1 | Initial plan. Adds a **Local / custom** provider for OpenAI-compatible endpoints: base URL, optional API key, model, JSON-mode toggle, and an optional context window. No migration and no new dependency — `rig` 0.46 already ships the dialect and base-URL/auth controls. WS0 replaces the per-test live flags with one `CAREER_OPS_LIVE=1` gate. |

---

## Purpose

Only DeepSeek is constructible today. A user running Ollama, `llama-server`,
vLLM, LM Studio, or any OpenAI-compatible gateway cannot use the evaluation,
cover-letter, triage, or suggestion features without sending data to DeepSeek.
Adding a **Local / custom** provider closes that gap and doubles as a privacy
control: a local endpoint means the CV and postings never leave the machine.

The provider is deliberately one id, not a separate provider per engine. The
endpoint shape is the same; the base URL, model, key, JSON-mode support, and
context window are user inputs.

### Evidence

- `LlmProvider` has four ids (`deepseek`, `openai`, `anthropic`, `gemini`); only
  `DeepSeek::is_supported()` is true (`crates/core/src/llm/config.rs`).
- `LlmConfig::build_client` rejects a blank key for every provider
  (`crates/core/src/llm/client.rs`), and five app call sites independently
  return `MissingApiKey` when the keychain has no entry.
- `capabilities(provider, model)` is a static table (`capabilities.rs`) called
  from evaluation, cover-letter, triage, suggest, `build_client`, and `status`.
- `LlmStatus::blocked_reason` blocks whenever `has_api_key` is false.
- `rig-core` 0.46 ships `providers::ollama` and `providers::llamacpp`; the
  `OLLAMA` OpenAI dialect uses `Auth::OptionalBearer`, and `OpenAIConfig`
  exposes `with_base_url`, `with_auth`, and a `/models` verify path.
- The Settings AI section lists every provider but disables all but DeepSeek and
  resets the model to `default_model()` on a provider change.

---

## Scope

| In scope | Out of scope |
| :--- | :--- |
| One `custom` provider id with a base URL and model | Auto-discovery of models from `/models` |
| Optional API key; empty key omits `Authorization` | Downloading or managing a local model |
| JSON-mode toggle and optional context window | A per-model capability editor |
| Settings fields, validation, and Test connection | A new HTTP client or `rig` fork |
| Config-aware capabilities (`PromptOnly`) | Anthropic/Gemini/OpenAI support |
| Usage rows labelled `custom` with `NULL` cost | Cost estimation for self-hosted hardware |
| One live-test gate (`CAREER_OPS_LIVE=1`) | Changing the offline suite's behavior |
| Tests, docs, manual steps, retrospective | Changing any prompt text |

---

## Decision check

1. **One provider, not one per engine.** Ollama, `llama-server`, vLLM, LM Studio,
   and hosted gateways all expose an OpenAI-compatible `/v1` route. A preset only
   fills the base URL; it does not change the client. Presets can be added later
   without a schema change.
2. **Generic OpenAI body, optional bearer, forced Chat Completions.** The client is
   `OpenAIConfig::with_key(&OPENAI, key_or_empty).with_base_url(url)
   .with_auth(Auth::OptionalBearer).with_route(Route::Chat)`. The `OPENAI`
   dialect defaults to the **Responses** API, which Ollama, `llama-server`, and
   most compatible servers do not implement; `Route::Chat` forces
   `/chat/completions`. This avoids the Ollama/llama.cpp body rewrites, which are
   engine-specific, and omits the `Authorization` header when the key is empty (a
   local server that rejects the header keeps working).
3. **Optional key changes five call sites.** `build_client` must accept an empty
   key when the provider does not require one, and every app caller must stop
   treating `None` as an error for Custom. `LlmStatus` blocks only when a key is
   required.
4. **JSON mode is not a hard capability.** A local model may not accept
   `response_format`. `StructuredOutput::PromptOnly` means "no provider
   enforcement; ask in the prompt, parse locally, repair once". It is usable, so
   the evaluation pipeline is not refused.
5. **An unknown context window disables the pre-flight check.** The character
   budget still applies, and a provider error still maps to
   `ContextLengthExceeded`. The user can enter the model's window.
6. **Cost is unknown, not zero.** `rig`'s catalog does not price a user-entered
   model, so the usage ledger stores `NULL` and the Usage screen shows a dash.
7. **A user-supplied URL is a trust boundary.** Allow `https://` anywhere and
   `http://` only for loopback (`localhost`, `127.0.0.1`, `::1`); warn for other
   plain-http hosts; reject embedded credentials.
8. **One live gate.** `CAREER_OPS_LIVE=1` replaces `RUN_LIVE_ATS_TESTS` and
   `CAREER_OPS_LIVE_REGISTRY`. Tests remain `#[ignore]` and still require their
   own prerequisites (`DEEPSEEK_API_KEY`, `OBSCURA_BIN`,
   `CAREER_OPS_LOCAL_LLM_URL`); individual tests are selected by name.

---

## Design decisions

### 1. Config and storage

`LlmProvider` gains `Custom` with stored form `"custom"` and label
**"Local / custom"**. `LlmConfig` gains:

```rust
pub base_url: String,              // required for Custom, empty for DeepSeek
pub json_mode: bool,               // default true; only meaningful for Custom
pub custom_context_window: Option<usize>, // None = do not pre-check
```

All three live in the existing `settings` JSON keys (`llm.base_url`,
`llm.json_mode`, `llm.custom_context_window`). No migration.

`LlmProvider::requires_api_key()` is `true` for DeepSeek and `false` for Custom.
`default_model()` stays `None` for Custom: the user must name the model, because
there is no safe default across engines.

`sanitized()` trims the base URL, drops a trailing `/`, and leaves the model
empty for Custom (no default to fill). A missing base URL or model is a
validation error, not a silent default.

### 2. Building the client

```rust
let config = rig::providers::openai::OpenAIConfig::with_key(
        &rig::providers::openai::wire::OPENAI,
        api_key.as_str(), // "" is allowed for Custom
    )
    .with_base_url(self.base_url.clone())
    .with_auth(rig::providers::openai::wire::Auth::OptionalBearer)
    // The OPENAI dialect defaults to the Responses API; local servers serve
    // `/chat/completions`, so force the Chat route.
    .with_route(rig::providers::openai::wire::Route::Chat);
let client = config.client();
```

Construction stays offline. DeepSeek keeps its current dialect. The generic
OpenAI body is the common denominator; a server that needs a non-standard body
rewrite is out of scope.

### 3. Capabilities

`capabilities` becomes `LlmConfig::capabilities()` (or takes `&LlmConfig`), so
Custom can answer from its fields:

| Setting | Capability |
| :--- | :--- |
| `json_mode = true` | `StructuredOutput::JsonObject` (send `response_format`) |
| `json_mode = false` | `StructuredOutput::PromptOnly` (ask in the prompt, parse locally) |
| `custom_context_window = Some(n)` | `context_window: Some(n)` |
| `custom_context_window = None` | `context_window: None` (no pre-flight check) |
| always | `streaming: true`, `tools: false` |

`StructuredOutput::is_usable()` returns true for `PromptOnly`. The evaluation
pipeline already parses and repairs once, so no service change is needed beyond
the capability call.

### 4. Key handling

Add `LlmProvider::requires_api_key()`. The five app call sites become:

```rust
let key = match llm::keys::get_api_key(config.provider)? {
    Some(key) => key,
    None if config.provider.requires_api_key() => {
        return Err(LlmError::MissingApiKey { provider: config.provider }.into());
    }
    None => Zeroizing::new(String::new()),
};
```

`LlmStatus::blocked_reason` checks `requires_api_key()` before reporting a
missing key. The Settings key row says "No key stored (optional)" for Custom.

### 5. Validation and Test connection

`LlmConfig::validate_custom()` returns a field-specific message:

- Base URL: must parse as `http`/`https`; no userinfo; plain `http` only for
  loopback; a trailing `/` is removed.
- Model: non-empty.
- Context window: if present, at least 1,024 tokens.

**Test connection** calls `LlmClient::verify()`, which uses the OpenAI dialect's
`/models` route and spends no tokens. If a server does not implement `/models`,
the message says so and suggests saving the settings and running an evaluation;
the plan does not add a paid completion fallback.

### 6. Settings UI

The AI section gains a **Local / custom** provider button. When selected:

- **Base URL** with the hint `http://localhost:11434/v1` (Ollama) or
  `http://localhost:8080/v1` (`llama-server`).
- **Model** (required), for example `llama3.2` or `qwen2.5:7b`.
- **API key (optional)**; the Save key/Delete key row explains that a local
  server usually needs none.
- **Use JSON mode** (default on) with a hint that some local models do not
  support it; turning it off still works because the reply is validated locally.
- **Context window (tokens, optional)** with a hint that leaving it blank
  disables the pre-flight check.
- **Test connection** enabled when the base URL and model validate.
- A warning for a non-loopback `http://` URL and the data-sharing note naming the
  endpoint.

Switching providers preserves the Custom fields and only changes the model when
the new provider has a default (DeepSeek). Switching to DeepSeek restores
`deepseek-flash`; switching to Custom clears the model if it was DeepSeek's.

### 7. Usage and status

Usage rows record `provider = "custom"` and the model the user chose. Cost is
`NULL`; the screen renders it as a dash, matching the existing "never invent a
price" rule. The status line names the configured host (not the key) and the
first unmet condition.

### 8. One live-test gate (WS0)

`CAREER_OPS_LIVE=1` is the only gate. `career_ops_test_support` gains:

```rust
pub fn live_enabled() -> bool;
pub fn require_live(test: &str) -> bool; // prints a skip line when unset
```

All live tests call `require_live` and stay `#[ignore]`; the `ats_live` and
`obscura` tests become `#[ignore]` too. Prerequisites remain separate
(`DEEPSEEK_API_KEY`, `OBSCURA_BIN`, `CAREER_OPS_LOCAL_LLM_URL`), as do overrides
(`CAREER_OPS_LIVE_PORTALS`, `CAREER_OPS_LIVE_PAGED_PORTAL`, `LIVE_*_SLUG`).

Commands:

```text
# every live test that has its prerequisite
CAREER_OPS_LIVE=1 cargo test --workspace --features career-ops-app/keychain -- --ignored

# one test
CAREER_OPS_LIVE=1 cargo test --test llm_live deepseek_accepts_the_configured_key -- --ignored
CAREER_OPS_LIVE=1 cargo test -p career-ops-app --features keychain live_fetch_reaches_the_pipeline -- --ignored
```

`docs/LIVE_TESTS.md` and the manual script's §8 commands are rewritten around the
single flag. The `obscura_print_to_pdf_if_supported` test gains the availability
check it is missing, so `--skip obscura` is no longer required for safety (it can
stay in the standard validation command).

---

## Workstream 0 — Live-test gate

**Estimated duration:** 0.5 day.

### Deliverables

1. `live_enabled()` / `require_live()` in `career_ops_test_support`.
2. Every live test gates on `CAREER_OPS_LIVE`; `RUN_LIVE_ATS_TESTS` and
   `CAREER_OPS_LIVE_REGISTRY` are removed.
3. `ats_live` and `obscura` tests marked `#[ignore]`; the missing obscura
   availability check added.
4. `docs/LIVE_TESTS.md` gate column and commands; manual script §8 commands.
5. A test that `require_live` returns false when the flag is absent (or a
   documentation-only assertion if env mutation is unsafe in parallel tests).

### Acceptance criteria

| Criterion | Verification |
| :--- | :--- |
| One flag enables every live test with its prerequisite | Manual command |
| One test can be selected by name | Manual command |
| The offline suite is unchanged without the flag | Full suite |
| No test reaches the network without the flag | Code review + tests |

---

## Workstream 1 — Config, provider, and validation

**Estimated duration:** 1 day.

### Deliverables

1. `LlmProvider::Custom`, `requires_api_key()`, `ALL`, parse/label tests.
2. `LlmConfig` fields and settings keys; `sanitized()`/`validate_custom()`.
3. URL validation (scheme, loopback rule, userinfo rejection, trailing slash).
4. Unit tests: round-trip, default model rules, optional key, URL cases,
   context-window bounds, provider switch semantics.

### Acceptance criteria

| Criterion | Verification |
| :--- | :--- |
| Custom has no default model and no required key | Unit tests |
| Invalid URLs are refused with a field-specific message | Unit tests |
| Stored settings survive a reload | Unit test |

---

## Workstream 2 — Client, capabilities, and call sites

**Estimated duration:** 1 day.

### Deliverables

1. `build_client` for Custom with `Auth::OptionalBearer`; blank key allowed only
   when `requires_api_key()` is false.
2. `StructuredOutput::PromptOnly`; `capabilities` config-aware; all six call
   sites updated.
3. Key handling in `start_evaluation`, `start_cover_letter`, `start_triage`,
   `start_suggest_portals`, and `start_llm_test`.
4. `LlmStatus::blocked_reason` and Settings key row.
5. Usage label and `NULL` cost.
6. Tests: offline client construction, optional key path, PromptOnly accepted by
   the evaluation pre-flight, DeepSeek unchanged, status messages.

### Acceptance criteria

| Criterion | Verification |
| :--- | :--- |
| A keyless local config builds and calls | Core + app tests with a mock backend |
| DeepSeek behavior is unchanged | Existing tests |
| JSON mode off does not block evaluation | Capability test |
| Missing key still blocks DeepSeek | App test |

---

## Workstream 3 — Settings UI, docs, live test, retrospective

**Estimated duration:** 1–1.5 days.

### Deliverables

1. Settings UI as in decision 6, with `egui_kittest` coverage for selection,
   validation, the optional-key copy, JSON-mode toggle, context window, and
   provider switching.
2. An ignored live test
   `custom_endpoint_answers_a_prompt` gated by `CAREER_OPS_LIVE=1` and
   `CAREER_OPS_LOCAL_LLM_URL` (with `CAREER_OPS_LOCAL_LLM_MODEL`), which returns
   early when the URL is unset.
3. Manual script: a Settings step for the local provider (base URL, model, Test
   connection) and one evaluation through it; note that the key is optional.
4. `docs/LIVE_TESTS.md` entry; `docs/llm-testing.md` note that local endpoints are
   a supported backend.
5. `main.md` v0.48 row and this plan's changelog; retrospective with the
   capability and optional-key decisions.

### Acceptance criteria

| Criterion | Verification |
| :--- | :--- |
| A user can configure and test Ollama/`llama-server` | Manual |
| One evaluation runs through the local endpoint | Manual + live test |
| Usage shows `custom` with a dash cost | Manual |

---

## Test Strategy

| Layer | Covers |
| :--- | :--- |
| Unit | provider parse/label, optional key, URL validation, context window, capability mapping |
| Core | client construction offline, `PromptOnly` accepted, DeepSeek unchanged |
| App | keyless call path, missing-key DeepSeek path, status messages |
| `egui_kittest` | provider selection, fields, validation, Test connection enablement, key copy |
| Live | `CAREER_OPS_LIVE=1` + `CAREER_OPS_LOCAL_LLM_URL` sends one tiny prompt |
| Manual | configure Ollama or `llama-server`, Test connection, one evaluation |

No migration. No new dependency. No prompt change.

---

## Risks

| ID | Risk | Impact | Mitigation |
| :--- | :--- | :--- | :--- |
| P3.5-R1 | **A local server rejects an empty `Authorization` header.** | Medium | `Auth::OptionalBearer` omits it when the key is empty. |
| P3.5-R2 | **The server has no `/models` route.** | Low | Test connection reports it and points to an evaluation; calls still work. |
| P3.5-R3 | **The model has no JSON mode.** | Medium | `PromptOnly` plus local parse/repair; a Settings toggle. |
| P3.5-R4 | **The context window is unknown.** | Low | `None` disables the pre-flight check; the character budget and provider error remain. |
| P3.5-R5 | **A user enters a plain-http remote URL.** | Medium | Loopback-only `http`; a visible warning for anything else. |
| P3.5-R6 | **Optional-key plumbing is missed in one call site.** | High | One helper and tests for all five paths. |
| P3.5-R7 | **The live-flag refactor breaks an existing command.** | Low | Rewrite `docs/LIVE_TESTS.md` and manual §8; run the new command. |

---

## Exit Criteria

1. **Local / custom** is selectable in Settings with a base URL, model, optional
   key, JSON-mode toggle, and optional context window.
2. A keyless local endpoint builds a client and completes an evaluation.
3. DeepSeek's key requirement, capabilities, and behavior are unchanged.
4. JSON mode off is usable (`PromptOnly`), not refused.
5. Test connection succeeds against `/models` or explains why it did not.
6. Usage rows show `custom` and a `NULL` cost.
7. `CAREER_OPS_LIVE=1` is the single live gate; individual tests are selectable
   by name.
8. Offline suite, fmt, and clippy are clean.
9. Manual §6d/§8, `docs/LIVE_TESTS.md`, the parent plan, and the retrospective
   are updated.

---

## Open Questions

| # | Question | Position |
| :--- | :--- | :--- |
| Q-64 | **Presets for Ollama / llama.cpp / LM Studio?** | Not now. A preset only prefills the base URL; add it when the single field proves annoying. |
| Q-65 | **Discover models from `/models`?** | Not now. The user types the model; discovery is a nice-to-have. |
| Q-66 | **Should the context window be required?** | No. Unknown means no pre-flight check; the provider's error is still classified. |
| Q-67 | **Should `CAREER_OPS_LIVE` cover future live tests?** | Yes. It is the single gate; new live tests use `require_live`. |

---

## What Phase 3.5 Is Not

- Not a model downloader or runtime manager.
- Not a new HTTP client, transport, or `rig` fork.
- Not support for Anthropic, Gemini, or OpenAI accounts (they stay disabled).
- Not a prompt or evaluation-semantics change.
- Not PII redaction (Phase 3.6).
