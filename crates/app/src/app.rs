//! The eframe application.
//!
//! `CareerOpsApp` owns:
//!
//! - `AppState`, the Phase 1 state machine and the source of truth for which
//!   screen is visible.
//! - `NavigationState`, the sidebar selection once the user reaches `Ready`.
//! - A `tokio::runtime::Runtime`, used by the lock and Portals screens to run
//!   service-layer calls off the UI thread.
//! - UI-only lock state, Portals and Pipeline view state, and the in-flight
//!   async work.
//!
//! ## Why the lock state is an `Option`
//!
//! Lock operations (`try_passphrase`, `try_keychain`, `complete_first_launch`)
//! are async `&mut self` methods on `AppState`. `AsyncTask` requires `'static`,
//! so the app cannot lend the state to the task. Instead it moves the state
//! into the future and restores it with the result. While the task runs,
//! `state` is `None` and the UI shows a spinner. This keeps `state.rs`
//! untouched and avoids holding a mutex across an await.
//!
//! Portals work does not move the state: it clones the `SqlitePool` and shares
//! the fetch source and browser backend through `Arc`. The state machine stays
//! available while a fetch or mutation runs.

use std::collections::HashMap;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::Arc;

use career_ops_core::acquisition::http;
use career_ops_core::acquisition::manual;
use career_ops_core::acquisition::router::{FetchSource, HttpFetchSource};
use career_ops_core::backend::{BrowserBackend, ObscuraBrowserBackend};
use career_ops_core::model::{ApplicationStatus, FetchTier, JobDescription};
use career_ops_core::portals::service::{self, BatchEntry, BatchReport, PortalWithHealth};
use career_ops_core::repo::applications as application_repo;
use career_ops_core::repo::job_descriptions as job_repo;
use career_ops_core::repo::portals::{self as portal_repo, NewPortal, RenderMode};
use career_ops_core::{CoreError, Result as CoreResult};
use tokio::runtime::Runtime;

use crate::async_bridge::{AsyncTask, TaskPoll};
use crate::navigation::{MainScreen, NavigationState};
use crate::recovery;
use crate::screens;
use crate::screens::lock::{LockAction, LockView, RESET_PHRASE};
use crate::screens::pipeline::{PipelineAction, PipelineRenderState, PipelineView};
use crate::screens::portals::{
    ManualSuccess, PortalForm, PortalFormData, PortalModal, PortalsAction, PortalsRenderState,
    PortalsView,
};
use crate::state::{AppState, Screen};
use crate::view_models::pipeline::PipelineViewModel;
use crate::view_models::portals::{BatchProgress, PortalsViewModel};

type StateTask = AsyncTask<(AppState, CoreResult<()>)>;

/// A page of recent jobs, paired with the status of the application attached
/// to each job id.
type PipelineLoad = CoreResult<(Vec<JobDescription>, HashMap<i64, ApplicationStatus>)>;

/// How many recent jobs the Pipeline loads at once.
///
/// Phase 2.1 renders the list in a plain `ScrollArea`; a bounded page keeps the
/// per-frame cost predictable until `TableBuilder` replaces it (`docs/patterns.md`).
const PIPELINE_LIMIT: i64 = 500;

/// What the active screen asked the app to do.
///
/// The shell renders exactly one screen, so this union lets `ui()` route the
/// result without the shell knowing about either screen.
enum ScreenAction {
    Portals(PortalsAction),
    Pipeline(PipelineAction),
}

enum PortalsTask {
    List {
        task: AsyncTask<CoreResult<Vec<PortalWithHealth>>>,
        close_modal: bool,
    },
    Fetch(AsyncTask<CoreResult<service::FetchReport>>),
    Batch(AsyncTask<CoreResult<BatchReport>>),
    Manual(AsyncTask<CoreResult<ManualOutcome>>),
}

enum PortalsPoll {
    List(CoreResult<Vec<PortalWithHealth>>, bool),
    Fetch(CoreResult<service::FetchReport>),
    Batch(CoreResult<BatchReport>),
    Manual(CoreResult<ManualOutcome>),
    Cancelled,
}

enum ManualOutcome {
    Url {
        title: String,
        company: String,
        tier: FetchTier,
        warnings: Vec<String>,
    },
    Text {
        title: String,
        company: String,
    },
}

/// The root application object passed to `eframe::run_native`.
pub struct CareerOpsApp {
    /// `None` only while an async lock transition owns the state.
    state: Option<AppState>,
    data_dir: PathBuf,
    navigation: NavigationState,
    runtime: Runtime,
    lock_view: LockView,
    pending: Option<StateTask>,
    /// Set only if an interrupted lock task lost the state and it could not be
    /// reopened. The app then renders a fatal screen instead of a spinner.
    fatal_error: Option<String>,

    fetch_source: Arc<dyn FetchSource>,
    browser: Arc<ObscuraBrowserBackend>,

    portals: PortalsViewModel,
    portals_view: PortalsView,
    portals_loaded: bool,
    portals_error: Option<String>,
    portals_message: Option<String>,
    portals_task: Option<PortalsTask>,
    batch_progress_rx: Option<Receiver<BatchEntry>>,
    batch_cancel: Arc<AtomicBool>,
    batch_ids: Vec<i64>,
    fetching_portal: Option<i64>,

    pipeline: PipelineViewModel,
    pipeline_view: PipelineView,
    pipeline_loaded: bool,
    pipeline_error: Option<String>,
    pipeline_task: Option<AsyncTask<PipelineLoad>>,
}

impl CareerOpsApp {
    pub fn new(state: AppState, runtime: Runtime) -> Self {
        let client = http::default_client().expect("build the HTTP client");
        Self::with_fetch_source(state, runtime, Arc::new(HttpFetchSource::new(client)))
    }

    /// Construct the app with a specific Tier 1 fetch source.
    ///
    /// Production always goes through `new`, which uses `HttpFetchSource`. This
    /// constructor exists so app-level tests can drive the real orchestration —
    /// router, storage, portal health, pipeline reload — with a scripted source,
    /// because the network paths in `docs/manual-test-script.md` cannot run in
    /// CI. The router already depends on the `FetchSource` trait, so this adds a
    /// test seam without changing any production behaviour.
    pub fn with_fetch_source(
        state: AppState,
        runtime: Runtime,
        fetch_source: Arc<dyn FetchSource>,
    ) -> Self {
        let data_dir = state.data_dir().clone();

        Self {
            state: Some(state),
            data_dir,
            navigation: NavigationState::new(),
            runtime,
            lock_view: LockView::default(),
            pending: None,
            fatal_error: None,
            fetch_source,
            browser: Arc::new(ObscuraBrowserBackend::with_defaults()),
            portals: PortalsViewModel::default(),
            portals_view: PortalsView::default(),
            portals_loaded: false,
            portals_error: None,
            portals_message: None,
            portals_task: None,
            batch_progress_rx: None,
            batch_cancel: Arc::new(AtomicBool::new(false)),
            batch_ids: Vec::new(),
            fetching_portal: None,
            pipeline: PipelineViewModel::default(),
            pipeline_view: PipelineView::default(),
            pipeline_loaded: false,
            pipeline_error: None,
            pipeline_task: None,
        }
    }

