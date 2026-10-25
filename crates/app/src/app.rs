//! The eframe application.
//!
//! `CareerOpsApp` owns:
//!
//! - `AppState`, the Phase 1 state machine and the source of truth for which
//!   screen is visible.
//! - `NavigationState`, the sidebar selection once the user reaches `Ready`.
//! - A `tokio::runtime::Runtime`, used by the lock screens and by Workstreams
//!   3–4 to run service-layer calls off the UI thread.
//! - UI-only lock state and the one pending async operation.
//!
//! ## Why the state is an `Option`
//!
//! Lock operations (`try_passphrase`, `try_keychain`, `complete_first_launch`)
//! are async `&mut self` methods on `AppState`. `AsyncTask` requires `'static`,
//! so the app cannot lend the state to the task. Instead it moves the state
//! into the future and restores it with the result. While the task runs,
//! `state` is `None` and the UI shows a spinner. This keeps `state.rs`
//! untouched and avoids holding a mutex across an await.
//!
//! The async hand-off pattern is documented in `docs/patterns.md`.

use std::future::Future;
use std::path::{Path, PathBuf};

use career_ops_core::{CoreError, Result as CoreResult};
use tokio::runtime::Runtime;

use crate::async_bridge::{AsyncTask, TaskPoll};
use crate::navigation::NavigationState;
use crate::recovery;
use crate::screens;
use crate::screens::lock::{LockAction, LockView, RESET_PHRASE};
use crate::state::{AppState, Screen};

type StateTask = AsyncTask<(AppState, CoreResult<()>)>;

/// The root application object passed to `eframe::run_native`.
pub struct CareerOpsApp {
    /// `None` only while an async state transition owns the state.
    state: Option<AppState>,
    data_dir: PathBuf,
    navigation: NavigationState,
    runtime: Runtime,
    lock_view: LockView,
    pending: Option<StateTask>,
    /// Set only if an interrupted task lost the state and it could not be
    /// reopened. The app then renders a fatal screen instead of a spinner.
    fatal_error: Option<String>,
}

impl CareerOpsApp {
    pub fn new(state: AppState, runtime: Runtime) -> Self {
        let data_dir = state.data_dir().clone();
        Self {
            state: Some(state),
            data_dir,
            navigation: NavigationState::new(),
            runtime,
            lock_view: LockView::default(),
            pending: None,
            fatal_error: None,
        }
    }

