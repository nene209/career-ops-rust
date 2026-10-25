//! The eframe application.
//!
//! `CareerOpsApp` owns the three pieces every later workstream needs:
//!
//! - `AppState`, the Phase 1 state machine and the source of truth for which
//!   screen is visible.
//! - `NavigationState`, the sidebar selection once the user reaches `Ready`.
//! - A `tokio::runtime::Runtime`, used by Workstreams 2–4 to run service-layer
//!   calls off the UI thread.
//!
//! The async hand-off pattern is documented in `docs/patterns.md`. In short:
//! spawn work with `AsyncTask::spawn`, store the task, poll it once per frame
//! with `AsyncTask::try_recv`, and call `egui::Context::request_repaint_after`
//! while it is pending. egui does not call `ui()` continuously, so omitting
//! the repaint request leaves the result unobserved.

use std::future::Future;

use tokio::runtime::Runtime;

use crate::async_bridge::AsyncTask;
use crate::navigation::NavigationState;
use crate::screens;
use crate::state::{AppState, Screen};

/// The root application object passed to `eframe::run_native`.
pub struct CareerOpsApp {
    state: AppState,
    navigation: NavigationState,
    runtime: Runtime,
}

impl CareerOpsApp {
    pub fn new(state: AppState, runtime: Runtime) -> Self {
        Self {
            state,
            navigation: NavigationState::new(),
            runtime,
        }
    }

    /// Read-only access for tests and for future workstreams that need to
    /// inspect the state machine from outside `ui()`.
    pub fn state(&self) -> &AppState {
        &self.state
    }

    /// The current sidebar selection. Before `Screen::Ready`, the shell
    /// renders lock screens and this value is not displayed.
    pub fn navigation(&self) -> &NavigationState {
        &self.navigation
    }

    /// The runtime used to spawn service-layer work. Exposed so tests and
    /// later screens can share the process-wide runtime.
    pub fn runtime(&self) -> &Runtime {
        &self.runtime
    }

    /// Spawn a future on the app's runtime.
    ///
    /// `AsyncTask::spawn` calls `tokio::spawn`, which requires the calling
    /// thread to already be inside a runtime context. eframe's UI thread is
    /// not, so the runtime is entered explicitly here. Screens should use this
    /// helper rather than calling `AsyncTask::spawn` directly.
    pub fn spawn<F, T>(&self, future: F) -> AsyncTask<T>
    where
        F: Future<Output = T> + Send + 'static,
        T: Send + 'static,
    {
        let _guard = self.runtime.enter();
        AsyncTask::spawn(future)
    }
}

impl eframe::App for CareerOpsApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        // Clone so the match does not hold a borrow of `self.state` while the
        // `Ready` arm mutates `self.navigation`.
        match self.state.current().clone() {
            Screen::Ready => screens::shell::show(ui, &mut self.navigation),
            screen => screens::lock::show(ui, &screen),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;
    use crate::async_bridge::TaskPoll;

    /// The eframe UI thread is not a tokio runtime thread. `CareerOpsApp::spawn`
    /// must enter the runtime before calling `AsyncTask::spawn`, or spawning a
    /// fetch from a screen would panic with "no reactor running".
    #[test]
    fn spawn_works_from_a_non_runtime_thread() {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = AppState::bootstrap(dir.path().to_path_buf()).expect("bootstrap");
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .expect("runtime");
        let app = CareerOpsApp::new(state, runtime);

        let mut task = app.spawn(async { 42u32 });
        let deadline = Instant::now() + Duration::from_secs(2);

        loop {
            match task.try_recv() {
                TaskPoll::Pending if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(5));
                }
                TaskPoll::Ready(value) => {
                    assert_eq!(value, 42);
                    break;
                }
                other => panic!(
                    "task did not complete in time: {:?}",
                    std::mem::discriminant(&other)
                ),
            }
        }
    }
}
