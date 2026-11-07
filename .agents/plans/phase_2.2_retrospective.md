# Phase 2.2 Retrospective and Verification Record

**Phase:** Acquisition Completeness
**Parent plan:** Draft Execution Plan: Career-Ops Rust Desktop Application (v0.8), §12
**Phase plan:** `.agents/plans/phase_2.2_acquisition_completeness.md` (v0.8)
**Period:** 2026-10-25 → 2026-11-03
**Estimate:** 4–6 days
**Actual:** three implementation days across two weeks, plus one verification pass
**Status:** Complete against all ten exit criteria. The final live run verified
the full Waymo path — 90 jobs from three pages with posting-level text — after
earlier attempts were blocked by the site's WAF.

---

## Summary

Phase 2.2 made fetched postings evaluable. A selector-configured page now follows
its pagination up to a cap, refetches its details through JSON-LD or a selector,
and stores the posting text; a SmartRecruiters board now stores one row per
posting, with its own URL and the four `jobAd.sections` composed into `raw_text`;
and the portal form can dry-run a draft config without writing anything.

The live runs answered the phase's central question — can a person configure a
real site and get usable text? — with **yes**. Four ATS boards stored 467 unique
postings in about 6.4 seconds. Waymo stored 90 postings from three pages with two
posting-level descriptions (longest 3,611 bytes) in 7.0 seconds on the final run;
earlier attempts were challenged by its WAF, and the live test reports that as a
skip rather than a false regression.

The verification pass found two gaps that the plan did not anticipate, both about
honesty rather than capability:

1. **A partial fetch looked complete.** When page 3 was challenged, the loop kept
   the 60 jobs it had and the portal became Healthy. Only a log line recorded the
   stop. A user saw "60 jobs" with no way to know the board had more.
2. **A Tier 1 failure could be masked by a Tier 2 failure.** The WAF produced a
   Tier 1 parse failure; the router escalated to the sidecar, which was not
   running, and the user saw "an unrecognized reason" instead of the parse cause.

Both were fixed immediately after the verification pass, within this phase, and
the final live run confirmed both: the result line reads `Fetched 90 jobs from 3
pages; 2 of 2 descriptions.` The fixes are recorded in F2 and F3 below with the
tests that pin them.

---

## Exit criteria status

| # | Criterion | Status | Evidence |
| :--- | :--- | :--- | :--- |
| 1 | A portal with a `pagination` config stores jobs from more than one page, and stops at `max_pages` | ✅ | Fixture tests `next_links_are_followed_and_resolved_against_the_page`, `a_url_template_builds_pages_from_two`, `max_pages_is_honoured` (all now assert `PaginationStop`); live Waymo final run 2026-11-03: 90 jobs from 3 pages (one page is 30) |
| 2 | A portal with a `detail` config stores the posting description as `raw_text`, truncated at 200 KB | ✅ | `detail_enrichment_replaces_card_text`, `jsonld_detail_enriches_without_selectors`, `an_oversize_description_is_truncated_on_a_character_boundary`; live ATS run longest description 11,278 bytes |
| 3 | A failed detail fetch keeps the list row | ✅ | `a_failed_detail_fetch_keeps_the_list_row`, `a_failed_smartrecruiters_detail_keeps_the_list_row` |
| 4 | Enrichment can be cancelled between jobs | ✅ | `cancellation_stops_before_the_first_detail_fetch`; batch cancellation in `fetch_batch_cancellable_stops_between_portals` |
| 5 | Re-fetching the same portal is idempotent | ✅ | `enrichment_is_idempotent`; live batch re-fetched the first portal and the stored total stayed 467 |
| 6 | SmartRecruiters rows have a per-posting `source_url` and non-empty `raw_text` | ✅ | Live 2026-11-03: `smartrecruiters descriptions 2/208 (all URLs per-posting)`; 2026-11-02: 206 unique rows, 2 of 2 descriptions, longest 6,031 bytes. Unit tests `smartrecruiters_source_url_is_the_posting_page`, `smartrecruiters_detail_url_targets_the_posting_api` |
| 7 | Test config shows counts, sample titles, and the classified error; writes nothing | ✅ | Core preview tests (counts, cap, invalid config, ATS detail, page-count edge); `config_preview_reports_and_never_writes`; four `portals_screen` harness tests |
| 8 | Waymo is fetched end to end, all pages up to the cap, with posting-level text, verified live | ✅ | 2026-11-03 20:38: 90 jobs from 3 pages, 2 of 2 descriptions, longest 3,611 bytes, 7.0 s (`Fetched 90 jobs from 3 pages; 2 of 2 descriptions.`). Earlier attempts on 2026-10-25 and 2026-11-03 were WAF-challenged; the test probed and skipped. Extraction from the real detail markup is also covered offline by `the_real_waymo_detail_page_yields_its_description` |
| 9 | The manual script records per-site results and the caps | ✅ | `docs/manual-test-script.md` §8: counts for the four boards, Waymo measurements by date, SmartRecruiters detail default, quoting pitfall |
| 10 | Retrospective recorded, including eager versus lazy enrichment | ✅ | This document |