    /// The state machine, if no async lock transition currently owns it.
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

    /// Execute an intent emitted by the Portals screen.
    ///
    /// Public so tests can drive the same path the UI uses without rendering.
    pub fn handle_portals_action(&mut self, action: PortalsAction) {
        match action {
            PortalsAction::Reload => {
                self.portals_message = None;
                self.start_portals_load(false);
            }
            PortalsAction::OpenAdd => {
                self.portals_view.modal = Some(PortalModal::Add(PortalForm::default()));
            }
            PortalsAction::OpenEdit(id) => {
                if let Some(row) = self.portals.row(id) {
                    self.portals_view.modal = Some(PortalModal::Edit {
                        id,
                        form: PortalForm::from_row(row),
                    });
                }
            }
            PortalsAction::OpenDelete(id) => {
                if let Some(row) = self.portals.row(id) {
                    self.portals_view.modal = Some(PortalModal::Delete {
                        id,
                        name: row.name.clone(),
                    });
                }
            }
            PortalsAction::OpenManual => self.portals_view.open_manual(),
            PortalsAction::CloseModal => self.portals_view.modal = None,
            PortalsAction::CreatePortal(data) => self.start_portal_mutation(None, data),
            PortalsAction::UpdatePortal { id, data } => {
                self.start_portal_mutation(Some(id), data);
            }
            PortalsAction::DeletePortal(id) => self.start_portal_delete(id),
            PortalsAction::FetchOne(id) => self.start_portal_fetch(id),
            PortalsAction::FetchAll => self.start_portal_batch(),
            PortalsAction::CancelBatch => {
                self.batch_cancel.store(true, Ordering::Relaxed);
                if let Some(batch) = &mut self.portals.batch {
                    batch.cancelled = true;
                    batch.in_flight = None;
                }
            }
            PortalsAction::DismissBatch => {
                self.portals.batch = None;
                self.batch_progress_rx = None;
                self.batch_ids.clear();
            }
            PortalsAction::ManualFromUrl(url) => self.start_manual_url(url),
            PortalsAction::ManualFromText {
                title,
                company,
                body,
            } => self.start_manual_text(title, company, body),
            PortalsAction::ShowInPipeline => {
                self.portals_view.modal = None;
                self.navigation.navigate_to(MainScreen::Pipeline);
            }
        }
    }

