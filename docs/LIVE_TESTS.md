# Live (Network) Tests

**Last reviewed:** 2026-11-06
**Why this file exists:** the default test suite must be offline, deterministic,
and fast. A few tests cannot be any of those things because their subject is the
outside world: a job board changed its JSON, a detail page is no longer served,
or the Obscura sidecar cannot render a PDF. This is the single index of those
tests, the gate that keeps them out of a normal run, and a standing review of
whether each one should use a mock instead.

Add a row here in the same change that adds or removes a live test.

---

## Rules for a live test

1. **It must not run by default.** Use `#[ignore = "hits the network; run with
   --ignored"]`. If the test is slow or redundant enough that a blanket
   `--ignored` run should not start it, add a second environment-variable gate
   (as the registry test does).
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
| `live_fetch_reaches_the_pipeline` | `crates/app/src/app.rs` | `#[ignore]` | `LIVE_PORTAL_URLS` by default (Ashby Linear, Lever Spotify, Greenhouse Figma, SmartRecruiters Ubisoft) or `CAREER_OPS_LIVE_PORTALS` | The full app path: create portals through the service, fetch with the production HTTP client, store, batch progress, health badges, and the Pipeline list. | **Keep live.** It is the only end-to-end check of transport + orchestration. Parsing is already fixture-covered. |
| `live_fetch_with_pagination_and_details` | `crates/app/src/app.rs` | `#[ignore]` | `CAREER_OPS_LIVE_PAGED_PORTAL` (default Waymo search) | Page 2 is still reachable and fetched by the cursor; a real posting's detail page still yields a description; WAF challenges skip. | **Keep live but weak.** Pagination arithmetic and detail extraction have offline tests. This one proves the live page still has the shape those tests assume. |
| `live_registry_sites_reach_the_pipeline` | `crates/app/src/app.rs` | `#[ignore]` **and** `CAREER_OPS_LIVE_REGISTRY=1` | Every entry in `acquisition::known_sites`: Waymo, NVIDIA, Figma (Greenhouse), Spotify (Lever), Linear (Ashby), Ubisoft (SmartRecruiters) | Each embedded config still reaches the Pipeline against its live site; a challenged entry is skipped without aborting the others. | **Keep live.** The registry's config plumbing is offline-tested; this is its only proof that the configs still work against the sites. |
| `greenhouse_live`, `lever_live`, `ashby_live`, `smartrecruiters_live`, `smartrecruiters_detail_live`, `eightfold_live` | `crates/core/tests/ats_live.rs` | `RUN_LIVE_ATS_TESTS=1`; the tests return early without it (they are not `#[ignore]`) | One default board per ATS, overridable per adapter (`LIVE_*_SLUG`, `LIVE_EIGHTFOLD_*`) | The upstream APIs still return the shapes the adapters parse, and the detail endpoints still carry text. | **Keep live.** Fixtures cannot detect an API change. The board-detection helper in the same file is offline and runs by default. |
| `obscura_returns_post_js_dom` | `crates/core/tests/obscura_spike.rs` | Binary availability (`vendor/obscura` or `OBSCURA_BIN`); returns early otherwise | `https://github.com/nene209/career-ops-rust` | The sidecar starts, CDP drives a page, post-JS DOM exists, and killing the sidecar is detectable. | **Candidate to make local.** Serve a page with inline JavaScript from a temp dir instead of GitHub; it proves the same thing without an external dependency. Do that if this spike is ever promoted beyond a Phase 2 gate. |
| `obscura_print_to_pdf_if_supported` | `crates/core/tests/obscura_spike.rs` | None — it does **not** call `obscura_available()` | The sidecar | Records whether `printToPDF` works (the Phase 2 assumption is that it does not). | **Fix the gate first.** It spawns the binary unconditionally, which is why the standard validation command skips the whole file with `--skip obscura`. Add the availability check before relying on `cargo test` without that skip. |
| `deepseek_accepts_the_configured_key` | `crates/core/tests/llm_live.rs` | `#[ignore]`; returns early without `DEEPSEEK_API_KEY` | `https://api.deepseek.com/user/balance` | The key is accepted and the provider is reachable. `verify()` asks for the balance, so a pass spends no tokens and produces no completion. | **Keep live.** It is the only check that a key and the endpoint still agree; Phase 3's structured-output calls will be cassette-backed. |
| `deepseek_scores_the_fixtures_in_their_bands` | `crates/core/tests/llm_eval_live.rs` | `#[ignore]`; returns early without `DEEPSEEK_API_KEY` | The curated Phase 3 fixtures (strong, weak, borderline) | The real model still lands each fixture in its documented score band, and the report stores. Also the cassette recorder when `CAREER_OPS_RECORD_CASSETTES=1`. | **Keep live.** The offline suite replays cassettes (`evaluation_fixtures.rs`); only the live run proves the model's judgment and keeps the recordings honest. |
| `deepseek_writes_a_usable_cover_letter` | `crates/core/tests/llm_eval_live.rs` | `#[ignore]`; returns early without `DEEPSEEK_API_KEY` | The strong fixture + a reference letter | The letter avoids the prompt's banned phrases and em dashes, and a model judge scores its semantic similarity against the reference at 0.7 or above. | **Keep live.** The banned-phrase check is also offline; the similarity judgment needs a model. |

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