**Verification commands and results (2026-11-03):**

| Command | Result |
| :--- | :--- |
| `cargo fmt --all --check` | clean |
| `cargo clippy --workspace --all-targets --offline -- -D warnings` | clean |
| `cargo test --workspace --features career-ops-app/keychain --offline -- --skip obscura` | all passed (`obscura` skipped: the sandbox forbids binding a port) |
| `cargo test -p career-ops-app --features keychain --lib -- --ignored live_fetch_reaches_the_pipeline` | 467 jobs across four boards, 7.5 s including startup |
| `cargo test -p career-ops-app --features keychain --lib -- --ignored live_fetch_with_pagination_and_details` | final run: 90 jobs / 3 pages / 2 of 2 descriptions / 7.0 s; earlier attempts were challenged and skipped |

---

## Live measurements

| Site | Date | Jobs | Time | Notes |
| :--- | :--- | ---: | :--- | :--- |
| Figma (Greenhouse) | 2026-11-03 | 144 | ~2.4 s single fetch | One list request; descriptions come from the list payload |
| Linear (Ashby) | 2026-11-03 | 31 | part of the 6.4 s batch | |
| Spotify (Lever) | 2026-11-03 | 84 | part of the 6.4 s batch | |
| Ubisoft2 (SmartRecruiters) | 2026-11-03 | 208 unique | ~3 s | `totalFound` ~294; deduplicated by `refNumber`; 2 detail requests, 2 descriptions |
| Waymo (selector config) | 2026-11-03 | 60 (2 pages) | 2.7 s | Page 3 and details challenged (`202`/0); card teaser 252 bytes stored. A later retry stored 30 jobs and was challenged on page 2; the next retry was challenged on request 1 |
| Waymo (selector config) | 2026-11-03 20:38 | 90 (3 pages) | 7.0 s | **Full path:** 2 of 2 descriptions, longest 3,611 bytes; result line named the pages |
| Waymo (selector config) | 2026-10-25 | 90 (3 pages) | — | Details challenged then too |
| Four-board batch | 2026-11-03 | 467 | ~6.4 s | Includes 2 SmartRecruiters details; first portal re-fetched without duplicates |

The pacing is 400 ms plus up to 400 ms of jitter per host. On the ATS boards it
was never the bottleneck; on Waymo the challenge is not rate-based — it arrived
on the first request of a later run — so more delay would not buy access.

---

## What the verification pass found

### F1 — The WAF makes the detail route unverifiable on demand (operational, recorded)

Waymo answers repeated traffic with `202 Accepted` and a zero-byte body. That is
a challenge, not a `200` with an empty page, and it can hit the list, a later page,
or a posting page. On 2026-11-03 the list survived two pages, then page 3 and both
detail requests were challenged. A later run stored one page (30 jobs) and was
challenged on page 2, and the following run was challenged on its first request.
The final run after a longer cool-down completed the full path: 90 jobs from three
pages, 2 of 2 descriptions, longest 3,611 bytes, in 7.0 seconds.

