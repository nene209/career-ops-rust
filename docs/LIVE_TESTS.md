# Live (Network) Tests

**Last reviewed:** 2026-11-12
**Why this file exists:** the default test suite must be offline, deterministic,
and fast. A few tests cannot be any of those things because their subject is the
outside world: a job board changed its JSON, a detail page is no longer served,
or the Obscura sidecar cannot render a PDF. This is the single index of those
tests, the gate that keeps them out of a normal run, and a standing review of
whether each one should use a mock instead.

Add a row here in the same change that adds or removes a live test.

---

## Rules for a live test

1. **It must not run by default.** Use `#[ignore = "live; set CAREER_OPS_LIVE=1
   and run with --ignored"]` and call
   `career_ops_test_support::require_live("test_name")` at the top. `CAREER_OPS_LIVE=1`
   is the only gate; a test still returns early when its own prerequisite is
   missing (`DEEPSEEK_API_KEY`, an Obscura binary).
2. **Assert the external contract, not our logic.** Live assertions are weak by
   design: the board answered, the payload still has the shape the adapter
   expects, a title and id are non-empty, a detail page still yields text.
   Assert pagination arithmetic, dedup, selectors, and normalization with
   fixtures instead. A live test that asserts counts or specific titles fails
   whenever a company opens or closes a role, which is noise.
3. **Never write the user's data directory.** Build a throwaway vault, as the
   app-level live tests do with `tempfile::tempdir()`.
4. **A WAF challenge is a skip, not a failure.** Waymo answers repeated traffic
   with `202` and an empty body. The test must say it skipped and why; a real
   extraction regression must still fail.
5. **Don't mock what only the network can prove.** A fixture can prove the
   parser handles a saved response; it cannot prove the provider still sends
   that response. Keep one weak live test per external contract.
6. **Let the app's log carry the diagnostics.** The app installs the only
   `tracing` subscriber, so a live test cannot install a second one. Use the
   `LogsOnFailure` guard (see `app.rs`): it prints
   `logging::handle().snapshot()` while the test unwinds, and `cargo test` shows
   that output exactly when the test fails.

---

## Inventory

