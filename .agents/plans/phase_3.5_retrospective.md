# Phase 3.5 — Local / Custom LLM Provider: Retrospective

**Date:** 2026-11-13
**Plan:** `.agents/plans/phase_3.5_local_custom_llm_provider.md` (v0.5)
**Outcome:** all four workstreams implemented; fmt, clippy, the offline suite, and
the new loopback-endpoint tests are green. The live check and manual §6g are the
owner's.

---

## What shipped

- **One live gate (WS0).** `career_ops_test_support::live_enabled()` and
  `require_live()` read only `CAREER_OPS_LIVE=1`. `RUN_LIVE_ATS_TESTS` and
  `CAREER_OPS_LIVE_REGISTRY` are gone; `ats_live` and `obscura_spike` joined the
  other live tests under `#[ignore]`, so `--ignored` alone can no longer reach
  the network. `obscura_print_to_pdf_if_supported` gained the availability check
  it had always been missing. The `live-ats` workflow moved to the new flag **and**
  gained `--ignored`.
- **Configuration (WS1).** `LlmProvider::Custom` (stored `custom`) with
  `requires_api_key()` false and no default model; `base_url`, `json_mode`, and
  `custom_context_window` under three new `settings` keys; `sanitized()` trims
  the URL; `validate_custom()` returns a typed, field-named `ConfigProblem`;
  `custom_warning()` flags a non-loopback plain-http endpoint without refusing
  it; `switched_to()` owns the model-reset rule.
- **Client and capabilities (WS2).** `build_client` builds from `sanitized()`,
  refuses an invalid custom config (`LlmError::InvalidConfiguration`), and
  constructs the custom client with `with_base_url`, `Auth::OptionalBearer`, and
  **`Route::Chat`**. Capabilities became `LlmConfig::capabilities()`, and
  `StructuredOutput::PromptOnly` is usable because the prompt carries the contract
  and the reply is validated locally. `sends_json_object()` gates
  `response_format`, so the field never reaches a server that rejects it. The five
  key call sites collapsed to `get_api_key(..)?.unwrap_or_default()`.
- **Settings, tests, and docs (WS3).** The AI section renders Base URL, the
  JSON-mode checkbox, and the context-window field only for the custom provider,
  names the endpoint in the data-sharing note, shows the warning and the field
  error inline, and enables **Test connection** without a key once the endpoint
  validates. `crates/core/tests/custom_endpoint.rs` answers `/models` and
  `/chat/completions` from a loopback listener, so the route, the optional
  `Authorization` header, the `response_format` gate, a 401, and a refused
  connection are all pinned offline. `custom_live.rs` adds the one live check.
  Manual §6g, `docs/LIVE_TESTS.md`, and `docs/llm-testing.md` describe all of it.

## What held up

- **One provider, not one per engine.** Ollama, `llama-server`, vLLM, LM Studio,
  and gateways really do share the wire format; the only differences the app
  exposes are a URL, a model name, and two capability flags.
- **The plan's `Route::Chat` warning was the whole ballgame.** Using the `OPENAI`
  dialect unmodified would have sent every local call to `/responses`, which none
  of these servers serve. The loopback test fails if that line is removed.
- **Making the key optional in one place.** Collapsing the five call sites to
  `unwrap_or_default()` removed a class of bug (one site forgetting the new rule)
  instead of adding five conditionals.
- **The fake server.** An in-process listener is cheap, deterministic, and shows
  things a `CompletionBackend` fake cannot. It caught nothing, which is the point:
  it now guards the three decisions that are invisible above the transport.

## Deviations, and why

1. **`custom_warning()` instead of refusing non-loopback http.** The plan said
   both "http only for loopback" and "warn for other plain-http hosts". Refusing
   would have blocked a home-lab server the user is deliberately reaching, so the
   loopback rule is a warning, not an error. The message names the host and the
   risk; **Test connection** stays enabled.
2. **`LlmError::InvalidConfiguration` is new.** Without it, an absent base URL
   would fall back to the dialect's own host — OpenAI's — and send the CV there.
   Refusing before a call is the only acceptable behavior, and the error is typed
   so `short_reason` can point at Settings.
3. **Custom was selectable one workstream before its fields existed** (WS2). That
   was visible: choosing it produced "enter the endpoint's base URL" with nowhere
   to type it until WS3. The ordering was the plan's; splitting it differently
   would have meant either a UI with no client or a client with no way to
   configure it.
4. **`capabilities(provider, model)` was removed**, not overloaded. A custom
   endpoint's answer comes from settings a pair cannot express; keeping both
   would have left a second, wrong answer available.
5. **The live test requires the model.** A default (`llama3.2`) would turn "you
   picked a different model" into a 404 that reads like a bug. Skipping with a
   message is honest; the product also refuses to invent a model name.

## Test strategy as built

| Layer | What it pins |
| :--- | :--- |
| Unit (`config.rs`, `capabilities.rs`, `usage.rs`) | provider parse/label, optional key, every URL case, the warning, the context-window bound, the switch rule, `PromptOnly`, the `response_format` gate, `custom` cost staying `NULL` |
| Core integration (`evaluation.rs`) | a prompt-only endpoint is not refused; a custom run stores `provider = custom` and no cost |
| Core integration (`custom_endpoint.rs`) | `POST /v1/chat/completions`, no `Authorization` for an empty key, `Bearer` when set, `response_format` only with JSON mode, `GET /v1/models` for verification, 401 mapping, refused connection |
| App (`settings_screen.rs`, `view_models/settings.rs`) | the three custom fields, dirty tracking, JSON-mode toggle, parsed context window, warning, inline error, Test connection gating, provider switching |
| Live (`custom_live.rs`) | a real engine answers the prompt and reports usage |
| Manual (§6g) | the same through the window, plus the settings copy |

## Follow-ups

- **Presets** (Q-64): a "fill in Ollama's URL" button is a small convenience; the
  single field has not proven annoying yet.
- **Model discovery** (Q-65): `/models` is already called for verification, so
  listing the models it returns is cheap. It is also a new state to design
  (a picker that can be stale, or a server with fifty models), so it waited.
- **Streaming on a local endpoint** is untested live. The client streams the same
  way for every provider, and the fake server answers one non-streamed request;
  a manual §6g run with a slow model would exercise it.
- **The live gate is worth revisiting** if a second kind of network test appears
  (for example a paid provider's structured-output check). One flag is right
  while every live test is "the outside world still agrees with us".

## Exit criteria

| Criterion | Status |
| :--- | :--- |
| 1. Local / custom selectable with URL, model, optional key, JSON mode, context window | Done (WS1–WS3) |
| 2. A keyless endpoint builds a client and completes an evaluation | Client and pipeline offline-tested; the live call is §6g.5 / `custom_live.rs` |
| 3. DeepSeek unchanged | Existing tests unchanged and green |
| 4. JSON mode off is usable | `PromptOnly` + evaluation pre-flight test |
| 5. Test connection explains a missing `/models` | `verify()` error surfaced; live test continues on a miss |
| 6. Usage shows `custom` and a `NULL` cost | Evaluation test + view-model label test |
| 7. One live gate | WS0; `docs/LIVE_TESTS.md` rewritten |
| 8. Offline suite, fmt, clippy clean | Verified |
| 9. Manual §6g, docs, parent plan, retrospective | Done |
