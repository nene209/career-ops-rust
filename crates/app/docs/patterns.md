# UI patterns (egui 0.37 / eframe 0.37)

These are the contracts Phase 3, 4, and 5 add screens against. The versions
matter: egui 0.37 changed enough of the API that older examples do not compile.
Two changes in particular are easy to trip over:

- `eframe::App` has `fn ui(&mut self, ui: &mut egui::Ui, frame: &mut eframe::Frame)`,
  not `fn update(&mut self, ctx: &egui::Context, frame: ...)`.
- `SidePanel` / `TopBottomPanel` no longer exist. Use `egui::Panel::left`,
  `Panel::right`, `Panel::top`, `Panel::bottom`. They take a `&mut Ui` and
  render inside it.

## Screen structure

`CareerOpsApp::ui` renders exactly one of two things, decided by
`AppState::current()`:

- `Screen::Ready` → `screens::shell::show(ui, &mut navigation)`.
- Any other state → `screens::lock::show(ui, &screen)`.

Screens do not keep their own copy of which screen is current. The state
machine is the source of truth; the sidebar only mutates `NavigationState`
after `Ready`.

The shell is:

```rust
egui::Panel::left("career_ops_sidebar")
    .resizable(false)
    .exact_size(190.0)
    .show(ui, |ui| { /* navigation */ });

egui::CentralPanel::default().show(ui, |ui| {
    ui.heading(nav.current().label());
    ui.separator();
    screen_body(ui, nav.current());
});
```

Panels must be shown before the central panel. `show` takes the root `Ui`, not
a `Context`.

`CareerOpsApp::ui` renders the active screen through `shell::show_with`, then
calls `portals::show_modal` so dialogs float above whichever screen is visible.
The shell never runs side effects: each screen returns an action enum
(`PortalsAction`, `PipelineAction`) and the app executes it.

## List pattern

For Phase 2.1's expected data volumes (a dozen portals, a few hundred jobs),
use a plain `ScrollArea` with one `ui.horizontal` per row. It is simpler and
correct:

```rust
egui::ScrollArea::vertical().show(ui, |ui| {
    for row in &view_model.rows {
        ui.horizontal(|ui| {
            ui.label(&row.name);
            ui.label(&row.url);
            row.health_badge(ui);
        });
        ui.separator();
    }
});
```

**Swap point:** when the Pipeline list grows past a few hundred rows, replace
the loop with `egui_extras::TableBuilder`, which virtualizes rows. Do not
prematurely use it in Phase 2.1. The Pipeline loads at most `PIPELINE_LIMIT`
(500) rows, so raise that bound only together with the swap.

Empty lists render `components::EmptyState`, not a blank panel. `EmptyState`
optionally takes an action label and returns `true` in the frame it is clicked.

## Status pattern

Use `components::StatusBadge` for portal health and sidecar state. Map the
domain enum onto a badge in the view-model or in one small function; do not
re-implement colour/label decisions per screen. `PortalHealth::suggested_action`
is already the user-facing drill-down text — render it directly. The Pipeline
maps its `StatusLabel` to the seven application-status badges the same way.

## Form pattern

Every text field goes through `components::labelled_input`:

```rust
labelled_input(ui, "Passphrase", &mut self.passphrase, true);
```

`TextEdit::hint_text` is visual only. It does not name the field in the
accessibility tree, so `egui_kittest::Queryable::get_by_label` cannot find it
and a screen reader has nothing to announce. `labelled_input` renders a visible
label and calls `Response::labelled_by(label_id)` so the association exists.
When egui grows a first-class labelled-field API, change this one function.

`egui_kittest::Harness::run` calls `check_accessibility` automatically; a new
input without a name fails the test.

## Async pattern

`AsyncTask<T>` from `async_bridge.rs` is unchanged from the Phase 1 GPUI work.
The eframe integration has one requirement that is easy to miss:

1. Spawn the future with `CareerOpsApp::spawn(...)`, which enters the app's
   tokio runtime before calling `AsyncTask::spawn`. Calling `AsyncTask::spawn`
   directly from `ui()` panics with "no reactor running", because the eframe UI
   thread is not a tokio worker thread.
2. Store the `AsyncTask` in the screen or app state.
3. Poll it once per `ui()` pass with `try_recv()`.
4. While it returns `TaskPoll::Pending`, call
   `ui.ctx().request_repaint_after(Duration::from_millis(50))`.
5. On `Ready`, apply the value, drop the task, and call `request_repaint()`.