**Response.** The dedicated live test probes the first request and skips with a
message when it is challenged; when only one page arrived, it probes page 2; when
no description was extracted, it probes one posting. It fails only if the site
served a document and the app extracted nothing. This keeps a network-condition
red run out of the suite without hiding extraction regressions. The real posting
markup remains covered by
`the_real_waymo_detail_page_yields_its_description`.

### F2 — Partial pagination is silent to the user (medium, real) — fixed

`fetch_pages` stops when a later page fails and keeps the earlier jobs, which is
the right data decision. But the result was only `Vec<NormalizedJob>`: the pages
fetched and the stop reason were not returned, so `FetchOutcome` reported
`jobs_fetched = 60` and the portal became Healthy. In the live run the only trace
was `pagination stopped: page extraction failed page=3` in the log.

**Fix.** `fetch_static_jobs` now returns a `PageReport` with `pages_fetched` and a
`PaginationStop` (`EndOfList`, `LimitReached`, `FetchFailed`,
`ExtractionFailed`, `NoNewJobs`, `RepeatedUrl`). `FetchOutcome` carries both, and
the fetch message says it: `Fetched 60 jobs from 2 pages (page 3 could not be
read).` Expected endings (the configured cap, the end of the list) stay silent.

**Evidence.** `no_pagination_config_fetches_one_page`,
`max_pages_is_honoured`, `a_failing_later_page_returns_the_earlier_jobs`,
`a_later_page_with_no_jobs_stops_the_loop`, `a_repeated_page_stops_the_loop`, and
`a_template_without_a_placeholder_stops_instead_of_looping` assert the stop
reason; `a_partial_pagination_is_reported_in_the_outcome` proves the router
carries it; `fetch_summary_reports_descriptions` asserts the wording.

### F3 — A Tier 1 failure is replaced by a Tier 2 failure (medium, real) — fixed

When Tier 1 failed with an escalatable cause, the router called `fetch_tier2`; if
that also failed, `?` returned the Tier 2 error and the Tier 1 cause was
discarded. The live Waymo run exposed it: the page was a WAF challenge
(`ParseError`), the sidecar was unavailable (`Other`), and the Portals hint said
"The fetch failed for an unrecognized reason" instead of "the page structure may
have changed".

**Fix.** On a failed escalation the router keeps the Tier 1 `FetchError`, appends
the fallback's detail (`; the browser fallback also failed: …`), and records the
tier 1 cause. A sidecar that is not running can no longer hide why the page
failed.

**Evidence.** `both_tiers_failing_keeps_the_tier1_cause` (renamed from
`both_tiers_failing_returns_classified_error`) asserts the `Blocked` cause and
both detail halves. `js_required_escalates_to_tier2` still covers the successful
fallback.

### F4 — ATS descriptions are opt-in, and the live test now proves it (documented)

A plain SmartRecruiters portal still stores titles, URLs, and metadata but no
text; enrichment requires a `detail` block. That is the WS2 rule, and it applies
to every board. The live test attaches `{"detail":{"max_jobs":2}}` to a
SmartRecruiters portal when no config is supplied, so the default live run
exercises WS3's path without changing the product default.

### F5 — The preview reports pages instead of fetching them (documented trade-off)

The "Test config" panel fetches one list page and at most three details. For a
selector portal it reports the configured page count rather than following it;
for an ATS board it fetches the whole board, because the adapter owns pagination.
Both are stated in the panel and in the plan's WS4 note. A preview that fetched
every page would be a fetch, not a preview.

---

## Cap values after measurement (Q-10)

