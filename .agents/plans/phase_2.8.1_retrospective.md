# Phase 2.8.1 Retrospective and Verification Record

**Phase:** Logging and Diagnostics
**Parent plan:** Draft Execution Plan: Career-Ops Rust Desktop Application (v0.17), §12
**Phase plan:** `.agents/plans/phase_2.8.1_logging_and_diagnostics.md` (v0.5)
**Period:** 2026-11-06
**Estimate:** 2–3 days
**Actual:** one session across four workstreams
**Status:** Complete against the automated criteria. Manual script §9 is written and awaits the owner's pass; the drawer's *rendering* (R6) is the one thing only that pass can confirm.

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
| 6 | The drawer opens from the sidebar toggle and `Cmd/Ctrl+L`, renders the same view, does not disturb the screen | ✅ (toggle + row rendering) / ⚠️ (layout) | `the_sidebar_offers_the_logs_drawer_toggle`, `the_drawer_opens_the_screen_and_closes`; the panel's *placement* is a manual step (§9.1) |
| 7 | **View logs** beside a Portals error and a stalled walk navigates | ✅ | `a_failed_fetch_is_one_click_from_its_log`, `a_stalled_walk_also_offers_its_log`, `show_logs_opens_the_logs_screen_and_closes_the_drawer` |
| 8 | A walk logs one line per page and one per chunk, naming the page and stop | ✅ | `a_walk_logs_the_page_it_stopped_at`, `a_page_is_logged_with_its_size_and_job_count` |
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
| `cargo test -p career-ops-core --features keychain --offline --test portals_service` | 14 passed |
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
