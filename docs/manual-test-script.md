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
| 4.3 | Enter a company name and a real job-board URL, keep **Static (Tier 1)**, save. | The modal closes and the row appears with "Never fetched". |
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

`crates/app/src/app.rs` contains an ignored test that runs real fetches through
the app: create portals → `FetchOne` on the first → `FetchAll` (batch, which
re-fetches the first and therefore also exercises dedup) → storage → Pipeline.

```bash
cargo test -p career-ops-app --features keychain -- --ignored live_fetch
```

It creates its own portals in a throwaway vault and never reads or writes your
data directory. The app does not ship example portals.

Default list, one board per supported ATS (last verified 2026-10-25):

| URL | Adapter | Jobs |
| :--- | :--- | ---: |
| `https://jobs.ashbyhq.com/linear` | Ashby | 34 |
| `https://jobs.lever.co/spotify` | Lever | 90 |
| `https://boards.greenhouse.io/figma` | Greenhouse | 139 |
| `https://jobs.smartrecruiters.com/Ubisoft2` | SmartRecruiters | 237 |

Override the list (comma-separated) without editing the file:

```bash
CAREER_OPS_LIVE_PORTALS=https://careers.withwaymo.com/jobs/search \
  cargo test -p career-ops-app --features keychain -- --ignored live_fetch
```

That run on 2026-10-25 stored 500 unique jobs across the four portals — exactly
the Pipeline page size — so the test also asserts the page bound rather than
assuming every row is visible.

Why Waymo's URL is not in the default list:

- Its search page is rendered by JS controllers and the static HTML carries no
  `JobPosting` JSON-LD. Tier 1b therefore fails and the fetch escalates to Tier
  2. On a machine without `vendor/obscura` the test fails with
  `Broken { cause: "other" }`; that is the expected outcome there, not a
  regression. With the sidecar present, success depends on the rendered DOM
  containing JSON-LD.
- A career page with neither JSON-LD nor a portal `scraper_config` cannot be
  fetched by either tier. The selector path exists in the core extractor, but
  the Phase 2.1 portal form does not expose a `scraper_config` field.
- This checks the fetch, storage, portals health, and Pipeline contents. The UI
  feedback while a fetch is in flight (spinner, per-portal progress, cancel
  button) is still section 4's job.

## 9. Sign-off

| Date | Runner | Build (`git rev-parse --short HEAD`) | Sections run | Result | Notes |
| :--- | :--- | :--- | :--- | :--- | :--- |
|  |  |  |  |  |  |

Record which 🌐 steps were skipped. A skipped network step is not a failure; a
skipped non-network step is.