    /// The state machine, if no async transition currently owns it.
    pub fn state(&self) -> Option<&AppState> {
        self.state.as_ref()
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

    /// True while an async lock operation is running.
    pub fn is_busy(&self) -> bool {
        self.pending.is_some()
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

    /// Execute an intent emitted by a lock screen.
    ///
    /// Public so tests can drive the same path the UI uses without rendering.
    pub fn handle_lock_action(&mut self, action: LockAction) {
        self.lock_view.error = None;
        self.lock_view.recovery.status = None;

        match action {
            LockAction::TryKeychain
            | LockAction::RetryKeychain
            | LockAction::TryPassphrase(_)
            | LockAction::CompleteFirstLaunch(_) => self.start_state_task(action),
            LockAction::OpenDataDir => {
                if let Err(error) = recovery::open_data_dir(&self.data_dir) {
                    self.lock_view.error =
                        Some(format!("Could not open the data directory: {error}"));
                }
            }
            LockAction::LocateKeyFile(source) => self.install_key_file(&source),
            LockAction::ExportDatabase(destination) => {
                match recovery::export_database(&self.data_dir, &destination) {
                    Ok(()) => {
                        self.lock_view.recovery.status =
                            Some(format!("Exported to {}", destination.display()));
                    }
                    Err(error) => {
                        self.lock_view.error = Some(format!("Export failed: {error}"));
                    }
                }
            }
            LockAction::StartFresh => self.start_fresh(),
        }
    }

    /// Poll the in-flight lock operation. Called once per frame from `ui()`.
    pub fn poll_pending(&mut self, ctx: &egui::Context) {
        let poll = match self.pending.as_mut() {
            Some(task) => task.try_recv(),
            None => return,
        };

        match poll {
            TaskPoll::Ready((state, result)) => {
                let reached_ready = matches!(state.current(), Screen::Ready);
                self.state = Some(state);
                self.pending = None;

                match result {
                    Ok(()) => {
                        self.lock_view.error = None;
                        if reached_ready {
                            // Drop the transient passphrase strings once the
                            // vault is open. The shell no longer needs them.
                            self.lock_view = LockView::default();
                        }
                    }
                    Err(error) => {
                        self.lock_view.error = Some(friendly_error(&error));

                        // A failed first launch can leave `dek.wrapped.json`
                        // behind without a database. Recomputing the state
                        // surfaces the Recovery screen instead of leaving the
                        // wizard to retry an initialization that must refuse.
                        let was_first_launch = self
                            .state
                            .as_ref()
                            .is_some_and(|state| state.current() == &Screen::FirstLaunch);
                        if was_first_launch {
                            if let Ok(bootstrapped) = AppState::bootstrap(self.data_dir.clone()) {
                                if bootstrapped.current() != &Screen::FirstLaunch {
                                    // The wizard can no longer be retried; drop
                                    // the passphrase it was holding.
                                    self.lock_view.first_launch.passphrase.clear();
                                    self.state = Some(bootstrapped);
                                }
                            }
                        }
                    }
                }

                ctx.request_repaint();
            }
            TaskPoll::Pending => {
                ctx.request_repaint_after(std::time::Duration::from_millis(50));
            }
            TaskPoll::Cancelled => {
                self.pending = None;
                self.lock_view.first_launch.passphrase.clear();
                match AppState::bootstrap(self.data_dir.clone()) {
                    Ok(state) => {
                        self.state = Some(state);
                        self.lock_view.error = Some(
                            "The operation was interrupted. Reopening the data directory.".into(),
                        );
                    }
                    Err(error) => {
                        self.state = None;
                        self.fatal_error = Some(format!(
                            "The operation was interrupted and the data directory could not \
                             be reopened: {error}"
                        ));
                    }
                }
                ctx.request_repaint();
            }
        }
    }

    fn start_state_task(&mut self, action: LockAction) {
        let Some(mut state) = self.state.take() else {
            return;
        };

        self.clear_secrets_for(&action);

        let task = self.spawn(async move {
            let result = match action {
                LockAction::TryKeychain | LockAction::RetryKeychain => state.try_keychain().await,
                LockAction::TryPassphrase(mut passphrase) => {
                    let result = state.try_passphrase(&passphrase).await;
                    passphrase.clear();
                    result
                }
                LockAction::CompleteFirstLaunch(mut passphrase) => {
                    let result = state.complete_first_launch(&passphrase).await;
                    passphrase.clear();
                    result
                }
                // The caller routes sync actions elsewhere.
                _ => Ok(()),
            };
            (state, result)
        });

        self.pending = Some(task);
    }

    /// Clear the fields whose contents the just-spawned task now owns.
    fn clear_secrets_for(&mut self, action: &LockAction) {
        match action {
            LockAction::TryPassphrase(_) => {
                self.lock_view.passphrase.clear();
                self.lock_view.recovery.passphrase.clear();
            }
            LockAction::CompleteFirstLaunch(_) => {
                self.lock_view.passphrase.clear();
                self.lock_view.first_launch.confirmation.clear();
                // Keep `first_launch.passphrase` so the Backup step can retry
                // after a recoverable failure. It is cleared on success, or
                // when the state leaves FirstLaunch.
            }
            _ => {}
        }
    }

    fn install_key_file(&mut self, source: &Path) {
        if let Err(error) = recovery::install_key_file(&self.data_dir, source) {
            self.lock_view.error = Some(format!("Could not use that key file: {error}"));
            return;
        }

        match AppState::bootstrap(self.data_dir.clone()) {
            Ok(state) => {
                self.state = Some(state);
                self.lock_view = LockView::default();
            }
            Err(error) => {
                // Do not leave an unreadable key file behind: it would make
                // the next launch fail even though this attempt did not help.
                let _ = std::fs::remove_file(
                    self.data_dir.join(career_ops_core::keystore::WRAPPED_FILE),
                );
                self.lock_view.error = Some(format!("That key file could not be opened: {error}"));
            }
        }
    }

    fn start_fresh(&mut self) {
        // The UI disables the button until the phrase matches, but check again
        // here so a future caller cannot bypass the confirmation.
        if self.lock_view.recovery.confirmation.trim() != RESET_PHRASE {
            self.lock_view.error = Some("Type DELETE MY DATA exactly to confirm the reset.".into());
            return;
        }

        match recovery::reset_data_dir(&self.data_dir) {
            Ok(()) => match AppState::bootstrap(self.data_dir.clone()) {
                Ok(state) => {
                    self.state = Some(state);
                    self.lock_view = LockView::default();
                }
                Err(error) => {
                    self.lock_view.error = Some(format!(
                        "The data was deleted, but reopening failed: {error}. \
                         Restart the application."
                    ));
                }
            },
            Err(error) => {
                self.lock_view.error = Some(format!("Could not delete the data: {error}"));
            }
        }
    }
}

fn friendly_error(error: &CoreError) -> String {
    match error {
        CoreError::Internal(_) => {
            "This action is not available because the wrapped key file is missing or \
             unreadable. Use \"Locate your key file\" or restore a backup."
                .into()
        }
        other => other.to_string(),
    }
}

impl eframe::App for CareerOpsApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll_pending(ui.ctx());

        if let Some(error) = &self.fatal_error {
            screens::lock::fatal(ui, error);
            return;
        }

        let screen = match self.state.as_ref() {
            Some(state) => state.current().clone(),
            None => {
                screens::lock::busy(ui, "Working...");
                return;
            }
        };

        match screen {
            Screen::Ready => screens::shell::show(ui, &mut self.navigation),
            screen => {
                if let Some(action) =
                    screens::lock::show(ui, &screen, &mut self.lock_view, &self.data_dir)
                {
                    self.handle_lock_action(action);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;
    use crate::async_bridge::TaskPoll;

    /// Serializes the tests that create a vault. The test keychain uses one
    /// fixed service/account pair for the whole process, so two first-launch
    /// tests running in parallel would overwrite each other's KEK.
    static KEYCHAIN_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    const TEST_PASSPHRASE: &str = "first-launch-ui-test-passphrase";

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

    /// First launch runs through the same async path the wizard uses: the
    /// state is moved into the task, moved back with the result, and reaches
    /// `Ready` without the UI thread blocking.
    #[test]
    fn complete_first_launch_reaches_ready_through_the_app() {
        let _guard = KEYCHAIN_TEST_LOCK.lock().expect("keychain test lock");
        career_ops_test_support::install();

        let dir = tempfile::tempdir().expect("tempdir");
        let state = AppState::bootstrap(dir.path().to_path_buf()).expect("bootstrap");
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("runtime");
        let mut app = CareerOpsApp::new(state, runtime);

        assert_eq!(app.state().expect("state").current(), &Screen::FirstLaunch);

        app.handle_lock_action(LockAction::CompleteFirstLaunch(TEST_PASSPHRASE.to_string()));
        assert!(app.is_busy(), "the state should be owned by the task");

        let ctx = egui::Context::default();
        let deadline = Instant::now() + Duration::from_secs(20);
        while app.is_busy() && Instant::now() < deadline {
            app.poll_pending(&ctx);
            std::thread::sleep(Duration::from_millis(10));
        }

        assert!(!app.is_busy(), "first launch should finish");
        assert_eq!(app.state().expect("state").current(), &Screen::Ready);
        assert!(app.state().expect("state").pool().is_some());
    }

    /// A returning user goes through the same async path with the passphrase
    /// action. This also asserts that the UI copy of the passphrase is cleared
    /// once the task takes ownership.
    #[test]
    fn unlock_through_the_app_reaches_ready() {
        let _guard = KEYCHAIN_TEST_LOCK.lock().expect("keychain test lock");
        career_ops_test_support::install();

        let dir = tempfile::tempdir().expect("tempdir");

        // Create the vault with a separate runtime, then drop it so the
        // database pool is closed before the app opens it again.
        {
            let setup_runtime = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(1)
                .enable_all()
                .build()
                .expect("setup runtime");
            let mut state = AppState::bootstrap(dir.path().to_path_buf()).expect("bootstrap");
            setup_runtime
                .block_on(state.complete_first_launch(TEST_PASSPHRASE))
                .expect("first launch");
        }

        let state = AppState::bootstrap(dir.path().to_path_buf()).expect("second bootstrap");
        assert_eq!(state.current(), &Screen::Unlocking);

        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("runtime");
        let mut app = CareerOpsApp::new(state, runtime);

        app.handle_lock_action(LockAction::TryPassphrase(TEST_PASSPHRASE.to_string()));
        assert!(app.is_busy(), "the state should be owned by the task");
        assert!(
            app.lock_view.passphrase.is_empty(),
            "the view-model copy of the passphrase must be cleared immediately"
        );

        let ctx = egui::Context::default();
        let deadline = Instant::now() + Duration::from_secs(20);
        while app.is_busy() && Instant::now() < deadline {
            app.poll_pending(&ctx);
            std::thread::sleep(Duration::from_millis(10));
        }

        assert!(!app.is_busy(), "unlock should finish");
        assert_eq!(app.state().expect("state").current(), &Screen::Ready);
    }
}