| Test | File | Gate | Target | What it proves | Should it be mocked? |
| :--- | :--- | :--- | :--- | :--- | :--- |
| `live_fetch_reaches_the_pipeline` | `crates/app/src/app.rs` | `#[ignore]` + `CAREER_OPS_LIVE=1` | `LIVE_PORTAL_URLS` by default (Ashby Linear, Lever Spotify, Greenhouse Figma, SmartRecruiters Ubisoft) or `CAREER_OPS_LIVE_PORTALS` | The full app path: create portals through the service, fetch with the production HTTP client, store, batch progress, health badges, and the Pipeline list holding every stored job (Phase 3.3 removed the page cap). | **Keep live.** It is the only end-to-end check of transport + orchestration. Parsing is already fixture-covered. |
| `live_fetch_with_pagination_and_details` | `crates/app/src/app.rs` | `#[ignore]` + `CAREER_OPS_LIVE=1` | `CAREER_OPS_LIVE_PAGED_PORTAL` (default Waymo search) | Page 2 is still reachable and fetched by the cursor; a real posting's detail page still yields a description; WAF challenges skip. | **Keep live but weak.** Pagination arithmetic and detail extraction have offline tests. This one proves the live page still has the shape those tests assume. |
| `live_registry_sites_reach_the_pipeline` | `crates/app/src/app.rs` | `#[ignore]` + `CAREER_OPS_LIVE=1` | Every entry in `acquisition::known_sites`: Waymo, NVIDIA, Figma (Greenhouse), Spotify (Lever), Linear (Ashby), Ubisoft (SmartRecruiters) | Each embedded config still reaches the Pipeline against its live site; a challenged entry is skipped without aborting the others. | **Keep live.** The registry's config plumbing is offline-tested; this is its only proof that the configs still work against the sites. |
| `greenhouse_live`, `lever_live`, `ashby_live`, `smartrecruiters_live`, `smartrecruiters_detail_live`, `eightfold_live` | `crates/core/tests/ats_live.rs` | `#[ignore]` + `CAREER_OPS_LIVE=1` | One default board per ATS, overridable per adapter (`LIVE_*_SLUG`, `LIVE_EIGHTFOLD_*`) | The upstream APIs still return the shapes the adapters parse, and the detail endpoints still carry text. | **Keep live.** Fixtures cannot detect an API change. The board-detection helper in the same file is offline and runs by default. |
| `obscura_returns_post_js_dom` | `crates/core/tests/obscura_spike.rs` | `#[ignore]` + `CAREER_OPS_LIVE=1`; binary availability (`vendor/obscura` or `OBSCURA_BIN`) | `https://github.com/nene209/career-ops-rust` | The sidecar starts, CDP drives a page, post-JS DOM exists, and killing the sidecar is detectable. | **Candidate to make local.** Serve a page with inline JavaScript from a temp dir instead of GitHub; it proves the same thing without an external dependency. Do that if this spike is ever promoted beyond a Phase 2 gate. |
| `obscura_print_to_pdf_if_supported` | `crates/core/tests/obscura_spike.rs` | `#[ignore]` + `CAREER_OPS_LIVE=1`; binary availability (the check was added in Phase 3.5 WS0) | The sidecar | Records whether `printToPDF` works (the Phase 2 assumption is that it does not). | **Keep live but weak.** The availability check now runs first, so a machine without the binary skips instead of failing. |
| `deepseek_accepts_the_configured_key` | `crates/core/tests/llm_live.rs` | `#[ignore]` + `CAREER_OPS_LIVE=1`; returns early without `DEEPSEEK_API_KEY` | `https://api.deepseek.com/user/balance` | The key is accepted and the provider is reachable. `verify()` asks for the balance, so a pass spends no tokens and produces no completion. | **Keep live.** It is the only check that a key and the endpoint still agree; Phase 3's structured-output calls will be cassette-backed. |
| `deepseek_scores_the_fixtures_in_their_bands` | `crates/core/tests/llm_eval_live.rs` | `#[ignore]` + `CAREER_OPS_LIVE=1`; returns early without `DEEPSEEK_API_KEY` | The curated Phase 3 fixtures (strong, weak, borderline) | The real model still lands each fixture in its documented score band, and the report stores. Also the cassette recorder when `CAREER_OPS_RECORD_CASSETTES=1`. | **Keep live.** The offline suite replays cassettes (`evaluation_fixtures.rs`); only the live run proves the model's judgment and keeps the recordings honest. |
| `deepseek_writes_a_usable_cover_letter` | `crates/core/tests/llm_eval_live.rs` | `#[ignore]` + `CAREER_OPS_LIVE=1`; returns early without `DEEPSEEK_API_KEY` | The strong fixture + a reference letter | The letter avoids the prompt's banned phrases and em dashes, and a model judge scores its semantic similarity against the reference at 0.7 or above. | **Keep live.** The banned-phrase check is also offline; the similarity judgment needs a model. |
| `deepseek_proposes_real_boards` | `crates/core/tests/suggest_live.rs` | `#[ignore]` + `CAREER_OPS_LIVE=1`; returns early without `DEEPSEEK_API_KEY` | DeepSeek, one suggestion call against a throwaway vault | The real model still returns the suggestion contract, and every derived URL sits on the engine's own host, so a reply cannot point the app at an arbitrary site. | **Keep live.** The offline suite uses a scripted backend; only the live run proves the prompt still elicits the contract. |
| `deepseek_triages_a_small_batch` | `crates/core/tests/triage_live.rs` | `#[ignore]` + `CAREER_OPS_LIVE=1`; returns early without `DEEPSEEK_API_KEY` | The four postings in `common::phase3::TRIAGE_SAMPLES` | The real model returns one decision per posting with a reason, the choices are valid, the queue and `triage_*` columns follow, and the call writes `task = 'triage'`. Also the cassette recorder when `CAREER_OPS_RECORD_CASSETTES=1`. | **Keep live.** The offline suite replays `tests/cassettes/triage.json`; only the live run proves the prompt still elicits the contract and keeps the recording honest. |

`live_probe`, `live_probe_challenged`, `registry_entry`, `live_portal_urls`,
`live_scraper_config`, and `live_portal_name` in `app.rs` are helpers for the
tests above, not tests of their own.

### Live tests that are not in the table

- `crates/core/tests/ats_adapters.rs` is **offline**: it parses saved fixtures
  and asserts the normalization. Its header mentions `reqwest` only to explain
  why the HTTP call is not there.
- `crates/core/tests/manual_entry.rs` uses `MockFetchSource`; the NVIDIA payload
  in it is a fixture.
- Everything in `crates/app/tests/*` is offline (`egui_kittest` plus temp vaults).

---

## Running them

The app-level tests need the keychain feature. `CAREER_OPS_LIVE=1` is the only
gate; `--ignored` selects the tests, and the flag proves the network is
intentional:

```bash
# Every live test whose prerequisite is present.
CAREER_OPS_LIVE=1 cargo test --workspace --features career-ops-app/keychain -- --ignored

# One test.
CAREER_OPS_LIVE=1 cargo test -p career-ops-app --features keychain -- --ignored \
    live_fetch_reaches_the_pipeline

# Overrides are not gates; they change what the test targets.
CAREER_OPS_LIVE=1 CAREER_OPS_LIVE_PORTALS=https://jobs.lever.co/spotify \
    cargo test -p career-ops-app --features keychain -- --ignored live_fetch
CAREER_OPS_LIVE=1 CAREER_OPS_LIVE_PAGED_PORTAL=https://careers.withwaymo.com/jobs/search \
    cargo test -p career-ops-app --features keychain -- --ignored \
    live_fetch_with_pagination_and_details

# The embedded registry, one command.
CAREER_OPS_LIVE=1 \
    cargo test -p career-ops-app --features keychain -- --ignored \
    live_registry_sites_reach_the_pipeline
```

