# Phase 2.8.1 Retrospective and Verification Record

**Phase:** Logging and Diagnostics
**Parent plan:** Draft Execution Plan: Career-Ops Rust Desktop Application (v0.17), §12
**Phase plan:** `.agents/plans/phase_2.8.1_logging_and_diagnostics.md` (v0.5)
**Period:** 2026-11-06
**Estimate:** 2–3 days
**Actual:** one session across four workstreams
**Status:** Complete. The owner's manual pass covered §9 and all thirteen steps pass: the two that failed first time (§9.4 Follow, §9.10 a failure missing from the log) were fixed and re-verified on 2026-11-06.

---

## Summary

The app had used `tracing` since Phase 1 and had nothing to show for it. Three
gaps, each sufficient on its own:

1. The only sink was **stderr** (`fmt()` in `main.rs`), which a windowed `.app`
   launched from Finder discards.
2. The level came only from `RUST_LOG`, so with it unset the app was silent —
   and setting it for a bundle means launching from a terminal.
3. The ~25 call sites were **failure-only**: `warn!` at the point something
   broke, `info!` for enrichment counts. There was no per-page line and no
   walk-chunk line, so "it stalled after 40 jobs" was the most a log could say.

Phase 2.8's stall (F8, F9, F10) is the evidence: the third round trip only
landed because a `serde_json` error happened to reach the UI inside a fetch
error. Nobody read a log, because there was nowhere to read.

Phase 2.8.1 adds one seam with two sinks and the events that describe a fetch:

- `career_ops_core::logging` — a rotated file at `<data_dir>/logs/`, a bounded
  in-process ring, a panic hook, and one idempotent initializer.
- Per-page, per-chunk, per-call, and sidecar-stderr events.
- **Logs** in the sidebar, and a bottom drawer reachable from every screen
  (`Cmd/Ctrl+L`), both rendering the same view.

It adds no user-facing feature except the ability to see what happened, which is
the point.

---

## Exit criteria status

| # | Criterion | Status | Evidence |
| :--- | :--- | :--- | :--- |
| 1 | An event reaches both sinks with level, target, message, and fields | ✅ | `a_line_reaches_the_buffer_with_its_fields` (buffer), `the_file_sink_installs_and_receives_events` (file, own process) |
| 2 | Release logs at `info` with no environment variable; `RUST_LOG` still overrides | ✅ | `DEFAULT_CAPTURE` in `logging::install`; `the_level_policy_admits_debug_only_for_the_app_targets` |
| 3 | The file lives under `<data_dir>/logs/`, rotates at 5 MB keeping five, flushes per line | ✅ | `the_file_sink_installs_and_receives_events`, `a_rotation_keeps_the_newest_archives`, `the_file_writer_appends_and_flushes_every_line` |
| 4 | A panic writes a line with its location without deadlocking | ✅ | `a_panic_is_written_with_its_location` |
| 5 | **Logs** is a sidebar screen with chips, search, pause, copy, folder, delete, dropped marker | ✅ | `crates/app/tests/logs_screen.rs` (9 tests) |
| 6 | The drawer opens from the sidebar toggle and `Cmd/Ctrl+L`, renders the same view, does not disturb the screen | ✅ | `the_sidebar_offers_the_logs_drawer_toggle`, `the_drawer_opens_the_screen_and_closes`; placement and resizing verified in §9.1 |
| 7 | **View logs** beside a Portals error and a stalled walk navigates | ✅ | `a_failed_fetch_is_one_click_from_its_log`, `a_stalled_walk_also_offers_its_log`, `show_logs_opens_the_logs_screen_and_closes_the_drawer` |
| 8 | A walk logs one line per page and one per chunk, naming the page and stop | ✅ | `a_walk_logs_the_page_it_stopped_at`, `a_page_is_logged_with_its_size_and_job_count` |
| 8a | A failed fetch is logged, including failures that never reach the network | ✅ | `a_fetch_failure_is_logged_with_its_cause` (found in §9.10) |
| 9 | Sidecar stderr is captured instead of discarded | ✅ | `sidecar_stderr_lines_are_forwarded_to_the_log` |
| 10 | No migration, no new external package, no change to fetch behavior | ✅ | no migration; `Cargo.lock` gained two edges on `career-ops-core` and lost the app's `tracing-subscriber` edge; the router's behavior is unchanged by logging |
| 11 | Offline suite, fmt, and clippy are clean | ✅ | See the commands below |
| 12 | `docs/logging.md`, manual script §9, `patterns.md`, parent plan, retrospective | ✅ | This workstream |

**Verification commands and results (2026-11-06):**

| Command | Result |
| :--- | :--- |
| `cargo fmt --all --check` | clean |
| `cargo clippy --workspace --all-targets --offline -- -D warnings` | clean |
| `cargo test --workspace --features career-ops-app/keychain --offline -- --skip obscura` | all green |
| `cargo test -p career-ops-core --features keychain --offline --lib` | 143 passed |
| `cargo test -p career-ops-core --features keychain --offline --test logging --test logging_file` | 11 + 1 passed |
| `cargo test -p career-ops-core --features keychain --offline --test portals_service` | 15 passed |
| `cargo test -p career-ops-app --features keychain --offline --lib` | 130 passed, 3 ignored (the live tests) |
| `cargo test -p career-ops-app --features keychain --offline --test logs_screen` | 9 passed |
| `cargo test -p career-ops-app --features keychain --offline --test portals_screen --test ui_shell` | 44 + 5 passed |