egui does **not** call `ui()` continuously. Without step 4 the event loop
sleeps and the result is never observed; the UI appears frozen even though the
tokio task finished. This is the single most common async/egui integration bug.

```rust
if let Some(task) = &mut self.pending_fetch {
    match task.try_recv() {
        TaskPoll::Ready(report) => {
            self.apply_fetch_result(report);
            self.pending_fetch = None;
            ui.ctx().request_repaint();
        }
        TaskPoll::Pending => {
            ui.ctx().request_repaint_after(Duration::from_millis(50));
        }
        TaskPoll::Cancelled => {
            self.pending_fetch = None;
            ui.ctx().request_repaint();
        }
    }
}
```

### Batch progress

`portals::service::fetch_batch` takes an `FnMut(&BatchEntry)` callback. In
eframe, the callback sends on a `std::sync::mpsc::Sender`; `ui()` drains with
`try_iter()` each frame. Keep requesting repaints after each poll while the
batch is running.

Cancellation is a shared `AtomicBool`. The callback checks it before applying
each entry; the in-flight fetch finishes first. That is coarse but correct, and
it avoids cancelling a half-written row.

### Idle CPU

When no task is pending, do not call `request_repaint` or
`request_repaint_after`. egui then only repaints on input, and idle CPU stays
near zero. This is an acceptance criterion for Workstream 1 and a habit later
screens should keep.

## Portals task orchestration (WS3)

The Portals screen returns `PortalsAction`; `CareerOpsApp` owns the side
effects. There is at most one `PortalsTask` at a time:

- `List` — initial load or post-mutation reload. `close_modal` tells the
  completion handler whether a successful reload should close the form.
- `Fetch` — `service::fetch_one`. The row's health badge becomes a spinner
  while `fetching_portal` matches.
- `Batch` — `service::fetch_batch_cancellable`. The callback sends entries on
  `std::sync::mpsc`; `drain_batch_progress` applies them and advances
  `in_flight`.
- `Manual` — URL or paste entry; the result is set on the manual modal.

Use `fetch_batch_cancellable` rather than `fetch_batch` when the user can
cancel. The callback cannot break the service loop, so the cancellation flag is
checked between portals. A fetch already in progress finishes, then the batch
returns a partial report.

The screen uses a plain `ScrollArea` for the list; `TableBuilder` is the swap
point when the Pipeline list grows. Health mapping lives in the view model; the
screen only maps `HealthBadge` to `StatusBadge`.

### Shared modal

The add/edit/delete/manual dialogs are not rendered by `portals::show`. The app
calls `portals::show_modal(ui, &mut portals_view, render_state.modal_state())`
after the active screen, so the manual-entry dialog is reachable from Pipeline
through `PortalsView::open_manual()`. `PortalsView` is modal-only state; the
Portals screen takes just the view model and the render state.

## Pipeline screen (WS4)

One task (`AsyncTask<PipelineLoad>`) reads `job_descriptions::list_recent(&pool,
PIPELINE_LIMIT)` and `applications::list(&pool)` and joins them by job id before
the view model is built. The screen is read-only apart from the filter: it
returns `PipelineAction::{AddJob, Reload, SetFilter}` and the app owns every
side effect.

- The filter is a view preference. `PipelineViewModel::replace_jobs` keeps it
  across reloads; `from_jobs` resets it to `All` only for a fresh page.
- A successful manual add sets `pipeline_loaded = false` and drops any in-flight
  load, so a result read before the insert cannot land after it. `ui()` reloads
  on the next frame while Pipeline is visible.
- `selected: Option<i64>` lives in `PipelineView`, not in the view model. A
  selection that no longer exists falls back to the list instead of rendering an
  empty detail panel.
- The detail view reads `source_url` and `raw_text` from the row, not from a
  second query.

## State-mutating async work

`AppState` methods such as `try_passphrase`, `try_keychain`, and
`complete_first_launch` take `&mut self` and are async. `AsyncTask` needs a
`'static` future, so `CareerOpsApp` moves the state out for the duration of the
operation:

- `CareerOpsApp` stores `state: Option<AppState>`.
- Starting an operation moves the state into the future and stores the task.
- `poll_pending` restores the state together with the result.
- While the state is `None`, the UI renders `lock::busy`.

Do not wrap the state in a mutex and hold it across an await. If an operation
is interrupted, the app re-bootstraps from the data directory; if that also
fails it shows the fatal screen rather than spinning forever.

Lock screens do not perform side effects. They return a `LockAction`, which
`CareerOpsApp` executes. This keeps the views testable with `egui_kittest` and
gives the app one place to clear secrets, spawn work, and apply filesystem
recovery actions.