| Cap | Value | Verdict | Reasoning |
| :--- | :--- | :--- | :--- |
| `pagination.max_pages` | 5 (ceiling 20) | Keep | One page is 0.4–0.8 s of pacing plus one response; five pages is a few seconds. Waymo's three pages plus two details took 7.0 s. |
| `detail.max_jobs` | 40 (ceiling 200) | Keep | At 0.5–1 s per detail with pacing, 40 is roughly half a minute for a user-initiated fetch. It is also the honest coverage boundary: on a 208-job board, 40 rows get text and 168 keep list text. |
| Request pacing | 400 ms + 0–400 ms | Keep | Never the bottleneck on ATS boards; irrelevant to the Waymo challenge, which is not rate-based. |
| Description truncation | 200 KB | Keep | The longest live description was 11 KB; the cap protects against embedded images, not normal postings. |

No cap changed as a result of the measurements. The data supports the original
judgement calls rather than replacing them.

---

## Eager versus lazy enrichment (Q-9, Q-11)

**Stay eager and bounded.**

Eager enrichment is what makes the Pipeline useful the moment a fetch completes:
Phase 3 can evaluate a row without a second network step, and the user sees the
text length in the result line. The cost is measured and bounded — about half a
minute for the default 40 details — and the pass is cancellable between jobs.

Lazy enrichment would make every evaluation depend on a live request and an error
path, and would leave the list view unable to show what it has. It is the better
design only when the user wants freshness more than coverage.

**Revisit condition.** When the detail view gains actions (Phase 3's Evaluate
flow), add an explicit "Refresh description" action per job. That covers the
staleness case (Q-11) without making the default fetch lazy.

---

## robots.txt posture (Q-8)

**Documented, not enforced.** The app fetches one page on a user action, like a
browser, and never crawls or schedules. Enforcing `robots.txt` would require a
policy engine that can silently refuse a page the user deliberately configured,
and the app already reports blocks and challenges instead of evading them. The
manual script and the HTTP module say this explicitly: an application that is
asked to stop must say so, never work around it. The user remains responsible for
the sites they configure.

---

## Estimate accuracy

| Workstream | Plan | Actual |
| :--- | :--- | :--- |
| WS1 pagination | 1 day | Part of 2026-10-25 |
| WS1b pacing | 0.5 day | Part of 2026-10-25 |
| WS2 detail enrichment | 1–2 days | Part of 2026-10-25 |
| WS3 SmartRecruiters | 0.5–1 day | 2026-11-02 |
| WS4 Test config | 0.5–1 day | 2026-11-02 |
| WS5 verification | 1 day | 2026-11-03 |
| **Total** | **4–6 days** | **Three implementation days across two weeks** |

The shape of the overestimate is the same as Phase 2.1: the plan priced design
and debugging, but the design was already written down and every decision had a
test seam. The verification pass was not cheaper — it produced the two findings
above — it was simply bounded by what a network and a WAF allowed.

---

## What to carry into Phase 3

1. **`raw_text` is markup.** SmartRecruiters sections and JSON-LD descriptions are
   HTML. The evaluation prompt must either accept tags or the pipeline must strip
   them; do not assume plain text.
2. **Coverage is partial and visible.** With `max_jobs` 40, most rows on a large
   board carry list-level text. Evaluation should record whether it used a
   description or a teaser, so a weak score cannot be mistaken for a weak match.
3. **Metadata is parsed but not stored.** Department and employment type reach
   `NormalizedJob` and are dropped at the `job_descriptions` boundary. Add columns
   with their own migration if evaluations need them.
4. **F2 and F3 are closed.** Other phases can trust the fetch result to say when
   a page was not read, and to name the real cause when a fallback failed.
5. **The known-site registry (Phase 2.3) should carry measurements, not just
   selectors.** Waymo's config already has dates and block notes in the manual
   script; move them with the entry.

---

## Open items for the human

- Execute the 🌐 rows of `docs/manual-test-script.md` on a machine with network
  and record the sign-off table.
- Re-run `live_fetch_with_pagination_and_details` periodically; a WAF challenge
  now skips instead of failing, and a quiet window reproduces the verified
  90-job/3-page result.
- Confirm the CI run for the Phase 2.2 commits is green on all three operating
  systems.