Three network tests remain ignored, as before: `live_fetch_reaches_the_pipeline`,
`live_fetch_with_pagination_and_details`, `live_registry_sites_reach_the_pipeline`.

---

## What the code taught us

### D1 — `try_init` already installs the `log` bridge

The plan expected to need `tracing_log::LogTracer::init()` by hand, because
replacing `fmt::init()` with a manual `registry()` drops `log`-crate records — and
some `sqlx` paths emit them. `SubscriberInitExt::try_init` does that itself, so
the direct dependency was unnecessary. `the_log_bridge_carries_log_crate_records`
emits a real `log::warn!` and asserts it reaches the buffer, so the assumption is
now a test rather than a comment.

Worth noting the trap that was avoided: calling `LogTracer::init()` *before*
`try_init()` makes `try_init` fail, which would have left the app with no
subscriber at all and no obvious symptom.

### D2 — The per-page line belongs in the engine, not the router

The plan put per-page logging in `router::fetch_portal_inner`, but the router
never sees a page: it delegates to `html::fetch_pages` and Eightfold's
`fetch_jobs_with`. Trying to log per page there would have meant either a
callback through the router or a second, duplicated paging loop.

The split that emerged is also the more useful one: the engines log **per page**
(what was read, how big, how many jobs), and the router logs **per call** (tier,
depth, stop, elapsed). For a walk that is `pages + 1` lines per chunk, and the
chunk line is the one that answers "where did it stop".

### D3 — A successful page's status is a constant, `bytes` is not

The plan asked for `status` on every per-page line. A successful page is 2xx by
construction — `HttpPageSource` rejects anything else before extraction — so the
field would have carried no information, at the cost of changing the private
`PageSource` trait and ~20 scripted fixtures.

`bytes` is the field that matters: it is what separates "the site answered" from
"the site answered with a shell", which is exactly the Waymo `202` / empty-body
case already documented in `docs/known-sites.md`. Non-2xx statuses still reach
the log through the classified error.

### D4 — A log target is a filter decision

The plan said the sidecar's stderr should be forwarded with `target: "obscura"`.
Under the default policy — `info`, plus `debug` for `career_ops_core` and
`career_ops_app` — a bare `obscura` target is filtered **out**, so the feature
would have shipped looking broken: the capture works, the lines never appear.

Using the module path (`career_ops_core::backend::obscura`, unset) makes the
lines visible in the buffer by default while the file stays at `info`. The
lesson generalises: a target is not a label, it is what the filter matches.

### D5 — CDP chatter was not worth logging

The plan also promised `trace!` for every CDP message the handler discards. That
is a firehose with large payloads, and `RUST_LOG=trace` would become unusable
exactly when someone needs it. The sidecar's own diagnostics are on stderr, which
is now captured; the message drain stayed a drain. Recorded as a deliberate
omission rather than a forgotten item.

### D6 — A shared buffer plus a fresh database per test means colliding ids

The first two versions of `a_walk_logs_the_page_it_stopped_at` failed for a test
reason, not a code reason: the buffer is process-global and every test's
throwaway vault hands out portal id 1, so the test picked up a *parallel* test's
chunk line (and asserted against it).

The fix is a pattern WS3 and later phases should copy: identify your own line.
The walk starts at page 41 and the search matches `first_page=41`, so no other
test in the binary can produce the line being asserted on.

### D7 — Two egui 0.37 renames, one of them only visible in use

`Context::wants_keyboard_input` is `egui_wants_keyboard_input`; `ScrollArea`'s
`State::offset` is a private field with a deprecated `offset()` getter, and
`clamped_offset()` is the current call. Neither is in the crate's migration list
that mattered here, and both surfaced only at compile time — which is the
argument for writing the screen before the documentation.

### D8 — A drawer toggle does not fit through a `&mut` closure

`show_with` renders the sidebar and calls a content closure. The toggle needs
`&mut bool`, and the closure also needs `&mut LogsView` — one field of which is
that bool. Passing `&mut logs_view.drawer_open` and also capturing `logs_view`
is two mutable borrows of one place.

The app copies `drawer_open` out, gives the shell `&mut` on the copy, and writes
it back after the frame. Three lines of glue, no view-model split, and the
borrow checker is satisfied honestly rather than with an `Rc<RefCell<_>>`.

### D9 — The live tests needed a drop guard, not a subscriber

Migrating the three ignored live tests was on the workstream list for a reason:
once the app installs the global subscriber, their `try_init` fails *silently*
and their `RUST_LOG` diagnostics would have been dead without anyone noticing.

