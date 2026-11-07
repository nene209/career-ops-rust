# Phase 2.8.1: Logging and Diagnostics — Detailed Execution Plan

**Parent plan:** Draft Execution Plan: Career-Ops Rust Desktop Application (v0.16), §12
**Status:** In progress — WS1–WS3 implemented; WS4 pending
**Last updated:** 2026-11-06
**Duration:** 2–3 days
**Depends on:** Phase 2.8 (the walk, its stop reasons, and the failures this phase makes readable)
**Blocks:** Nothing hard. Phase 2.9's description batch and Phase 3's LLM calls both benefit from it, which is why it is sequenced before them.

---

## Change Log

| Version | Change |
| :--- | :--- |
| v0.1 | Initial plan. Implements **Option B** from the phase review — a durable log file plus an in-app Logs screen — with the owner's four decisions recorded: **Option B, with Option C deferred** to its own section; numbered **2.8.1** and sequenced before 2.9; the screen **visible in release builds** at `info`; and the **bottom drawer in scope now**, not later. No migration, no new external dependency, no change to fetch behavior. |
| v0.2 | WS1 implemented. Two details the code settled. **The `log` bridge needs no direct dependency:** `SubscriberInitExt::try_init` installs `LogTracer` itself, and `the_log_bridge_carries_log_crate_records` pins it, so `tracing-log` is not added. **One level policy, not per-event logic:** the subscriber's global `EnvFilter` is the union (`info`, plus `debug` for `career_ops_core`/`career_ops_app`) and the file layer adds its own `info` filter on top, which is what makes the buffer deeper than the file without any branch in the layer. `LogHandle` also exposes `policy()` (so the screen can state where its level came from) and `dropped()`. `Cargo.lock` gains two dependency edges on the core package and no new packages. |
| v0.3 | WS2 implemented. Three deviations, all recorded rather than left as drift. **The per-page line lives in the engines, not the router:** `fetch_portal_inner` delegates paging, so it logs one line per *call* (tier, pages, jobs, details, stop, elapsed) and the page loops log per page — which also folds the old enrichment-only line into one. **No `status` field on a successful page:** a successful page is 2xx by construction, the non-2xx cases already carry the status through the classified error, and adding it would mean changing the `PageSource` seam and ~20 fixtures for a constant; `bytes` is the field that distinguishes "answered" from "answered with a shell". **CDP message chatter is not logged per message:** it is a firehose with large payloads, and the sidecar's own diagnostics are on stderr, which is now captured. The sidecar target is the module path so the default debug policy keeps those lines in the buffer. |
| v0.4 | WS3 implemented. What the code settled. **The drawer toggle crosses the shell boundary:** `show_with` gained `&mut bool`, and the app passes a *local copy* of `logs_view.drawer_open` because the content closure also borrows `logs_view`; the copy is written back after the frame. **No manual scroll bookkeeping:** egui persists scroll per widget, so `LogsView` holds only `follow`, and the list clears it when `clamped_offset` shows the user is no longer at the bottom. **`Copy visible` needs no action:** it calls `ctx.copy_text`; the action enum is only for what a screen cannot do (`OpenFolder`, the delete confirmation pair, `OpenScreen`, `CloseDrawer`). **egui 0.37 names:** `egui_wants_keyboard_input`, `ScrollAreaOutput::state.clamped_offset()`, `Panel::bottom(..).default_size(180.0)` (which compiles; its *rendering* is still a manual check, R6). **The live tests moved to a drop guard:** `LogsOnFailure` prints the captured buffer only when a test fails, which replaces the `try_init` that would now fail silently — and the app no longer depends on `tracing-subscriber` at all. |

---

## Purpose

This phase adds no product feature. It exists because of a specific failure: the NVIDIA stall in Phase 2.8 took three round trips to diagnose, and the third one only landed because a parse error text happened to survive into a UI message. The app already uses `tracing` at ~25 sites, but the signal never reaches the person debugging.

Three gaps, each independently sufficient to explain that:

1. **The sink was invisible.** `crates/app/src/main.rs` used to initialise `tracing_subscriber::fmt()` with `EnvFilter::from_default_env()` (replaced in WS1). `fmt()` writes to **stderr**, and a windowed `.app` launched from Finder discards stderr. The logger existed; nobody could read it.
2. **The level is off unless an environment variable says otherwise.** With `RUST_LOG` unset the app produced no output during the stall, and setting it for a bundled app means launching from a terminal — a step the user should not have to know.
3. **The events are failure-only.** Almost every call site is `warn!` *at* a failure (`router.rs`, `eightfold.rs`, `router::detail`). There is no per-page line and no walk-chunk line, so "it stalled after 40 jobs" is the most the log can ever say. F10 was diagnosed only because the `serde_json` error text happened to reach the UI inside a fetch error — not because anyone could read a log.

A fourth gap was going to bite next: **the browser backend was mute by construction.** `crates/core/src/backend/obscura.rs` sent the sidecar's stdout and stderr to `Stdio::null()`, and the connection handler discarded its messages. When a Tier 2 fetch failed, the sidecar's own diagnostics were gone. WS2 captures the stderr; the CDP chatter stays unlogged on purpose (see v0.3).

The goal is not "logging". It is: **when something goes wrong during manual testing, the answer is on screen in one click, and it survives a crash.**

### Evidence

- `crates/app/src/main.rs` — the old `fmt()` + `EnvFilter::from_default_env()` subscriber (stderr, no file; replaced by `logging::init` in WS1).
- `crates/core/src/backend/obscura.rs` — the sidecar's `stdout`/`stderr` were `Stdio::null()` and its messages were discarded (stderr is captured in WS2).
- `crates/core/src/portals/service.rs` — `FetchAllPagesReport` carries `stopped_reason`, but only the app's one-line summary reads it.
- `crates/core/src/lib.rs:28-36` — `data_dir()`, with the `CAREER_OPS_DATA_DIR` override that makes file logging testable.
- `crates/app/src/screens/shell.rs:30-41` — the shell is `Panel::left` + `CentralPanel`; a root-level `Panel::bottom` is the natural drawer.
- `crates/app/src/navigation.rs:13-41` — `MainScreen` with `label()`/`all()`, and `all_screens_have_unique_labels` will enforce the new label.
- `crates/core/Cargo.toml` — `tracing` only; `tracing-subscriber` is a workspace dependency already in `Cargo.lock` through the app.

---

## Scope

| In scope | Out of scope |
| :--- | :--- |
| A file log under `data_dir()/logs/` with rotation | Structured JSON logs (Option C) |
| An in-process ring buffer feeding the UI | Spans per task / per-page timing budgets (Option C) |
| `MainScreen::Logs`: level chips, search, pause, copy, open folder, delete | A Settings section for log level (Phase 5 screen) |
| A toggleable bottom drawer, watching a fetch live | Log shipping, metrics, or telemetry of any kind |
| Per-page, per-chunk, and sidecar events | Changing any fetch behavior or stop reason |
| A panic hook that writes to the log | A log *reader* that parses the file |
| `docs/logging.md` and the manual script section | Any retention policy beyond "5 files, 5 MB each" |

---

## Design decisions

### 1. One seam, two sinks, and the UI never reads the file

`tracing` stays the only instrumentation seam. Two `Layer`s subscribe to it:

| Sink | Purpose | Lifetime |
| :--- | :--- | :--- |
| `<data_dir>/logs/career-ops.log` | The durable record: survives a crash, is what a bug report attaches | Rotated, 5 files |
| `LogBuffer` (ring, 2,000 lines) | What the UI renders, live, with structure intact | This session |

The screen must **not** tail the file: that reparses text back into structure the buffer already has, and it loses the level/target/field split the chips and the viewer depend on. If the buffer is empty, the file is still there — the screen says so and offers **Open log folder**.

### 2. Level policy: `info` everywhere, deeper for the app's own targets in the buffer

| Context | File | Buffer |
| :--- | :--- | :--- |
| Release | `info` global | `info` global, `debug` for `career_ops_core` / `career_ops_app` |
| Debug build | same | same |
| `RUST_LOG` set | overrides both | overrides both |

`info` is enough for every event this phase adds (per page, per chunk, per stop reason), so release builds are diagnosable without an environment variable. The buffer takes one step deeper for the app's own targets because it is bounded and cheap, and because a chip that can reveal a captured `debug` line is more useful than one that can only hide `info`. The file stays quiet: `debug` here means roughly one line per enriched description, which would bury the page lines in a 40-job fetch.