The DeepSeek key check is also gated by the key itself:

```bash
CAREER_OPS_LIVE=1 DEEPSEEK_API_KEY=sk-... \
    cargo test -p career-ops-core --test llm_live -- --ignored --nocapture
```

The Phase 3 fixture checks spend tokens and run on demand. Add
`CAREER_OPS_RECORD_CASSETTES=1` to rewrite the offline cassettes (review the
diff before committing; see `docs/llm-testing.md`):

```bash
# Score the three fixtures and judge the cover letter.
CAREER_OPS_LIVE=1 DEEPSEEK_API_KEY=sk-... \
    cargo test -p career-ops-core --test llm_eval_live -- --ignored --nocapture

# One suggestion call, printing the proposals it accepted.
CAREER_OPS_LIVE=1 DEEPSEEK_API_KEY=sk-... \
    cargo test -p career-ops-core --test suggest_live -- --ignored --nocapture

# One triage batch, printing the four decisions it stored.
CAREER_OPS_LIVE=1 DEEPSEEK_API_KEY=sk-... \
    cargo test -p career-ops-core --test triage_live -- --ignored --nocapture
```

The triage cassette is the one live reply the offline suite replays. Re-record it
from a passing run (the four sample ids are `1..=4` on a fresh vault):

```bash
CAREER_OPS_LIVE=1 CAREER_OPS_RECORD_CASSETTES=1 DEEPSEEK_API_KEY=sk-... \
    cargo test -p career-ops-core --test triage_live -- --ignored --nocapture
```

Review the diff: the cassette is a fixture, and a changed decision is a changed
expectation. The suggestion call records no cassette; its assertion is about the
derived URL, which a replayed reply cannot prove.

```bash
# Re-record crates/core/tests/cassettes from a passing run.
CAREER_OPS_LIVE=1 CAREER_OPS_RECORD_CASSETTES=1 DEEPSEEK_API_KEY=sk-... \
    cargo test -p career-ops-core --test llm_eval_live -- --ignored --nocapture
```

**Usage ledger:** a live evaluation and letter also write `llm_usage` rows,
and so do triage and portal suggestions (`task = 'triage'` and
`task = 'suggest_portals'`). The ignored tests use a throwaway vault, so those
rows are not visible in the app; when a live evaluation is run through the app
(manual §6e), the **Usage** screen should show `source = reported` for DeepSeek.
A row that says `estimated` means the provider sent no usage block and the
four-characters-per-token fallback ran. What the ledger stores and what the cost
means are in `docs/usage.md`.

The ATS adapters are `#[ignore]`d like the rest; selecting one by name is the
usual way to run a single board:

```bash
CAREER_OPS_LIVE=1 cargo test -p career-ops-core --test ats_live -- --ignored --nocapture
CAREER_OPS_LIVE=1 LIVE_GREENHOUSE_SLUG=stripe \
    cargo test -p career-ops-core --test ats_live -- --ignored --nocapture greenhouse_live
```

The Obscura spike needs the browser binary:

```bash
CAREER_OPS_LIVE=1 OBSCURA_BIN=$PWD/vendor/obscura \
    cargo test -p career-ops-core --test obscura_spike -- --ignored --nocapture
```

### Registry run notes

The registry now covers six boards. A full run takes roughly two minutes:
Ubisoft's entry enriches 25 SmartRecruiters details (one request each), and
Waymo's 90 detail requests dominate when its WAF serves the run. A board page
that redirects is fine: the probe follows redirects, so Figma's
`job-boards.greenhouse.io/figma` reaches `figma.com/careers` while the adapter
fetches `boards-api.greenhouse.io`. Run this check from an unrestricted network
(or allow the redirect host through a proxy) so a redirect is not misread as a
challenge. A skipped entry keeps its previous `verified_at`; a skip is not a
date update.

### How the normal suite excludes them

`#[ignore]` excludes every live test from the default run. The standard command
is unchanged:

```bash
cargo test --workspace --features career-ops-app/keychain --offline -- --skip obscura
```

`--skip obscura` is now redundant for safety (the spike is `#[ignore]`d and checks
the binary), but it stays so the documented command and the habit agree.

---

## Standing mock review

The question this file answers is not "can this be mocked?" but "what would the
mock stop proving?"

- **No app-level live test should become a mock.** They exist to exercise the
  real transport and the real boards. The service layer they call is already
  tested with `MockFetchSource`.
- **No ATS live test should become a fixture test.** The fixture tests already
  exist and are stronger about parsing; these six tests are the only signal that
  an upstream payload changed.
- **The pagination live test is the closest to redundant.** If it becomes flaky
  for reasons other than a WAF (for example, the default board removes its
  next-page link), replace the board rather than loosening the assertion.
- **The Obscura spike is the only genuine mock candidate.** A local page with
  inline JavaScript keeps the post-JS assertion and removes the last external
  dependency from a Phase 2 gate.

When adding a live test, prefer a weak assertion and a skip over a strong
assertion that has to be edited every hiring season.