    /// Execute an intent emitted by the Pipeline screen.
    ///
    /// Public so tests can drive the same path the UI uses without rendering.
    pub fn handle_pipeline_action(&mut self, action: PipelineAction) {
        match action {
            PipelineAction::AddJob => self.portals_view.open_manual(),
            PipelineAction::Reload => self.start_pipeline_load(),
            PipelineAction::SetFilter(filter) => self.pipeline.filter = filter,
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
                            // Any Portals or Pipeline data belongs to the
                            // previous vault or session; load it fresh after
                            // this unlock.
                            self.reset_ready_state();
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

    // ------------------------------------------------------------------
    // Portals
    // ------------------------------------------------------------------

    fn reset_ready_state(&mut self) {
        self.portals = PortalsViewModel::default();
        self.portals_view = PortalsView::default();
        self.portals_loaded = false;
        self.portals_error = None;
        self.portals_message = None;
        self.portals_task = None;
        self.batch_progress_rx = None;
        self.batch_cancel = Arc::new(AtomicBool::new(false));
        self.batch_ids.clear();
        self.fetching_portal = None;

        self.pipeline = PipelineViewModel::default();
        self.pipeline_view = PipelineView::default();
        self.pipeline_loaded = false;
        self.pipeline_error = None;
        self.pipeline_task = None;
    }

    fn pool(&self) -> Option<sqlx::SqlitePool> {
        self.state.as_ref().and_then(|state| state.pool().cloned())
    }

    fn begin_portals_task(&mut self) -> bool {
        if self.portals_task.is_some() {
            return false;
        }

        self.portals_error = None;
        self.portals_message = None;
        self.fetching_portal = None;
        true
    }

    fn start_portals_load(&mut self, close_modal: bool) {
        if !self.begin_portals_task() {
            return;
        }

        let Some(pool) = self.pool() else {
            self.portals_error = Some("The database is not open.".into());
            return;
        };

        let task = self.spawn(async move { service::list_with_health(&pool).await });
        self.portals_task = Some(PortalsTask::List { task, close_modal });
    }

    fn start_portal_mutation(&mut self, id: Option<i64>, data: PortalFormData) {
        let input = match new_portal_input(&data) {
            Ok(input) => input,
            Err(error) => {
                self.portals_view.set_form_error(error);
                return;
            }
        };

        if !self.begin_portals_task() {
            return;
        }

        let Some(pool) = self.pool() else {
            self.portals_error = Some("The database is not open.".into());
            return;
        };

        let task = self.spawn(async move {
            match id {
                Some(id) => {
                    portal_repo::update(&pool, id, input).await?;
                }
                None => {
                    portal_repo::create(&pool, input).await?;
                }
            }
            service::list_with_health(&pool).await
        });

        self.portals_task = Some(PortalsTask::List {
            task,
            close_modal: true,
        });
    }

    fn start_portal_delete(&mut self, id: i64) {
        if !self.begin_portals_task() {
            return;
        }

        let Some(pool) = self.pool() else {
            self.portals_error = Some("The database is not open.".into());
            return;
        };

        let task = self.spawn(async move {
            portal_repo::delete(&pool, id).await?;
            service::list_with_health(&pool).await
        });

        self.portals_task = Some(PortalsTask::List {
            task,
            close_modal: true,
        });
    }

    fn start_portal_fetch(&mut self, id: i64) {
        if !self.begin_portals_task() {
            return;
        }

        let Some(pool) = self.pool() else {
            self.portals_error = Some("The database is not open.".into());
            return;
        };

        let source = Arc::clone(&self.fetch_source);
        let browser = Arc::clone(&self.browser);
        let task = self.spawn(async move {
            service::fetch_one(&pool, id, source.as_ref(), Some(browser.as_ref())).await
        });

        self.fetching_portal = Some(id);
        self.portals_task = Some(PortalsTask::Fetch(task));
    }

    fn start_portal_batch(&mut self) {
        let ids: Vec<i64> = self.portals.rows.iter().map(|row| row.id).collect();
        if ids.is_empty() || !self.begin_portals_task() {
            return;
        }

        let Some(pool) = self.pool() else {
            self.portals_error = Some("The database is not open.".into());
            return;
        };

        let source = Arc::clone(&self.fetch_source);
        let browser = Arc::clone(&self.browser);
        let cancel = Arc::new(AtomicBool::new(false));
        let cancel_for_task = Arc::clone(&cancel);
        let (sender, receiver) = mpsc::channel();

        self.batch_progress_rx = Some(receiver);
        self.batch_cancel = cancel;
        self.batch_ids = ids.clone();
        self.portals.batch = Some(BatchProgress {
            total: ids.len(),
            completed: Vec::new(),
            in_flight: ids.first().copied(),
            cancelled: false,
        });

        let task = self.spawn(async move {
            let progress = move |entry: &BatchEntry| {
                let _ = sender.send(entry.clone());
            };
            service::fetch_batch_cancellable(
                &pool,
                &ids,
                source.as_ref(),
                Some(browser.as_ref()),
                cancel_for_task.as_ref(),
                progress,
            )
            .await
        });

        self.portals_task = Some(PortalsTask::Batch(task));
    }

    fn start_manual_url(&mut self, url: String) {
        if !self.begin_portals_task() {
            return;
        }

        let Some(pool) = self.pool() else {
            self.portals_error = Some("The database is not open.".into());
            return;
        };

        if let Some(manual) = self.portals_view.manual_mut() {
            manual.error = None;
            manual.result = None;
        }

        let source = Arc::clone(&self.fetch_source);
        let browser = Arc::clone(&self.browser);
        let task = self.spawn(async move {
            manual::add_job_from_url(&pool, &url, source.as_ref(), Some(browser.as_ref()))
                .await
                .map(|result| ManualOutcome::Url {
                    title: result.stored.title,
                    company: result.stored.company,
                    tier: result.tier,
                    warnings: result.warnings,
                })
        });

        self.portals_task = Some(PortalsTask::Manual(task));
    }

    fn start_manual_text(&mut self, title: String, company: String, body: String) {
        if !self.begin_portals_task() {
            return;
        }

        let Some(pool) = self.pool() else {
            self.portals_error = Some("The database is not open.".into());
            return;
        };

        if let Some(manual) = self.portals_view.manual_mut() {
            manual.error = None;
            manual.result = None;
        }

        let task = self.spawn(async move {
            manual::add_job_from_text(&pool, &title, &company, &body)
                .await
                .map(|stored| ManualOutcome::Text {
                    title: stored.title,
                    company: stored.company,
                })
        });

        self.portals_task = Some(PortalsTask::Manual(task));
    }

    fn poll_portals_task(&mut self, ctx: &egui::Context) {
        let Some(task) = &mut self.portals_task else {
            return;
        };

        let mut finished = false;
        let poll = match task {
            PortalsTask::List { task, close_modal } => match task.try_recv() {
                TaskPoll::Ready(result) => {
                    finished = true;
                    Some(PortalsPoll::List(result, *close_modal))
                }
                TaskPoll::Pending => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(50));
                    None
                }
                TaskPoll::Cancelled => {
                    finished = true;
                    Some(PortalsPoll::Cancelled)
                }
            },
            PortalsTask::Fetch(task) => match task.try_recv() {
                TaskPoll::Ready(result) => {
                    finished = true;
                    Some(PortalsPoll::Fetch(result))
                }
                TaskPoll::Pending => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(50));
                    None
                }
                TaskPoll::Cancelled => {
                    finished = true;
                    Some(PortalsPoll::Cancelled)
                }
            },
            PortalsTask::Batch(task) => match task.try_recv() {
                TaskPoll::Ready(result) => {
                    finished = true;
                    Some(PortalsPoll::Batch(result))
                }
                TaskPoll::Pending => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(50));
                    None
                }
                TaskPoll::Cancelled => {
                    finished = true;
                    Some(PortalsPoll::Cancelled)
                }
            },
            PortalsTask::Manual(task) => match task.try_recv() {
                TaskPoll::Ready(result) => {
                    finished = true;
                    Some(PortalsPoll::Manual(result))
                }
                TaskPoll::Pending => {
                    ctx.request_repaint_after(std::time::Duration::from_millis(50));
                    None
                }
                TaskPoll::Cancelled => {
                    finished = true;
                    Some(PortalsPoll::Cancelled)
                }
            },
        };

        if finished {
            self.portals_task = None;
        }

        let Some(poll) = poll else {
            return;
        };

        match poll {
            PortalsPoll::List(result, close_modal) => match result {
                Ok(entries) => {
                    self.portals.replace_entries(entries);
                    self.portals_loaded = true;
                    self.portals_error = None;
                    if close_modal {
                        self.portals_view.modal = None;
                    }
                }
                Err(error) => {
                    let message = error.to_string();
                    self.portals_error = Some(message.clone());
                    if close_modal {
                        self.portals_view.set_form_error(message);
                    }
                }
            },
            PortalsPoll::Fetch(result) => {
                self.fetching_portal = None;
                match result {
                    Ok(report) => {
                        let id = report.portal.id;
                        self.portals.apply_fetch_result(id, &report);
                    }
                    Err(error) => {
                        self.portals_error = Some(error.to_string());
                    }
                }
            }
            PortalsPoll::Batch(result) => {
                self.drain_batch_progress();
                match result {
                    Ok(report) => {
                        let cancelled = self.batch_cancel.load(Ordering::Relaxed);
                        let total = self.batch_ids.len();

                        // If the user dismissed the progress bar while the
                        // last fetch was finishing, the channel is gone but
                        // the report still carries the rows' outcomes.
                        if self.portals.batch.is_none() {
                            for entry in &report.results {
                                self.portals.apply_batch_entry(entry);
                            }
                        }

                        if let Some(batch) = &mut self.portals.batch {
                            batch.cancelled |= cancelled;
                            batch.in_flight = None;
                        }

                        self.portals_message = Some(if cancelled {
                            format!(
                                "Fetch cancelled after {} of {} portals.",
                                report.results.len(),
                                total
                            )
                        } else {
                            format!(
                                "Fetched {} portals: {} succeeded, {} failed.",
                                report.results.len(),
                                report.succeeded(),
                                report.failed()
                            )
                        });
                    }
                    Err(error) => {
                        if let Some(batch) = &mut self.portals.batch {
                            batch.in_flight = None;
                        }
                        self.portals_error = Some(format!("Batch fetch failed: {error}"));
                    }
                }
            }
            PortalsPoll::Manual(result) => match result {
                Ok(outcome) => {
                    let success = match outcome {
                        ManualOutcome::Url {
                            title,
                            company,
                            tier,
                            warnings,
                        } => ManualSuccess {
                            title,
                            company,
                            detail: format!("Added via {}.", tier.as_str()),
                            warnings,
                        },
                        ManualOutcome::Text { title, company } => ManualSuccess {
                            title,
                            company,
                            detail: "Added manually.".into(),
                            warnings: Vec::new(),
                        },
                    };

                    if let Some(manual) = self.portals_view.manual_mut() {
                        manual.error = None;
                        manual.result = Some(success);
                    }

                    // A new job invalidates the loaded Pipeline page. Drop an
                    // in-flight refresh so its pre-insert result cannot land
                    // after this one; `ui()` reloads once Pipeline is visible.
                    self.pipeline_task = None;
                    self.pipeline_loaded = false;
                }
                Err(error) => {
                    let message = error.to_string();
                    if let Some(manual) = self.portals_view.manual_mut() {
                        manual.error = Some(message);
                    } else {
                        self.portals_error = Some(message);
                    }
                }
            },
            PortalsPoll::Cancelled => {
                self.fetching_portal = None;
                if let Some(batch) = &mut self.portals.batch {
                    batch.in_flight = None;
                    batch.cancelled = true;
                }
                self.portals_error = Some("The operation was interrupted.".into());
            }
        }

        ctx.request_repaint();
    }

    fn drain_batch_progress(&mut self) {
        let entries: Vec<BatchEntry> = match &self.batch_progress_rx {
            Some(receiver) => receiver.try_iter().collect(),
            None => return,
        };

        for entry in entries {
            self.portals.apply_batch_entry(&entry);
            self.update_batch_in_flight();
        }
    }

    fn update_batch_in_flight(&mut self) {
        let ids = self.batch_ids.clone();
        let Some(batch) = &mut self.portals.batch else {
            return;
        };

        if batch.cancelled || batch.completed.len() >= batch.total {
            batch.in_flight = None;
            return;
        }

        let completed: Vec<i64> = batch
            .completed
            .iter()
            .map(|entry| entry.portal_id)
            .collect();
        batch.in_flight = ids.iter().copied().find(|id| !completed.contains(id));
    }

    // ------------------------------------------------------------------
    // End Portals
    // ------------------------------------------------------------------

    // ------------------------------------------------------------------
    // Pipeline
    // ------------------------------------------------------------------

    fn start_pipeline_load(&mut self) {
        if self.pipeline_task.is_some() {
            return;
        }

        self.pipeline_error = None;

        let Some(pool) = self.pool() else {
            self.pipeline_error = Some("The database is not open.".into());
            return;
        };

        let task = self.spawn(async move {
            // Two reads joined in memory. The Pipeline shows one status per job,
            // because a job description has at most one application.
            let jobs = job_repo::list_recent(&pool, PIPELINE_LIMIT).await?;
            let applications = application_repo::list(&pool).await?;
            let statuses = applications
                .into_iter()
                .filter_map(|application| {
                    ApplicationStatus::parse(&application.status)
                        .map(|status| (application.job_description_id, status))
                })
                .collect();
            Ok((jobs, statuses))
        });

        self.pipeline_task = Some(task);
    }

    fn poll_pipeline_task(&mut self, ctx: &egui::Context) {
        let poll = match self.pipeline_task.as_mut() {
            Some(task) => task.try_recv(),
            None => return,
        };

        match poll {
            TaskPoll::Ready(result) => {
                self.pipeline_task = None;
                match result {
                    Ok((jobs, statuses)) => {
                        self.pipeline.replace_jobs(jobs, &statuses);
                        self.pipeline_loaded = true;
                        self.pipeline_error = None;
                    }
                    Err(error) => self.pipeline_error = Some(error.to_string()),
                }
                ctx.request_repaint();
            }
            TaskPoll::Pending => {
                ctx.request_repaint_after(std::time::Duration::from_millis(50));
            }
            TaskPoll::Cancelled => {
                self.pipeline_task = None;
                self.pipeline_error = Some("The operation was interrupted.".into());
                ctx.request_repaint();
            }
        }
    }

    // ------------------------------------------------------------------
    // End Pipeline
    // ------------------------------------------------------------------
}