The app-level tests need the keychain feature:

```bash
# Every ignored app live test (skips the registry test on its own second gate).
cargo test -p career-ops-app --features keychain -- --ignored

# One test.
cargo test -p career-ops-app --features keychain -- --ignored \
    live_fetch_reaches_the_pipeline

# A different board or the Waymo pagination run.
CAREER_OPS_LIVE_PORTALS=https://jobs.lever.co/spotify \
    cargo test -p career-ops-app --features keychain -- --ignored live_fetch
CAREER_OPS_LIVE_PAGED_PORTAL=https://careers.withwaymo.com/jobs/search \
    cargo test -p career-ops-app --features keychain -- --ignored \
    live_fetch_with_pagination_and_details

# The embedded registry, one entry at a time.
CAREER_OPS_LIVE_REGISTRY=1 \
    cargo test -p career-ops-app --features keychain -- --ignored \
    live_registry_sites_reach_the_pipeline
```

The DeepSeek key check is `#[ignore]`d and returns early when the key is not
set, so `--ignored` can be used for every live test at once:

```bash
DEEPSEEK_API_KEY=sk-... \
    cargo test -p career-ops-core --test llm_live -- --ignored --nocapture
```

The Phase 3 fixture checks spend tokens and run on demand. The same command
with `CAREER_OPS_RECORD_CASSETTES=1` rewrites the offline cassettes (review the
diff before committing; see `docs/llm-testing.md`):

```bash
# Score the three fixtures and judge the cover letter.
DEEPSEEK_API_KEY=sk-... \
    cargo test -p career-ops-core --test llm_eval_live -- --ignored --nocapture

# Re-record crates/core/tests/cassettes from a passing run.
CAREER_OPS_RECORD_CASSETTES=1 DEEPSEEK_API_KEY=sk-... \
    cargo test -p career-ops-core --test llm_eval_live -- --ignored --nocapture
```

The ATS adapters are gated by an environment variable rather than `#[ignore]`,
so they run as normal tests that print "skipping" and return:

```bash
RUN_LIVE_ATS_TESTS=1 \
    cargo test -p career-ops-core --test ats_live -- --nocapture
```

The Obscura spike needs the browser binary:

```bash
OBSCURA_BIN=$PWD/vendor/obscura \
    cargo test -p career-ops-core --test obscura_spike -- --nocapture
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

`cargo test --workspace --features career-ops-app/keychain --offline -- --skip
obscura` is the standard validation command. It excludes the app live tests,
the DeepSeek key and fixture checks, by `#[ignore]`; the ATS tests by their
environment gate; and the spike by name. Dropping `--skip obscura` on a machine
with the binary runs the spike too.

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