**Honest limit, stated in the UI:** a line that never entered the buffer cannot be revealed by the chips. The Logs screen shows its capture level, and `docs/logging.md` says `RUST_LOG=debug` (or a target filter) is how to go deeper. No restart-free level switching.

### 3. The logging module lives in `core`, not `app`

`career_ops_core::logging` owns initialization, the buffer, and the file writer. Core integration tests are where the interesting assertions live (`portals_service.rs` already drives scripted walks), and they cannot depend on the app crate. The app only renders a `LogHandle`.

This adds `tracing-subscriber.workspace = true` to `crates/core/Cargo.toml`. It is **not a new external crate**: the package is already a workspace dependency and already in `Cargo.lock` via the app, so nothing is downloaded — only the dependency edge changes.

The `log` bridge is handled by `SubscriberInitExt::try_init`, which installs `LogTracer` itself (that is what `fmt::init()` used to do). `tracing-log` is therefore **not** a direct dependency; `the_log_bridge_carries_log_crate_records` in `crates/core/tests/logging.rs` pins the behaviour, so a future switch away from `try_init` fails a test rather than silently dropping `sqlx`'s `log` records.

### 4. `LogLine` is structured, because every existing event is

```rust
pub struct LogLine {
    pub seq: u64,
    pub timestamp: DateTime<Local>,
    pub level: tracing::Level,
    pub target: &'static str,
    pub message: String,
    pub fields: Vec<(String, String)>,
}
```

The buffer layer is a `Layer` with a `Visit`, not a `MakeWriter`. A `MakeWriter` would receive only formatted text, and then level filtering would have to re-parse it. Implementing only `Visit::record_debug` is sufficient — `record_str`, `record_i64` and friends default to forwarding to it — which keeps the visitor to one method.

The UI renders `message` followed by `k=v` for each field, which is exactly how the events are written (`page`, `status`, `jobs`, `stop`).

### 5. The file writer is hand-rolled; `tracing-appender` stays out

`MakeWriter` over a `Mutex<File>` opened in append mode, flushing after every line. Volume is tiny (a 50-page walk is ~50 lines), so buffering buys nothing and costs the tail on a crash.

- **Rotation** happens at startup when the current file exceeds **5 MB**: rename to `career-ops.<YYYYMMDD-HHMMSS>.log`, keep the **5** newest. A pure `should_rotate(size, limit)` function makes this testable without sleeping.
- **No `tracing-appender`.** It would add `time` and `crossbeam-channel` for a feature set this phase does not use. Because the boundary is `MakeWriter`, swapping in `tracing_appender::rolling` later is a two-line change — recorded, not needed.
- **A failed write never fails an operation.** Log a warning once, then stay quiet; a full disk must not break a fetch.

### 6. Failure modes are part of the design

- **`init` is idempotent, and order matters once.** Tests share a process, so the global subscriber is installed with `try_init` and the handle lives in a `OnceLock`. `logging::init(LogMode::File)` is called only from `main.rs`; `logging::handle()` lazily installs a buffer-only instance, which is what tests and `CareerOpsApp::new` get. A later `init(LogMode::File)` after `handle()` **cannot add the file sink** — the subscriber is already installed — so it returns the installed handle and warns; `main` must be the first caller, which it is. Tests must assert on content (or a `seq` watermark), never on emptiness.
- **The panic hook opens its own handle.** It writes the payload and `Location` with a fresh `OpenOptions::append` file, so a buffer mutex poisoned by the very panic cannot deadlock the report. It chains to the previous hook so the default behaviour (and `catch_unwind` semantics in tests) is preserved.
- **A wake callback, not a core→egui dependency.** `LogHandle::set_wake(Arc<dyn Fn() + Send + Sync>)`; the app installs `ctx.request_repaint()`. Without it, a line emitted by a background task sits unrendered until the next incidental repaint, and a drawer that only updates when you move the mouse reads as broken.

### 7. The buffer is bounded and honest about it

2,000 lines, dropping the oldest, with a `dropped` counter. When it is non-zero the screen renders "N earlier lines dropped" above the first row. An unbounded buffer in a long session is a slow leak; a silently truncated one is a lie.

