# LLM testing: fixtures, cassettes, and live checks

Phase 3 has three layers of coverage, with different jobs:

| Layer | Where | Runs by default? | What it proves |
| :--- | :--- | :--- | :--- |
| Pipeline tests | `crates/core/tests/evaluation.rs`, `cover_letter.rs` | Yes | Assembly, validation, storage, status, cancellation — with a fake backend. |
| Fixture replay | `crates/core/tests/evaluation_fixtures.rs` | Yes | The whole pipeline on three curated postings, using recorded provider replies. The output contract is stable. |
| Live checks | `crates/core/tests/llm_eval_live.rs` | No (`#[ignore]`, `DEEPSEEK_API_KEY`) | The real model still lands the fixtures in their bands and writes a usable letter. |

## Fixtures

`crates/core/tests/fixtures/phase3/` holds the candidate summary and three
postings:

| Fixture | Posting | Expected score | Expected recommendation |
| :--- | :--- | :--- | :--- |
| `strong` | Senior Rust / desktop / SQLite / egui, matching the CV | 0.80–1.00 | `strong_match` |
| `weak` | Brand marketing manager in New York | 0.00–0.30 | `no_match` |
| `borderline` | Rust backend with Kubernetes, gRPC, and on-call | 0.40–0.60 | `no_match` (below the apply threshold) |

The set is deliberately small. A large fixture set with ambiguous expectations
is worse than three with clear ones.

## Cassettes

`crates/core/tests/cassettes/` holds one JSON file per fixture:

```json
{
  "fixture": "strong",
  "model": "deepseek-flash",
  "synthetic": true,
  "recorded_at": "2026-11-07",
  "note": "Hand-authored placeholder ...",
  "reply": { "match_score": 0.9, "recommendation": "strong_match", ... }
}
```

`common::phase3::CassetteBackend` replays the reply and records the prompt it was
given. The fixture test asserts the posting reached the prompt fenced as
untrusted data, that the reply parsed and stored, and that the score sits in the
fixture's band.

**The shipped cassettes are `synthetic`.** They are hand-authored so the default
suite has a complete, deterministic path today. They exercise the parser,
storage, lifecycle, and prompt assembly, but they are *not* evidence about the
model's judgment. Re-record them to make them real:

```bash
CAREER_OPS_RECORD_CASSETTES=1 DEEPSEEK_API_KEY=sk-... \
    cargo test -p career-ops-core --test llm_eval_live -- --ignored --nocapture
```

A passing live run rewrites `crates/core/tests/cassettes/` with
`synthetic: false`. Review the diff before committing: a cassette is a fixture,
and a changed reply is a changed expectation.

**When to re-record:**

- the compact prompts change;
- the model name changes (the provider's output can change even when the name
  does not, so re-record deliberately);
- a normalizer or the fence changes;
- the fixture postings or CV change.

The offline test fails loudly if the reply no longer parses, the band is missed,
or the prompt no longer contains the fixture's distinctive phrase.

## Live checks

`llm_eval_live.rs` contains two `#[ignore]`d tests and returns early when
`DEEPSEEK_API_KEY` is not set:

- `deepseek_scores_the_fixtures_in_their_bands`: one evaluation per fixture,
  asserting the documented bands and printing the scores. One sample per fixture
  is a low bar; the bands are wide and the purpose is to catch a regression, not
  to measure variance. If a band becomes flaky, re-review the fixture rather than
  loosening it silently.
- `deepseek_writes_a_usable_cover_letter`: generates a letter, checks the
  prompt's banned phrases and the em-dash rule, and asks the model for a semantic
  similarity score against `REFERENCE_LETTER` (threshold 0.7). It prints the
  score so drift is visible.

`docs/LIVE_TESTS.md` indexes those with the repository's single gate convention.

## Local and custom endpoints

Any OpenAI-compatible endpoint is a supported backend (Phase 3.5): Ollama,
`llama-server`, vLLM, LM Studio, or a gateway. Two things are worth knowing when
testing one.

- **The request shape is not the OpenAI default.** The client forces
  `Route::Chat` (`/chat/completions`) because the `OPENAI` dialect otherwise
  defaults to the Responses API, which local servers do not implement. It also
  omits `Authorization` when the key is empty (`Auth::OptionalBearer`). Both are
  pinned offline by `crates/core/tests/custom_endpoint.rs`, which runs a fake
  server on a loopback port.
- **JSON mode is a setting, not a capability.** With it on, the request carries
  `response_format: json_object`; with it off (`StructuredOutput::PromptOnly`),
  the prompt asks for JSON and the reply is parsed and repaired once, exactly as
  a DeepSeek reply is. A local model that rejects `response_format` is therefore
  usable, not refused.

A local call is never priced: the catalog reference is scoped to
`custom/<model>`, so the usage ledger stores `NULL` and **Usage** shows a dash.
The live check is `custom_endpoint_answers_a_prompt` in `custom_live.rs`; it
needs `CAREER_OPS_LOCAL_LLM_URL` and `CAREER_OPS_LOCAL_LLM_MODEL`, and the key is
optional.

## Why not `rig::evals`?

The Phase 3 plan named `rig::evals` (`LlmScoreMetric`, `SemanticSimilarityMetric`)
as the prompt-quality tool. `rig` 0.46 does not ship an `evals` module and has no
`experimental` feature, so that API no longer exists at the pinned version. The
fixture-and-cassette suite plus the live judge is the substitute: it is smaller,
stays inside the app's own `CompletionBackend` seam, and does not add a second
test framework. If a later `rig` reintroduces evals, the fixtures are the inputs
it would need.