`LogsOnFailure` is the replacement: a value whose `Drop` checks
`std::thread::panicking()` and prints `logging::handle().snapshot()`. Cargo shows
a failing test's captured stderr, so it is silent on success and complete on
failure — and it cost one line per test instead of restructuring them.

---

## Manual pass findings

The owner ran §9 on 2026-11-06. Eleven of thirteen steps passed; two did not.

### F11 — Follow did not return to the tail (§9.4)

The chip turned on and nothing moved.

`ScrollArea::stick_to_bottom` is not "scroll to the bottom": it only *keeps* a
view at the end. egui recomputes `state.scroll_stuck_to_end` every frame as
`offset == max_offset`, so a view scrolled up is never reclassified even with
stick enabled — and the frame after the click still reported the old offset, so
the screen's own "scrolled up?" check turned the flag straight back off. The
click was undone by the frame it happened in.

Two things had to change:

- **The jump is explicit.** `LogsView` keeps a per-mount `pending` flag, set
  when Follow is switched on (and initialised set, so a first render lands at
  the tail). While it is set, the list passes an offset that egui clamps to the
  content height. The flag clears when the view reports the landing, not on a
  timer, because the frame carrying a jump still sees the old offset.
- **The offset is finite.** The first attempt used `f32::MAX`, and the kittest
  suite caught it immediately: `content_size - f32::MAX` is `-inf`, egui
  computes a NaN rect, and `ui.rs` panics with "max_rect is NaN". A constant of
  1e6 clamps to the same place and stays finite.

The per-mount flag also fixes a case the manual pass did not reach: a surface
that renders for the first time *after* the click (the screen opened from the
drawer) lands at the tail rather than at the top.

**Re-verified 2026-11-06.** Clicking **Follow** from the top of a long log
returns to the newest line, and the chip stays on.

### F12 — A failed fetch left nothing in the log (§9.10)

Step 9.10 expects the log to explain a configuration error. It did not, and the
reason is structural rather than one missing line: `fetch_portal_inner` returns
early at every `?` — the scraper config, the URL, the missing portal — and its
`fetch finished` line is at the end, so *no* failure produced a log line at all.
The UI's classified error message was the only record.

The fix puts the record where every path passes: `portals::service` logs
`fetch failed` with `portal`, `cause`, and `detail` for `fetch_one`/`fetch_more`,
for batch entries, and for walk chunks. That also covers failures the router
never sees (a portal row that vanished mid-fetch), and it keeps the router from
logging something its success line would duplicate.

`a_fetch_failure_is_logged_with_its_cause` creates a portal with
`{"provider":"workday"}` and asserts the line names the provider — the manual
step, as a test.

**Re-verified 2026-11-06.** §9.10 now shows `fetch failed` with `portal`,
`cause`, and a `detail` naming the provider, on the Logs screen.

### F13 — The buffer could store lines out of sequence order

Found by the full suite while closing the phase, not by hand. The sequence
number came from an atomic **before** the buffer lock, so two threads could
claim 5 and 6 and append them in the other order. Invisible to the eye, and not
harmless: the UI refreshes its snapshot when `latest_seq` changes, so a line
appended behind a newer one could be skipped until the next event.

The counter now lives inside the buffer's `Mutex`, which makes sequence order
and storage order the same thing, and `latest_seq()` reads it under the same
lock — a frame that sees `N` also sees every line up to `N`.
`concurrent_writers_keep_the_snapshot_in_sequence_order` pins it with four
threads × 50 lines, and the assertion that caught it
(`snapshot order follows the sequence`) stays in the field test.

---

## Estimate accuracy

| Workstream | Plan | Actual |
| :--- | :--- | :--- |
| WS1 logging core | 1 day | Part of 2026-11-06 |
| WS2 the events | 0.5 day | Part of 2026-11-06 |
| WS3 screen and drawer | 1 day | Part of 2026-11-06 |
| WS4 docs and retrospective | 0.5 day | Part of 2026-11-06 |
| **Total** | **2–3 days** | **One session** |

The estimate held because the work was additive: no schema, no fetch semantics,
and the only cross-cutting edit was `shell::show_with`'s signature. The three
plan corrections (D1, D2, D3) were found by writing the code, not by a manual
pass, which is what the sequencing was for.

---

## Deferred, unchanged

- **Option C** (JSON sink, per-task spans, diagnostic bundle, retention policy
  in Settings) — recorded in the phase plan with its triggers. Phase 3's cost and
  latency accounting is the first one that will actually need spans.
- **Q-45, a log level in Settings** — Settings is still a placeholder (Phase 2.9
  adds one section; Phase 5 completes it). The Logs screen states the capture
  policy and points at `RUST_LOG` until then.
- **Q-47, terminal echo** — `main` no longer writes to stderr. The file is
  `tail -f`-able and the drawer exists; echoing to a TTY needs an optional boxed
  layer and buys little.
- **Risk R6, the drawer's rendering** — compile- and kittest-verified, but
  whether it *looks* like a drawer in a real window is manual script §9.1.

Phase 2.9 (preferences and the job queue) is next. Its
**Fetch descriptions** batch is the first operation that will be easier to watch
in the drawer than to infer from a count, which is why this phase came first.
