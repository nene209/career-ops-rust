# Manual Test Script — Phase 2.1 (UI Shell)

**Owner:** whoever is about to declare Phase 2.1 done, or is about to start Phase 3.
**Last updated:** 2026-10-25
**Applies to:** the `career-ops` binary built from this workspace.

Automated tests cover the state machine, the view models, and the screens with
`egui_kittest`. They cannot cover four things: the real window, the real OS
keychain, the filesystem recovery actions, and network fetches. This script is
the check for those.

Steps marked **🌐** need network access and a real job board; skip them only if
you are re-running the script to check a UI-only change.

---

## 0. Setup

1. Build:

   ```bash
   cargo build -p career-ops-app
   ```

2. Never run section 1 or 3 against your real data directory — they create and
   delete vaults. Use a disposable directory:

   ```bash
   export CAREER_OPS_DATA_DIR=/tmp/career-ops-manual
   mkdir -p /tmp/career-ops-manual
   ```

   An empty `CAREER_OPS_DATA_DIR` is what "clean machine" means for this script.
   Unset it when you are done to return to the OS data directory.

3. For Tier 2 checks, `vendor/obscura` must exist (see README, "Vendored Obscura
   binary"). Without it, a Tier 2 fetch fails with a sidecar error — which is
   itself worth seeing once, but it is not a pass for step 4.3.

4. Start the app from the workspace root: `./target/debug/career-ops`.

---

## 1. First launch (empty data directory)

| # | Step | Expected |
| :-- | :--- | :--- |
| 1.1 | Launch with an empty data directory. | Window titled "Career-Ops" opens. A "Set up Career-Ops" window shows "Step 1 of 3 — Welcome". |
| 1.2 | Click **Continue**. | "Step 2 of 3 — Create a passphrase". Both passphrase fields are empty; **Continue** is disabled. |
| 1.3 | Type a passphrase of 3 characters. | A "Strength:" line appears and says weak. It is feedback only; **Continue** stays disabled. |
| 1.4 | Type a different confirmation. | "The passphrases do not match." in red. |
| 1.5 | Fix the confirmation, leave "Acknowledgement" empty. | **Continue** still disabled. The acknowledgement phrase is shown above the field. |
| 1.6 | Type the acknowledgement phrase exactly as shown. | **Continue** enables. |
| 1.7 | Click **Continue**. | "Step 3 of 3 — Back up your data" shows the two file paths under the data directory. |
| 1.8 | Click **Open data folder**. | The file manager opens the data directory (empty at this point). |
| 1.9 | Click **Create vault**. | A spinner shows briefly, then the main layout: sidebar on the left with Pipeline, Portals, CV / Profile, Prompts, Settings. Pipeline is selected. |
| 1.10 | Inspect the data directory. | `career-ops.db` and `dek.wrapped.json` both exist. |
| 1.11 | Quit and relaunch. | The window opens to "Unlock Career-Ops", not the wizard. |

## 2. Unlock (returning user)

| # | Step | Expected |
| :-- | :--- | :--- |
| 2.1 | Click **Try keychain**. | Reaches the main layout. The keychain wrap is used, no passphrase needed. |
| 2.2 | Quit, relaunch, type a wrong passphrase, click **Unlock**. | "Recovery required" with a reason that mentions the passphrase. The main layout is not reachable. |
| 2.3 | In "1. Unlock with your passphrase", type the correct passphrase, click **Unlock with passphrase**. | Reaches the main layout. |

If 2.1 reports "Keychain unavailable", the OS keychain is locked or the entry was
removed; 2.3 is the fallback and must still work. Record which one happened.

## 3. Recovery

Run each sub-section on its own disposable data directory unless stated
otherwise.

### 3.1 Missing key file

| # | Step | Expected |
| :-- | :--- | :--- |
| 3.1.1 | With the app quit, move `dek.wrapped.json` out of the data directory. | The directory still contains `career-ops.db`. |
| 3.1.2 | Relaunch. | "Recovery required", reason: database exists but the wrapped key is missing. |
| 3.1.3 | In "4. Export the encrypted database", enter an empty folder and click **Export**. | Red error: both files must exist. (Export requires the key file.) |
| 3.1.4 | In "3. Locate your key file", enter the full path of the file you moved and click **Use this key file**. | The "Unlock Career-Ops" screen appears. |
| 3.1.5 | Unlock with the passphrase. | Main layout. |

### 3.2 Corrupt the wrapped key (and export from Recovery)

| # | Step | Expected |
| :-- | :--- | :--- |
| 3.2.1 | With the app quit, replace `dek.wrapped.json` with `{}`. Relaunch. | "Recovery required"; the reason says the key cannot be read. |
| 3.2.2 | In "4. Export the encrypted database", enter an empty folder and click **Export**. | Green "Exported to …"; both files are copied, including the unreadable key. Export is a byte copy, not a validation. |
| 3.2.3 | Click **Export** again into the same folder. | Red error: the destination file already exists. |
| 3.2.4 | Restore the real key file, relaunch, unlock with the passphrase. | Main layout. The exported copy is not needed for this. |

### 3.3 Start fresh (destructive — disposable directory only)

| # | Step | Expected |
| :-- | :--- | :--- |
| 3.3.1 | Reach Recovery (3.2.1 or 2.2). | Five numbered options are visible. |
| 3.3.2 | Type `delete my data` (wrong case) in "5. Start fresh". | **Delete my data and start fresh** stays disabled. |
| 3.3.3 | Type `DELETE MY DATA` exactly and click the button. | The first-launch wizard appears; the data directory no longer contains `career-ops.db` or `dek.wrapped.json`. |

## 4. Portals

| # | Step | Expected |
| :-- | :--- | :--- |
| 4.1 | Select **Portals** in the sidebar. | Empty state "No portals configured yet" with **Add your first portal**; header shows "Sidecar: Not started". |
| 4.2 | Click **Add portal** and enter `not a url` in the URL field. | "Enter a valid http(s) URL." appears under the field and **Save portal** is disabled. |
| 4.2a | Close the dialog and click **Add portal**. | A chooser lists every known site with its provider and verification date (NVIDIA before Waymo, alphabetically), each with an explanatory hover text, plus **Other site** and **Cancel**. Nothing is typed to see them. |
| 4.3 | Choose **Other site**, enter a company name and a real job-board URL, keep **Static (Tier 1)**, save. | The modal closes and the row appears with "Never fetched". |
| 4.3a | Click **Add portal** and choose **NVIDIA (Eightfold) - verified <date>** from the list. | The add form opens with the URL, company name, provider, and domain already filled, and "Known site: NVIDIA (Eightfold) - verified <date>" above the provider selector. It is the same form typing the URL produces. |
| 4.3b | For a career page with no JSON-LD that the registry does not know, paste its selector config into **Scraper config (JSON, optional)** before saving. | Invalid JSON shows "Scraper config is not valid JSON." under the field and disables **Save portal**. With valid JSON the portal saves, `Fetch` returns jobs, and the row becomes Healthy with tier HTML. |
| 4.3b.1 | Give the config an unknown `provider` (for example `{"provider":"workday"}`) and fetch. | The form accepts valid JSON, but `Fetch` fails as a configuration error ("The portal configuration is invalid. Check the URL and scraper config.") with no jobs stored. A config with no `provider` still uses selectors. |
| 4.3b.2 | For Waymo, type `https://careers.withwaymo.com/jobs/search` in the URL field. | "Known site: Waymo (Selectors) - verified <date>" appears, the company name and the selector config are filled in, and **Save portal** enables. Editing the config shows "Modified from the known config." and a **Restore known config** button; editing the URL away and back does not overwrite the edit. Restoring rewrites the entry and clears the marker. |
| 4.3b.3 | For NVIDIA, type `https://jobs.nvidia.com/careers` in the URL field. 🌐 | "Known site: NVIDIA (Eightfold) - verified <date>" appears, **Provider** shows Eightfold, the domain is `nvidia.com`, and the advanced-options field stays empty. `Fetch` stores positions from `/api/pcsx/search` with descriptions from `/api/pcsx/position_details`; no selector config is needed. The rows are recorded with the ATS tier, because Eightfold is a structured provider API. |
| 4.3b.4 | For a site the registry does not know, change **Provider** to Eightfold, enter a domain, and clear it again. | "Domain cannot be empty." appears and **Save portal** and **Test config** are disabled until a domain is entered. |
| 4.3c | In the add/edit dialog, click **Test config** with the URL and config filled in. 🌐 | A panel reports "Found N jobs on page 1; the saved config follows up to M pages.", up to three titles with their resolved URLs, and the first description length (or "No detail enrichment configured."). Nothing is saved and the Pipeline count does not change. On an ATS board the panel says "the adapter paginates internally" instead of a page count. |
| 4.3d | Edit the URL or config after a preview, or enter invalid JSON. | The previous report disappears when the inputs change; invalid JSON disables **Test config** and **Save portal** so a bad config never reaches the network or the database. |
| 4.4 | Click **Fetch** on the row. 🌐 | The health badge is replaced by a spinner. Afterwards: "Healthy" and "just now", or "Broken" with a cause and a suggested action line. |
| 4.5 | Click **Edit**, change the company name, save. | The row shows the new name; health is unchanged. |
| 4.6 | Add a second portal configured for **Browser (Tier 2)**, then **Fetch all**. 🌐 | A progress bar counts `n / N`; each row resolves to Healthy or Broken; the sidecar badge ends as "Running" or "Stopped". |
| 4.7 | Click **Fetch all** again and press **Cancel** while it runs. 🌐 | Progress stops within one portal. "Fetch cancelled after X of Y portals." Dismissing the bar clears it. |
| 4.8 | Click **Delete** on a row. | A confirmation names the portal and states that fetched jobs are kept. **Delete portal** removes only that row. |

## 5. Manual entry

| # | Step | Expected |
| :-- | :--- | :--- |
| 5.1 | From **Portals**, click **Add job**. | The "Add a job" dialog opens with From URL / Paste text. |
| 5.2 | **From URL**, paste a real posting URL, click **Fetch and add**. 🌐 | "Job added" with title, company, "Added via …", and warnings if extraction was partial. |
| 5.3 | Click **View in Pipeline**. | The dialog closes and Pipeline is selected. |
| 5.4 | Back on **Portals**, **Add job** → **Paste text**: title, company, body. | **Add pasted job** is disabled until title and body are non-empty. |
| 5.5 | Submit. | "Job added", "Added manually." |
| 5.6 | Select **Pipeline**, click **Add job** in the header. | The same dialog opens. This is the shared modal, not a second implementation. |
| 5.7 | Close the dialog. | The Pipeline list is unchanged. |

## 6. Pipeline

| # | Step | Expected |
| :-- | :--- | :--- |
| 6.1 | Look at the list after 5.5. | One row per job: `Title — Company`, a tier label (ATS / HTML / Tier 2 / Manual), the status badge when an application exists, and a relative time. The header shows "N jobs". |
| 6.2 | Click each filter: All, Not applied, Applied, Rejected. | The list narrows without a reload. A filter with no matches shows "No jobs match this filter". |
| 6.3 | Click a row. | The detail view replaces the list: title, company, tier, status, "Fetched …", the source URL in monospace, and the raw description. |
| 6.4 | Hover **Evaluate** and **Generate cover letter**. | Both are disabled and the tooltip says "Available in a later phase". |
| 6.5 | Click **Back to list**. | The list returns with the filter you had selected. |
| 6.6 | Open a detail view, then click a different filter. | The list returns (the filter applies to the list). |
| 6.7 | Click **Refresh**. | The list reloads; the selected filter is still selected. |

## 7. Idle behaviour

| # | Step | Expected |
| :-- | :--- | :--- |
| 7.1 | Leave the app on the unlock screen for 30 seconds without input. | CPU in Activity Monitor / Task Manager settles at ~0% after the window appears. |
| 7.2 | Leave it on a finished Pipeline list for 30 seconds. | Same: no continuous repaint. |
| 7.3 | Start a batch fetch, then cancel it. | CPU falls back to idle once the batch reports completion. |

A CPU reading that stays above a few percent while nothing is happening means a
repaint is being requested in a loop. That is the `P2.1-R6` regression; report
it with the screen name.

## 8. Automated live check (optional)

`crates/app/src/app.rs` contains ignored tests that run real fetches through
the app: create portals → `FetchOne` on the first → `FetchAll` (batch, which
re-fetches the first and therefore also exercises dedup) → storage → Pipeline.

```bash
cargo test -p career-ops-app --features keychain -- --ignored live_fetch
```

It creates its own portals in a throwaway vault and never reads or writes your
data directory. The app does not ship example portals.

Default list, one board per supported ATS (last verified 2026-11-03):

| URL | Adapter | Jobs |
| :--- | :--- | ---: |
| `https://jobs.ashbyhq.com/linear` | Ashby | 31 |
| `https://jobs.lever.co/spotify` | Lever | 84 |
| `https://boards.greenhouse.io/figma` | Greenhouse | 144 |
| `https://jobs.smartrecruiters.com/Ubisoft2` | SmartRecruiters | 208 |

The counts move with the boards. SmartRecruiters is **deduplicated across
languages**: its `totalFound` was ~294 on 2026-11-03 while 208 unique postings
were stored. When no `CAREER_OPS_LIVE_SCRAPER_CONFIG` is set, the test attaches
`{"detail":{"max_jobs":2}}` to a SmartRecruiters portal, because that adapter's
list endpoint carries no body; the run therefore verifies descriptions and
per-posting URLs by default (2026-11-03: `2/208 descriptions, all URLs
per-posting`).

Override the list (comma-separated) without editing the file:

```bash
CAREER_OPS_LIVE_PORTALS=https://careers.withwaymo.com/jobs/search \
  cargo test -p career-ops-app --features keychain -- --ignored live_fetch
```

That run on 2026-11-03 stored 467 unique jobs across the four portals (144
Figma, 31 Linear, 84 Spotify, 208 Ubisoft) in about 6.4 s of fetching,
including two SmartRecruiters detail requests. The batch re-fetches the first
portal and the stored total stayed 467, so deduplication held. The test also
asserts the Pipeline page bound rather than assuming every row is visible.

### Registry check

The known-site registry has its own live check: it probes, creates, and fetches
every entry in `crates/core/src/acquisition/known_sites.rs`, then asserts each
one stored at least one posting with text. It is gated twice, because it is
slower than the board checks:

```bash
CAREER_OPS_LIVE_REGISTRY=1 \
  cargo test -p career-ops-app --features keychain -- --ignored \
  live_registry_sites_reach_the_pipeline
```

A WAF challenge skips that entry and says so; a served page that stores nothing
fails. The skip rules themselves (probe before fetch, no retry probe after a
healthy fetch) are covered offline by `cargo test -p career-ops-app --lib --
registry`, so this run is about the sites, not about the branching. The final line
names the verified and skipped entries. After a passing run, commit
`verified_at` in `known_sites.rs` and update `docs/known-sites.md`; the rules are
in that file. First full run (2026-11-03, no skips): Waymo 90 jobs from 3 pages
with 2 of 2 descriptions in 3.7 s, NVIDIA 50 jobs from 5 pages with 40 of 40
descriptions in 27.2 s.

### Making Waymo's search page work

The page is **server-rendered** (Rails + Stimulus), but it carries no
`JobPosting` JSON-LD, so the default extraction fails. It does not need
JavaScript: a selector config extracts the cards as-is, the pagination block
follows the site's `rel=next` link, and the detail block enriches the first
postings from their own pages (those pages *do* carry `JobPosting` JSON-LD, so no
selector is needed there).

Run the dedicated variant. It probes the site first and **skips with a message
when the WAF answers `202`**, because a challenge is a site condition rather
than a code failure:

```bash
cargo test -p career-ops-app --features keychain -- --ignored \
  live_fetch_with_pagination_and_details
```

Measurements:

| Date | Result |
| :--- | :--- |
| 2026-10-25 | 90 jobs from 3 pages of 30, Tier 1b, details WAF-challenged (`202`/0 bytes). |
| 2026-11-03 | 60 jobs from 2 pages in 2.7 s; page 3 returned a WAF challenge, and the 2 detail requests were challenged too (`0 of 2 descriptions`, longest card text 252 bytes). A later retry stored 30 jobs and was challenged on page 2; the next retry was challenged on request 1 (`202`/0 bytes) and skipped. |
| 2026-11-03 20:38 | **Full path:** 90 jobs from 3 pages in 7.0 s, 2 of 2 descriptions, longest 3,611 bytes. Result line: `Fetched 90 jobs from 3 pages; 2 of 2 descriptions.` |

When the WAF is not challenging, the same run reports posting-level text and
the longest description. That was verified on 2026-11-03 20:38: 90 jobs from 3
pages, 2 of 2 descriptions, longest 3,611 bytes, 7.0 s. Extraction from the real
posting markup is also covered offline by
`the_real_waymo_detail_page_yields_its_description`, so a challenge does not
leave the detail path untested.

A selector fetch now names a page it could not read in the result line, for
example `Fetched 60 jobs from 2 pages (page 3 could not be read); 0 of 2
descriptions.` Hitting the configured `max_pages` cap is an expected ending and
stays silent.

The older generic check still works with the same config:

```bash
CAREER_OPS_LIVE_PORTALS=https://careers.withwaymo.com/jobs/search \
CAREER_OPS_LIVE_SCRAPER_CONFIG='{
  "list": "article.job-search-results-card-col",
  "fields": {
    "title": "h3.job-search-results-card-title a",
    "location": "li.job-component-location span",
    "department": "li.job-component-department span",
    "employment_type": "li.job-component-employment-type span",
    "description": "p.job-search-results-summary"
  },
  "pagination": {
    "next": "a[rel=next]",
    "url_template": "https://careers.withwaymo.com/jobs/search?page={page}",
    "max_pages": 3
  },
  "detail": { "max_jobs": 2 }
}' \
  cargo test -p career-ops-app --features keychain -- --ignored live_fetch
```

Either pagination mechanism works; `next` is used when the link exists and
`url_template` is the fallback. `max_pages` includes the first page (default 5,
ceiling 20). Omitting the block keeps the single-page behaviour.

`detail` fetches each posting page and replaces the card teaser with the full
description: JSON-LD first, then `detail.fields.description` if configured.
`max_jobs` defaults to 40 and is capped at 200; every detail fetch consumes one
request, so keep it small on sites that challenge automated traffic. Omitting
`detail` means no enrichment.

A **detail-only** config (`{"detail": {"max_jobs": 1}}`) is valid and is how an
ATS board — which already has full list data — opts into refreshing descriptions.
Verified on Lever: `Fetched 85 jobs; 1 of 1 description.`

### SmartRecruiters descriptions

SmartRecruiters' list endpoint carries no body. The adapter reads
`GET https://api.smartrecruiters.com/v1/companies/{slug}/postings/{id}` and
composes `jobAd.sections` (company description, job description,
qualifications, additional information, in that order). No selectors are
involved, and the same `detail.max_jobs` bound applies:

```bash
CAREER_OPS_LIVE_PORTALS=https://jobs.smartrecruiters.com/Ubisoft2 \
CAREER_OPS_LIVE_SCRAPER_CONFIG='{"detail":{"max_jobs":2}}' \
  cargo test -p career-ops-app --features keychain -- --ignored live_fetch
```

Verified on 2026-11-02: 206 unique postings (294 raw, deduplicated across
languages), `Fetched 206 jobs; 2 of 2 descriptions.`, longest description 6,031
bytes. Keep the JSON in **single quotes**: unquoted, the shell strips the double
quotes, the config fails to parse, and the fetch fails as a configuration error
instead of enriching.

`crates/core/tests/ats_adapters.rs::selectors_extract_a_server_rendered_vendor_card`
pins the same selector shape against a fixture, so a change to the extractor
fails a test instead of silently fetching zero jobs.

### NVIDIA (Eightfold)

NVIDIA's careers page is an Eightfold shell: no job markup, no JSON-LD. It is
fetched by the `eightfold` provider, not by selectors:

```bash
CAREER_OPS_LIVE_PORTALS=https://jobs.nvidia.com/careers \
CAREER_OPS_LIVE_SCRAPER_CONFIG='{
  "provider": "eightfold",
  "domain": "nvidia.com",
  "pagination": { "max_pages": 2 },
  "detail": { "max_jobs": 2 }
}' \
  cargo test -p career-ops-app --features keychain -- --ignored live_fetch
```

Verified on 2026-11-03: 20 jobs from 2 pages,
`Fetched 20 jobs from 2 pages; 2 of 2 descriptions.`, longest 6,253 bytes; 1.9 s
for the core path, 5.5 s through the app including startup. The board reports
~2,750 positions, so `max_pages` (default 5) is what keeps the fetch bounded.
Eightfold descriptions are on by default — the `detail` block only lowers the
cap. The app's **Add portal** form takes the same information as fields: typing
`https://jobs.nvidia.com/careers` prefills **Provider** = Eightfold and the
domain, and stores `{"provider":"eightfold","domain":"nvidia.com"}`.

**If the fetch suddenly fails with `Broken { cause: "other" }` after working:**
Waymo's AWS WAF answers repeated automated fetches with `202` and an empty body.
This can affect the list route, the detail route, or both — a 90-job list fetch
can succeed while the very next detail fetch is challenged. The app spaces
requests to the same host (400 ms minimum plus up to 400 ms of jitter, see
README "Request pacing"), which helps but does not make a quota disappear. Check
with
`curl -sS -o /dev/null -w '%{http_code} %{size_download}\n' https://careers.withwaymo.com/jobs/search`
— a `202 0` means "challenged", not "extraction broken". Wait a few minutes
before retrying; do not raise `max_pages` or `max_jobs` to get around it.

The config is enterable in the app: **Add portal** / **Edit** → *Provider* selects
the engine. *Selectors* shows a JSON field for the selector config; *Eightfold*
shows a domain field and an optional advanced-options field for the caps. The
result is stored in `portals.scraper_config` as one JSON object and re-validated
on save; empty selectors means "no config". A known host prefills these fields
(see §4.3b.2 and §4.3b.3) and never overwrites a value you edited.

A page with neither JSON-LD nor a selector config still fails on both tiers
(`Parse: no jobs found in HTML`), because Tier 2 reuses the same extraction on
the rendered DOM.

This check covers the fetch, storage, portal health, and Pipeline contents. The
UI feedback while a fetch is in flight (spinner, per-portal progress, cancel
button) is still section 4's job.

## 9. Sign-off

| Date | Runner | Build (`git rev-parse --short HEAD`) | Sections run | Result | Notes |
| :--- | :--- | :--- | :--- | :--- | :--- |
|  |  |  |  |  |  |

Record which 🌐 steps were skipped. A skipped network step is not a failure; a
skipped non-network step is.
