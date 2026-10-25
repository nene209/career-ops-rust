# Phase 2.1: UI Shell — Detailed Execution Plan

**Parent plan:** Draft Execution Plan: Career-Ops Rust Desktop Application (v0.5), §12
**Status:** Complete — all five workstreams implemented (2026-10-25); two exit criteria need a human with network access (see `phase_2.1_retrospective.md`)
**Last updated:** 2026-10-25
**Duration:** 1.5–2 weeks
**Depends on:** Phase 2 completion (2026-10-23) and Phase 2.01 (2026-10-24)

---

## Change Log

| Version | Change |
| :--- | :--- |
| v0.1 | Initial Phase 2.1 plan written against GPUI. |
| v0.2 | Rewritten against egui/eframe after Phase 2.01 found GPUI's test harness and API stability unsuitable for a five-phase dependency. View-model, navigation, and async bridge survive unchanged. View layer, test patterns, and toolchain requirements are replaced. The accessibility gap found during the egui prototype (text inputs have no accessible label) is addressed as an implementation note and a Phase 5 audit item. |
| v0.3 | Entry conditions updated to record the actual repo state at entry: `main.rs` is a state-bootstrap placeholder, the framework-independent modules from the aborted GPUI start survive, and `screens` is a directory module. Added WS1 deliverable 3a: `data_dir()` cleanup, replacing the `CAREER_OPS_DATA_DIR` env-var requirement with a `directories`-crate lookup. Toolchain requirement reworded to defer to the workspace `rust-version` field rather than pinning 1.80. Added a clean-machine acceptance criterion for WS1. |
| v0.4 | Workstream 1 implemented and the plan corrected against the actual egui/eframe 0.37 API: `eframe::App` exposes `ui(&mut self, &mut egui::Ui, ...)`, not `update`; `SidePanel`/`TopBottomPanel` are replaced by `Panel::left` and friends; `Response::labelled_by(Id)` exists at 0.37, so the accessible-input workaround is a real association rather than a documented limitation. Added `CareerOpsApp::spawn` because `AsyncTask::spawn` calls `tokio::spawn` and panics outside a runtime context. Recorded the WS1 implementation files and tests. |
| v0.5 | Workstream 2 implemented: unlock, first-launch wizard, and recovery screens; filesystem recovery actions (locate key file, export encrypted bundle, explicit reset); async state hand-off. Recorded that `AppState` is moved into the task rather than borrowed across an await, that secrets are cleared when the task takes ownership, and that a partially completed first launch is re-bootstrapped to surface Recovery. Added `screens/lock.rs`, `recovery.rs`, and `tests/lock_screens.rs`. |
| v0.6 | Workstream 3 implemented: Portals list with health badges, CRUD forms, delete confirmation, fetch-one, cancellable batch with `mpsc` progress, sidecar indicator, and the manual-entry modal. Added `portals::service::fetch_batch_cancellable` because the callback cannot stop `fetch_batch`'s loop. The Portals view model carries `render_mode` and `scraper_config` so the edit form round-trips them without a second read. Added `view_models/portals.rs`, `screens/portals.rs`, and `tests/portals_screen.rs`; app-level tests cover CRUD and manual paste against a real database. |
| v0.7 | Workstream 4 implemented: Pipeline list with tier labels and application-status badges, four client-side filters, read-only detail view with disabled Phase 3 placeholders, and a bounded 500-row load. The manual modal is now app-level chrome (`portals::show_modal`), rendered after the active screen so "Add job" works from Pipeline without a second implementation; `show` no longer takes `PortalsView`. `PipelineViewModel::from_jobs` takes the application statuses as a second argument so the join happens in the load task, and `replace_jobs` preserves the user's filter across refreshes. Added `tests/pipeline_screen.rs` and an app-level test that loads a real database, joins a status, and checks that a manual add invalidates the pipeline page. |
| v0.8 | Workstream 5: verification pass and retrospective. `docs/manual-test-script.md` created (criterion 14's document half; execution of the network rows is pending). The pass found four gaps. Two were closed with tests: five small functions had no unit test (`PortalsViewModel::replace_entries`, both `row` lookups, `HealthBadge::cause`, `TierLabel::label`), and nothing drove Recovery → `LocateKeyFile` → `Unlocking` → `Ready` end to end. The third was the missing manual script. The fourth was documentation drift: criterion 4 is reworded because `MainScreen` has five screens and no Dashboard, whose content is Pipeline's. Added `.agents/plans/phase_2.1_retrospective.md`, which also records P2-R9's resolution. |

---

## Purpose

Phase 2 delivered a service layer — `list_with_health`, `fetch_one`, `fetch_batch`, `add_job_from_url`, `add_job_from_text` — with no consumer. Phase 2.1 makes the application runnable by building the shell that the user interacts with, wiring the Phase 1 unlock flow to real screens, and consuming the Phase 2 service layer from the Portals and Pipeline screens.

The phase has three defining characteristics:

1. **First real consumer of the service layer.** If `PortalHealth` has a field the UI cannot use, or lacks a field the UI needs, this phase will find out. That is the point.
2. **Establishes the view patterns Phase 3, 4, and 5 reuse.** List, form, async-with-progress, navigation, status indicator. Inventing them once is cheaper than inventing them per-screen across three phases.
3. **Concentrates the framework risk.** The framework was chosen in Phase 2.01 after four prototypes. This phase is the first real test of that choice. If egui turns out to be wrong for a data-dense application, the cost of finding out is two weeks and a small set of screens, not five phases.

---

## Entry Conditions

Phase 2.1 begins after Phase 2.01 (confirmed 2026-10-24). Consumed outputs:

| Source | How Phase 2.1 Uses It |
| :--- | :--- |
| `portals::service::list_with_health` | Portals screen data source |
| `portals::service::fetch_one`, `fetch_batch` | Portals action handlers |
| `portals::service::FetchReport`, `BatchReport`, `BatchEntry` | Shapes the view renders |
| `portals::health::PortalHealth`, `HealthState`, `suggested_action` | Health badges and drill-down text |
| `acquisition::manual::add_job_from_url`, `add_job_from_text` | Manual entry form handlers |
| `repo::job_descriptions::list_recent` | Pipeline screen data source |
| `repo::portals::create`, `update`, `delete` | Portal CRUD from the UI |
| `ObscuraBrowserBackend` | Constructed by the app, passed into `fetch_one`/`fetch_batch` |
| Phase 1 `AppState` state machine | Drives unlock → first-launch → recovery → ready transitions |
| Phase 2.01 recommendation | egui/eframe as the framework |

**Repo state at entry.** The tree is not empty. Three things from the aborted GPUI start survive, and one thing needs replacing:

| Item | State at entry | What Phase 2.1 does with it |
| :--- | :--- | :--- |
| `crates/app/src/main.rs` | State-bootstrap placeholder — builds `AppState`, prints the current screen to stderr, exits | **Replaced** by WS1 with the `eframe::run_native` entry point |
| `crates/app/src/state.rs` | Complete `AppState` state machine, tested | Unchanged |
| `crates/app/src/navigation.rs` | `MainScreen` enum and `NavigationState`, tested | Unchanged; consumed by WS1's sidebar |
| `crates/app/src/async_bridge.rs` | `AsyncTask<T>` primitive, tested | Unchanged; consumed by WS2 and WS3 |
| `crates/app/src/view_models/mod.rs` | Empty placeholder with commented module declarations | WS3 and WS4 add `portals.rs` and `pipeline.rs` |
| `crates/app/src/screens/` | Empty directory module (`screens/mod.rs`) | WS1–WS4 add submodules; the Phase 1 `screens.rs` file must not also exist |
| `career_ops_core::data_dir()` | Reads `CAREER_OPS_DATA_DIR`, panics if unset | **Replaced** by WS1 deliverable 3a |

The three framework-independent modules — `state`, `navigation`, `async_bridge` — are the payoff from the aborted GPUI start. They compile today and need no changes. The framework swap in Phase 2.01 touched only the view layer and the test patterns.

**Toolchain requirement:** the workspace's `rust-version` field in the root `Cargo.toml` is authoritative. The 1.95.0 requirement from the GPUI version of this plan no longer applies, because GPUI's `std::hint::cold_path` dependency is gone. Verify the field before pinning anything, and raise it if an egui/eframe dependency requires more. Do not lower it below whatever `career-ops-core` already requires.

**Framework versions:** `egui 0.37`, `eframe 0.37`, `egui_kittest 0.37` (with `snapshot` and `wgpu` features). These are the versions current as of the Phase 2.01 evaluation. Pin them in `Cargo.toml`.

---

## Scope

| In scope | Out of scope |
| :--- | :--- |
| `eframe::App` implementation, window, sidebar, content panel | Visual polish, theming, animations (Phase 5) |
| Async bridge for tokio → eframe update loop | Settings screen (Phase 5) |
| Unlock, first-launch, and recovery screens | Accessibility compliance beyond the input-label fix (Phase 5) |
| Portals screen: list with health, add/edit/delete, fetch one, batch with progress | Evaluation UI (Phase 3) |
| Pipeline screen: job list, filtering | CV editor (Phase 4) |
| Manual entry form: URL fetch and paste | PDF preview (Phase 4) |
| Sidecar status indicator | Prompt management (Phase 3) |
| View-model layer with unit tests | Onboarding wizard beyond first-launch (Phase 5) |
| `data_dir()` lookup via `directories` | Full keyboard navigation (Phase 5) |
| Verification pass | |

---

## Sequencing Rationale

```
W1: WS1 Shell infrastructure + WS2 Unlock/first-launch/recovery
    └── W2: WS3 Portals screen + WS4 Pipeline screen and manual entry
        └── W2.5: WS5 Verification pass and retrospective
```

**Shell infrastructure comes first** because every other workstream builds on it. If the async bridge or the panel layout is wrong, every screen changes.

**Unlock flow comes next** because it is the entry point. The state machine already decides the transitions; the shell renders them.

**Portals before Pipeline** because Portals is the harder screen — health badges, batch fetch, sidecar status — and it consumes more of the service layer. If the service layer has gaps, Portals finds them first.

**Verification pass is folded into the second half of Week 2.** UI phases are especially prone to "the test passes but the screen is blank." The verification pass is where that is caught.

---

## Workstream 1 — Shell Infrastructure

**Priority:** First.
**Estimated duration:** 2–3 days

### Deliverables

1. **`eframe::App` implementation.** The binary's `main.rs` replaces the Phase 1 state-bootstrap placeholder with `eframe::run_native`. `CareerOpsApp` lives in `crates/app/src/app.rs` so tests can construct it without spawning the binary. It holds the Phase 1 `AppState`, the `NavigationState` from `navigation.rs`, and a `tokio::runtime::Runtime`, and implements `eframe::App::ui`:

   ```rust
   fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame)
   ```

   The runtime is constructed in `main` before `run_native` and moved into the app, so its lifetime is tied to the process. Later workstreams must spawn through `CareerOpsApp::spawn`, which enters the runtime before calling `AsyncTask::spawn`; calling `AsyncTask::spawn` directly from `ui()` panics because the eframe UI thread is not a tokio worker thread.

   `main.rs` at the end of this deliverable:

   ```rust
   fn main() -> anyhow::Result<()> {
       tracing_subscriber::fmt()
           .with_env_filter(EnvFilter::from_default_env())
           .init();

       let state = AppState::bootstrap(career_ops_core::data_dir())?;
       let runtime = tokio::runtime::Builder::new_multi_thread()
           .enable_all()
           .build()?;

       let options = eframe::NativeOptions {
           viewport: egui::ViewportBuilder::default()
               .with_inner_size([1024.0, 720.0])
               .with_min_inner_size([640.0, 480.0]),
           ..Default::default()
       };

       eframe::run_native(
           "Career-Ops",
           options,
           Box::new(move |_cc| Ok(Box::new(CareerOpsApp::new(state, runtime)))),
       )?;

       Ok(())
   }
   ```

   `main` returns `anyhow::Result`, not `eframe::Result`: `AppState::bootstrap` returns `CoreError`, which `eframe::Error` does not absorb. The `?` on `run_native` still works because `anyhow` accepts `eframe::Error`.

2. **Panel layout.** egui 0.37 renamed `SidePanel`/`TopBottomPanel` to `Panel`; `SidePanel` no longer exists. Use `egui::Panel::left` for the navigation sidebar, with one `selectable_label` per `MainScreen` variant, and `egui::CentralPanel` for the currently-selected screen. Both `show` methods take the root `&mut egui::Ui` that `eframe::App::ui` receives, not a `Context`. The two panels are the frame for every screen that follows.

3. **Async bridge for the eframe update loop.** The `AsyncTask` type from `async_bridge.rs` survives unchanged. The pattern that connects it to eframe is:

   ```rust
   // In CareerOpsApp::ui (eframe 0.37 gives a &mut egui::Ui, not a Context)
   if let Some(task) = &mut self.pending_fetch {
       match task.try_recv() {
           TaskPoll::Ready(report) => {
               self.apply_fetch_result(report);
               self.pending_fetch = None;
               ui.ctx().request_repaint();
           }
           TaskPoll::Pending => {
               // Ask egui to re-run ui() soon so we poll again.
               ui.ctx().request_repaint_after(Duration::from_millis(50));
           }
           TaskPoll::Cancelled => { self.pending_fetch = None; }
       }
   }
   ```

   The key difference from GPUI: egui does not call `ui()` continuously by default. `request_repaint_after` schedules the next poll; without it, the event loop sleeps and the task result is never observed. That is the one non-obvious thing about integrating async with eframe, and it is documented in `patterns.md`.

   Spawning has a second trap: `AsyncTask::spawn` calls `tokio::spawn`, which requires the current thread to be inside a runtime context. The eframe UI thread is not. Use `CareerOpsApp::spawn(future)`, which enters `self.runtime` before delegating, or enter the runtime explicitly with `let _guard = self.runtime.enter();`.

3a. **`data_dir()` cleanup.** The Phase 1 placeholder reads `CAREER_OPS_DATA_DIR` from the environment and panics if it is unset. Replace it with a `directories`-crate lookup for the OS-appropriate app data directory, with the env var retained as an override for development:

   ```rust
   pub fn data_dir() -> std::path::PathBuf {
       if let Some(dir) = std::env::var_os("CAREER_OPS_DATA_DIR") {
           return std::path::PathBuf::from(dir);
       }
       directories::ProjectDirs::from("", "", "career-ops")
           .map(|d| d.data_dir().to_path_buf())
           .unwrap_or_else(|| std::path::PathBuf::from("."))
   }
   ```

   Add `directories = "5"` to `crates/core/Cargo.toml`. Without this change, the shell cannot be demonstrated on a clean machine without setting the env var on every invocation, which is not the behaviour of a shipped desktop app. The WS1 acceptance criterion "opens on a clean machine" depends on this deliverable.

4. **Three shared components:**
   - `EmptyState` — an icon, a message, an optional action button. Used by Portals ("no portals configured"), Pipeline ("no jobs yet"), and any future screen with a list.
   - `StatusBadge` — a colored label with an icon and text. Variants for the four `HealthState` values plus the sidecar `NotStarted`/`Running`/`Stopped`.
   - `LoadingSpinner` — `egui::Spinner` wrapped in a helper that matches the project's spacing.

5. **Accessible-input helper.** egui's `TextEdit` does not expose `hint_text` as an accessible label — a gap found during the Phase 2.01 prototype and confirmed in the accessibility tree. A helper function:

   ```rust
   pub fn labelled_input(
       ui: &mut egui::Ui,
       label: &str,
       value: &mut String,
       password: bool,
   ) -> egui::Response
   ```

   renders `ui.label(label)` and then a `TextEdit`, and associates them so a screen reader reads the label with the field. egui 0.37 provides `Response::labelled_by(Id)`, verified against the crate source; the helper returns `ui.add(edit).labelled_by(label_response.id)`. No fallback or Phase 5 workaround is needed. The helper remains the single place the pattern lives, so a later API change updates one function.

6. **`crates/app/docs/patterns.md`** documenting the list, form, and async-with-progress patterns against egui. These are the contract Phase 3, 4, and 5 add screens against. The async pattern must call out `request_repaint_after` explicitly, because omitting it produces a UI that silently never updates.

### Acceptance Criteria

| Criterion | Verification |
| :--- | :--- |
| `cargo run -p career-ops-app` opens a window | Manual test |
| `cargo run -p career-ops-app` opens without requiring any env var to be set | Manual test on a clean checkout |
| Sidebar navigation changes the rendered screen | Manual test |
| `EmptyState`, `StatusBadge`, `LoadingSpinner` render in isolation | Manual test on a scratch screen |
| Async task completes and the UI reflects the result | Unit tests on `AsyncTask` and `CareerOpsApp::spawn` plus manual test |
| Idle CPU is near zero with no async task running | Manual test — `top` or `htop`, 30-second sample |
| `labelled_input` produces an accessible label | `egui_kittest` test asserting the input is queryable by label |
| `patterns.md` documents all three patterns, including the repaint requirement | Documentation review |

**Implementation note (2026-10-24).** Workstream 1 landed as `crates/app/src/app.rs` (`CareerOpsApp`), `crates/app/src/screens/components.rs` (shared components), `crates/app/src/screens/shell.rs` (sidebar + content), `crates/app/src/screens/lock.rs` (pre-`Ready` placeholders), and `crates/app/tests/ui_shell.rs`. The snippets above are the code that compiled. The changes from earlier plan text are API corrections, not design changes: `App::ui` instead of `App::update`, `Panel::left` instead of `SidePanel`, and `CareerOpsApp::spawn` to enter the runtime before `AsyncTask::spawn`.

---

## Workstream 2 — Unlock, First-Launch, Recovery Screens

**Priority:** After WS1.
**Estimated duration:** 2–3 days

### Deliverables

1. **Unlock screen.** Rendered when `AppState::current() == Screen::Unlocking`. Uses `labelled_input` for the passphrase field, two buttons, and an error area. The "Unlock" button calls `AppState::try_passphrase` via `AsyncTask`; the "Try keychain" button calls `AppState::try_keychain`.

2. **First-launch wizard.** Rendered when `AppState::current() == Screen::FirstLaunch`. Three steps as a single `egui::Window` with a step indicator:
   - Welcome — one paragraph explaining what the application does and that data is encrypted at rest.
   - Passphrase creation — two `labelled_input` fields (passphrase, confirmation), strength feedback, and a typed acknowledgement phrase ("I understand that if I lose my passphrase and my keychain entry, my data cannot be recovered"). The Continue button stays disabled until the acknowledgement matches.
   - Backup guidance — explains that `dek.wrapped.json` and `career-ops.db` should be backed up together, with a button that opens the data directory.

   On completion, calls `AppState::complete_first_launch(passphrase)` via `AsyncTask` and transitions.

3. **Recovery screen.** Rendered when `AppState::current() == Screen::Recovery { reason }`. Displays the reason in a header, then five cards for the options from §7 of the main plan. The "Start fresh" card requires a typed confirmation before its button enables.

4. **The "ready" shell.** Once `AppState::current() == Screen::Ready`, the main layout renders with the sidebar and the currently-selected screen.

### Critical implementation notes

1. **The state machine is the source of truth.** Screens render `AppState::current()`; they do not maintain navigation state of their own. When `AppState` transitions, `ui()` renders the new state on the next frame.

2. **The passphrase is never logged.** Grep `tracing::debug!` calls in the shell before merging. Also confirm the passphrase `String` is cleared from the view-model after unlock — egui's `TextEdit` keeps its binding alive, so an explicit `passphrase.clear()` after success is worth doing.

3. **First-launch acknowledgement is typed, not checked.** The typed-phrase requirement is deliberate friction from the Phase 1 plan. A checkbox is skippable by reflex; typing is not.

4. **Recovery is not a modal.** It is a full-screen flow, because a user seeing it is already worried and a modal over a locked shell is more alarming than a calm full-screen explanation.

5. **The state moves into the async task, not into a mutex.** `AppState` methods are async `&mut self`, while `AsyncTask` requires a `'static` future. `CareerOpsApp` holds `Option<AppState>`, moves it into the task, and restores it with the result. While the task runs, the UI shows a spinner. Do not wrap the state in a mutex and hold it across an await. If the task is interrupted, re-bootstrap from the data directory; if that fails, show the fatal screen rather than spinning forever.

6. **Secrets are cleared when the task starts.** The view-model copies of the passphrase, confirmation, and acknowledgement are cleared immediately after the action is captured. The task's owned copy is cleared after it is used. Nothing logs a passphrase.

### Acceptance Criteria

| Criterion | Verification |
| :--- | :--- |
| Window opens to unlock when `dek.wrapped.json` exists | Manual test |
| Window opens to first-launch when no files exist | Manual test |
| Window opens to recovery when unlock fails | Manual test |
| Successful unlock reaches the main layout | `CareerOpsApp` async test reaches `Ready`; `ui_shell` test renders the layout |
| First-launch completion reaches the main layout | `CareerOpsApp` async test reaches `Ready`; wizard test covers the gate |
| Recovery screen offers all five options | `egui_kittest` test plus manual test |
| Typed acknowledgement is required | `egui_kittest` test asserting the button is disabled until the phrase matches |
| No passphrase appears in logs | Code review + grep |

**Implementation note (2026-10-24).** Workstream 2 landed as `crates/app/src/screens/lock.rs` (unlock, wizard, recovery, `LockAction`), `crates/app/src/recovery.rs` (opening the data directory, locating a key file, exporting the bundle, and the confirmed reset), and `crates/app/tests/lock_screens.rs`. `CareerOpsApp` now stores `Option<AppState>` and moves it into each async lock operation; `app.rs` tests cover first-launch and returning-user unlock through that path. The recovery screen wires passphrase unlock and keychain retry to the state machine, and the locate/export/reset actions to the filesystem helpers. The locate action validates that the selected file is a wrapped-key bundle before copying it, and the first-launch passphrase is retained until success so a failed attempt can be retried without an empty-passphrase path.

---

## Workstream 3 — Portals Screen

**Priority:** After WS2. The primary consumer of the service layer.
**Estimated duration:** 3–4 days

### Deliverables

1. **Portals list.** Renders `portals::service::list_with_health` via a `PortalsViewModel`. Each row shows:
   - Company name
   - URL
   - Health badge
   - Last attempt timestamp (relative)
   - Suggested action (Degraded/Broken only, as an inline hint)

   egui's `egui_extras::TableBuilder` provides virtualized row rendering if the list grows; for Phase 2.1's expected portal count (a dozen or fewer), a plain `ScrollArea` with `ui.horizontal` per row is simpler and correct. Note the swap point in `patterns.md` so Phase 3's Pipeline (which will have many more rows) uses the right primitive.

2. **Add/edit portal forms.** `egui::Window` modals with `labelled_input` fields and a `ComboBox` for render mode. Submit calls `portals::create` / `portals::update` via `AsyncTask`.

3. **Delete confirmation.** `egui::Window` with a two-button confirmation.

4. **"Fetch now" per row.** Small `Fetch`/`Edit`/`Delete` buttons (three actions did not justify a menu button). Calls `portals::service::fetch_one`. During the fetch, the row shows `LoadingSpinner` in place of the health badge. On completion, the row updates from `FetchReport.portal`.

5. **"Fetch all" batch.** A button in the screen header. Calls `portals::service::fetch_batch_cancellable` (added in Workstream 3 because the callback cannot stop `fetch_batch`'s loop). Because the service takes an `FnMut(&BatchEntry)` callback, the callback sends entries through an `mpsc` channel that `ui()` drains each frame. That is the eframe equivalent of the callback: the router's plain-closure design pays off here.

   During the batch:
   - Each row shows the batch state (spinner or result)
   - A progress bar in the header shows `completed / total`
   - A "Cancel" button sets a shared `AtomicBool` that `fetch_batch_cancellable` checks between portals; the batch stops after the current fetch

6. **Sidecar status indicator.** A small badge in the screen header showing the last Tier 2 fetch outcome: "Sidecar: not started" before any, "Sidecar: running" after a successful Tier 2 fetch, "Sidecar: stopped" after a failed one. Coarse by design; Phase 5's Settings screen will replace it with real status from the backend.

7. **Manual entry modal.** Reachable from both Portals and Pipeline. Two tabs:
   - From URL — `labelled_input` for the URL, "Fetch and add" button, calls `add_job_from_url`. On success, shows the job's title and company plus a "View in Pipeline" link. On failure, shows the error message returned by the service. Mapping that to `suggested_action` would require the service to preserve the classified cause; that is a candidate follow-up.
   - Paste text — `labelled_input` fields for title and company, `labelled_multiline` for the body. Submit calls `add_job_from_text`.

### View models (survive unchanged from the GPUI version)

`crates/app/src/view_models/portals.rs`:

```rust
pub struct PortalsViewModel {
    pub rows: Vec<PortalRow>,
    pub sidecar: SidecarStatus,
    pub batch: Option<BatchProgress>,
}

pub struct PortalRow {
    pub id: i64,
    pub name: String,
    pub url: String,
    // Added during implementation: the edit form round-trips these without a
    // second database read and without dropping the scraper configuration.
    pub render_mode: String,
    pub scraper_config: Option<String>,
    pub health: HealthBadge,
    pub last_attempt: Option<String>,
    pub suggested_action: Option<String>,
}

pub enum HealthBadge {
    NeverFetched,
    Healthy,
    Degraded { cause: String },
    Broken { cause: String },
}

pub enum SidecarStatus {
    NotStarted,
    Running,
    Stopped,
}

pub struct BatchProgress {
    pub total: usize,
    pub completed: Vec<BatchEntry>,
    pub in_flight: Option<i64>,
    pub cancelled: bool,
}

impl PortalsViewModel {
    pub fn from_entries(entries: Vec<PortalWithHealth>) -> Self;
    pub fn apply_fetch_result(&mut self, portal_id: i64, report: &FetchReport);
    pub fn apply_batch_entry(&mut self, entry: &BatchEntry);
}
```

These are framework-independent. They compile today. Every function has a unit test that constructs inputs by hand and asserts on outputs.

### Critical implementation notes

1. **The health drill-down uses `suggested_action` directly.** No reimplementation of message-mapping in the view.

2. **Batch progress flows through an `mpsc` channel.** `fetch_batch_cancellable`'s `FnMut(&BatchEntry)` callback sends on `std::sync::mpsc::Sender`; `ui()` drains with `try_iter()` each frame. `ui.ctx().request_repaint_after` keeps the frame loop live while the batch runs.

3. **Cancellation uses a shared `AtomicBool`.** The callback cannot stop `fetch_batch`'s loop, so Workstream 3 adds `fetch_batch_cancellable`, which checks the flag before starting each portal. A fetch in progress finishes first — that is coarse but correct. The callback still records every completed entry, so the progress bar and rows stay current.

4. **`egui_extras::TableBuilder` is available but not used in Phase 2.1.** A dozen portals do not need virtualization. Phase 3's Pipeline screen will, and that is when the primitive changes. `patterns.md` records the swap point.

### Acceptance Criteria

| Criterion | Verification |
| :--- | :--- |
| Portals screen lists all portals with health badges | `egui_kittest` test plus manual test |
| Add/edit forms create and update portals | App integration test plus `egui_kittest` form gates |
| Delete confirmation removes a portal | App integration test plus `egui_kittest` dialog |
| "Fetch now" updates the row's health without a full refresh | View-model test plus manual test (network) |
| "Fetch all" shows per-portal progress and updates all rows | View-model and progress-plumbing tests plus manual test (network) |
| Batch can be cancelled | Core service test plus app cancel-state test |
| Sidecar status reflects the last Tier 2 outcome | View-model test plus manual test |
| "Add job from URL" fetches and stores | Service test plus manual test (network) |
| "Add job from paste" stores | App integration test |
| `PortalsViewModel::from_entries` maps health states correctly | Unit test (4 cases) |
| `PortalsViewModel::apply_fetch_result` updates the row | Unit test |
| `PortalsViewModel::apply_batch_entry` accumulates progress | Unit test |

**Implementation note (2026-10-24).** Workstream 3 landed as `crates/app/src/view_models/portals.rs`, `crates/app/src/screens/portals.rs`, and `crates/app/tests/portals_screen.rs`. `CareerOpsApp` owns an `Arc<HttpFetchSource>`, an `Arc<ObscuraBrowserBackend>`, a single `PortalsTask` (list/fetch/batch/manual), an `mpsc` receiver for batch progress, and a shared cancel flag. The core service gained `fetch_batch_cancellable` and a cancellation test. App integration tests cover create, update, manual paste, delete, and batch-progress plumbing against a real encrypted database; fetch actions that need the network remain manual tests. The manual modal lives in `PortalsView`, so the Pipeline header entry point is a WS4 wiring task rather than a second implementation.

---

## Workstream 4 — Pipeline Screen and Manual Entry

**Priority:** After WS3.
**Estimated duration:** 2 days

### Deliverables

1. **Pipeline list.** Renders `job_descriptions::list_recent` via a `PipelineViewModel`. Rows show title, company, tier label, fetched-at timestamp, and application status badge when attached. The load is bounded to the 500 most recent jobs; the plain `ScrollArea` stays correct at that size, and `TableBuilder` is the swap point if the bound ever moves.

   Application statuses come from `applications::list`, joined by job id inside the load task. A job description has at most one application, so the view model carries `Option<StatusLabel>` rather than a list.

2. **Filter.** A horizontal `SelectableLabel` row for All / Not applied / Applied / Rejected. Filtering is client-side over the loaded list.

3. **Job detail placeholder.** Clicking a row replaces the central panel with a detail view: title, company, tier, timestamps, raw description. Two buttons — "Evaluate" and "Generate cover letter" — are disabled with a tooltip ("Available in a later phase"). This reserves the space Phase 3 fills.

4. **"Add job" button** in the header, opening the manual entry modal from WS3. The modal is rendered by the app after the active screen, so the same dialog serves Portals and Pipeline.

### View models (survive unchanged)

`crates/app/src/view_models/pipeline.rs`:

```rust
pub struct PipelineViewModel {
    pub rows: Vec<JobRow>,
    pub filter: JobFilter,
}

pub struct JobRow {
    pub id: i64,
    pub title: String,
    pub company: String,
    pub tier: TierLabel,
    pub fetched_at: String,
    pub status: Option<StatusLabel>,
    // Added during implementation: the detail view renders the posting from
    // the row instead of issuing a second database read.
    pub source_url: String,
    pub raw_text: String,
}

pub enum TierLabel { Ats, Html, Tier2, Manual, Other }
pub enum StatusLabel { Discovered, Evaluating, Ready, Applied, Interviewing, Rejected, Offer }
pub enum JobFilter { All, NotApplied, Applied, Rejected }

impl PipelineViewModel {
    pub fn from_jobs(jobs: Vec<JobDescription>, statuses: &HashMap<i64, ApplicationStatus>) -> Self;
    pub fn replace_jobs(&mut self, jobs: Vec<JobDescription>, statuses: &HashMap<i64, ApplicationStatus>);
    pub fn apply_filter(&self) -> impl Iterator<Item = &JobRow>;
}
```

`from_jobs` takes the application statuses as a second argument rather than reading them itself, which keeps the view model pure. `replace_jobs` keeps the current filter so a refresh does not reset the user's selection. `TierLabel::Other` exists because the database stores `fetch_tier` as text; an unknown value must still render.

### Acceptance Criteria

| Criterion | Verification |
| :--- | :--- |
| Pipeline lists all jobs | Manual test |
| Tier label matches `fetch_tier` | `egui_kittest` test with jobs from each tier |
| All four filters work | `egui_kittest` test plus manual test |
| Job detail shows raw description | Manual test |
| Placeholder buttons are disabled with tooltip | Manual test |
| Manual entry from Pipeline works | Manual test |
| `PipelineViewModel::from_jobs` maps tiers correctly | Unit test |
| `PipelineViewModel::apply_filter` filters correctly | Unit test (4 cases) |

**Implementation note (2026-10-25).** Workstream 4 landed as `crates/app/src/view_models/pipeline.rs`, `crates/app/src/screens/pipeline.rs`, and `crates/app/tests/pipeline_screen.rs`. `CareerOpsApp` owns one `AsyncTask<PipelineLoad>` that reads `job_descriptions::list_recent` and `applications::list` and joins them in memory; the screen never touches the database. The `PipelineAction` set is `AddJob`, `Reload`, and `SetFilter`. `AddJob` opens the shared manual modal, and a successful manual add drops any in-flight pipeline load and marks the page stale so the next visit reloads it. `StatusBadge` gained the seven application-status variants it needs.

Two structural decisions are worth recording. First, the modal is no longer rendered inside `portals::show`: the app renders `portals::show_modal` after the active screen, so the dialog is reachable from Pipeline and `PortalsView` is now modal-only state. Second, the shell's content closure returns a small `ScreenAction` enum (`Portals` | `Pipeline`) so `ui()` routes the result without the shell knowing either screen. `tests/portals_screen.rs` renders `show` and `show_modal` in that order, matching the app.

---

## Workstream 5 — Verification Pass and Retrospective

**Priority:** Last.
**Estimated duration:** 1 day

Same discipline as Phase 1 and Phase 2:

1. **Walk every exit criterion.** For each, confirm the test or manual check exercises the property, not a proxy. Record what was checked and what was found.

2. **Manual smoke test on a clean machine.** Clone, build, run without setting any env var, complete first launch, add a portal, fetch it, add a job manually, filter the pipeline. The check that no automated test covers, and the one that `data_dir()` cleanup makes possible.

3. **Write the retrospective:**
   - Whether the Phase 2 finding P2-R9 (service/view divergence) was resolved or whether the shell had to reshape the service layer
   - Whether egui's immediate-mode model showed any performance concern on a real screen
   - Whether the accessibility gap affected the unlock flow in practice
   - Whether the async bridge pattern (`request_repaint_after` polling) was ergonomic
   - Estimate accuracy, with a section on how the framework swap affected the total

**Implementation note (2026-10-25).** The pass is recorded in
`.agents/plans/phase_2.1_retrospective.md`. It produced four findings: five
small functions (including both `row` lookups and the tier-label mapping) had no
unit test, no test drove the recovery path end to end, the manual script did not
exist, and criterion 4 named a Dashboard the navigation model does not have. The
first three are closed with five tests and `docs/manual-test-script.md`; the
fourth is a rewording, not a feature. Two criteria remain open for a human:
executing the network-dependent manual steps, and observing CI.

---

## Test Strategy

### View-model tests (unit)

Pure Rust, no egui dependency. Every view-model function has a unit test that constructs inputs by hand and asserts on outputs. These run under `cargo test -p career-ops-app` and cover the logic that decides what the user sees.

### View tests (`egui_kittest`)

egui's harness supports the assertions this phase needs:

- `Harness::builder().build_ui_state(|ui, app| app.ui(ui), app)` for state-bearing screens
- `harness.get_by_label("...")`, `harness.get_by_role(Role::Button)` for semantic queries
- `harness.state()` for state assertions
- `harness.click("...")`, `harness.type_text("...")` for interaction
- `harness.snapshot("...")` for visual regression (feature-gated on `snapshot` + `wgpu`)

The screens that get harness tests in this phase are the ones with the highest decision value:

- Unlock (wrong passphrase / correct passphrase)
- First-launch (acknowledgement gating)
- Portals add form (validation)
- Portals delete confirmation (two-button semantics)
- Pipeline filter (four filters)

Screens that are layout-only (Dashboard placeholder, Settings placeholder) are covered by manual smoke test rather than harness tests, on the grounds that a harness test for a screen with no interactive content proves nothing the smoke test does not.

### Manual test script

`docs/manual-test-script.md` documents:

- First launch (no files, no env vars set)
- Unlock (with the created files)
- Recovery (induced by corrupting `dek.wrapped.json`)
- Portals: add, edit, delete, fetch one, fetch all, cancel batch
- Manual entry: URL and paste
- Pipeline: list, filter, job detail

Each step lists the expected outcome. This is the "does the application work" check for anyone before Phase 3 starts.

---

## Risks

| ID | Risk | Impact | Mitigation |
| :--- | :--- | :--- | :--- |
| P2.1-R1 | **egui is the wrong framework after all.** The data-dense screens cost more CPU than expected, or immediate-mode ergonomics do not scale. | Medium | Two weeks to find out. If the framework has to change, the view-model and navigation layers survive unchanged — the rewrite is the view layer only. Record the finding in the retrospective as evidence for the next evaluation. |
| P2.1-R2 | **Accessibility compliance is broader than the input-label fix.** `labelled_input` can associate the label with the field at 0.37, but focus order, keyboard navigation, and contrast are not addressed. | Medium | The helper localizes the input-label work. Treat the rest as Phase 5 audit items; do not let them block Phase 2.1. |
| P2.1-R3 | **The service layer's shapes are awkward to render.** | Medium | This is what the phase is for. If a shape is wrong, fix it in the service layer (test-first) rather than in the view. |
| P2.1-R4 | **Scope creep into polish.** | High | Explicitly out of scope. Polish is Phase 5. |
| P2.1-R5 | **Scope creep into Phase 3.** | High | Buttons are disabled placeholders. |
| P2.1-R6 | **`request_repaint_after` polling causes a busy loop or a frozen UI.** | Low | `request_repaint_after(Duration::from_millis(50))` during an active task, no repaint request when idle. Verify manually that idle CPU is near zero. |
| P2.1-R7 | **Manual test script gets skipped.** | Medium | The script is an exit criterion, not a suggestion. |
| P2.1-R8 | **`data_dir()` migration breaks an existing local database.** Moving from the env-var path to the OS-appropriate path changes where the app looks for `career-ops.db`. | Low | The new path is documented in the README. The env var override remains, so a developer with an existing database can set it to the old path. There is no shipped data to migrate. |

---

## Weekly Budget

| Week | Focus | Deliverable |
| :--- | :--- | :--- |
| **1** | WS1 shell infrastructure + WS2 unlock/first-launch/recovery | Window opens on a clean machine, unlock flow works, recovery is reachable |
| **2** | WS3 Portals + WS4 Pipeline + WS5 verification | Portals CRUD + fetch + batch; Pipeline list + filter + manual entry; verification memo |

**Total:** 1.5–2 weeks. The upper end assumes egui friction on the async bridge or the accessibility helper. The lower end assumes the port from the GPUI plan's framework-independent layers works as designed.

---

## Exit Criteria

Phase 2.1 is complete when **all** of the following are true:

1. The application opens to the unlock screen when `dek.wrapped.json` exists, the first-launch wizard when it does not, and the recovery screen when unlock fails.
2. `cargo run -p career-ops-app` works on a clean checkout with no environment variables set.
3. Successful unlock, first-launch completion, and recovery-path resolution all reach the main layout.
4. Sidebar navigation works between Pipeline, Portals, CV/Profile (placeholder), Prompts (placeholder), and Settings (placeholder). *(Reworded in v0.8: the earlier list included a "Dashboard (placeholder)" that `MainScreen` does not have; its content is Pipeline's.)*
5. Portals lists all portals with health badges, last attempt timestamps, and suggested actions.
6. Portals supports add, edit, delete, fetch one, and batch fetch with per-portal progress.
7. Batch fetch can be cancelled.
8. Pipeline lists all jobs with tier labels and timestamps, and supports filtering by status.
9. Job detail renders the raw description with disabled placeholder buttons.
10. Manual entry works from both Portals and Pipeline, for both URL fetch and paste.
11. Sidecar status indicator reflects the last Tier 2 outcome.
12. Every view-model function has a corresponding unit test.
13. `crates/app/docs/patterns.md` documents the list, form, and async patterns, including the `request_repaint_after` requirement.
14. `docs/manual-test-script.md` documents the flows and has been executed end-to-end on a clean machine.

    *(Verification 2026-10-25: the document exists; the offline sections were walked, the 🌐 rows are listed as pending in the sign-off table and in the retrospective. The criterion stays open until they are recorded.)*
15. `labelled_input` produces an accessible label, or the limitation is recorded as a Phase 5 audit item with a comment naming the fix location.
16. CI is green on all platforms.
17. The verification pass is complete: every criterion above has been checked against the code, with a note on how it was verified and whether the check exercises the property.
18. A retrospective memo records P2-R9's status, egui-specific findings, the async bridge experience, and estimate accuracy.

---

## What Phase 2.1 Is Not

Phase 2.1 makes the application runnable. It does not evaluate jobs, generate cover letters, render PDFs, or polish any of the screens. Specifically:

- **No evaluation.** The Evaluate button is a disabled placeholder.
- **No cover letter generation.** Same.
- **No PDF rendering.** The CV/Profile screen is a placeholder.
- **No prompt management.** The Prompts screen is a placeholder.
- **No settings screen.** Settings is a placeholder.
- **No accessibility work** beyond `labelled_input`. Contrast, keyboard navigation, and focus order are Phase 5.
- **No visual polish.** egui's defaults; theming is Phase 5.
- **No auto-update, telemetry, or crash reporting.**
- **No localization.**

The temptation to "just add the evaluate button while I'm here" is the most common way shell phases overrun. Phase 3 adds the evaluate button; Phase 2.1 reserves the space and disables it.

---

The single most important thing to take from this plan: **the shell exists to make the service layer honest, not to look good.** Every screen calls a service-layer function and renders the result. If the service layer's shapes fit the view-models without adjustment, Phase 2's design was right. If they do not, the fix is in the service layer, not the view.

The second: **the view-model layer is where correctness lives.** egui is the chosen framework, but the view-models do not know what egui is. Phase 3, 4, and 5 add screens by extending the view-models and wiring new views to them. The view code is replaceable; the view-model code is the application.