# Known Sites

A **known site** is a profile, not an engine: a host, the engine that can fetch
it (a config-selected provider or a URL-selected ATS adapter), the payload that
engine needs, and the date its live check last passed. The entries are typed
constants in `crates/core/src/acquisition/known_sites.rs` and ship inside the
binary, so a wrong entry is fixed by a release rather than a migration.

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

| Board | Name | Engine | Verified | Caps | Notes |
| :--- | :--- | :--- | :--- | :--- | :--- |
| `https://careers.withwaymo.com/jobs/search` | Waymo | `selectors` | 2026-11-03 | `pagination.max_pages: 3`, `detail.max_jobs: 90` | Rails + Stimulus, server-rendered, no `JobPosting` JSON-LD. 30 cards per page, so 90 covers every posting on the three fetched pages. AWS WAF intermittently answers `202` with an empty body; a challenged detail keeps the card teaser. Detail pages do carry `JobPosting` JSON-LD. |
| `https://jobs.nvidia.com/careers` | NVIDIA | `eightfold` | 2026-11-03 | `pagination.max_pages` default (5), `detail.max_jobs` default (40) | Client-rendered Eightfold shell; postings come from `/api/pcsx/*`. The board reports ~2,750 positions and returns 10 per request, so the page cap is what bounds a fetch. The `pcsx` endpoints are not a public contract. Descriptions are stored as HTML. Some postings carry `postedTs: 0.0`; the parser accepts an integer, a float, or a numeric string, treats zero as no date, and skips a posting it still cannot read rather than failing the page. |
| `https://job-boards.greenhouse.io/figma` | Figma | `greenhouse` (ATS) | 2026-11-05 | none | Greenhouse board; the list carries descriptions. The board page redirects to `figma.com/careers`; the adapter fetches the Greenhouse API. Other Greenhouse boards work through **Other site**. Greenhouse's old host (`boards.greenhouse.io`) redirects here and is still detected. |
| `https://jobs.lever.co/spotify` | Spotify | `lever` (ATS) | 2026-11-05 | none | Lever board; the list carries descriptions. Other Lever boards work through **Other site**. |
| `https://jobs.ashbyhq.com/linear` | Linear | `ashby` (ATS) | 2026-11-05 | none | Ashby board; the list carries descriptions. Other Ashby boards work through **Other site**. |
| `https://jobs.smartrecruiters.com/Ubisoft2` | Ubisoft | `smartrecruiters` (ATS) | 2026-11-05 | `detail.max_jobs: 25` | SmartRecruiters carries no descriptions in its list; the first 25 postings are enriched, one request each. Other boards work through **Other site**. |

Caps are the embedded values, not universal defaults: `pagination.max_pages`
includes the first page (hard ceiling 20), and `detail.max_jobs` falls back to 40
(capped at 200).

## Enrichment is opt-in (Phase 2.8)

A fetch enriches descriptions only when the portal asks it to, through
`scraper_config.detail.enabled`. The rules, in order:

1. **A `detail` block without `enabled` keeps enriching.** Waymo's and
   Ubisoft's entries are this shape and were deliberately **not rewritten**: a
   stored portal keeps behaving exactly as it did, and the form shows the
   toggle on.
2. **`"enabled": false` turns a stored block off** without deleting its
   selectors, so a portal can be paused and resumed without losing `fields` or
   `max_jobs`.
3. **A config with no `detail` block does not enrich**, except for Eightfold
   boards (NVIDIA): their list carries no description, so the router fills that
   gap by default. The form shows the toggle on for them and writes
   `"enabled": false` when it is turned off.
4. **The form's toggle writes the field into the JSON.** It applies only while
   the JSON is unedited; after a hand edit it steps aside and points at
   `detail.enabled`, because two editable representations of one field would
   drift.

`pagination.max_pages` still bounds one **Refresh** or one **Continue**.
**Fetch all pages** repeats Continue up to its own 50-page per-click budget, and
stops early at the end of the list, at Stop, at a page the site refused, or when
a board cannot continue at all. A refused page records the pages that worked, so
the next attempt starts at the page that failed; the Portals screen then offers
**Skip page N and continue**, which moves the cursor onto N and resumes at N+1.
Skipping is always the user's decision — the page's postings stay unread.

### Adapter entries

An entry whose engine is an ATS adapter (`greenhouse`, `lever`, `ashby`,
`smartrecruiters`) is a **board**, not a platform. Its stored config is
`{"provider":"selectors"}` with no `list` or `fields`: the router detects the
adapter from the board URL, so there is nothing to scrape. The chooser and the
attribution line show the adapter name (`Figma (Greenhouse) - verified …`).

Three rules keep these entries honest, and all three are offline tests:

1. The config parses and names the same provider the engine stores.
2. The entry's `url()` is detected by the adapter whose name the engine carries.
3. The entry has no selector `list`, so it cannot fall back to scraping the
   board page as HTML.

The SmartRecruiters cap is an upper bound: a board with fewer than 25 postings
enriches all of them, and a larger board enriches the first 25. Raise the cap in
the portal form if a specific board needs more.

## Portal suggestions (Phase 3.2)

**Suggest portals** on the Portals screen asks the configured model for 5–10
companies whose careers page uses an engine the app can fetch. It is a
suggestion, never an automatic addition:

- The model returns a company, an engine, an optional role count, and one
  sentence. It never returns a URL. The app builds the URL from the engine and
  a board slug, and a slug that is not one safe path segment is refused.
- A derived URL that matches an entry in this registry keeps that entry's exact
  config (Ubisoft's detail cap, Figma's `{"provider":"selectors"}`).
- A board the registry does not know gets no config for an ATS engine, because
  the URL selects the adapter, and `{"provider":"eightfold","domain":…}` for
  Eightfold.
- **Nothing is added until the user clicks Approve and add.** Skip leaves the
  portal list untouched, and **Fetch after adding** is off by default.
- Approving a board that is already a portal is refused, so the existing row
  and its continuation cursor are kept.
- The `{{known_sites}}` block in the prompt is generated from this registry and
  lists only the engines a slug can address. Waymo's selector config is not
  something the model can propose.

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
CAREER_OPS_LIVE=1 \
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
   server-rendered HTML, `eightfold` for the `pcsx` API, or an ATS adapter for a
   recognised board host).
2. Add the `KnownSite` constant with the embedded config and `verified_at: None`.
   For an ATS board use `SiteEngine::Ats("greenhouse")` (and so on) with
   `{"provider":"selectors"}`; the URL selects the adapter.
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