impl Drop for CareerOpsApp {
    fn drop(&mut self) {
        // Abort any in-flight work before asking the sidecar to exit.
        self.portals_task = None;
        self.pipeline_task = None;
        self.pending = None;

        if tokio::runtime::Handle::try_current().is_err() {
            let _ = self.runtime.block_on(self.browser.shutdown());
        }
    }
}

fn new_portal_input(data: &PortalFormData) -> std::result::Result<NewPortal, String> {
    let company_name = data.company_name.trim();
    let url = data.url.trim();

    if company_name.is_empty() {
        return Err("Company name cannot be empty.".into());
    }

    let parsed = url::Url::parse(url).map_err(|error| format!("Invalid URL: {error}"))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err("The URL must use http or https.".into());
    }

    let render_mode =
        RenderMode::parse(&data.render_mode).ok_or_else(|| "Unknown render mode.".to_string())?;
    let scraper_config = match data.scraper_config.as_deref() {
        Some(raw) if !raw.trim().is_empty() => Some(
            serde_json::from_str(raw)
                .map_err(|error| format!("Scraper config is not valid JSON: {error}"))?,
        ),
        _ => None,
    };

    Ok(NewPortal {
        company_name: company_name.to_string(),
        url: url.to_string(),
        render_mode,
        scraper_config,
    })
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
            Screen::Ready => {
                self.poll_portals_task(ui.ctx());
                self.poll_pipeline_task(ui.ctx());
                self.drain_batch_progress();

                if self.navigation.current() == MainScreen::Portals
                    && self.portals_task.is_none()
                    && !self.portals_loaded
                    && self.portals_error.is_none()
                {
                    self.start_portals_load(false);
                }

                if self.navigation.current() == MainScreen::Pipeline
                    && self.pipeline_task.is_none()
                    && !self.pipeline_loaded
                    && self.pipeline_error.is_none()
                {
                    self.start_pipeline_load();
                }

                let render_state = PortalsRenderState {
                    loading: matches!(&self.portals_task, Some(PortalsTask::List { .. })),
                    busy: self.portals_task.is_some(),
                    manual_busy: matches!(&self.portals_task, Some(PortalsTask::Manual(_))),
                    fetching_portal: self.fetching_portal,
                    error: self.portals_error.as_deref(),
                    message: self.portals_message.as_deref(),
                };
                let pipeline_state = PipelineRenderState {
                    loading: self.pipeline_task.is_some(),
                    error: self.pipeline_error.as_deref(),
                };

                let navigation = &mut self.navigation;
                let portals = &self.portals;
                let pipeline = &self.pipeline;
                let pipeline_view = &mut self.pipeline_view;

                let action = screens::shell::show_with(ui, navigation, |ui, screen| match screen {
                    MainScreen::Portals => {
                        screens::portals::show(ui, portals, render_state).map(ScreenAction::Portals)
                    }
                    MainScreen::Pipeline => {
                        screens::pipeline::show(ui, pipeline, pipeline_view, pipeline_state)
                            .map(ScreenAction::Pipeline)
                    }
                    other => {
                        screens::shell::placeholder(ui, other);
                        None
                    }
                });

                // The modal is app-level chrome: it is reachable from both
                // Portals and Pipeline, so it renders after the active screen.
                let modal_action = screens::portals::show_modal(
                    ui,
                    &mut self.portals_view,
                    render_state.modal_state(),
                );

                match action {
                    Some(ScreenAction::Portals(action)) => self.handle_portals_action(action),
                    Some(ScreenAction::Pipeline(action)) => self.handle_pipeline_action(action),
                    None => {}
                }

                if let Some(action) = modal_action {
                    self.handle_portals_action(action);
                }
            }
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
    use crate::view_models::pipeline::{JobFilter, StatusLabel, TierLabel};
    use crate::view_models::portals::{HealthBadge, PortalRow};

    use career_ops_core::acquisition::ats::{AtsAdapter, NormalizedJob};
    use career_ops_core::acquisition::error::AtsResult;
    use career_ops_core::acquisition::html::selectors::SelectorConfig;

    /// Serializes the tests that create a vault. The test keychain uses one
    /// fixed service/account pair for the whole process, so two first-launch
    /// tests running in parallel would overwrite each other's KEK.
    static KEYCHAIN_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    const TEST_PASSPHRASE: &str = "first-launch-ui-test-passphrase";

    fn test_app() -> (CareerOpsApp, tempfile::TempDir) {
        let dir = tempfile::tempdir().expect("tempdir");
        let state = AppState::bootstrap(dir.path().to_path_buf()).expect("bootstrap");
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .expect("runtime");
        (CareerOpsApp::new(state, runtime), dir)
    }

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

    /// The recovery path resolves back to the main layout: a database without
    /// its wrapped key reaches Recovery, `LocateKeyFile` reinstalls the key and
    /// returns to `Unlocking`, and the passphrase then reaches `Ready`.
    #[test]
    fn recovery_restores_a_key_file_and_reaches_ready() {
        let _guard = KEYCHAIN_TEST_LOCK.lock().expect("keychain test lock");
        career_ops_test_support::install();

        let dir = tempfile::tempdir().expect("tempdir");

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

        // Move the wrapped key aside so bootstrap sees a database without a
        // key — the recovery case a misplaced backup produces.
        let backup = tempfile::tempdir().expect("backup tempdir");
        let wrapped = career_ops_core::keystore::WRAPPED_FILE;
        std::fs::rename(dir.path().join(wrapped), backup.path().join(wrapped))
            .expect("move the wrapped key aside");

        let state = AppState::bootstrap(dir.path().to_path_buf()).expect("recovery bootstrap");
        assert!(
            matches!(state.current(), Screen::Recovery { .. }),
            "a database without a wrapped key must reach Recovery"
        );

        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("runtime");
        let mut app = CareerOpsApp::new(state, runtime);

        app.handle_lock_action(LockAction::LocateKeyFile(backup.path().join(wrapped)));
        assert_eq!(
            app.state().expect("state").current(),
            &Screen::Unlocking,
            "installing the key file must return to the unlock screen"
        );

        let ctx = egui::Context::default();
        app.handle_lock_action(LockAction::TryPassphrase(TEST_PASSPHRASE.to_string()));
        wait_until(&mut app, &ctx, |app| {
            app.state()
                .is_some_and(|state| state.current() == &Screen::Ready)
        });
        assert!(app.state().expect("state").pool().is_some());
    }

    #[test]
    fn open_edit_populates_the_form_from_the_row() {
        let (mut app, _dir) = test_app();
        app.portals.rows.push(PortalRow {
            id: 7,
            name: "Acme".into(),
            url: "https://example.com/jobs".into(),
            render_mode: "browser".into(),
            scraper_config: Some("{\"title\":\"h1\"}".into()),
            health: HealthBadge::NeverFetched,
            last_attempt: None,
            suggested_action: None,
        });

        app.handle_portals_action(PortalsAction::OpenEdit(7));

        match &app.portals_view.modal {
            Some(PortalModal::Edit { id, form }) => {
                assert_eq!(*id, 7);
                assert_eq!(form.company_name, "Acme");
                assert_eq!(form.url, "https://example.com/jobs");
                assert_eq!(form.render_mode.as_str(), "browser");
                assert_eq!(form.scraper_config.as_deref(), Some("{\"title\":\"h1\"}"));
            }
            _ => panic!("expected the edit modal"),
        }
    }

    #[test]
    fn drain_batch_progress_updates_rows_and_advances_in_flight() {
        let (mut app, _dir) = test_app();
        app.portals.rows.push(PortalRow {
            id: 1,
            name: "One".into(),
            url: "https://example.com/1".into(),
            render_mode: "static".into(),
            scraper_config: None,
            health: HealthBadge::NeverFetched,
            last_attempt: None,
            suggested_action: None,
        });
        app.portals.rows.push(PortalRow {
            id: 2,
            name: "Two".into(),
            url: "https://example.com/2".into(),
            render_mode: "static".into(),
            scraper_config: None,
            health: HealthBadge::NeverFetched,
            last_attempt: None,
            suggested_action: None,
        });
        app.portals.batch = Some(BatchProgress {
            total: 2,
            in_flight: Some(1),
            ..Default::default()
        });
        app.batch_ids = vec![1, 2];

        let (sender, receiver) = mpsc::channel();
        app.batch_progress_rx = Some(receiver);
        sender
            .send(BatchEntry {
                portal_id: 1,
                portal_name: "One".into(),
                result: Err(career_ops_core::acquisition::router::FetchError {
                    cause: career_ops_core::acquisition::error::FailureCause::Timeout,
                    detail: "timed out".into(),
                }),
            })
            .expect("send progress");

        app.drain_batch_progress();

        let batch = app.portals.batch.as_ref().expect("batch");
        assert_eq!(batch.completed.len(), 1);
        assert_eq!(batch.in_flight, Some(2));
        assert_eq!(
            app.portals.rows[0].health,
            HealthBadge::Broken {
                cause: "timeout".into()
            }
        );
        assert!(app.portals.rows[0].suggested_action.is_some());
    }

    #[test]
    fn cancel_batch_sets_the_shared_flag_and_marks_progress() {
        let (mut app, _dir) = test_app();
        app.portals.batch = Some(BatchProgress {
            total: 2,
            in_flight: Some(1),
            ..Default::default()
        });

        app.handle_portals_action(PortalsAction::CancelBatch);

        assert!(app.batch_cancel.load(Ordering::Relaxed));
        let batch = app.portals.batch.as_ref().expect("batch");
        assert!(batch.cancelled);
        assert!(batch.in_flight.is_none());
    }

    #[test]
    fn dismiss_batch_clears_progress_state() {
        let (mut app, _dir) = test_app();
        app.portals.batch = Some(BatchProgress {
            total: 2,
            ..Default::default()
        });
        app.batch_ids = vec![1, 2];

        app.handle_portals_action(PortalsAction::DismissBatch);

        assert!(app.portals.batch.is_none());
        assert!(app.batch_ids.is_empty());
    }

    #[test]
    fn pipeline_add_job_opens_the_shared_manual_modal() {
        let (mut app, _dir) = test_app();

        app.handle_pipeline_action(PipelineAction::AddJob);

        assert!(matches!(
            app.portals_view.modal,
            Some(PortalModal::Manual(_))
        ));
    }

    #[test]
    fn pipeline_set_filter_only_mutates_the_view_model() {
        let (mut app, _dir) = test_app();

        app.handle_pipeline_action(PipelineAction::SetFilter(JobFilter::Rejected));

        assert_eq!(app.pipeline.filter, JobFilter::Rejected);
        assert!(!app.pipeline_loaded, "changing a filter must not load");
    }

    fn wait_until(
        app: &mut CareerOpsApp,
        ctx: &egui::Context,
        condition: impl FnMut(&CareerOpsApp) -> bool,
    ) {
        wait_until_within(app, ctx, Duration::from_secs(20), condition);
    }

    fn wait_until_within(
        app: &mut CareerOpsApp,
        ctx: &egui::Context,
        timeout: Duration,
        mut condition: impl FnMut(&CareerOpsApp) -> bool,
    ) {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            app.poll_pending(ctx);
            app.poll_portals_task(ctx);
            app.poll_pipeline_task(ctx);
            app.drain_batch_progress();
            if condition(app) {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!("condition not met within {timeout:?}");
    }

    /// Exercises the Portals task path against a real encrypted database:
    /// create, update, manual paste, and delete. Fetching is covered by the
    /// core service tests; it needs the network and is not run here.
    #[test]
    fn portals_crud_and_paste_through_the_app() {
        let _guard = KEYCHAIN_TEST_LOCK.lock().expect("keychain test lock");
        career_ops_test_support::install();

        let dir = tempfile::tempdir().expect("tempdir");

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
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("runtime");
        let mut app = CareerOpsApp::new(state, runtime);
        let ctx = egui::Context::default();

        app.handle_lock_action(LockAction::TryPassphrase(TEST_PASSPHRASE.to_string()));
        wait_until(&mut app, &ctx, |app| {
            app.state()
                .is_some_and(|state| state.current() == &Screen::Ready)
        });

        app.handle_portals_action(PortalsAction::CreatePortal(PortalFormData {
            company_name: "Acme".into(),
            url: "https://example.com/jobs".into(),
            render_mode: "static".into(),
            scraper_config: None,
        }));
        wait_until(&mut app, &ctx, |app| {
            app.portals.rows.len() == 1 && app.portals_task.is_none()
        });

        let id = app.portals.rows[0].id;
        assert_eq!(app.portals.rows[0].name, "Acme");

        app.handle_portals_action(PortalsAction::UpdatePortal {
            id,
            data: PortalFormData {
                company_name: "Acme Updated".into(),
                url: "https://example.com/jobs".into(),
                render_mode: "browser".into(),
                scraper_config: None,
            },
        });
        wait_until(&mut app, &ctx, |app| {
            app.portals.rows.len() == 1
                && app.portals.rows[0].name == "Acme Updated"
                && app.portals_task.is_none()
        });
        assert_eq!(app.portals.rows[0].render_mode, "browser");

        app.handle_portals_action(PortalsAction::OpenManual);
        app.handle_portals_action(PortalsAction::ManualFromText {
            title: "Engineer".into(),
            company: "Acme".into(),
            body: "Build things.".into(),
        });
        wait_until(&mut app, &ctx, |app| {
            app.portals_view.manual_result().is_some()
        });

        let manual = app.portals_view.manual_mut().expect("manual modal");
        let result = manual.result.as_ref().expect("manual result");
        assert_eq!(result.title, "Engineer");
        assert_eq!(result.company, "Acme");
        assert_eq!(result.detail, "Added manually.");

        app.handle_portals_action(PortalsAction::DeletePortal(id));
        wait_until(&mut app, &ctx, |app| {
            app.portals.rows.is_empty() && app.portals_task.is_none()
        });
        assert!(app.portals_view.modal.is_none());
    }

    /// Exercises the Pipeline task against a real encrypted database: the list
    /// loads, application statuses are joined in, the filter survives a
    /// reload, and a manual add invalidates the loaded page.
    #[test]
    fn pipeline_joins_statuses_and_manual_add_invalidates_it() {
        let _guard = KEYCHAIN_TEST_LOCK.lock().expect("keychain test lock");
        career_ops_test_support::install();

        let dir = tempfile::tempdir().expect("tempdir");

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
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("runtime");
        let mut app = CareerOpsApp::new(state, runtime);
        let ctx = egui::Context::default();

        app.handle_lock_action(LockAction::TryPassphrase(TEST_PASSPHRASE.to_string()));
        wait_until(&mut app, &ctx, |app| {
            app.state()
                .is_some_and(|state| state.current() == &Screen::Ready)
        });

        app.start_pipeline_load();
        wait_until(&mut app, &ctx, |app| {
            app.pipeline_loaded && app.pipeline_task.is_none()
        });
        assert!(app.pipeline.rows.is_empty());

        // The Pipeline's "Add job" opens the same modal Portals uses.
        app.handle_pipeline_action(PipelineAction::AddJob);
        app.handle_portals_action(PortalsAction::ManualFromText {
            title: "Engineer".into(),
            company: "Acme".into(),
            body: "Build things.".into(),
        });
        wait_until(&mut app, &ctx, |app| {
            app.portals_view.manual_result().is_some()
        });
        assert!(
            !app.pipeline_loaded,
            "a manual add must invalidate the loaded page"
        );

        app.start_pipeline_load();
        wait_until(&mut app, &ctx, |app| {
            app.pipeline_loaded && app.pipeline_task.is_none()
        });
        assert_eq!(app.pipeline.rows.len(), 1);
        assert_eq!(app.pipeline.rows[0].title, "Engineer");
        assert_eq!(app.pipeline.rows[0].tier, TierLabel::Manual);
        assert!(app.pipeline.rows[0].status.is_none());

        let job_id = app.pipeline.rows[0].id;
        let pool = app.pool().expect("pool");
        app.runtime()
            .block_on(application_repo::create(&pool, job_id))
            .expect("create application");

        app.handle_pipeline_action(PipelineAction::SetFilter(JobFilter::Applied));
        assert_eq!(app.pipeline.apply_filter().count(), 0);

        app.start_pipeline_load();
        wait_until(&mut app, &ctx, |app| {
            app.pipeline_loaded && app.pipeline_task.is_none()
        });
        assert_eq!(
            app.pipeline.rows[0].status,
            Some(StatusLabel::Discovered),
            "the application status is joined by job id"
        );
        assert_eq!(
            app.pipeline.filter,
            JobFilter::Applied,
            "a reload keeps the user's filter"
        );
        assert_eq!(
            app.pipeline.apply_filter().count(),
            0,
            "a discovered application is not in the applied filter"
        );
    }

    /// A `FetchSource` that returns one scripted HTML job and records the URLs
    /// it was asked for.
    ///
    /// The network half of the manual test script cannot run in CI, so this
    /// substitutes only the HTTP layer: the router, storage, portal health, and
    /// the pipeline load are the real ones.
    #[derive(Default)]
    struct StubFetchSource {
        calls: std::sync::Mutex<Vec<String>>,
    }

    impl StubFetchSource {
        fn calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }
    }

    #[async_trait::async_trait]
    impl FetchSource for StubFetchSource {
        async fn fetch_ats(
            &self,
            _adapter: &dyn AtsAdapter,
            _slug: &str,
            _company_name: &str,
        ) -> AtsResult<Vec<NormalizedJob>> {
            Ok(Vec::new())
        }

        async fn fetch_html(
            &self,
            url: &url::Url,
            _company_name: &str,
            _config: Option<&SelectorConfig>,
        ) -> AtsResult<Vec<NormalizedJob>> {
            self.calls.lock().unwrap().push(url.to_string());
            Ok(vec![NormalizedJob {
                external_id: "job-1".into(),
                title: "Engineer".into(),
                company: "Acme".into(),
                location: Some("Remote".into()),
                department: Some("Engineering".into()),
                employment_type: None,
                workplace_type: None,
                source_url: url.clone(),
                raw_text: "Build things.".into(),
                posted_at: None,
            }])
        }
    }

    /// Drives a portal fetch through the app with a scripted HTTP source, then
    /// asserts that the stored job reaches the Pipeline. This is the
    /// orchestration half of manual-test-script §4.4 and §5.2 without a
    /// network.
    #[test]
    fn portal_fetch_stores_a_job_that_the_pipeline_lists() {
        let _guard = KEYCHAIN_TEST_LOCK.lock().expect("keychain test lock");
        career_ops_test_support::install();

        let dir = tempfile::tempdir().expect("tempdir");

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
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("runtime");
        let source = Arc::new(StubFetchSource::default());
        let mut app = CareerOpsApp::with_fetch_source(state, runtime, source.clone());
        let ctx = egui::Context::default();

        app.handle_lock_action(LockAction::TryPassphrase(TEST_PASSPHRASE.to_string()));
        wait_until(&mut app, &ctx, |app| {
            app.state()
                .is_some_and(|state| state.current() == &Screen::Ready)
        });

        // A non-ATS URL in static mode routes to Tier 1b, which is the mock.
        app.handle_portals_action(PortalsAction::CreatePortal(PortalFormData {
            company_name: "Acme".into(),
            url: "https://example.com/jobs".into(),
            render_mode: "static".into(),
            scraper_config: None,
        }));
        wait_until(&mut app, &ctx, |app| {
            app.portals.rows.len() == 1 && app.portals_task.is_none()
        });

        let portal_id = app.portals.rows[0].id;
        app.handle_portals_action(PortalsAction::FetchOne(portal_id));
        wait_until(&mut app, &ctx, |app| app.portals_task.is_none());

        assert_eq!(source.calls(), vec!["https://example.com/jobs".to_string()]);
        assert_eq!(app.portals.rows[0].health, HealthBadge::Healthy);
        assert!(app.portals.rows[0].last_attempt.is_some());

        app.handle_pipeline_action(PipelineAction::Reload);
        wait_until(&mut app, &ctx, |app| {
            app.pipeline_loaded && app.pipeline_task.is_none()
        });

        assert_eq!(app.pipeline.rows.len(), 1);
        assert_eq!(app.pipeline.rows[0].title, "Engineer");
        assert_eq!(app.pipeline.rows[0].company, "Acme");
        assert_eq!(app.pipeline.rows[0].tier, TierLabel::Html);
        assert_eq!(app.pipeline.rows[0].raw_text, "Build things.");
    }

    /// Boards verified against this code on 2026-10-25, one per supported ATS.
    ///
    /// Order matters only for reporting: the test fetches the first one
    /// individually, then all of them as a batch. Override the whole list
    /// without editing this file:
    ///
    /// ```text
    /// CAREER_OPS_LIVE_PORTALS=https://careers.withwaymo.com/jobs/search \
    ///     cargo test -p career-ops-app --features keychain -- --ignored live_fetch
    /// ```
    ///
    /// A board on a supported ATS exercises Tier 1a. Any other server-rendered
    /// page exercises Tier 1b, which needs JSON-LD `JobPosting` markup or a
    /// portal `scraper_config`. Waymo's search page has neither — its job table
    /// is loaded by JavaScript — so it fails Tier 1b and escalates to Tier 2;
    /// expect `Broken { cause: "other" }` for it on a machine without the
    /// sidecar. That is why it is not in the default list.
    const LIVE_PORTAL_URLS: &[&str] = &[
        "https://jobs.ashbyhq.com/linear",
        "https://jobs.lever.co/spotify",
        "https://boards.greenhouse.io/figma",
        "https://jobs.smartrecruiters.com/Ubisoft2",
    ];

    /// The URLs for this run: `CAREER_OPS_LIVE_PORTALS` (comma-separated) or the
    /// default list.
    fn live_portal_urls() -> Vec<String> {
        match std::env::var("CAREER_OPS_LIVE_PORTALS") {
            Ok(raw) => raw
                .split(',')
                .map(str::trim)
                .filter(|url| !url.is_empty())
                .map(str::to_string)
                .collect(),
            Err(_) => LIVE_PORTAL_URLS.iter().map(|url| url.to_string()).collect(),
        }
    }

    /// The portal name for a live URL: host plus first path segment, so the
    /// per-board assertion output says which board produced which jobs.
    fn live_portal_name(url: &str) -> String {
        let parsed = url::Url::parse(url).expect("valid live URL");
        let host = parsed.host_str().unwrap_or("live");
        match parsed
            .path_segments()
            .and_then(|mut segments| segments.next())
        {
            Some(slug) if !slug.is_empty() => format!("{host}/{slug}"),
            _ => host.to_string(),
        }
    }

    /// The network test the manual script cannot automate in CI: real portal
    /// URLs, the production `HttpFetchSource`, the real router, storage, portal
    /// health, batch progress, and the real Pipeline.
    ///
    /// It creates its own portals in a throwaway vault; nothing is read from or
    /// written to the user's data directory, and the app never seeds example
    /// portals into a real database.
    ///
    /// Ignored by default. Run it with:
    ///
    /// ```text
    /// cargo test -p career-ops-app --features keychain -- --ignored live_fetch
    /// ```
    #[test]
    #[ignore = "hits the network; run with --ignored"]
    fn live_fetch_reaches_the_pipeline() {
        let _guard = KEYCHAIN_TEST_LOCK.lock().expect("keychain test lock");
        career_ops_test_support::install();

        let urls = live_portal_urls();
        assert!(!urls.is_empty(), "CAREER_OPS_LIVE_PORTALS produced no URLs");
        let dir = tempfile::tempdir().expect("tempdir");

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
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(2)
            .enable_all()
            .build()
            .expect("runtime");
        // The production HTTP source, deliberately: this test exists to prove
        // that the real one works, not that the seam does.
        let mut app = CareerOpsApp::new(state, runtime);
        let ctx = egui::Context::default();

        app.handle_lock_action(LockAction::TryPassphrase(TEST_PASSPHRASE.to_string()));
        wait_until(&mut app, &ctx, |app| {
            app.state()
                .is_some_and(|state| state.current() == &Screen::Ready)
        });

        // One at a time: the app runs a single `PortalsTask`, so a create that
        // arrives while the reload is in flight is dropped by design.
        for (index, url) in urls.iter().enumerate() {
            app.handle_portals_action(PortalsAction::CreatePortal(PortalFormData {
                company_name: live_portal_name(url),
                url: url.clone(),
                render_mode: "static".into(),
                scraper_config: None,
            }));
            wait_until(&mut app, &ctx, |app| {
                app.portals.rows.len() == index + 1 && app.portals_task.is_none()
            });
        }

        // Single fetch first: the one-portal path, including Tier 2 escalation
        // when the URL is not a supported ATS. 90s allows a sidecar start.
        let first = app.portals.rows[0].id;
        app.handle_portals_action(PortalsAction::FetchOne(first));
        wait_until_within(&mut app, &ctx, Duration::from_secs(90), |app| {
            app.portals_task.is_none()
        });

        let portal = &app.portals.rows[0];
        assert_eq!(
            portal.health,
            HealthBadge::Healthy,
            "fetching {} did not succeed (health: {:?}, hint: {:?}). \
             Tier 1b needs JSON-LD markup; a JS-only page escalates to Tier 2, \
             which needs vendor/obscura.",
            portal.url,
            portal.health,
            portal.suggested_action
        );

        // Then the batch over every portal. It re-fetches the first one, which
        // also exercises dedup against rows that already exist.
        app.handle_portals_action(PortalsAction::FetchAll);
        wait_until_within(&mut app, &ctx, Duration::from_secs(240), |app| {
            app.portals_task.is_none()
        });

        for row in &app.portals.rows {
            assert_eq!(
                row.health,
                HealthBadge::Healthy,
                "portal {} failed in the batch (hint: {:?})",
                row.url,
                row.suggested_action
            );
        }
        assert!(
            app.portals_error.is_none(),
            "portals error: {:?}",
            app.portals_error
        );
        assert!(
            app.portals_message.is_some(),
            "the batch should report a summary"
        );

        // Storage truth: every portal must have stored jobs. Read the table
        // directly so the per-board count is not capped by the Pipeline page.
        let pool = app.pool().expect("pool");
        let stored = app
            .runtime()
            .block_on(job_repo::list_recent(&pool, 10_000))
            .expect("list stored jobs");

        for portal in &app.portals.rows {
            let jobs = stored
                .iter()
                .filter(|job| job.company == portal.name)
                .count();
            assert!(
                jobs > 0,
                "{} fetched successfully but stored no jobs",
                portal.url
            );
            eprintln!("live fetch: {} -> {jobs} jobs", portal.name);
        }

        // Then the Pipeline page: the most recent `PIPELINE_LIMIT` jobs.
        app.handle_pipeline_action(PipelineAction::Reload);
        wait_until(&mut app, &ctx, |app| {
            app.pipeline_loaded && app.pipeline_task.is_none()
        });

        assert!(!app.pipeline.rows.is_empty(), "the Pipeline is empty");
        for row in &app.pipeline.rows {
            assert!(
                !row.title.trim().is_empty(),
                "a stored job from the live run has an empty title"
            );
        }
        assert_eq!(
            app.pipeline.rows.len(),
            stored.len().min(PIPELINE_LIMIT as usize),
            "the Pipeline page should hold min(stored, {PIPELINE_LIMIT}) jobs"
        );

        eprintln!(
            "live fetch: {} unique jobs stored across {} portals; Pipeline shows {} (first tier: {})",
            stored.len(),
            app.portals.rows.len(),
            app.pipeline.rows.len(),
            app.pipeline.rows[0].tier.label()
        );
    }

    #[test]
    fn new_portal_input_rejects_invalid_urls_and_render_modes() {
        let mut data = PortalFormData {
            company_name: "Acme".into(),
            url: "not a url".into(),
            render_mode: "static".into(),
            scraper_config: None,
        };
        assert!(new_portal_input(&data).is_err());

        data.url = "ftp://example.com/jobs".into();
        assert!(new_portal_input(&data).is_err());

        data.url = "https://example.com/jobs".into();
        data.render_mode = "unknown".into();
        assert!(new_portal_input(&data).is_err());
    }

    #[test]
    fn new_portal_input_preserves_scraper_config() {
        let data = PortalFormData {
            company_name: "Acme".into(),
            url: "https://example.com/jobs".into(),
            render_mode: "static".into(),
            scraper_config: Some("{\"title\":\"h1\"}".into()),
        };

        let input = new_portal_input(&data).expect("valid input");
        assert_eq!(
            input.scraper_config,
            Some(serde_json::json!({"title": "h1"}))
        );
    }
}
