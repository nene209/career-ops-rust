# Known Sites

A **known site** is a profile, not an engine: a host, the provider engine that
can fetch it, the payload that engine needs, and the date its live check last
passed. The entries are typed constants in
`crates/core/src/acquisition/known_sites.rs` and ship inside the binary, so a
wrong entry is fixed by a release rather than a migration.

The registry is a convenience, never a gate. In the app:

- **Choosing a site.** **Add portal** opens a chooser listing every entry with
  its provider and verification date; **Other site** opens the blank form. The
  entries are sorted by name and their notes are on hover.
- **Attribution.** A URL that matches an entry shows
  `Known site: <name> (<provider>) - verified <date>` in the add and edit forms.
  An unverified entry says `- unverified`.
- **Editing and restoring.** The user can change anything. When the saved config
  differs from the entry, the form says `Modified from the known config.` and
  offers **Restore known config**, which rewrites the provider, domain, and
  config from the entry. Whitespace and key order are not edits.

The provider and config fields always remain editable, and hosts the registry
does not know use the same form with no attribution.

## Entries

| Board | Name | Provider | Verified | Caps | Notes |
| :--- | :--- | :--- | :--- | :--- | :--- |
| `https://careers.withwaymo.com/jobs/search` | Waymo | `selectors` | 2026-11-03 | `pagination.max_pages: 3`, `detail.max_jobs: 90` | Rails + Stimulus, server-rendered, no `JobPosting` JSON-LD. 30 cards per page, so 90 covers every posting on the three fetched pages. AWS WAF intermittently answers `202` with an empty body; a challenged detail keeps the card teaser. Detail pages do carry `JobPosting` JSON-LD. |
| `https://jobs.nvidia.com/careers` | NVIDIA | `eightfold` | 2026-11-03 | `pagination.max_pages` default (5), `detail.max_jobs` default (40) | Client-rendered Eightfold shell; postings come from `/api/pcsx/*`. The board reports ~2,750 positions and returns 10 per request, so the page cap is what bounds a fetch. The `pcsx` endpoints are not a public contract. Descriptions are stored as HTML. |

Caps are the embedded values, not universal defaults: `pagination.max_pages`
includes the first page (hard ceiling 20), and `detail.max_jobs` falls back to 40
(capped at 200). Eightfold descriptions are enabled by default because its list
carries none; the `detail` block only lowers the cap.

## What "verified" means

`verified_at` is the date the registry live check below passed for that entry.
It is committed by hand after a passing run; the test never writes it.

- **A skip leaves the date untouched.** A WAF challenge, an unreachable page, or
  a transport failure is a site condition. The run reports that entry as skipped
  and moves on; the entry keeps its previous date, or `None` if it never passed.
- **A served page that yields no text fails.** If the entry's page answers
  normally but the fetch stores no job with non-empty text, the run fails: a
  challenge cannot hide an extraction regression.
- **`None` is a legitimate state.** The entry still appears in the form, labelled
  "unverified". It is not deleted because a site is having a bad day.

The sequencing behind those rules is unit-tested without a network:
`a_challenged_registry_page_is_skipped_without_fetching`,
`a_served_registry_page_is_fetched_once_and_not_probed_again`,
`a_failed_registry_fetch_on_a_served_page_is_a_failure`, and
`a_failed_registry_fetch_on_a_challenged_retry_is_skipped`. The live check calls
the same sequencer, so changing a rule fails a fast offline test before it can
silently change what a live run reports.

## Running the check

```bash
CAREER_OPS_LIVE_REGISTRY=1 \
  cargo test -p career-ops-app --features keychain -- --ignored \
  live_registry_sites_reach_the_pipeline
```

The test creates its own throwaway vault, so it never reads or writes your data
directory. Each entry is probed, created, fetched individually with a 180 s
bound, and then checked in storage: at least one job with non-empty text, and
every `source_url` absolute. Requests are paced as usual (see README, "Request
pacing"); `RUST_LOG=info` shows the per-portal router lines.

Read the final line it prints — `live registry: verified [...]; skipped [...]` —
and update `verified_at` (and this table) for the verified entries only.

## Curation rules

1. An entry ships only with a passing live run. No date, no row.
2. An entry that fails is fixed or removed in the same commit that bumps the
   release. Do not leave a red entry behind for later.
3. A skipped run is not a pass. Do not copy a date from an earlier run.
4. Keep the embedded config minimal. A cap belongs in the entry only when the
   site needs it; every extra cap risks a silent truncation.
5. No site-specific code in the fetch path. If an entry needs code, it needs a
   provider, and a provider needs its own justification and fixtures. A provider
   is an engine; a known site is data.

## Onboarding a site

Both cases end with a `KnownSite` constant, a docs row, and a passing registry
run; only the first step differs.

**An existing engine can fetch it** (the Waymo case):

1. Probe the board and confirm which engine reads it (`selectors` for
   server-rendered HTML, `eightfold` for the `pcsx` API).
2. Add the `KnownSite` constant with the embedded config and `verified_at: None`.
3. Add or refresh a fixture for its payload, so a parser change fails a test
   instead of silently returning zero jobs.
4. Add the docs row above with the caps and any block or rate limit observed.
5. Run the registry check, then commit `verified_at` and the date in this table.
   The entry appears in the add-flow chooser automatically; there is no separate
   UI registration.

**No engine can** (the NVIDIA case):

1. Add the provider module: endpoints, parsing, fixtures, and a `FetchSource`
   method. It must be reusable by every site on that platform.
2. Dispatch it from the router by the stored `provider`, with no per-host branch.
3. Continue from step 2 above: `KnownSite` constant, fixture, docs row, live run.

A change to a provider or a `KnownSite` constant should also update
`crates/core/src/acquisition/known_sites.rs` unit tests, which assert that every
entry parses and that its stored JSON names the same provider as the constant.