The UI snapshots the buffer **only when `latest_seq` changes** (`logs_snapshot` + `logs_snapshot_seq` on the app). Cloning 2,000 lines every frame is pure waste in an immediate-mode UI.

### 8. Pause is a watermark, and one view drives both mounts

**Pause** freezes at a sequence watermark rather than copying the list: the view renders `seq <= watermark`, resume clears it, and the paused header reports "N new lines" from `latest_seq - watermark`. One `LogsView` (level, query, pause, drawer_open, follow) is shared by the screen and the drawer, so pausing while watching the drawer does not leave the screen moving. Each mount pushes its own egui id so the two scroll independently.

### 9. Two mounts of one view

| Surface | Why | Cost |
| :--- | :--- | :--- |
| `MainScreen::Logs` | Reading, searching, copying a report | One enum variant, one screen module |
| Root `Panel::bottom("career_ops_logs_drawer")` | Watching a fetch fail *while* looking at Portals | ~20 lines in `app.rs` |

The drawer is shown in `Screen::Ready` **before** `screens::shell::show_with`, so it claims the bottom strip of the whole window and the sidebar/central panels fill the rest — a console at the bottom of the app, not a panel inside one screen. It is resizable, defaults to 180 px, and closes from its own header, from a sidebar toggle, or from `Cmd/Ctrl+L` (ignored while a text field has focus, via `ctx.wants_keyboard_input()`).

Egui's `Panel::bottom` inside the root `Ui` is the one structural unknown. It is spiked first in WS3; because both mounts share the view-model, dropping the drawer if it misbehaves costs nothing but the toggle.

### 10. The events that make the next stall a 30-second read

| Where | Event | Why |
| :--- | :--- | :--- |
| `acquisition::router::fetch_portal_inner` | per fetch call, `info!`: `portal`, `tier`, `first_page`, `pages`, `jobs`, `details_attempted`, `details_enriched`, `stop`, `elapsed_ms` | The missing signal for F8/F10; it replaces the old enrichment-only line, so a fetch is one line |
| `html` / `eightfold` page loops | per page, `info!`: `page`, `jobs`, `added`, plus `url` and `bytes` (HTML) or `offset` (Eightfold) | Covers both engines, and one query finds both because `page`/`jobs`/`added` are shared |
| `portals::service::fetch_all_pages` | per chunk, `info!`: `portal`, `first_page`, `pages`, `deepest`, `jobs`, `stop` | Turns "it stalled" into "it read 26–29 and stopped at 30" |
| `eightfold::parse_page` | the skip `warn!` gains `index` and `positions`; a second `warn!` carries `positions`/`skipped` | Payload bug vs. transport bug becomes obvious |
| `backend::obscura::spawn_sidecar` | stderr piped and drained into `tracing` at `debug` (module target, so the default policy keeps it in the buffer) | The sidecar stops being a black box |
| `main.rs` | one startup `info!`: version, profile, data dir, log path | Makes every future bug report reproducible |

Per-job enrichment detail stays at `debug`; it is the one event that would otherwise multiply by the detail cap.

### 11. Privacy is a stated rule, not an emergent property

The database is encrypted; a log file is not. The phase states the rule in code, in docs, and on the screen:

- **Never logged:** passphrases, the wrapped key or any key material, CV/profile text, prompts or completions (Phase 3), job description bodies, HTTP response bodies.
- **Logged:** URLs, external ids, counts, byte sizes, HTTP statuses, tiers, stop reasons, error strings.
- **Anything body-adjacent goes through `truncate_for_log(value, 200)`**, which cuts on a character boundary and appends `…`.
- Logs are plaintext and outside the vault. The Logs screen says so in one line alongside the path and offers **Delete logs** (with a confirmation; this is not the Pipeline's typed gate — nothing of value is behind it).
- `docs/logging.md` repeats the list, and Phase 3's plan gains a cross-reference so prompt logging is decided once, deliberately.

### 12. No migration, no new external crate, no behavior change

No schema change. No change to any fetch, stop reason, cursor, or health rule. The only user-visible change outside logs is a **View logs** button next to the Portals error line.

---

## Workstream 1 — Logging core

**Estimated duration:** 1 day.

### Deliverables

1. `crates/core/src/logging/mod.rs`:
   - `pub enum LogMode { File, BufferOnly }`
   - `pub fn init(mode: LogMode) -> LogHandle` — idempotent (`try_init`, `OnceLock`); a second call returns the installed handle.
   - `pub fn handle() -> LogHandle` — lazily installs `BufferOnly`.
   - `pub struct LogHandle { .. }` — `buffer()`, `path() -> Option<&Path>`, `policy()`, `latest_seq()`, `snapshot() -> Vec<LogLine>`, `dropped()`, `set_wake(Arc<dyn Fn() + Send + Sync>)`, `clear()`.
2. `logging/line.rs` — `LogLine` as in decision 4, plus `compact()` (the copyable one-line form) and `timestamp_short()`.
3. `logging/buffer.rs` — `LogBuffer { capacity, dropped, .. }` with `push`, `snapshot`, `latest_seq`, `dropped`, `len`, `clear`; `BufferLayer` + a `Visit` that needs only `record_debug` and `record_str`. The level policy is the subscriber's global `EnvFilter` (`info`, `debug` for `career_ops_core`/`career_ops_app`), with the file layer adding its own `info` filter — so the buffer sees more than the file without any per-event logic.
4. `logging/file.rs` — `LogFile` (`Mutex<File>`, append, flush per write), `MakeWriter` impl, `should_rotate(size, limit)`, `rotate_if_needed(dir, limit)`, `prune_archives(dir, keep)`; log directory `<data_dir>/logs/` created on demand.
5. `logging/panic_hook.rs` — `install_panic_hook(path)` writing payload + location through its own handle, then chaining.
6. `logging::truncate_for_log(value: &str, max_chars: usize) -> String`.
7. Tests (`crates/core/tests/logging.rs`):
   - a line reaches the buffer with its message and fields;
   - the buffer drops the oldest and increments `dropped` at the cap;
   - the level policy admits `debug` for a `career_ops_*` target and hides it for another target;
   - `init` is idempotent and returns the same buffer;
   - `should_rotate` is true only above the limit; rotation keeps the newest five;
   - the panic hook writes a line containing the panic message and file, verified with `catch_unwind` so the test does not abort;
   - `truncate_for_log` cuts on a char boundary and appends `…`;
   - `LogMode::BufferOnly` creates no file.

### Acceptance criteria

| Criterion | Verification |
| :--- | :--- |
| A `tracing` event reaches both sinks with level, target, message, and fields | Buffer test + file test |
| `RUST_LOG` still overrides the default policy | Test setting a target filter |
| The buffer is bounded and reports what it dropped | Cap test |
| Rotation keeps five files and only runs at startup | Unit test with a tempdir |
| A panic writes a readable line without deadlocking | `catch_unwind` test |
| Initialisation is safe from two callers in one process | Idempotence test |
| No new package in `Cargo.lock` | `git diff Cargo.lock` shows only the two dependency edges on the core package entry (`tracing-subscriber`, and `log` as a dev-dependency) |

---

## Workstream 2 — The events

**Estimated duration:** 0.5 day.

### Deliverables

1. One `info!` per fetch call in `router::fetch_portal_inner` (tier, first page, pages read, jobs, details, stop, elapsed). The router does not page — it delegates to the engines — so this is the call-level line, not a per-page one. It replaces the old "enriched job descriptions" line, so a fetch stays one line.
2. Per-page `info!` in the Eightfold list loop and in the HTML page loop, sharing `page`/`jobs`/`added` so one query finds both; the HTML loop adds `url` and `bytes`, the Eightfold loop adds `offset`.
3. Per-chunk `info!` in `portals::service::fetch_all_pages`, with the range and the stop label.
4. `eightfold::parse_page`: the per-posting skip warning names `index` and `positions`; a second warning reports `positions`/`skipped` for the page.
5. `obscura::spawn_sidecar`: `Stdio::piped()` for stderr plus a drain task, `stdout` still null. The target is the module path, so the default debug policy keeps these lines in the buffer; a bare `obscura` target would be filtered out.
6. The startup line in `main.rs`, after `logging::init`, before `AppState::bootstrap`.
7. `PaginationStop::as_str()` as the one stable label used by the router, the walk, and any summary.
8. Tests:
   - `crates/core/tests/portals_service.rs`: `a_walk_logs_the_page_it_stopped_at` — a four-page chunk stopping at `FetchFailed`, asserting the chunk line's range and stop. The walk resumes from page 41 so the assertion cannot pick up a parallel test's line.
   - `crates/core/src/acquisition/html/mod.rs` (unit): `a_page_is_logged_with_its_size_and_job_count`.
   - `crates/core/src/acquisition/eightfold.rs` (unit): `a_skipped_posting_names_its_index_and_the_page_size`, asserting one skip warning and the page summary.
   - `crates/core/src/backend/obscura.rs` (unit): `sidecar_stderr_lines_are_forwarded_to_the_log` drives the drain function over a `tokio::io::duplex` pipe; no child process.

### Acceptance criteria

| Criterion | Verification |
| :--- | :--- |
| One page of a walk produces exactly one `info!` line with page number, URL, size, and job count | Scripted-source test |
| A stalled walk's log names the page range and the stop reason | `a_walk_logs_the_page_it_stopped_at` |
| A parse failure is diagnosable from the log alone (index + page size) | Unit test |
| Sidecar stderr is captured instead of discarded | Drain-function test |
| A 50-page walk adds roughly 50 lines, not 500 | Manual check in the drawer; per-job lines stay `debug` |

---

## Workstream 3 — The Logs screen and the drawer

**Estimated duration:** 1 day.

### Deliverables

1. `crates/app/src/view_models/logs.rs` — pure, no egui:
   - `LevelFilter { Error, Warn, Info, Debug, Trace, All }` with `label()` and `admits(Level)`;
   - `LogsView { level, query, paused_at: Option<u64>, follow: bool, drawer_open: bool }`;
   - `filtered<'a>(&self, lines: &'a [LogLine]) -> impl Iterator<Item = &'a LogLine>`;
   - `new_since(&self, latest_seq: u64) -> u64` for the paused header;
   - `to_plain_text(&self, lines: &[LogLine]) -> String` for **Copy visible**.
2. `crates/app/src/screens/logs.rs`:
   - `show(ui, &[LogLine], &mut LogsView, LogsRenderState) -> Option<LogsAction>` — header row (chips, search, **Pause/Resume**, **Follow**, **Copy visible**, **Open log folder**, **Delete logs**, the capture-level line, the in-memory caveat), then a `ScrollArea` of monospace rows;
   - `drawer(ui, ..)` — the same rows with a compact header and a close button;
   - rows render `HH:MM:SS.mmm LEVEL target message key=value`, with the level coloured, wrapping disabled, horizontal scroll on;
   - follow-the-tail sticks to the bottom unless the user scrolled up (the rule the Pipeline list already uses).
3. `MainScreen::Logs` (+ `label()`, `all()`, the `placeholder` arm); `sidebar()` draws a separator before it.
4. Root drawer in `CareerOpsApp::ui`'s `Screen::Ready` branch, before `shell::show_with`: `egui::Panel::bottom("career_ops_logs_drawer").resizable(true).default_height(180.0)`, gated on `logs_view.drawer_open`.
5. Sidebar footer: a **Logs drawer** checkbox, plus `Cmd/Ctrl+L` toggling it when no text field has focus.
6. `PortalsAction::ShowLogs` and a **View logs** button beside the error line and beside a stalled-walk message; the handler navigates to `MainScreen::Logs` and closes the drawer.
7. Snapshot polling: refresh `logs_snapshot` only when `latest_seq` changes; install the wake callback in `CareerOpsApp::new`.
8. **Migrate the ignored live tests off their own subscriber.** `live_fetch_reaches_the_pipeline`, `live_fetch_with_pagination_and_details`, and `live_registry_sites_reach_the_pipeline` currently `try_init` a stderr `fmt` subscriber to make `RUST_LOG` visible. Once `CareerOpsApp::new` installs the global subscriber, that `try_init` fails and their diagnostics silently go to the buffer instead. They must read `logging::handle().snapshot()` (or print the buffer on failure) rather than installing a second subscriber.
9. `crates/app/tests/logs_screen.rs`:
   - lines render with their `k=v` fields;
   - a level chip narrows the list and **All** restores it;
   - the search matches message, target, and field values;
   - **Pause** freezes the rendered lines and reports "N new lines"; **Resume** catches up;
   - the drawer opens from the sidebar toggle and closes from its header;
   - **View logs** from a Portals error navigates to `MainScreen::Logs`;
   - the "earlier lines dropped" marker appears when `dropped > 0`;
   - `MainScreen::Logs` has a unique label (existing `all_screens_have_unique_labels` test).
9. `crates/app/tests/portals_screen.rs`: the **View logs** button emits `PortalsAction::ShowLogs`.

### Acceptance criteria

| Criterion | Verification |
| :--- | :--- |
| The Logs screen is reachable from the sidebar in release builds and shows live lines | `egui_kittest` + manual |
| Chips, search, and pause all operate on captured lines, and the capture level is stated | `egui_kittest` |
| The drawer can be opened and closed without disturbing the active screen | `egui_kittest` + manual |
| A failed portal fetch is one click from its log | `egui_kittest` |
| Nothing in the screen reads or tails the log file | Code review; the screen takes `&[LogLine]` only |

---

## Workstream 4 — Docs, verification, retrospective

**Estimated duration:** 0.5 day.

1. `docs/logging.md`: the two sinks, the level policy and `RUST_LOG`, the file location and rotation, the never-log list, the drawer/screen, and how to attach a log to a bug report.
2. `docs/manual-test-script.md`: new **§9 Logs and diagnostics** (drawer toggle, screen, chips, pause, copy, open folder, delete, and "reproduce a failed fetch and read the line that names the page"); the sign-off section becomes **§10**.
3. `docs/patterns.md`: a line on the root-level `Panel::bottom` drawer and the follow-the-tail scroll rule, next to the existing `Panel::left` note.
4. `docs/LIVE_TESTS.md`: unchanged — this phase adds no network path.
5. Validation: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets --offline -- -D warnings`, `cargo test --workspace --features career-ops-app/keychain --offline -- --skip obscura`.
6. Parent plan: v0.16 changelog row, the Phase 2.8.1 row in §12, the **Logs** row in §3's screen table, the estimate sentence, and Q-44/Q-45 in Appendix B.
7. Short retrospective in `.agents/plans/phase_2.8.1_retrospective.md`.

---

## Test Strategy

| Layer | Covers |
| :--- | :--- |
| Core unit | Buffer cap, level policy, rotation decision, truncation, panic hook, idempotent init |
| Core integration | A scripted walk logs the page it stopped at; a parse failure is readable with its offset |
| `egui_kittest` | Chips, search, pause watermark, dropped marker, drawer, **View logs** |
| App | Snapshot polling and the wake callback; drawer toggle through the app |
| Manual | §9: reproduce a failed fetch and read the line; drawer during a walk |
| Live | None. No new network path. |

No network for any new test. No new external dependency. No migration.

---

## Risks

| ID | Risk | Impact | Mitigation |
| :--- | :--- | :--- | :--- |
| P2.8.1-R1 | **The buffer floods and hides the signal.** A `debug` app line per enriched job is 40 lines per fetch. | Medium | Per-job lines stay `debug`; `info` is per page. Chips narrow; the marker says what was dropped. |
| P2.8.1-R2 | **File writes on the calling thread stall the UI** on a slow disk. | Low | Per-line volume is tiny and the file is small; if profiling ever shows it, move the writer behind a channel. Documented, not fixed. |
| P2.8.1-R3 | **Sensitive text leaks into a log later** (Phase 3 prompts are the obvious candidate). | High | The never-log list, `truncate_for_log`, a review checklist item, and a cross-reference in Phase 3's plan. |
| P2.8.1-R4 | **The panic hook deadlocks** on a poisoned buffer mutex. | Medium | The hook opens its own file handle and never touches the buffer lock. |
| P2.8.1-R5 | **The in-memory buffer is mistaken for the record.** | Low | The screen says "this session"; the path and **Open log folder** are always visible. |
| P2.8.1-R6 | **`Panel::bottom` inside the root `Ui` does not draw as a drawer** at egui 0.37. | Medium | Spiked first in WS3; both mounts share the view-model, so dropping the drawer costs only the toggle. |
| P2.8.1-R7 | **Global subscriber state leaks between tests.** | Medium | `init` is idempotent and returns the same buffer; tests assert content or a `seq` watermark, never emptiness. |
| P2.8.1-R8 | **Replacing `fmt::init()` silently drops `log`-crate records** (some `sqlx` paths). | Low | `try_init` installs the `LogTracer` bridge; `the_log_bridge_carries_log_crate_records` fails if that ever stops being true. |

---

## Exit Criteria

1. A `tracing` event reaches both the buffer and the file, with level, target, message, and fields.
2. Release builds log at `info` with no environment variable; `RUST_LOG` still overrides.
3. The file lives at `<data_dir>/logs/`, rotates at 5 MB keeping five files, and flushes per line.
4. A panic writes a line with its location to the file without deadlocking.
5. **Logs** is a sidebar screen: level chips, search, pause with a new-line count, copy, open folder, delete, dropped-line marker.
6. The drawer opens from the sidebar toggle and `Cmd/Ctrl+L`, renders the same view, and does not disturb the active screen.
7. **View logs** beside a Portals error and a stalled walk navigates to the screen.
8. A completed or stalled walk logs one line per page and one per chunk, naming the page and the stop reason.
9. Sidecar stderr is captured instead of discarded.
10. No migration, no new external package, no change to fetch behavior; `Cargo.lock` gains only dependency edges on the core package, no new entries.
11. Offline suite, fmt, and clippy are clean.
12. `docs/logging.md`, manual script §9, `docs/patterns.md`, the parent plan, and a retrospective are updated.

---

## Open Questions

| # | Question | Position |
| :--- | :--- | :--- |
| Q-44 | **Should logs be structured JSON and exportable as a diagnostic bundle?** | Deferred to Option C (below). The transcript viewer is the higher-value half for one user on one machine; JSON pays off when logs are aggregated, which this app does not do. |
| Q-45 | **Should the log level be settable from Settings instead of `RUST_LOG`?** | Not now. Settings is still a placeholder (Phase 2.9 adds one section, Phase 5 completes it). Until then the Logs screen states its capture level and points at `RUST_LOG`. Revisit in Phase 5's error-handling audit. |
| Q-46 | **Should the drawer also show progress (fetch, batch, save) alongside log lines?** | No. Progress already has a home in each screen's header; mixing it into the drawer would duplicate state and make the log harder to scan. |
| Q-47 | **Should a terminal run also echo to stderr?** | Not in this phase. `main` used to write to stderr; it now writes the file and the buffer, and the file is `tail -f`-able. Echoing to stderr when `std::io::IsTerminal` would need an optional boxed layer and buys nothing the file does not already give. Revisit if live-test or CI diagnosis turns out to want it. |

---

## Deferred: Option C (observability)

Recorded here so the decision is not re-litigated from scratch. **Not in this phase.**

| Item | Why it belongs with the others | Trigger to revisit |
| :--- | :--- | :--- |
| JSON sink (`tracing_subscriber::fmt::layer().json()`) | Same seam, different encoder; only worth it once something reads JSON | Log shipping or multi-machine debugging |
| Spans per task (`#[instrument]` on fetch, walk, enrichment, LLM calls) | Turns "two lines with the same portal" into one timeline | Phase 3's per-call cost and latency accounting |
| Per-call cost/timing records | Phase 3 needs `input_chars`, `elapsed`, `estimated/actual cost` — spans are the natural carrier | Phase 3 |
| Diagnostic bundle export (zip of logs + version + schema version + redacted config) | Support workflow; nothing to support yet beyond one user | Before Phase 6's release candidate |
| Retention policy and log-level controls in Settings | Needs the Settings screen | Phase 5 |

The rule for adding it later: it must not change the events this phase adds. JSON and spans are encoders around the same call sites, which is the payoff of keeping `tracing` as the single seam.

---

## What Phase 2.8.1 Is Not

- Not an observability platform. Two sinks, one screen, one drawer.
- Not a change to acquisition: no fetch, stop reason, cursor, or health rule moves.
- Not a metrics system: no counters, no timing dashboards, no telemetry that leaves the machine.
- Not a Settings section: the log path and level are stated where the logs are read.
- Not a replacement for the error messages the app already shows. It is the detail behind them.
