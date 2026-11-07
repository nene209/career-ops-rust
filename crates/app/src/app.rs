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

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex};

use career_ops_core::acquisition::http;
use career_ops_core::acquisition::manual;
use career_ops_core::acquisition::report::PaginationStop;
use career_ops_core::acquisition::router::{
    FetchError, FetchOutcome, FetchSource, HttpFetchSource, PreviewReport,
};
use career_ops_core::backend::{BrowserBackend, ObscuraBrowserBackend};
use career_ops_core::logging::{self, LogHandle, LogLine};
use career_ops_core::model::{ApplicationStatus, CvEntry, FetchTier, JobDescription, Profile};
use career_ops_core::portals::health::suggested_action;
use career_ops_core::portals::service::{
    self, BatchEntry, BatchReport, FetchAllPagesReport, FetchAllProgress, FetchAllStop,
    PortalWithHealth,
};
use career_ops_core::preferences::Preferences;
use career_ops_core::repo::applications as application_repo;
use career_ops_core::repo::cv_entries as cv_entry_repo;
use career_ops_core::repo::job_descriptions as job_repo;
use career_ops_core::repo::portals::{self as portal_repo, NewPortal, RenderMode};
use career_ops_core::repo::profile::{self as profile_repo, ProfileDetails};
use career_ops_core::{CoreError, Result as CoreResult};
use tokio::runtime::Runtime;

use crate::async_bridge::{AsyncTask, TaskPoll};
use crate::navigation::{MainScreen, NavigationState};
use crate::recovery;
use crate::screens;
use crate::screens::cv_profile::{CvProfileAction, CvProfileRenderState, CvProfileView};
use crate::screens::lock::{LockAction, LockView, RESET_PHRASE};
use crate::screens::logs::{LogsAction, LogsRenderState};
use crate::screens::pipeline::{PipelineAction, PipelineRenderState, PipelineView};
use crate::screens::portals::{
    ConfigPreview, ManualSuccess, PortalForm, PortalFormData, PortalModal, PortalsAction,
    PortalsRenderState, PortalsView, PreviewInputs, PreviewSample, StalledWalk,
};
use crate::screens::settings::{SettingsAction, SettingsRenderState};
use crate::state::{AppState, Screen};
use crate::view_models::cv_profile::{CvEntryDraft, CvProfileViewModel};
use crate::view_models::logs::LogsView;
use crate::view_models::pipeline::{PipelineViewModel, StatusLabel};
use crate::view_models::portals::{BatchProgress, PortalsViewModel};
use crate::view_models::settings::SettingsViewModel;

type StateTask = AsyncTask<(AppState, CoreResult<()>)>;

/// A page of recent jobs, paired with the status of the application attached
/// to each job id and the ids that carry user-authored state.
type PipelineLoad = CoreResult<(
    Vec<JobDescription>,
    HashMap<i64, ApplicationStatus>,
    HashSet<i64>,
    Preferences,
)>;

/// The profile singleton and its CV entries, loaded together so the editor can
/// render both from one task.
type CvProfileLoad = CoreResult<(Profile, Vec<CvEntry>)>;

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
    CvProfile(CvProfileAction),
    Settings(SettingsAction),
    Logs(LogsAction),
}

/// One write against the CV entry table.
///
/// Every write reloads the list afterwards, so the task returns the fresh
/// entries rather than the one row it touched.
enum EntryWrite {
    Create(CvEntryDraft),
    Update { id: i64, draft: CvEntryDraft },
    Delete(i64),
    Reorder(Vec<i64>),
}

/// What the finished write did, so the completion message is accurate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EntryWriteOutcome {
    Created,
    Updated,
    Deleted,
    Reordered,
}

struct EntryTask {
    task: AsyncTask<CoreResult<Vec<CvEntry>>>,
    outcome: EntryWriteOutcome,
}

enum PortalsTask {
    List {
        task: AsyncTask<CoreResult<Vec<PortalWithHealth>>>,
        close_modal: bool,
    },
    Fetch(AsyncTask<CoreResult<service::FetchReport>>),
    Batch(AsyncTask<CoreResult<BatchReport>>),
    Manual(AsyncTask<CoreResult<ManualOutcome>>),
    Preview {
        task: AsyncTask<std::result::Result<PreviewReport, FetchError>>,
        /// The form inputs this run describes; the view compares them so an
        /// edited form cannot show a stale report.
        inputs: PreviewInputs,
    },
}

enum PortalsPoll {
    List(CoreResult<Vec<PortalWithHealth>>, bool),
    Fetch(CoreResult<service::FetchReport>),
    Batch(CoreResult<BatchReport>),
    Manual(CoreResult<ManualOutcome>),
    Preview(
        std::result::Result<PreviewReport, FetchError>,
        PreviewInputs,
    ),
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
    /// A **Fetch all pages** walk: the task, the portal it belongs to, and the
    /// shared progress cell the task writes after each chunk.
    fetch_all_task: Option<AsyncTask<CoreResult<FetchAllPagesReport>>>,
    fetch_all_portal: Option<i64>,
    fetch_all_progress: Arc<Mutex<Option<FetchAllProgress>>>,
    fetch_all_cancel: Arc<AtomicBool>,
    /// A walk that stopped at a page it could not read, so the screen can offer
    /// **Skip page N**.
    fetch_all_stalled: Option<StalledWalk>,

    pipeline: PipelineViewModel,
    pipeline_view: PipelineView,
    pipeline_loaded: bool,
    pipeline_error: Option<String>,
    pipeline_message: Option<String>,
    pipeline_task: Option<AsyncTask<PipelineLoad>>,
    pipeline_refresh_task: Option<AsyncTask<CoreResult<JobDescription>>>,
    refreshing_job: Option<i64>,
    pipeline_status_task: Option<AsyncTask<CoreResult<()>>>,
    marking_job: Option<i64>,
    pipeline_delete_task: Option<AsyncTask<CoreResult<()>>>,
    deleting_job: Option<i64>,

    cv_profile: CvProfileViewModel,
    cv_profile_view: CvProfileView,
    cv_profile_loaded: bool,
    cv_profile_error: Option<String>,
    cv_profile_message: Option<String>,
    cv_profile_task: Option<AsyncTask<CvProfileLoad>>,
    cv_profile_save_task: Option<AsyncTask<CoreResult<Profile>>>,
    cv_profile_entry_task: Option<EntryTask>,
    cv_profile_deleting: Option<i64>,

    /// The acquisition preferences form and the value storage holds.
    settings: SettingsViewModel,
    settings_loaded: bool,
    settings_error: Option<String>,
    settings_message: Option<String>,
    settings_task: Option<AsyncTask<CoreResult<Preferences>>>,
    settings_save_task: Option<AsyncTask<CoreResult<Preferences>>>,

    /// The logging seam. `main` installs the file sink; tests get the buffer.
    logs: LogHandle,
    logs_view: LogsView,
    /// The rendered lines, refreshed only when a new one arrives.
    logs_snapshot: Vec<LogLine>,
    logs_snapshot_seq: u64,
    logs_message: Option<String>,
    logs_confirm_delete: bool,
    /// Set once the wake callback is installed; it needs an egui context.
    logs_wake_installed: bool,
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
        // `main` has already installed the file sink in production; in tests this
        // lazily installs the in-process buffer only.
        let logs = logging::handle();

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
            fetch_all_task: None,
            fetch_all_portal: None,
            fetch_all_progress: Arc::new(Mutex::new(None)),
            fetch_all_cancel: Arc::new(AtomicBool::new(false)),
            fetch_all_stalled: None,
            pipeline: PipelineViewModel::default(),
            pipeline_view: PipelineView::default(),
            pipeline_loaded: false,
            pipeline_error: None,
            pipeline_message: None,
            pipeline_task: None,
            pipeline_refresh_task: None,
            refreshing_job: None,
            pipeline_status_task: None,
            marking_job: None,
            pipeline_delete_task: None,
            deleting_job: None,
            cv_profile: CvProfileViewModel::default(),
            cv_profile_view: CvProfileView::default(),
            cv_profile_loaded: false,
            cv_profile_error: None,
            cv_profile_message: None,
            cv_profile_task: None,
            cv_profile_save_task: None,
            cv_profile_entry_task: None,
            cv_profile_deleting: None,
            settings: SettingsViewModel::default(),
            settings_loaded: false,
            settings_error: None,
            settings_message: None,
            settings_task: None,
            settings_save_task: None,
            logs,
            logs_view: LogsView::default(),
            logs_snapshot: Vec::new(),
            logs_snapshot_seq: 0,
            logs_message: None,
            logs_confirm_delete: false,
            logs_wake_installed: false,
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
                // The chooser is the first step: the embedded registry is only
                // useful if a user can see it without already knowing a URL.
                self.portals_view.open_add();
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
            PortalsAction::FetchOne(id) => self.start_portal_fetch(id, false),
            PortalsAction::FetchMore(id) => self.start_portal_fetch(id, true),
            PortalsAction::FetchAllPages(id) => self.start_fetch_all_pages(id, None),
            PortalsAction::SkipPage { portal_id, page } => {
                // Skip means "read the next page": the cursor lands on the page
                // that failed, so the resumed walk starts one page later.
                self.start_fetch_all_pages(portal_id, Some(page as i64));
            }
            PortalsAction::CancelFetchAllPages => {
                // A walk finishes the chunk it is in; the flag is checked before
                // the next one, so Stop means "within one chunk", not "now".
                self.fetch_all_cancel.store(true, Ordering::Relaxed);
                self.portals_message = Some("Stopping after the current page...".into());
            }
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
            PortalsAction::TestConfig(data) => self.start_config_preview(data),
            PortalsAction::ShowInPipeline => {
                self.portals_view.modal = None;
                self.navigation.navigate_to(MainScreen::Pipeline);
            }
            PortalsAction::ShowLogs => {
                // The drawer would sit on top of the screen it duplicates.
                self.logs_view.drawer_open = false;
                self.navigation.navigate_to(MainScreen::Logs);
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
            PipelineAction::SetSearch(query) => self.pipeline.search = query,
            PipelineAction::RefreshJob(job_id) => self.start_job_refresh(job_id),
            PipelineAction::MarkNotAMatch(job_id) => self.start_not_a_match(job_id),
            PipelineAction::DeleteJob(job_id) => self.start_job_delete(job_id),
        }
    }

    /// Refresh the rendered lines only when a new one arrived.
    ///
    /// egui is immediate-mode, so cloning a 2,000-line ring every frame would be
    /// pure waste; the snapshot is keyed on the sequence number instead.
    fn refresh_logs_snapshot(&mut self) {
        let latest = self.logs.latest_seq();
        if latest != self.logs_snapshot_seq {
            self.logs_snapshot = self.logs.snapshot();
            self.logs_snapshot_seq = latest;
        }
    }

    fn reset_logs_snapshot(&mut self) {
        self.logs_snapshot.clear();
        self.logs_snapshot_seq = self.logs.latest_seq();
    }

    /// Ask for a repaint when a line arrives.
    ///
    /// Without this, a line emitted by a background task would sit in the buffer
    /// until the next incidental frame, which reads as a broken drawer.
    fn install_logs_wake(&mut self, ctx: &egui::Context) {
        if self.logs_wake_installed {
            return;
        }
        let ctx = ctx.clone();
        self.logs.set_wake(Arc::new(move || ctx.request_repaint()));
        self.logs_wake_installed = true;
    }

    /// Execute an intent emitted by the Logs screen or the drawer.
    pub fn handle_logs_action(&mut self, action: LogsAction) {
        match action {
            LogsAction::OpenFolder => self.open_log_folder(),
            LogsAction::RequestDelete => self.logs_confirm_delete = true,
            LogsAction::CancelDelete => self.logs_confirm_delete = false,
            LogsAction::ConfirmDelete => self.delete_logs(),
            LogsAction::OpenScreen => {
                self.logs_view.drawer_open = false;
                self.navigation.navigate_to(MainScreen::Logs);
            }
            LogsAction::CloseDrawer => self.logs_view.drawer_open = false,
        }
    }

    fn open_log_folder(&mut self) {
        let Some(dir) = self
            .logs
            .path()
            .and_then(|path| path.parent().map(|dir| dir.to_path_buf()))
        else {
            self.logs_message =
                Some("No log file this session; these lines live in memory only.".into());
            return;
        };

        let opener = if cfg!(target_os = "macos") {
            "open"
        } else if cfg!(target_os = "windows") {
            "explorer"
        } else {
            "xdg-open"
        };
        if let Err(error) = std::process::Command::new(opener).arg(dir).spawn() {
            self.logs_message = Some(format!("Could not open the log folder: {error}"));
        }
    }

    /// Delete the log file and its archives, then clear the buffer.
    ///
    /// The writer keeps its open handle, so the file is unlinked rather than
    /// emptied and a fresh one appears on the next launch. The message says so
    /// instead of implying that file logging stopped for good.
    fn delete_logs(&mut self) {
        self.logs_confirm_delete = false;
        let Some(dir) = self
            .logs
            .path()
            .and_then(|path| path.parent().map(|dir| dir.to_path_buf()))
        else {
            self.logs.clear();
            self.reset_logs_snapshot();
            self.logs_message =
                Some("No log file this session; the in-memory lines were cleared.".into());
            return;
        };

        match logging::delete_logs(&dir) {
            Ok(0) => self.logs_message = Some("There was no log file to delete.".into()),
            Ok(removed) => {
                self.logs.clear();
                self.reset_logs_snapshot();
                self.logs_message = Some(format!(
                    "Deleted {removed} log file(s). A new one starts the next time the app launches."
                ));
            }
            Err(error) => self.logs_message = Some(format!("Could not delete the logs: {error}")),
        }
    }

    /// Execute an intent emitted by the Settings screen.
    ///
    /// Public so tests can drive the same path the UI uses without rendering.
    pub fn handle_settings_action(&mut self, action: SettingsAction) {
        match action {
            SettingsAction::Reload => {
                self.settings_loaded = false;
                self.start_settings_load();
            }
            SettingsAction::Save(preferences) => self.start_preferences_save(preferences),
        }
    }

    /// Execute an intent emitted by the CV / Profile screen.
    ///
    /// Public so tests can drive the same path the UI uses without rendering.
    pub fn handle_cv_profile_action(&mut self, action: CvProfileAction) {
        match action {
            CvProfileAction::Reload => {
                self.cv_profile_view = CvProfileView::default();
                self.cv_profile_loaded = false;
                self.start_cv_profile_load();
            }
            CvProfileAction::SaveProfile(details) => self.start_profile_save(details),
            CvProfileAction::CreateEntry(draft) => {
                self.start_entry_write(EntryWrite::Create(draft));
            }
            CvProfileAction::UpdateEntry { id, draft } => {
                self.start_entry_write(EntryWrite::Update { id, draft });
            }
            CvProfileAction::DeleteEntry(id) => self.start_entry_write(EntryWrite::Delete(id)),
            CvProfileAction::MoveEntry { id, direction } => {
                // The move is a pure reordering of the loaded rows. It returns
                // the order to persist, or `None` when the row is already at
                // the edge of its group and nothing should be written.
                if let Some(order) = self.cv_profile.move_entry(id, direction) {
                    self.start_entry_write(EntryWrite::Reorder(order));
                }
            }
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
        self.fetch_all_task = None;
        self.fetch_all_portal = None;
        self.fetch_all_progress = Arc::new(Mutex::new(None));
        self.fetch_all_cancel = Arc::new(AtomicBool::new(false));
        self.fetch_all_stalled = None;

        self.pipeline = PipelineViewModel::default();
        self.pipeline_view = PipelineView::default();
        self.pipeline_loaded = false;
        self.pipeline_error = None;
        self.pipeline_message = None;
        self.pipeline_task = None;
        self.pipeline_refresh_task = None;
        self.refreshing_job = None;
        self.pipeline_status_task = None;
        self.marking_job = None;
        self.pipeline_delete_task = None;
        self.deleting_job = None;

        self.cv_profile = CvProfileViewModel::default();
        self.cv_profile_view = CvProfileView::default();
        self.cv_profile_loaded = false;
        self.cv_profile_error = None;
        self.cv_profile_message = None;
        self.cv_profile_task = None;
        self.cv_profile_save_task = None;
        self.cv_profile_entry_task = None;
        self.cv_profile_deleting = None;

        self.settings = SettingsViewModel::default();
        self.settings_loaded = false;
        self.settings_error = None;
        self.settings_message = None;
        self.settings_task = None;
        self.settings_save_task = None;
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
        // Any new fetch makes the previous stall stale.
        self.fetch_all_stalled = None;
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

    fn start_portal_fetch(&mut self, id: i64, resume: bool) {
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
            if resume {
                service::fetch_more(&pool, id, source.as_ref(), Some(browser.as_ref())).await
            } else {
                service::fetch_one(&pool, id, source.as_ref(), Some(browser.as_ref())).await
            }
        });

        self.fetching_portal = Some(id);
        self.portals_task = Some(PortalsTask::Fetch(task));
    }

    /// Walk a portal's list until it ends, the budget, or Stop.
    ///
    /// Unlike `start_portal_fetch`, this is not a `PortalsTask`: it reports
    /// cumulative progress instead of a single outcome, so it keeps its own
    /// shared cell that the UI reads each frame.
    ///
    /// `from_cursor` moves the continuation cursor before the walk starts. It is
    /// how **Skip page N** turns a page the site refuses into a decision: the
    /// cursor lands on N, so the walk resumes at N+1.
    fn start_fetch_all_pages(&mut self, portal_id: i64, from_cursor: Option<i64>) {
        if self.fetch_all_task.is_some() || !self.begin_portals_task() {
            return;
        }
        self.fetch_all_stalled = None;

        let Some(pool) = self.pool() else {
            self.portals_error = Some("The database is not open.".into());
            return;
        };

        let source = Arc::clone(&self.fetch_source);
        let browser = Arc::clone(&self.browser);
        let cancel = Arc::new(AtomicBool::new(false));
        // Start at zero rather than `None` so the row shows the budget while the
        // first chunk is in flight, instead of only "Starting...".
        let progress = Arc::new(Mutex::new(Some(FetchAllProgress {
            pages_fetched: 0,
            jobs_stored: 0,
            budget: service::FETCH_ALL_PAGE_BUDGET,
        })));

        let task_cancel = Arc::clone(&cancel);
        let task_progress = Arc::clone(&progress);
        let task = self.spawn(async move {
            if let Some(cursor) = from_cursor {
                portal_repo::set_last_page_fetched(&pool, portal_id, cursor).await?;
            }
            service::fetch_all_pages(
                &pool,
                portal_id,
                source.as_ref(),
                Some(browser.as_ref()),
                &task_cancel,
                |chunk| {
                    if let Ok(mut slot) = task_progress.lock() {
                        *slot = Some(*chunk);
                    }
                },
            )
            .await
        });

        self.fetch_all_cancel = cancel;
        self.fetch_all_progress = progress;
        self.fetch_all_portal = Some(portal_id);
        self.fetch_all_task = Some(task);
    }

    fn poll_fetch_all_pages(&mut self, ctx: &egui::Context) {
        let poll = match self.fetch_all_task.as_mut() {
            Some(task) => task.try_recv(),
            None => return,
        };

        match poll {
            TaskPoll::Ready(result) => {
                self.fetch_all_task = None;
                self.fetch_all_portal = None;
                self.fetch_all_progress = Arc::new(Mutex::new(None));

                match result {
                    Ok(report) => {
                        self.portals.apply_portal(&report.portal);
                        self.portals_message = Some(fetch_all_summary(&report));
                        self.portals_error = match &report.stopped_reason {
                            FetchAllStop::Failed(error) => {
                                Some(format!("The walk stopped: {}", error.detail))
                            }
                            _ => None,
                        };
                        // A page the site refused turns into a decision the user
                        // can act on (skip it) instead of a loop of failures.
                        self.fetch_all_stalled = match report.stopped_reason {
                            FetchAllStop::Stopped {
                                unread_page: Some(page),
                            } => Some(StalledWalk {
                                portal_id: report.portal.id,
                                page,
                            }),
                            _ => None,
                        };
                    }
                    Err(error) => {
                        self.portals_message = None;
                        self.portals_error = Some(error.to_string());
                    }
                }
                ctx.request_repaint();
            }
            TaskPoll::Pending => {
                // The progress cell changes without the task finishing.
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
            }
            TaskPoll::Cancelled => {
                self.fetch_all_task = None;
                self.fetch_all_portal = None;
                self.fetch_all_progress = Arc::new(Mutex::new(None));
                self.portals_error = Some("The fetch was interrupted.".into());
                ctx.request_repaint();
            }
        }
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

    fn start_config_preview(&mut self, data: PortalFormData) {
        if !self.begin_portals_task() {
            return;
        }

        let inputs = PreviewInputs {
            url: data.url.trim().to_string(),
            render_mode: data.render_mode.clone(),
            scraper_config: data.scraper_config.clone().unwrap_or_default(),
        };

        if let Some(form) = self.portals_view.form_mut() {
            form.preview.busy = true;
            form.preview.result = None;
            form.preview.error = None;
            form.preview.for_inputs = None;
        }

        let source = Arc::clone(&self.fetch_source);
        let browser = Arc::clone(&self.browser);
        let company_name = data.company_name.clone();
        let task = self.spawn(async move {
            service::preview_config(
                &data.url,
                &company_name,
                &data.render_mode,
                data.scraper_config.as_deref(),
                source.as_ref(),
                Some(browser.as_ref()),
            )
            .await
        });

        self.portals_task = Some(PortalsTask::Preview { task, inputs });
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
            PortalsTask::Preview { task, inputs } => match task.try_recv() {
                TaskPoll::Ready(result) => {
                    finished = true;
                    Some(PortalsPoll::Preview(result, inputs.clone()))
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
                        match &report.outcome {
                            Ok(outcome) => {
                                self.portals_message = Some(fetch_summary(outcome));
                                self.portals_error = None;
                            }
                            // A failed fetch used to change only the health badge,
                            // so the classified reason was invisible. The
                            // suggested action is the same text the drill-down
                            // shows, so the two never disagree.
                            Err(error) => {
                                self.portals_message = None;
                                self.portals_error = Some(classified_error_message(error));
                            }
                        }
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
            PortalsPoll::Preview(result, inputs) => {
                if let Some(form) = self.portals_view.form_mut() {
                    form.preview.busy = false;
                    form.preview.for_inputs = Some(inputs);
                    match result {
                        Ok(report) => {
                            form.preview.error = None;
                            form.preview.result = Some(config_preview(report));
                        }
                        Err(error) => {
                            form.preview.result = None;
                            form.preview.error = Some(classified_error_message(&error));
                        }
                    }
                }
            }
            PortalsPoll::Cancelled => {
                self.fetching_portal = None;
                if let Some(form) = self.portals_view.form_mut() {
                    form.preview.busy = false;
                }
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
            let touched = job_repo::user_touched_ids(&pool).await?;
            // Loaded with the rows so the badges and the header summary agree:
            // a separate load could badge a page with the previous preferences.
            let preferences = profile_repo::preferences(&pool).await?;
            Ok((jobs, statuses, touched, preferences))
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
                    Ok((jobs, statuses, touched, preferences)) => {
                        self.pipeline
                            .replace_jobs(jobs, &statuses, &touched, preferences);
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

    /// Re-fetch one posting's description. The row is updated in place; the
    /// pipeline is not reloaded, because nothing about ordering changed.
    fn start_job_refresh(&mut self, job_id: i64) {
        if self.refreshing_job.is_some() {
            return;
        }

        let Some(pool) = self.pool() else {
            self.pipeline_error = Some("The database is not open.".into());
            return;
        };

        let source = Arc::clone(&self.fetch_source);
        let task =
            self.spawn(async move { manual::refresh_job(&pool, job_id, source.as_ref()).await });

        self.pipeline_error = None;
        self.pipeline_message = None;
        self.refreshing_job = Some(job_id);
        self.pipeline_refresh_task = Some(task);
    }

    fn poll_pipeline_refresh(&mut self, ctx: &egui::Context) {
        let poll = match self.pipeline_refresh_task.as_mut() {
            Some(task) => task.try_recv(),
            None => return,
        };

        match poll {
            TaskPoll::Pending => {
                ctx.request_repaint_after(std::time::Duration::from_millis(50));
            }
            TaskPoll::Ready(result) => {
                self.pipeline_refresh_task = None;
                self.refreshing_job = None;
                match result {
                    Ok(job) => {
                        if let Some(row) =
                            self.pipeline.rows.iter_mut().find(|row| row.id == job.id)
                        {
                            row.raw_text = job.raw_text.clone();
                        }
                        self.pipeline_message = Some(format!(
                            "Description refreshed: {} bytes.",
                            job.raw_text.len()
                        ));
                        self.pipeline_error = None;
                    }
                    Err(error) => {
                        self.pipeline_message = None;
                        self.pipeline_error =
                            Some(format!("Could not refresh the description: {error}"));
                    }
                }
                ctx.request_repaint();
            }
            TaskPoll::Cancelled => {
                self.pipeline_refresh_task = None;
                self.refreshing_job = None;
                self.pipeline_error = Some("The refresh was interrupted.".into());
                ctx.request_repaint();
            }
        }
    }

    /// Mark a job's application as not worth pursuing. Creates the application
    /// row when the job has none, which is the first production writer.
    fn start_not_a_match(&mut self, job_id: i64) {
        if self.marking_job.is_some() {
            return;
        }

        let Some(pool) = self.pool() else {
            self.pipeline_error = Some("The database is not open.".into());
            return;
        };

        let task = self.spawn(async move {
            application_repo::mark_not_a_match(&pool, job_id)
                .await
                .map(|_| ())
        });

        self.pipeline_error = None;
        self.pipeline_message = None;
        self.marking_job = Some(job_id);
        self.pipeline_status_task = Some(task);
    }

    fn poll_pipeline_status(&mut self, ctx: &egui::Context) {
        let poll = match self.pipeline_status_task.as_mut() {
            Some(task) => task.try_recv(),
            None => return,
        };

        match poll {
            TaskPoll::Pending => {
                ctx.request_repaint_after(std::time::Duration::from_millis(50));
            }
            TaskPoll::Ready(result) => {
                self.pipeline_status_task = None;
                let job_id = self.marking_job.take();
                match result {
                    Ok(()) => {
                        if let Some(id) = job_id {
                            if let Some(row) =
                                self.pipeline.rows.iter_mut().find(|row| row.id == id)
                            {
                                row.status = Some(StatusLabel::NotAMatch);
                                // The status itself is user state: deleting
                                // this row now needs the typed gate.
                                row.has_user_state = true;
                            }
                        }
                        self.pipeline_message = Some("Marked as not a match.".into());
                        self.pipeline_error = None;
                    }
                    Err(error) => {
                        self.pipeline_message = None;
                        self.pipeline_error = Some(format!("Could not update the status: {error}"));
                    }
                }
                ctx.request_repaint();
            }
            TaskPoll::Cancelled => {
                self.pipeline_status_task = None;
                self.marking_job = None;
                self.pipeline_error = Some("The status update was interrupted.".into());
                ctx.request_repaint();
            }
        }
    }

    /// Delete a job description. Its application, evaluation reports, and cover
    /// letters cascade, which is why a touched row is gated.
    fn start_job_delete(&mut self, job_id: i64) {
        if self.deleting_job.is_some() {
            return;
        }

        let Some(pool) = self.pool() else {
            self.pipeline_error = Some("The database is not open.".into());
            return;
        };

        let task = self.spawn(async move { job_repo::delete(&pool, job_id).await });

        self.pipeline_error = None;
        self.pipeline_message = None;
        self.deleting_job = Some(job_id);
        self.pipeline_delete_task = Some(task);
    }

    fn poll_pipeline_delete(&mut self, ctx: &egui::Context) {
        let poll = match self.pipeline_delete_task.as_mut() {
            Some(task) => task.try_recv(),
            None => return,
        };

        match poll {
            TaskPoll::Pending => {
                ctx.request_repaint_after(std::time::Duration::from_millis(50));
            }
            TaskPoll::Ready(result) => {
                self.pipeline_delete_task = None;
                let job_id = self.deleting_job.take();
                match result {
                    Ok(()) => {
                        if let Some(id) = job_id {
                            self.pipeline.rows.retain(|row| row.id != id);
                            if self.pipeline_view.selected == Some(id) {
                                self.pipeline_view.selected = None;
                            }
                        }
                        self.pipeline_view.delete = None;
                        self.pipeline_message = Some("Job deleted.".into());
                        self.pipeline_error = None;
                    }
                    Err(error) => {
                        // The gate stays open so the user can retry or cancel.
                        self.pipeline_message = None;
                        self.pipeline_error = Some(format!("Could not delete the job: {error}"));
                    }
                }
                ctx.request_repaint();
            }
            TaskPoll::Cancelled => {
                self.pipeline_delete_task = None;
                self.deleting_job = None;
                self.pipeline_error = Some("The delete was interrupted.".into());
                ctx.request_repaint();
            }
        }
    }

    // ------------------------------------------------------------------
    // End Pipeline
    // ------------------------------------------------------------------

    // ------------------------------------------------------------------
    // CV / Profile
    // ------------------------------------------------------------------

    fn start_cv_profile_load(&mut self) {
        if self.cv_profile_task.is_some() {
            return;
        }

        let Some(pool) = self.pool() else {
            self.cv_profile_error = Some("The database is not open.".into());
            return;
        };

        self.cv_profile_error = None;
        let task = self.spawn(async move {
            // This screen is the first thing that touches the profile, so it
            // creates the singleton instead of leaving the row to Phase 3.
            let profile = profile_repo::ensure_singleton(&pool).await?;
            let entries = cv_entry_repo::list_for_profile(&pool, profile.id).await?;
            Ok((profile, entries))
        });
        self.cv_profile_task = Some(task);
    }

    fn poll_cv_profile_task(&mut self, ctx: &egui::Context) {
        let poll = match self.cv_profile_task.as_mut() {
            Some(task) => task.try_recv(),
            None => return,
        };

        match poll {
            TaskPoll::Ready(result) => {
                self.cv_profile_task = None;
                match result {
                    Ok((profile, entries)) => {
                        self.cv_profile = CvProfileViewModel::from_profile(profile, entries);
                        self.cv_profile_loaded = true;
                        self.cv_profile_error = None;
                    }
                    Err(error) => self.cv_profile_error = Some(error.to_string()),
                }
                ctx.request_repaint();
            }
            TaskPoll::Pending => {
                ctx.request_repaint_after(std::time::Duration::from_millis(50));
            }
            TaskPoll::Cancelled => {
                self.cv_profile_task = None;
                self.cv_profile_error = Some("The operation was interrupted.".into());
                ctx.request_repaint();
            }
        }
    }

    fn start_profile_save(&mut self, details: ProfileDetails) {
        if self.cv_profile_save_task.is_some() {
            return;
        }

        let Some(pool) = self.pool() else {
            self.cv_profile_error = Some("The database is not open.".into());
            return;
        };

        self.cv_profile_error = None;
        self.cv_profile_message = None;
        let task = self.spawn(async move { profile_repo::update_details(&pool, &details).await });
        self.cv_profile_save_task = Some(task);
    }

    fn poll_cv_profile_save(&mut self, ctx: &egui::Context) {
        let poll = match self.cv_profile_save_task.as_mut() {
            Some(task) => task.try_recv(),
            None => return,
        };

        match poll {
            TaskPoll::Ready(result) => {
                self.cv_profile_save_task = None;
                match result {
                    Ok(profile) => {
                        // The stored row is the trimmed one, so adopting it is
                        // what clears the dirty state.
                        self.cv_profile.apply_saved_profile(profile);
                        self.cv_profile_message = Some("Profile saved.".into());
                        self.cv_profile_error = None;
                    }
                    Err(error) => {
                        // The form keeps the user's text; the error says why it
                        // was not stored.
                        self.cv_profile_message = None;
                        self.cv_profile_error =
                            Some(format!("Could not save the profile: {error}"));
                    }
                }
                ctx.request_repaint();
            }
            TaskPoll::Pending => {
                ctx.request_repaint_after(std::time::Duration::from_millis(50));
            }
            TaskPoll::Cancelled => {
                self.cv_profile_save_task = None;
                self.cv_profile_error = Some("The save was interrupted.".into());
                ctx.request_repaint();
            }
        }
    }

    fn start_settings_load(&mut self) {
        if self.settings_task.is_some() {
            return;
        }

        let Some(pool) = self.pool() else {
            self.settings_error = Some("The database is not open.".into());
            return;
        };

        self.settings_error = None;
        let task = self.spawn(async move { profile_repo::preferences(&pool).await });
        self.settings_task = Some(task);
    }

    fn poll_settings_task(&mut self, ctx: &egui::Context) {
        let poll = match self.settings_task.as_mut() {
            Some(task) => task.try_recv(),
            None => return,
        };

        match poll {
            TaskPoll::Ready(result) => {
                self.settings_task = None;
                match result {
                    Ok(preferences) => {
                        self.settings = SettingsViewModel::from_preferences(preferences);
                        self.settings_loaded = true;
                        self.settings_error = None;
                    }
                    Err(error) => self.settings_error = Some(error.to_string()),
                }
                ctx.request_repaint();
            }
            TaskPoll::Pending => {
                ctx.request_repaint_after(std::time::Duration::from_millis(50));
            }
            TaskPoll::Cancelled => {
                self.settings_task = None;
                self.settings_error = Some("The operation was interrupted.".into());
                ctx.request_repaint();
            }
        }
    }

    fn start_preferences_save(&mut self, preferences: Preferences) {
        if self.settings_save_task.is_some() {
            return;
        }

        let Some(pool) = self.pool() else {
            self.settings_error = Some("The database is not open.".into());
            return;
        };

        self.settings_error = None;
        self.settings_message = None;
        let task = self.spawn(async move {
            // Round-trip through storage: the form adopts the value the column
            // now holds, so a serialization surprise shows up as a dirty form
            // rather than as a silent mismatch.
            let profile = profile_repo::update_preferences(&pool, &preferences).await?;
            Ok(Preferences::from_json(profile.preferences.as_deref()))
        });
        self.settings_save_task = Some(task);
    }

    fn poll_preferences_save(&mut self, ctx: &egui::Context) {
        let poll = match self.settings_save_task.as_mut() {
            Some(task) => task.try_recv(),
            None => return,
        };

        match poll {
            TaskPoll::Ready(result) => {
                self.settings_save_task = None;
                match result {
                    Ok(preferences) => {
                        // The Pipeline badges from these, so re-badge what is
                        // already loaded instead of waiting for a reload.
                        self.pipeline.set_preferences(preferences.clone());
                        self.settings.mark_saved(preferences);
                        self.settings_message = Some("Preferences saved.".into());
                        self.settings_error = None;
                    }
                    Err(error) => {
                        self.settings_message = None;
                        self.settings_error =
                            Some(format!("Could not save the preferences: {error}"));
                    }
                }
                ctx.request_repaint();
            }
            TaskPoll::Pending => {
                ctx.request_repaint_after(std::time::Duration::from_millis(50));
            }
            TaskPoll::Cancelled => {
                self.settings_save_task = None;
                self.settings_error = Some("The save was interrupted.".into());
                ctx.request_repaint();
            }
        }
    }

    fn start_entry_write(&mut self, write: EntryWrite) {
        if self.cv_profile_entry_task.is_some() {
            return;
        }

        let Some(pool) = self.pool() else {
            self.cv_profile_error = Some("The database is not open.".into());
            return;
        };

        let profile_id = self.cv_profile.profile.id;
        let outcome = match &write {
            EntryWrite::Create(_) => EntryWriteOutcome::Created,
            EntryWrite::Update { .. } => EntryWriteOutcome::Updated,
            EntryWrite::Delete(_) => EntryWriteOutcome::Deleted,
            EntryWrite::Reorder(_) => EntryWriteOutcome::Reordered,
        };
        self.cv_profile_error = None;
        self.cv_profile_message = None;
        self.cv_profile_deleting = match &write {
            EntryWrite::Delete(id) => Some(*id),
            _ => None,
        };

        let task = self.spawn(async move {
            match write {
                EntryWrite::Create(draft) => {
                    cv_entry_repo::create(&pool, draft.to_new_entry(profile_id, None)).await?;
                }
                EntryWrite::Update { id, draft } => {
                    // The editor does not own `metadata`; carry whatever the
                    // stored row has so an edit never clears it.
                    let metadata = cv_entry_repo::get(&pool, id)
                        .await?
                        .and_then(|entry| entry.metadata);
                    let input = draft.to_new_entry(profile_id, metadata.as_deref());
                    cv_entry_repo::update(&pool, id, input).await?;
                }
                EntryWrite::Delete(id) => cv_entry_repo::delete(&pool, id).await?,
                EntryWrite::Reorder(order) => {
                    cv_entry_repo::reorder(&pool, profile_id, &order).await?;
                }
            }
            // Reload rather than patch the list: `reorder` renumbers the whole
            // table, and the reloaded rows are the canonical order.
            cv_entry_repo::list_for_profile(&pool, profile_id).await
        });

        self.cv_profile_entry_task = Some(EntryTask { task, outcome });
    }

    fn poll_cv_profile_entry(&mut self, ctx: &egui::Context) {
        let poll = match self.cv_profile_entry_task.as_mut() {
            Some(entry_task) => entry_task.task.try_recv(),
            None => return,
        };

        match poll {
            TaskPoll::Ready(result) => {
                let Some(entry_task) = self.cv_profile_entry_task.take() else {
                    return;
                };
                self.cv_profile_deleting = None;
                match result {
                    Ok(entries) => {
                        self.cv_profile.replace_entries(entries);
                        match entry_task.outcome {
                            EntryWriteOutcome::Created | EntryWriteOutcome::Updated => {
                                self.cv_profile_view.adding = None;
                                self.cv_profile_view.editing = None;
                                self.cv_profile_message = Some("Entry saved.".into());
                            }
                            EntryWriteOutcome::Deleted => {
                                self.cv_profile_view.deleting = None;
                                self.cv_profile_message = Some("Entry deleted.".into());
                            }
                            EntryWriteOutcome::Reordered => {
                                self.cv_profile_message = Some("Order updated.".into());
                            }
                        }
                        self.cv_profile_error = None;
                    }
                    Err(error) => {
                        // A failed write keeps the form and the confirmation on
                        // screen so the user can retry or cancel.
                        self.cv_profile_message = None;
                        self.cv_profile_error =
                            Some(format!("Could not update the CV entry: {error}"));
                    }
                }
                ctx.request_repaint();
            }
            TaskPoll::Pending => {
                ctx.request_repaint_after(std::time::Duration::from_millis(50));
            }
            TaskPoll::Cancelled => {
                self.cv_profile_entry_task = None;
                self.cv_profile_deleting = None;
                self.cv_profile_error = Some("The operation was interrupted.".into());
                ctx.request_repaint();
            }
        }
    }

    // ------------------------------------------------------------------
    // End CV / Profile
    // ------------------------------------------------------------------
}

impl Drop for CareerOpsApp {
    fn drop(&mut self) {
        // Abort any in-flight work before asking the sidecar to exit.
        self.portals_task = None;
        self.pipeline_task = None;
        self.pipeline_refresh_task = None;
        self.pipeline_status_task = None;
        self.pipeline_delete_task = None;
        self.cv_profile_task = None;
        self.cv_profile_save_task = None;
        self.cv_profile_entry_task = None;
        self.fetch_all_task = None;
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

/// The one-line result shown after a **Fetch all pages** walk.
///
/// Every stop reason names itself, because "Fetched 240 jobs" without saying
/// why it stopped reads like the board ended when it did not.
fn fetch_all_summary(report: &FetchAllPagesReport) -> String {
    let jobs = count_with_noun(report.jobs_stored, "job");
    let pages = count_with_noun(report.pages_fetched, "page");

    match &report.stopped_reason {
        FetchAllStop::EndOfList => format!("Fetched {jobs} across {pages}; end of list."),
        FetchAllStop::BudgetExhausted => format!(
            "Fetched {jobs} across {pages}. There is more; Continue or Fetch all pages again."
        ),
        FetchAllStop::Cancelled => format!("Stopped after {jobs} across {pages}."),
        FetchAllStop::NoProgress => {
            format!("Fetched {jobs}; this board has no page to continue from.")
        }
        FetchAllStop::Stopped {
            unread_page: Some(page),
        } => format!(
            "Fetched {jobs} across {pages}; page {page} could not be read. Continue retries it."
        ),
        FetchAllStop::Stopped { unread_page: None } => format!(
            "Fetched {jobs} across {pages}; the board stopped advancing. Continue retries the \
             last range."
        ),
        FetchAllStop::Failed(error) => {
            format!("Stopped after {jobs} across {pages}: {}", error.detail)
        }
    }
}

/// The one-line result shown after a single-portal fetch.
///
/// Description counts appear only when the portal has a `detail` config, so a
/// plain board keeps the shorter message. A selector fetch that stopped early
/// names the page it could not read: a partial list must not look complete.
fn fetch_summary(outcome: &FetchOutcome) -> String {
    let jobs = count_with_noun(outcome.jobs_fetched, "job");
    let first = outcome.first_page.max(1);
    let mut summary = match outcome.pages_fetched {
        Some(pages) if pages > 1 && first > 1 => {
            format!("Fetched {jobs} from pages {first}-{}", first + pages - 1)
        }
        Some(pages) if pages > 1 => format!("Fetched {jobs} from {pages} pages"),
        Some(1) if first > 1 => format!("Fetched {jobs} from page {first}"),
        _ => format!("Fetched {jobs}"),
    };

    if let Some(note) = pagination_stop_note(outcome) {
        summary.push_str(&format!(" ({note})"));
    }

    if outcome.details_attempted == 0 {
        summary.push('.');
    } else {
        summary.push_str(&format!(
            "; {} of {}.",
            outcome.details_enriched,
            count_with_noun(outcome.details_attempted, "description")
        ));
    }

    summary
}

/// The parenthetical note for a selector fetch that stopped before the list
/// ended. Expected endings (end of list, configured cap) say nothing.
fn pagination_stop_note(outcome: &FetchOutcome) -> Option<String> {
    let pages = outcome.pages_fetched?;
    let next = outcome.first_page.max(1) + pages;
    match outcome.pagination_stopped? {
        PaginationStop::FetchFailed | PaginationStop::ExtractionFailed => {
            Some(format!("page {next} could not be read"))
        }
        PaginationStop::NoNewJobs => Some(format!("page {next} returned no new jobs")),
        PaginationStop::RepeatedUrl => Some("pagination pointed back at the same page".into()),
        PaginationStop::EndOfList | PaginationStop::LimitReached => None,
    }
}

fn count_with_noun(count: usize, noun: &str) -> String {
    if count == 1 {
        format!("{count} {noun}")
    } else {
        format!("{count} {noun}s")
    }
}

/// Convert a core preview report into the form's render shape.
fn config_preview(report: PreviewReport) -> ConfigPreview {
    ConfigPreview {
        jobs_found: report.jobs_found,
        pages_planned: report.pages_planned,
        tier: report.tier.as_str().to_string(),
        sample: report
            .sample
            .into_iter()
            .map(|job| PreviewSample {
                title: job.title,
                url: job.source_url.to_string(),
            })
            .collect(),
        first_description_bytes: report.first_description_bytes,
        details_attempted: report.details_attempted,
    }
}

/// A failed fetch as the user reads it.
///
/// The detail says what went wrong, then the same suggested action the
/// portal's health drill-down shows, so the two never disagree. Used by both
/// the form's preview error line and the Portals header after a failed fetch.
fn classified_error_message(error: &FetchError) -> String {
    format!("{} {}", error.detail, suggested_action(error.cause))
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
                self.poll_fetch_all_pages(ui.ctx());
                self.poll_pipeline_task(ui.ctx());
                self.poll_pipeline_refresh(ui.ctx());
                self.poll_pipeline_status(ui.ctx());
                self.poll_pipeline_delete(ui.ctx());
                self.poll_cv_profile_task(ui.ctx());
                self.poll_cv_profile_save(ui.ctx());
                self.poll_cv_profile_entry(ui.ctx());
                self.poll_settings_task(ui.ctx());
                self.poll_preferences_save(ui.ctx());
                self.drain_batch_progress();

                // The buffer is where background tasks report; without a wake
                // callback a line would wait for the next incidental frame.
                self.install_logs_wake(ui.ctx());
                self.refresh_logs_snapshot();

                // Cmd+L (Ctrl+L) toggles the drawer, unless the user is typing.
                if !ui.ctx().egui_wants_keyboard_input()
                    && ui.input(|input| input.modifiers.command && input.key_pressed(egui::Key::L))
                {
                    self.logs_view.drawer_open = !self.logs_view.drawer_open;
                }

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

                if self.navigation.current() == MainScreen::CvProfile
                    && self.cv_profile_task.is_none()
                    && !self.cv_profile_loaded
                    && self.cv_profile_error.is_none()
                {
                    self.start_cv_profile_load();
                }

                if self.navigation.current() == MainScreen::Settings
                    && self.settings_task.is_none()
                    && !self.settings_loaded
                    && self.settings_error.is_none()
                {
                    self.start_settings_load();
                }

                let render_state = PortalsRenderState {
                    loading: matches!(&self.portals_task, Some(PortalsTask::List { .. })),
                    busy: self.portals_task.is_some() || self.fetch_all_task.is_some(),
                    manual_busy: matches!(&self.portals_task, Some(PortalsTask::Manual(_))),
                    fetching_portal: self.fetching_portal,
                    fetch_all_portal: self.fetch_all_portal,
                    fetch_all_progress: self
                        .fetch_all_progress
                        .lock()
                        .ok()
                        .and_then(|progress| *progress),
                    stalled: self.fetch_all_stalled,
                    error: self.portals_error.as_deref(),
                    message: self.portals_message.as_deref(),
                };
                let pipeline_state = PipelineRenderState {
                    loading: self.pipeline_task.is_some(),
                    error: self.pipeline_error.as_deref(),
                    message: self.pipeline_message.as_deref(),
                    refreshing: self.refreshing_job,
                    marking: self.marking_job,
                    deleting: self.deleting_job,
                };
                let cv_profile_state = CvProfileRenderState {
                    loading: self.cv_profile_task.is_some(),
                    error: self.cv_profile_error.as_deref(),
                    message: self.cv_profile_message.as_deref(),
                    saving: self.cv_profile_save_task.is_some()
                        || self.cv_profile_entry_task.is_some(),
                    deleting: self.cv_profile_deleting,
                };

                let settings_state = SettingsRenderState {
                    loading: self.settings_task.is_some(),
                    saving: self.settings_save_task.is_some(),
                    error: self.settings_error.as_deref(),
                    message: self.settings_message.as_deref(),
                };

                let log_path = self.logs.path().map(|path| path.display().to_string());
                let logs_state = LogsRenderState {
                    path: log_path.as_deref(),
                    policy: self.logs.policy().label(),
                    latest_seq: self.logs.latest_seq(),
                    dropped: self.logs.dropped(),
                    confirming_delete: self.logs_confirm_delete,
                    message: self.logs_message.as_deref(),
                };

                // Shown before the shell panels, so it claims the full-width
                // bottom strip and the sidebar/central panels fill the rest.
                let mut drawer_action = None;
                if self.logs_view.drawer_open {
                    drawer_action = egui::Panel::bottom("career_ops_logs_drawer")
                        .resizable(true)
                        .default_size(180.0)
                        .show(ui, |ui| {
                            screens::logs::drawer(
                                ui,
                                &self.logs_snapshot,
                                &mut self.logs_view,
                                logs_state,
                            )
                        })
                        .inner;
                }

                // The sidebar toggle edits a local copy: the closure below also
                // borrows `logs_view`, and the two cannot share a field borrow.
                let mut drawer_open = self.logs_view.drawer_open;

                let navigation = &mut self.navigation;
                let portals = &self.portals;
                let pipeline = &self.pipeline;
                let pipeline_view = &mut self.pipeline_view;
                let cv_profile = &mut self.cv_profile;
                let cv_profile_view = &mut self.cv_profile_view;
                let settings = &mut self.settings;
                let logs_snapshot = &self.logs_snapshot;
                let logs_view = &mut self.logs_view;

                let action =
                    screens::shell::show_with(ui, navigation, &mut drawer_open, |ui, screen| {
                        match screen {
                            MainScreen::Portals => {
                                screens::portals::show(ui, portals, render_state)
                                    .map(ScreenAction::Portals)
                            }
                            MainScreen::Pipeline => {
                                screens::pipeline::show(ui, pipeline, pipeline_view, pipeline_state)
                                    .map(ScreenAction::Pipeline)
                            }
                            MainScreen::CvProfile => screens::cv_profile::show(
                                ui,
                                cv_profile,
                                cv_profile_view,
                                cv_profile_state,
                            )
                            .map(ScreenAction::CvProfile),
                            MainScreen::Logs => {
                                screens::logs::show(ui, logs_snapshot, logs_view, logs_state)
                                    .map(ScreenAction::Logs)
                            }
                            MainScreen::Settings => {
                                screens::settings::show(ui, settings, settings_state)
                                    .map(ScreenAction::Settings)
                            }
                            other => {
                                screens::shell::placeholder(ui, other);
                                None
                            }
                        }
                    });

                self.logs_view.drawer_open = drawer_open;

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
                    Some(ScreenAction::CvProfile(action)) => self.handle_cv_profile_action(action),
                    Some(ScreenAction::Settings(action)) => self.handle_settings_action(action),
                    Some(ScreenAction::Logs(action)) => self.handle_logs_action(action),
                    None => {}
                }

                if let Some(action) = drawer_action {
                    self.handle_logs_action(action);
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
    use crate::view_models::cv_profile::MoveDirection;
    use crate::view_models::pipeline::{JobFilter, StatusLabel, TierLabel};
    use crate::view_models::portals::{HealthBadge, PortalRow};

    use career_ops_core::acquisition::ats::{AtsAdapter, NormalizedJob};
    use career_ops_core::acquisition::config::{Provider, ScraperConfig};
    use career_ops_core::acquisition::error::AtsResult;
    use career_ops_core::acquisition::html::selectors::SelectorConfig;
    use career_ops_core::acquisition::known_sites;
    use career_ops_core::acquisition::known_sites::SiteEngine;
    use career_ops_core::acquisition::report::PageReport;
    use career_ops_core::repo::cv_entries::CvEntryKind;

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
                assert_eq!(form.scraper_config, "{\"title\":\"h1\"}");
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

    /// The add flow starts with the embedded registry. An app test, not only a
    /// screen test, because the initial state is set by an action handler.
    #[test]
    fn open_add_opens_the_known_site_chooser() {
        let (mut app, _dir) = test_app();

        app.handle_portals_action(PortalsAction::OpenAdd);

        assert!(
            matches!(
                &app.portals_view.modal,
                Some(PortalModal::ChooseSite { .. })
            ),
            "the add flow starts with the registry, not a blank form"
        );
        assert!(app.portals_view.form().is_none());

        app.handle_portals_action(PortalsAction::CloseModal);
        assert!(app.portals_view.modal.is_none());
    }

    #[test]
    fn pipeline_set_filter_only_mutates_the_view_model() {
        let (mut app, _dir) = test_app();

        app.handle_pipeline_action(PipelineAction::SetFilter(JobFilter::Rejected));

        assert_eq!(app.pipeline.filter, JobFilter::Rejected);
        assert!(!app.pipeline_loaded, "changing a filter must not load");
    }

    /// The Settings screen's round trip: save through the app, then read the
    /// stored value back the way a fresh load would.
    #[test]
    fn preferences_save_and_reload_through_the_app() {
        let _guard = KEYCHAIN_TEST_LOCK.lock().expect("keychain test lock");
        career_ops_test_support::install();
        let dir = tempfile::tempdir().expect("tempdir");
        let (mut app, ctx) = unlocked_app(dir.path());

        app.handle_settings_action(SettingsAction::Save(Preferences {
            keywords: vec!["rust".into()],
            excluded_companies: vec!["RecruiterCo".into()],
            ..Default::default()
        }));
        wait_until(&mut app, &ctx, |app| {
            app.settings_save_task.is_none() && app.settings_message.is_some()
        });

        assert!(!app.settings.is_dirty(), "the stored value is adopted");
        assert_eq!(app.settings.form.keywords, "rust");
        assert_eq!(app.settings.form.excluded_companies, "RecruiterCo");

        // Forget the form and load again: the value must come from the row.
        app.settings = SettingsViewModel::default();
        app.settings_loaded = false;
        app.handle_settings_action(SettingsAction::Reload);
        wait_until(&mut app, &ctx, |app| {
            app.settings_task.is_none() && app.settings_loaded
        });

        assert_eq!(app.settings.form.keywords, "rust");
        assert!(app.settings.is_configured());
        assert!(!app.settings.is_dirty());
    }

    /// The Logs screen is one click from a fetch failure, and opening it must
    /// not leave the drawer that would duplicate it on top.
    #[test]
    fn show_logs_opens_the_logs_screen_and_closes_the_drawer() {
        let _guard = KEYCHAIN_TEST_LOCK.lock().expect("keychain test lock");
        career_ops_test_support::install();
        let dir = tempfile::tempdir().expect("tempdir");
        let (mut app, _ctx) = unlocked_app(dir.path());

        assert_eq!(app.navigation().current(), MainScreen::Pipeline);
        app.logs_view.drawer_open = true;

        app.handle_portals_action(PortalsAction::ShowLogs);

        assert_eq!(app.navigation().current(), MainScreen::Logs);
        assert!(
            !app.logs_view.drawer_open,
            "the drawer would sit on top of the screen it duplicates"
        );
    }

    /// The snapshot is keyed on the sequence number, so a line emitted after the
    /// last refresh is picked up exactly once.
    #[test]
    fn the_logs_snapshot_follows_the_buffer() {
        let _guard = KEYCHAIN_TEST_LOCK.lock().expect("keychain test lock");
        career_ops_test_support::install();
        let dir = tempfile::tempdir().expect("tempdir");
        let (mut app, _ctx) = unlocked_app(dir.path());

        app.refresh_logs_snapshot();
        let before = app.logs_snapshot.len();

        tracing::info!(target: "career_ops_core::test", "snapshot-marker");
        app.refresh_logs_snapshot();

        assert!(
            app.logs_snapshot
                .iter()
                .any(|line| line.message == "snapshot-marker"),
            "a line emitted since the last refresh must reach the snapshot"
        );
        assert!(app.logs_snapshot.len() > before);
    }

    /// Print the captured log when a live test fails.
    ///
    /// The app installs the only subscriber now, so a second `try_init` would
    /// fail silently and the old `RUST_LOG` stderr setup would be dead. Cargo
    /// captures stderr and shows it only for a failing test, so dropping this
    /// guard is silent on success and prints the whole run on failure.
    struct LogsOnFailure(career_ops_core::logging::LogHandle);

    impl Drop for LogsOnFailure {
        fn drop(&mut self) {
            if std::thread::panicking() {
                eprintln!("--- captured log ---");
                for line in self.0.snapshot() {
                    eprintln!("{}", line.compact());
                }
            }
        }
    }

    /// Create a vault in `dir` and return an app that has unlocked it.
    ///
    /// The caller holds `KEYCHAIN_TEST_LOCK`; the helper does not, so the guard
    /// outlives the app it protects.
    fn unlocked_app(dir: &Path) -> (CareerOpsApp, egui::Context) {
        {
            let setup_runtime = tokio::runtime::Builder::new_multi_thread()
                .worker_threads(1)
                .enable_all()
                .build()
                .expect("setup runtime");
            let mut state = AppState::bootstrap(dir.to_path_buf()).expect("bootstrap");
            setup_runtime
                .block_on(state.complete_first_launch(TEST_PASSPHRASE))
                .expect("first launch");
        }

        let state = AppState::bootstrap(dir.to_path_buf()).expect("second bootstrap");
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

        (app, ctx)
    }

    /// The titles in the Experience group, in display order.
    fn experience_titles(app: &CareerOpsApp) -> Vec<String> {
        app.cv_profile.groups()[0]
            .rows
            .iter()
            .map(|row| row.title.clone())
            .collect()
    }

    /// The CV screen's database path: the load creates the singleton, the
    /// profile saves, an entry round-trips, and the stored `cv_summary` is the
    /// editor's preview.
    #[test]
    fn cv_profile_saves_entries_and_reloads_through_the_app() {
        let _guard = KEYCHAIN_TEST_LOCK.lock().expect("keychain test lock");
        career_ops_test_support::install();

        let dir = tempfile::tempdir().expect("tempdir");
        let (mut app, ctx) = unlocked_app(dir.path());

        app.handle_cv_profile_action(CvProfileAction::Reload);
        wait_until(&mut app, &ctx, |app| {
            app.cv_profile_loaded && app.cv_profile_task.is_none()
        });
        assert_eq!(app.cv_profile.profile.id, 1, "the load creates the row");
        assert!(!app.cv_profile.is_dirty());

        app.handle_cv_profile_action(CvProfileAction::SaveProfile(ProfileDetails {
            name: "Ada Lovelace".into(),
            email: "ada@example.com".into(),
            phone: Some("+44 20 0000 0000".into()),
            location: Some("London".into()),
            summary: Some("Mathematician.".into()),
        }));
        wait_until(&mut app, &ctx, |app| {
            app.cv_profile_save_task.is_none() && !app.cv_profile.is_dirty()
        });
        assert_eq!(app.cv_profile.profile.name, "Ada Lovelace");
        assert_eq!(app.cv_profile_message.as_deref(), Some("Profile saved."));

        let mut draft = CvEntryDraft::new(CvEntryKind::Experience);
        draft.title = "Analytical Engine".into();
        draft.organization = "Babbage".into();
        draft.start_date = "1842".into();
        app.handle_cv_profile_action(CvProfileAction::CreateEntry(draft));
        wait_until(&mut app, &ctx, |app| {
            app.cv_profile_entry_task.is_none() && app.cv_profile.entries.len() == 1
        });
        assert_eq!(app.cv_profile_message.as_deref(), Some("Entry saved."));

        let entry_id = app.cv_profile.entries[0].id;

        // A reload is what a restart would do: everything comes from the
        // database, not from the in-memory view model.
        app.handle_cv_profile_action(CvProfileAction::Reload);
        wait_until(&mut app, &ctx, |app| {
            app.cv_profile_loaded && app.cv_profile_task.is_none()
        });

        assert_eq!(app.cv_profile.form.name, "Ada Lovelace");
        assert_eq!(app.cv_profile.form.summary, "Mathematician.");
        assert_eq!(app.cv_profile.entries.len(), 1);
        assert_eq!(app.cv_profile.entries[0].title, "Analytical Engine");
        assert_eq!(
            app.cv_profile.entries[0].organization.as_deref(),
            Some("Babbage")
        );

        // The preview and Phase 3's helper are the same renderer.
        let preview = app.cv_profile.preview().expect("preview");
        let pool = app.pool().expect("pool");
        let stored = app
            .runtime()
            .block_on(profile_repo::cv_summary(&pool))
            .expect("cv_summary")
            .expect("a populated profile has a summary");
        assert_eq!(stored, preview);
        assert!(
            stored.starts_with("Ada Lovelace\nLondon | ada@example.com | +44 20 0000 0000"),
            "{stored}"
        );

        app.handle_cv_profile_action(CvProfileAction::DeleteEntry(entry_id));
        wait_until(&mut app, &ctx, |app| {
            app.cv_profile_entry_task.is_none() && app.cv_profile.entries.is_empty()
        });
        assert!(app.cv_profile_view.deleting.is_none());
    }

    /// A move rewrites the stored order, not just the loaded rows. A reload is
    /// the only way to tell the difference.
    #[test]
    fn moving_a_cv_entry_persists_the_display_order() {
        let _guard = KEYCHAIN_TEST_LOCK.lock().expect("keychain test lock");
        career_ops_test_support::install();

        let dir = tempfile::tempdir().expect("tempdir");
        let (mut app, ctx) = unlocked_app(dir.path());

        app.handle_cv_profile_action(CvProfileAction::Reload);
        wait_until(&mut app, &ctx, |app| {
            app.cv_profile_loaded && app.cv_profile_task.is_none()
        });

        for title in ["First", "Second"] {
            let mut draft = CvEntryDraft::new(CvEntryKind::Experience);
            draft.title = title.into();
            app.handle_cv_profile_action(CvProfileAction::CreateEntry(draft));
            wait_until(&mut app, &ctx, |app| app.cv_profile_entry_task.is_none());
        }
        assert_eq!(experience_titles(&app), vec!["First", "Second"]);

        let first_id = app.cv_profile.entries[0].id;
        app.handle_cv_profile_action(CvProfileAction::MoveEntry {
            id: first_id,
            direction: MoveDirection::Down,
        });
        wait_until(&mut app, &ctx, |app| app.cv_profile_entry_task.is_none());
        assert_eq!(experience_titles(&app), vec!["Second", "First"]);

        app.handle_cv_profile_action(CvProfileAction::Reload);
        wait_until(&mut app, &ctx, |app| {
            app.cv_profile_loaded && app.cv_profile_task.is_none()
        });
        assert_eq!(
            experience_titles(&app),
            vec!["Second", "First"],
            "the stored sort order must match what the screen showed"
        );
    }

    /// A failed save keeps the user's text and surfaces the error, so nothing
    /// typed is lost and a retry is possible.
    #[test]
    fn a_failed_profile_save_keeps_the_edits() {
        let _guard = KEYCHAIN_TEST_LOCK.lock().expect("keychain test lock");
        career_ops_test_support::install();

        let dir = tempfile::tempdir().expect("tempdir");
        let (mut app, ctx) = unlocked_app(dir.path());

        app.handle_cv_profile_action(CvProfileAction::Reload);
        wait_until(&mut app, &ctx, |app| {
            app.cv_profile_loaded && app.cv_profile_task.is_none()
        });

        // Closing the pool makes the next write fail without breaking the
        // schema or the rows.
        let pool = app.pool().expect("pool");
        app.runtime().block_on(pool.close());

        app.cv_profile.form.name = "  Grace Hopper  ".into();
        app.handle_cv_profile_action(CvProfileAction::SaveProfile(app.cv_profile.details()));
        wait_until(&mut app, &ctx, |app| {
            app.cv_profile_save_task.is_none() && app.cv_profile_error.is_some()
        });

        assert_eq!(app.cv_profile.form.name, "  Grace Hopper  ");
        assert!(app.cv_profile.is_dirty());
        assert!(
            app.cv_profile_error
                .as_deref()
                .is_some_and(|error| error.contains("Could not save the profile")),
            "{:?}",
            app.cv_profile_error
        );
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
            app.poll_fetch_all_pages(ctx);
            app.poll_pipeline_task(ctx);
            app.poll_pipeline_refresh(ctx);
            app.poll_pipeline_status(ctx);
            app.poll_pipeline_delete(ctx);
            app.poll_cv_profile_task(ctx);
            app.poll_cv_profile_save(ctx);
            app.poll_cv_profile_entry(ctx);
            app.poll_settings_task(ctx);
            app.poll_preferences_save(ctx);
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
                // The provider-bearing selector config must survive the write
                // path and come back through the view model, not just through
                // the modal.
                scraper_config: Some(
                    r#"{"provider":"selectors","list":"article.job"}"#.to_string(),
                ),
            },
        });
        wait_until(&mut app, &ctx, |app| {
            app.portals.rows.len() == 1
                && app.portals.rows[0].name == "Acme Updated"
                && app.portals_task.is_none()
        });
        assert_eq!(app.portals.rows[0].render_mode, "browser");

        let stored_config = app.portals.rows[0]
            .scraper_config
            .as_deref()
            .expect("the scraper config must be stored");
        let parsed: serde_json::Value =
            serde_json::from_str(stored_config).expect("stored config is JSON");
        assert_eq!(parsed["list"], "article.job");
        assert_eq!(parsed["provider"], "selectors");

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
        page_calls: std::sync::Mutex<Vec<String>>,
        eightfold_calls: std::sync::Mutex<Vec<(String, String)>>,
        /// The `start_page` of each Eightfold list request, in call order.
        eightfold_starts: std::sync::Mutex<Vec<usize>>,
        eightfold_jobs: std::sync::Mutex<Vec<NormalizedJob>>,
    }

    impl StubFetchSource {
        /// The jobs an Eightfold list request returns. Empty by default, so a
        /// preview that hits the provider still works without configuring it.
        fn with_eightfold_jobs(jobs: Vec<NormalizedJob>) -> Self {
            Self {
                eightfold_jobs: std::sync::Mutex::new(jobs),
                ..Self::default()
            }
        }

        fn calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }

        fn page_calls(&self) -> Vec<String> {
            self.page_calls.lock().unwrap().clone()
        }

        fn eightfold_calls(&self) -> Vec<(String, String)> {
            self.eightfold_calls.lock().unwrap().clone()
        }

        fn eightfold_starts(&self) -> Vec<usize> {
            self.eightfold_starts.lock().unwrap().clone()
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
            _start_page: usize,
        ) -> AtsResult<PageReport> {
            self.calls.lock().unwrap().push(url.to_string());
            Ok(PageReport::single(vec![NormalizedJob {
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
            }]))
        }

        async fn fetch_eightfold(
            &self,
            host: &str,
            domain: &str,
            _company_name: &str,
            start_page: usize,
            _max_pages: usize,
        ) -> AtsResult<PageReport> {
            self.eightfold_calls
                .lock()
                .unwrap()
                .push((host.to_string(), domain.to_string()));
            self.eightfold_starts.lock().unwrap().push(start_page);
            Ok(PageReport::single(
                self.eightfold_jobs.lock().unwrap().clone(),
            ))
        }

        async fn fetch_page(&self, url: &url::Url) -> AtsResult<String> {
            self.page_calls.lock().unwrap().push(url.to_string());
            Ok(r#"<html><body><div class="job-description">Full description from the detail page.</div></body></html>"#.into())
        }
    }

    /// The Fetch-all-pages task path: the walk runs through the app, updates the
    /// portal row it touched, and lands a message naming the stop reason.
    #[test]
    fn fetch_all_pages_runs_through_the_app() {
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
        let source = Arc::new(StubFetchSource::with_eightfold_jobs(vec![NormalizedJob {
            external_id: "eightfold:nvidia.com:1".into(),
            title: "Engineer".into(),
            company: "NVIDIA".into(),
            location: Some("Remote".into()),
            department: None,
            employment_type: None,
            workplace_type: None,
            source_url: url::Url::parse("https://jobs.nvidia.com/careers/job/1").expect("url"),
            raw_text: String::new(),
            posted_at: None,
        }]));
        let mut app = CareerOpsApp::with_fetch_source(state, runtime, source);
        let ctx = egui::Context::default();

        app.handle_lock_action(LockAction::TryPassphrase(TEST_PASSPHRASE.to_string()));
        wait_until(&mut app, &ctx, |app| {
            app.state()
                .is_some_and(|state| state.current() == &Screen::Ready)
        });

        app.handle_portals_action(PortalsAction::CreatePortal(PortalFormData {
            company_name: "NVIDIA".into(),
            url: "https://jobs.nvidia.com/careers".into(),
            render_mode: "static".into(),
            scraper_config: Some(r#"{"provider":"eightfold","domain":"nvidia.com"}"#.into()),
        }));
        wait_until(&mut app, &ctx, |app| {
            app.portals.rows.len() == 1 && app.portals_task.is_none()
        });

        let id = app.portals.rows[0].id;
        app.handle_portals_action(PortalsAction::FetchAllPages(id));
        wait_until(&mut app, &ctx, |app| {
            app.fetch_all_task.is_none() && app.portals_message.is_some()
        });

        assert!(app.fetch_all_portal.is_none(), "the row is handed back");
        assert_eq!(
            app.portals_message.as_deref(),
            Some("Fetched 1 job across 1 page; end of list.")
        );
        assert!(app.portals_error.is_none());
        assert_eq!(app.portals.rows[0].health, HealthBadge::Healthy);
    }

    /// **Skip page N** moves the cursor onto N and walks from N+1, which is the
    /// only way past a page a site keeps refusing.
    #[test]
    fn skipping_a_page_resumes_the_walk_after_it() {
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
        let source = Arc::new(StubFetchSource::with_eightfold_jobs(vec![NormalizedJob {
            external_id: "eightfold:nvidia.com:1".into(),
            title: "Engineer".into(),
            company: "NVIDIA".into(),
            location: None,
            department: None,
            employment_type: None,
            workplace_type: None,
            source_url: url::Url::parse("https://jobs.nvidia.com/careers/job/1").expect("url"),
            raw_text: String::new(),
            posted_at: None,
        }]));
        let mut app = CareerOpsApp::with_fetch_source(state, runtime, {
            let source: Arc<dyn FetchSource> = source.clone();
            source
        });
        let ctx = egui::Context::default();

        app.handle_lock_action(LockAction::TryPassphrase(TEST_PASSPHRASE.to_string()));
        wait_until(&mut app, &ctx, |app| {
            app.state()
                .is_some_and(|state| state.current() == &Screen::Ready)
        });

        app.handle_portals_action(PortalsAction::CreatePortal(PortalFormData {
            company_name: "NVIDIA".into(),
            url: "https://jobs.nvidia.com/careers".into(),
            render_mode: "static".into(),
            scraper_config: Some(r#"{"provider":"eightfold","domain":"nvidia.com"}"#.into()),
        }));
        wait_until(&mut app, &ctx, |app| {
            app.portals.rows.len() == 1 && app.portals_task.is_none()
        });

        let id = app.portals.rows[0].id;
        app.handle_portals_action(PortalsAction::SkipPage {
            portal_id: id,
            page: 30,
        });
        wait_until(&mut app, &ctx, |app| {
            app.fetch_all_task.is_none() && app.portals_message.is_some()
        });

        assert_eq!(
            source.eightfold_starts(),
            vec![31],
            "skipping page 30 resumes at page 31"
        );
        assert!(app.fetch_all_stalled.is_none());
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

    /// The per-job refresh through the app: a job stored with no text gets the
    /// posting's description, in place, without a second row.
    #[test]
    fn refresh_description_updates_the_row_in_place() {
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

        // The portal supplies the description selector for this host.
        app.handle_portals_action(PortalsAction::CreatePortal(PortalFormData {
            company_name: "Acme".into(),
            url: "https://example.com/careers".into(),
            render_mode: "static".into(),
            scraper_config: Some(
                r#"{"list":"article.job","fields":{"title":"h3 a"},"detail":{"fields":{"description":".job-description"},"max_jobs":3}}"#
                    .into(),
            ),
        }));
        wait_until(&mut app, &ctx, |app| {
            app.portals.rows.len() == 1 && app.portals_task.is_none()
        });

        let pool = app.pool().expect("pool");
        let stored = app
            .runtime()
            .block_on(job_repo::upsert(
                &pool,
                job_repo::NewJobDescription {
                    title: "Engineer".into(),
                    company: "Acme".into(),
                    source_url: "https://example.com/careers".into(),
                    raw_text: String::new(),
                    fetch_tier: FetchTier::Tier1bHtml,
                    external_id: None,
                },
            ))
            .expect("store");

        app.handle_pipeline_action(PipelineAction::Reload);
        wait_until(&mut app, &ctx, |app| {
            app.pipeline_loaded && app.pipeline_task.is_none()
        });
        assert_eq!(app.pipeline.rows[0].raw_text, "");

        app.handle_pipeline_action(PipelineAction::RefreshJob(stored.id));
        wait_until(&mut app, &ctx, |app| {
            app.refreshing_job.is_none() && app.pipeline_message.is_some()
        });

        assert_eq!(
            app.pipeline.rows[0].raw_text,
            "Full description from the detail page."
        );
        assert!(
            app.pipeline_message
                .as_deref()
                .is_some_and(|message| message.starts_with("Description refreshed:")),
            "message: {:?}",
            app.pipeline_message
        );
    }

    /// Marking a job creates the application row and updates the pipeline in
    /// place. This is the first production writer of `applications`.
    #[test]
    fn mark_not_a_match_updates_the_row_and_creates_the_application() {
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
        let mut app = CareerOpsApp::with_fetch_source(state, runtime, source);
        let ctx = egui::Context::default();

        app.handle_lock_action(LockAction::TryPassphrase(TEST_PASSPHRASE.to_string()));
        wait_until(&mut app, &ctx, |app| {
            app.state()
                .is_some_and(|state| state.current() == &Screen::Ready)
        });

        let pool = app.pool().expect("pool");
        let stored = app
            .runtime()
            .block_on(job_repo::upsert(
                &pool,
                job_repo::NewJobDescription {
                    title: "Engineer".into(),
                    company: "Acme".into(),
                    source_url: "https://example.com/jobs/1".into(),
                    raw_text: "body".into(),
                    fetch_tier: FetchTier::Tier1bHtml,
                    external_id: None,
                },
            ))
            .expect("store");

        app.handle_pipeline_action(PipelineAction::Reload);
        wait_until(&mut app, &ctx, |app| {
            app.pipeline_loaded && app.pipeline_task.is_none()
        });
        assert_eq!(app.pipeline.rows[0].status, None);

        app.handle_pipeline_action(PipelineAction::MarkNotAMatch(stored.id));
        wait_until(&mut app, &ctx, |app| {
            app.marking_job.is_none() && app.pipeline_message.is_some()
        });

        assert_eq!(app.pipeline.rows[0].status, Some(StatusLabel::NotAMatch));
        assert!(
            app.pipeline.rows[0].has_user_state,
            "the status alone makes the row gated"
        );
        assert_eq!(
            app.pipeline_message.as_deref(),
            Some("Marked as not a match.")
        );

        let applications = app
            .runtime()
            .block_on(application_repo::list(&pool))
            .expect("list applications");
        assert_eq!(applications.len(), 1);
        assert_eq!(applications[0].job_description_id, stored.id);
        assert_eq!(applications[0].status, "not_a_match");
    }

    /// Deleting through the app removes the row from the view model, clears the
    /// selection, and closes the gate. The cascade onto the application is a
    /// core test; this covers the app's orchestration.
    #[test]
    fn delete_a_job_removes_the_row_from_the_pipeline() {
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
        let mut app = CareerOpsApp::with_fetch_source(state, runtime, source);
        let ctx = egui::Context::default();

        app.handle_lock_action(LockAction::TryPassphrase(TEST_PASSPHRASE.to_string()));
        wait_until(&mut app, &ctx, |app| {
            app.state()
                .is_some_and(|state| state.current() == &Screen::Ready)
        });

        let pool = app.pool().expect("pool");
        let stored = app
            .runtime()
            .block_on(job_repo::upsert(
                &pool,
                job_repo::NewJobDescription {
                    title: "Engineer".into(),
                    company: "Acme".into(),
                    source_url: "https://example.com/jobs/delete".into(),
                    raw_text: "body".into(),
                    fetch_tier: FetchTier::Tier1bHtml,
                    external_id: None,
                },
            ))
            .expect("store");

        app.handle_pipeline_action(PipelineAction::Reload);
        wait_until(&mut app, &ctx, |app| {
            app.pipeline_loaded && app.pipeline_task.is_none()
        });
        assert_eq!(app.pipeline.rows.len(), 1);
        app.pipeline_view.selected = Some(stored.id);

        app.handle_pipeline_action(PipelineAction::DeleteJob(stored.id));
        wait_until(&mut app, &ctx, |app| {
            app.deleting_job.is_none() && app.pipeline_message.is_some()
        });

        assert!(app.pipeline.rows.is_empty());
        assert_eq!(app.pipeline_view.selected, None);
        assert!(app.pipeline_view.delete.is_none());
        assert_eq!(app.pipeline_message.as_deref(), Some("Job deleted."));
        assert!(app
            .runtime()
            .block_on(job_repo::get(&pool, stored.id))
            .expect("get")
            .is_none());
    }

    /// Every stop reason has to read differently, or a partial walk looks like a
    /// completed one.
    #[test]
    fn fetch_all_summary_names_what_stopped_the_walk() {
        let report = |stopped_reason| FetchAllPagesReport {
            portal: career_ops_core::model::Portal {
                id: 1,
                company_name: "Acme".into(),
                url: "https://example.com/jobs".into(),
                render_mode: "static".into(),
                scraper_config: None,
                last_verified_at: None,
                last_fetch_attempt_at: None,
                last_failure_cause: None,
                last_page_fetched: 0,
            },
            pages_fetched: 4,
            jobs_stored: 40,
            stopped_reason,
        };

        assert_eq!(
            fetch_all_summary(&report(FetchAllStop::EndOfList)),
            "Fetched 40 jobs across 4 pages; end of list."
        );
        assert_eq!(
            fetch_all_summary(&report(FetchAllStop::BudgetExhausted)),
            "Fetched 40 jobs across 4 pages. There is more; Continue or Fetch all pages again."
        );
        assert_eq!(
            fetch_all_summary(&report(FetchAllStop::Stopped {
                unread_page: Some(5)
            })),
            "Fetched 40 jobs across 4 pages; page 5 could not be read. Continue retries it."
        );
        assert_eq!(
            fetch_all_summary(&report(FetchAllStop::Stopped { unread_page: None })),
            "Fetched 40 jobs across 4 pages; the board stopped advancing. Continue retries the \
             last range."
        );
        assert_eq!(
            fetch_all_summary(&report(FetchAllStop::NoProgress)),
            "Fetched 40 jobs; this board has no page to continue from."
        );
    }

    #[test]
    fn fetch_summary_reports_descriptions() {
        let with_details = FetchOutcome {
            jobs_fetched: 1,
            tier: FetchTier::Tier1bHtml,
            first_page: 1,
            pages_fetched: Some(1),
            pagination_stopped: Some(PaginationStop::EndOfList),
            next_page: None,
            end_of_list: true,
            details_attempted: 1,
            details_enriched: 1,
        };
        assert_eq!(
            fetch_summary(&with_details),
            "Fetched 1 job; 1 of 1 description."
        );

        let multi_page = FetchOutcome {
            jobs_fetched: 60,
            tier: FetchTier::Tier1bHtml,
            first_page: 1,
            pages_fetched: Some(2),
            pagination_stopped: Some(PaginationStop::ExtractionFailed),
            next_page: None,
            end_of_list: false,
            details_attempted: 2,
            details_enriched: 2,
        };
        assert_eq!(
            fetch_summary(&multi_page),
            "Fetched 60 jobs from 2 pages (page 3 could not be read); 2 of 2 descriptions."
        );

        let capped = FetchOutcome {
            jobs_fetched: 150,
            tier: FetchTier::Tier1bHtml,
            first_page: 1,
            pages_fetched: Some(5),
            pagination_stopped: Some(PaginationStop::LimitReached),
            next_page: Some(6),
            end_of_list: false,
            details_attempted: 0,
            details_enriched: 0,
        };
        assert_eq!(
            fetch_summary(&capped),
            "Fetched 150 jobs from 5 pages.",
            "hitting the configured cap is an expected ending"
        );

        let no_details = FetchOutcome {
            jobs_fetched: 3,
            tier: FetchTier::Tier1bHtml,
            first_page: 1,
            pages_fetched: Some(1),
            pagination_stopped: Some(PaginationStop::EndOfList),
            next_page: None,
            end_of_list: true,
            details_attempted: 0,
            details_enriched: 0,
        };
        assert_eq!(fetch_summary(&no_details), "Fetched 3 jobs.");

        let resumed = FetchOutcome {
            jobs_fetched: 50,
            tier: FetchTier::Tier1aAts,
            first_page: 6,
            pages_fetched: Some(5),
            pagination_stopped: Some(PaginationStop::LimitReached),
            next_page: Some(11),
            end_of_list: false,
            details_attempted: 0,
            details_enriched: 0,
        };
        assert_eq!(
            fetch_summary(&resumed),
            "Fetched 50 jobs from pages 6-10.",
            "a resumed fetch names the range it read"
        );
    }

    /// Detail enrichment through the app: the scripted source returns a card
    /// list and a detail document, and the app stores the detail text and
    /// reports how many descriptions it filled in.
    #[test]
    fn portal_fetch_enriches_descriptions_and_reports_it() {
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

        app.handle_portals_action(PortalsAction::CreatePortal(PortalFormData {
            company_name: "Acme".into(),
            url: "https://example.com/jobs".into(),
            render_mode: "static".into(),
            scraper_config: Some(
                r#"{"list":"article.job","fields":{"title":"h3 a"},"detail":{"fields":{"description":".job-description"},"max_jobs":3}}"#
                    .into(),
            ),
        }));
        wait_until(&mut app, &ctx, |app| {
            app.portals.rows.len() == 1 && app.portals_task.is_none()
        });

        let portal_id = app.portals.rows[0].id;
        app.handle_portals_action(PortalsAction::FetchOne(portal_id));
        wait_until(&mut app, &ctx, |app| app.portals_task.is_none());

        assert_eq!(source.page_calls().len(), 1, "one detail fetch per job");
        assert_eq!(
            app.portals_message.as_deref(),
            Some("Fetched 1 job; 1 of 1 description.")
        );

        app.handle_pipeline_action(PipelineAction::Reload);
        wait_until(&mut app, &ctx, |app| {
            app.pipeline_loaded && app.pipeline_task.is_none()
        });

        assert_eq!(
            app.pipeline.rows[0].raw_text,
            "Full description from the detail page."
        );
    }

    /// A provider chosen by hand for a host the registry does not know.
    ///
    /// This is the escape hatch that makes known sites a convenience rather
    /// than a gate: the form's output is stored verbatim, and the router
    /// dispatches on the stored `provider`, not on the host.
    #[test]
    fn a_manually_chosen_provider_fetches_an_unknown_host() {
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
        let source = Arc::new(StubFetchSource::with_eightfold_jobs(vec![NormalizedJob {
            external_id: "7".into(),
            title: "GPU Engineer".into(),
            company: "Acme".into(),
            location: None,
            department: None,
            employment_type: None,
            workplace_type: None,
            source_url: url::Url::parse("https://careers.acme.example/job/7").expect("url"),
            raw_text: "Build accelerators.".into(),
            posted_at: None,
        }]));
        let mut app = CareerOpsApp::with_fetch_source(state, runtime, source.clone());
        let ctx = egui::Context::default();

        app.handle_lock_action(LockAction::TryPassphrase(TEST_PASSPHRASE.to_string()));
        wait_until(&mut app, &ctx, |app| {
            app.state()
                .is_some_and(|state| state.current() == &Screen::Ready)
        });

        // The form is the real path for a site the registry does not know.
        let form = PortalForm {
            company_name: "Acme".into(),
            url: "https://careers.acme.example/jobs".into(),
            provider: Provider::Eightfold,
            domain: "acme.com".into(),
            ..Default::default()
        };
        assert!(
            known_sites::lookup(&url::Url::parse(&form.url).expect("url")).is_none(),
            "the registry must not know this host, or the test proves nothing"
        );

        app.handle_portals_action(PortalsAction::CreatePortal(form.data()));
        wait_until(&mut app, &ctx, |app| {
            app.portals.rows.len() == 1 && app.portals_task.is_none()
        });

        let stored = app.portals.rows[0]
            .scraper_config
            .as_deref()
            .expect("stored config");
        let parsed = ScraperConfig::parse(stored).expect("stored config parses");
        assert_eq!(parsed.provider(), Provider::Eightfold);
        assert_eq!(parsed.eightfold().expect("eightfold").domain, "acme.com");

        let portal_id = app.portals.rows[0].id;
        app.handle_portals_action(PortalsAction::FetchOne(portal_id));
        wait_until(&mut app, &ctx, |app| app.portals_task.is_none());

        assert_eq!(
            source.eightfold_calls(),
            vec![("careers.acme.example".to_string(), "acme.com".to_string())],
            "the stored provider decides the engine"
        );
        assert!(
            source.calls().is_empty(),
            "an eightfold portal must not fetch HTML"
        );

        app.handle_pipeline_action(PipelineAction::Reload);
        wait_until(&mut app, &ctx, |app| {
            app.pipeline_loaded && app.pipeline_task.is_none()
        });

        assert_eq!(app.pipeline.rows.len(), 1);
        assert_eq!(app.pipeline.rows[0].title, "GPU Engineer");
        assert_eq!(app.pipeline.rows[0].raw_text, "Build accelerators.");
    }

    /// Every registry entry takes the same path: form -> stored JSON -> router.
    ///
    /// Waymo is a `selectors` profile, NVIDIA an `eightfold` one, and the ATS
    /// entries store an empty selectors config whose board URL select the
    /// adapter. This proves the dispatch follows the stored URL and config rather
    /// than a special case for the host.
    #[test]
    fn every_known_site_fetches_through_its_provider() {
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
        let source = Arc::new(StubFetchSource::with_eightfold_jobs(vec![NormalizedJob {
            external_id: "42".into(),
            title: "GPU Engineer".into(),
            company: "NVIDIA".into(),
            location: None,
            department: None,
            employment_type: None,
            workplace_type: None,
            source_url: url::Url::parse("https://jobs.nvidia.com/careers/job/42").expect("url"),
            raw_text: "Design GPUs.".into(),
            posted_at: None,
        }]));
        let mut app = CareerOpsApp::with_fetch_source(state, runtime, source.clone());
        let ctx = egui::Context::default();

        app.handle_lock_action(LockAction::TryPassphrase(TEST_PASSPHRASE.to_string()));
        wait_until(&mut app, &ctx, |app| {
            app.state()
                .is_some_and(|state| state.current() == &Screen::Ready)
        });

        let mut waymo_url = String::new();
        for (index, site) in known_sites::all().iter().enumerate() {
            let url = site.url();
            if site.engine == SiteEngine::Provider(Provider::Selectors) {
                waymo_url = url.clone();
            }

            // The same two calls the modal makes when it prefills an empty form.
            let mut form = PortalForm {
                url,
                ..Default::default()
            };
            form.apply_known_site(site);

            app.handle_portals_action(PortalsAction::CreatePortal(form.data()));
            wait_until(&mut app, &ctx, |app| {
                app.portals.rows.len() == index + 1 && app.portals_task.is_none()
            });
        }

        // The list is sorted by name, not by creation, so match by name.
        for site in known_sites::all() {
            let row = app
                .portals
                .rows
                .iter()
                .find(|row| row.name == site.name)
                .unwrap_or_else(|| panic!("{} row", site.name));
            let stored = row.scraper_config.as_deref().expect("stored config");
            let parsed = ScraperConfig::parse(stored).expect("stored config parses");
            assert_eq!(parsed.provider(), site.engine.provider(), "{}", site.name);
        }

        let ids: Vec<i64> = app.portals.rows.iter().map(|row| row.id).collect();
        for id in ids {
            app.handle_portals_action(PortalsAction::FetchOne(id));
            wait_until(&mut app, &ctx, |app| app.portals_task.is_none());
        }

        assert_eq!(
            source.calls(),
            vec![waymo_url],
            "the selectors entry is the only HTML fetch"
        );
        assert_eq!(
            source.eightfold_calls(),
            vec![("jobs.nvidia.com".to_string(), "nvidia.com".to_string())]
        );

        app.handle_pipeline_action(PipelineAction::Reload);
        wait_until(&mut app, &ctx, |app| {
            app.pipeline_loaded && app.pipeline_task.is_none()
        });

        let titles: Vec<&str> = app
            .pipeline
            .rows
            .iter()
            .map(|row| row.title.as_str())
            .collect();
        assert!(titles.contains(&"Engineer"), "{titles:?}");
        assert!(titles.contains(&"GPU Engineer"), "{titles:?}");
    }

    /// "Test config" through the app: one list page, at most three details,
    /// and nothing written to the database. The second half asserts that an
    /// unparsable config is reported as a classified error instead of silently
    /// previewing without it.
    #[test]
    fn config_preview_reports_and_never_writes() {
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

        let config = r#"{
          "list": "article.job",
          "fields": { "title": "h2" },
          "pagination": { "next": "a[rel=next]", "max_pages": 5 },
          "detail": { "fields": { "description": ".job-description" }, "max_jobs": 40 }
        }"#;

        app.handle_portals_action(PortalsAction::OpenAdd);
        // `OpenAdd` opens the known-site chooser; the preview needs a form.
        app.portals_view.open_blank_add();
        app.handle_portals_action(PortalsAction::TestConfig(PortalFormData {
            company_name: "Acme".into(),
            url: "https://example.com/careers".into(),
            render_mode: "static".into(),
            scraper_config: Some(config.into()),
        }));
        wait_until(&mut app, &ctx, |app| {
            app.portals_view
                .form()
                .is_some_and(|form| !form.preview.busy && form.preview.result.is_some())
        });

        let form = app.portals_view.form().expect("the add form is open");
        let preview = form.preview.result.as_ref().expect("preview result");
        assert_eq!(preview.jobs_found, 1);
        assert_eq!(preview.pages_planned, 5, "reported, not fetched");
        assert_eq!(preview.sample.len(), 1);
        assert_eq!(preview.sample[0].url, "https://example.com/careers");
        assert_eq!(
            preview.first_description_bytes,
            Some("Full description from the detail page.".len())
        );
        assert_eq!(source.calls().len(), 1, "one list page");
        assert_eq!(source.page_calls().len(), 1, "one detail request");

        let pool = app.pool().expect("pool");
        let stored = app
            .runtime()
            .block_on(job_repo::list_recent(&pool, 10))
            .expect("list jobs");
        assert!(
            stored.is_empty(),
            "a preview must not write to the database"
        );

        // Invalid JSON: the preview must classify it, not silently drop it the
        // way a real fetch does.
        app.handle_portals_action(PortalsAction::TestConfig(PortalFormData {
            company_name: "Acme".into(),
            url: "https://example.com/careers".into(),
            render_mode: "static".into(),
            scraper_config: Some(r#"{"list": 5}"#.into()),
        }));
        wait_until(&mut app, &ctx, |app| {
            app.portals_view
                .form()
                .is_some_and(|form| !form.preview.busy && form.preview.error.is_some())
        });

        let form = app.portals_view.form().expect("the add form is open");
        let error = form.preview.error.as_deref().expect("preview error");
        assert!(error.contains("selectors config is invalid"), "{error}");
        assert!(
            error.contains("The portal configuration is invalid"),
            "the classified action must match the Portals list: {error}"
        );
        assert!(form.preview.result.is_none());
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
    /// portal `scraper_config`; Waymo's search page needs the config in
    /// `docs/manual-test-script.md` §8. A page whose jobs are not in the HTML at
    /// all (client-rendered boards, e.g. Eightfold) escalates to Tier 2 and
    /// needs `vendor/obscura`.
    const LIVE_PORTAL_URLS: &[&str] = &[
        "https://jobs.ashbyhq.com/linear",
        "https://jobs.lever.co/spotify",
        "https://job-boards.greenhouse.io/figma",
        "https://jobs.smartrecruiters.com/Ubisoft2",
    ];

    /// Waymo's search-page config, kept in sync with
    /// `docs/manual-test-script.md` §8. The page is server-rendered but carries
    /// no `JobPosting` JSON-LD, so it needs the card selectors and pagination.
    const WAYMO_CONFIG: &str = r#"{
      "list": "article.job-search-results-card-col",
      "fields": {
        "title": "h3.job-search-results-card-title a",
        "location": "li.job-component-location span",
        "department": "li.job-component-department span",
        "employment_type": "li.job-component-employment-type span",
        "description": "p.job-search-results-summary"
      },
      "pagination": {
        "next": "a[rel=next]",
        "url_template": "https://careers.withwaymo.com/jobs/search?page={page}",
        "max_pages": 3
      },
      "detail": { "max_jobs": 2 }
    }"#;

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

    /// Optional `scraper_config` for every portal in the run, as raw JSON.
    ///
    /// Pages that expose no JSON-LD `JobPosting` need a selector config. Set it
    /// with `CAREER_OPS_LIVE_SCRAPER_CONFIG`; see `docs/manual-test-script.md`
    /// §8 for the Waymo config. A SmartRecruiters board gets a detail-only
    /// default when this is unset, because its list endpoint carries no text.
    fn live_scraper_config() -> Option<String> {
        std::env::var("CAREER_OPS_LIVE_SCRAPER_CONFIG")
            .ok()
            .filter(|raw| !raw.trim().is_empty())
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
        // Let the app's own subscriber carry the diagnostics; the guard prints
        // them if this test fails.
        let _logs = LogsOnFailure(career_ops_core::logging::handle());

        let _guard = KEYCHAIN_TEST_LOCK.lock().expect("keychain test lock");
        career_ops_test_support::install();

        let urls = live_portal_urls();
        let scraper_config = live_scraper_config();
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
                scraper_config: scraper_config.clone().or_else(|| {
                    // The SmartRecruiters list endpoint has no body, so without
                    // a detail block the live check would never exercise the
                    // description path. Two requests keep the default fast.
                    url.contains("smartrecruiters.com")
                        .then(|| r#"{"detail":{"max_jobs":2}}"#.to_string())
                }),
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
        eprintln!("live fetch: single-portal result {:?}", app.portals_message);

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

        // When the config asks for detail enrichment, at least one stored
        // description must be longer than a card teaser. The threshold is
        // deliberately low: this checks that the detail path ran, not that a
        // particular site's posting is long.
        let longest = stored
            .iter()
            .map(|job| job.raw_text.len())
            .max()
            .unwrap_or(0);
        let detail_requested = scraper_config
            .as_deref()
            .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
            .is_some_and(|config| config.get("detail").is_some());
        if detail_requested {
            assert!(
                longest > 500,
                "detail enrichment produced no text (longest description: {longest} bytes)"
            );
        }

        eprintln!("live fetch: longest description {longest} bytes");

        // SmartRecruiters stores one row per posting, so every row must point
        // at its own page; the detail block (explicit or the default above) must
        // also produce text, which the list endpoint cannot supply.
        let smart_detail_requested = app.portals.rows.iter().any(|row| {
            row.url.contains("smartrecruiters.com")
                && row
                    .scraper_config
                    .as_deref()
                    .and_then(|raw| serde_json::from_str::<serde_json::Value>(raw).ok())
                    .is_some_and(|config| config.get("detail").is_some())
        });
        if smart_detail_requested {
            if let Some(portal) = app
                .portals
                .rows
                .iter()
                .find(|row| row.url.contains("smartrecruiters.com"))
            {
                let rows: Vec<_> = stored
                    .iter()
                    .filter(|job| job.company == portal.name)
                    .collect();
                assert!(!rows.is_empty(), "SmartRecruiters stored no jobs");
                let described = rows
                    .iter()
                    .filter(|job| !job.raw_text.trim().is_empty())
                    .count();
                assert!(
                    described > 0,
                    "SmartRecruiters stored no description after a detail request"
                );
                for job in &rows {
                    let posting_id = job.source_url.rsplit('/').next().unwrap_or_default();
                    assert!(
                        job.source_url
                            .starts_with("https://jobs.smartrecruiters.com/")
                            && !posting_id.is_empty()
                            && posting_id.chars().all(|c| c.is_ascii_digit()),
                        "SmartRecruiters row is not per-posting: {}",
                        job.source_url
                    );
                }
                eprintln!(
                    "live fetch: smartrecruiters descriptions {described}/{} (all URLs per-posting)",
                    rows.len()
                );
            }
        }

        // Eightfold's list has no description, so a portal using the provider
        // must have produced text by default, and its URLs must be per position.
        if let Some(portal) = app.portals.rows.iter().find(|row| {
            let Some(raw) = row.scraper_config.as_deref() else {
                return false;
            };
            let Ok(config) = serde_json::from_str::<serde_json::Value>(raw) else {
                return false;
            };
            config.get("provider").and_then(|value| value.as_str()) == Some("eightfold")
        }) {
            let rows: Vec<_> = stored
                .iter()
                .filter(|job| job.company == portal.name)
                .collect();
            assert!(!rows.is_empty(), "Eightfold stored no jobs");
            let described = rows
                .iter()
                .filter(|job| !job.raw_text.trim().is_empty())
                .count();
            assert!(
                described > 0,
                "Eightfold stored no description; the provider enriches by default"
            );
            for job in &rows {
                assert!(
                    job.source_url.starts_with("https://"),
                    "Eightfold row is not an absolute posting URL: {}",
                    job.source_url
                );
            }
            eprintln!(
                "live fetch: eightfold descriptions {described}/{} (all URLs per-posting)",
                rows.len()
            );
        }

        eprintln!(
            "live fetch: {} unique jobs stored across {} portals; Pipeline shows {} (first tier: {})",
            stored.len(),
            app.portals.rows.len(),
            app.pipeline.rows.len(),
            app.pipeline.rows[0].tier.label()
        );
    }

    /// GET a URL through the production client. `Some((success, status, bytes))`
    /// or `None` when the request itself failed. Used to tell a WAF challenge
    /// from an extraction regression in the live tests.
    fn live_probe(app: &CareerOpsApp, url: &str) -> Option<(bool, u16, usize)> {
        let client = http::default_client().expect("http client");
        app.runtime().block_on(async {
            match http::get_with_retry(&client, url).await {
                Ok(response) => {
                    let status = response.status();
                    let bytes = response.text().await.map(|body| body.len()).unwrap_or(0);
                    Some((status.is_success(), status.as_u16(), bytes))
                }
                Err(error) => {
                    eprintln!("live probe failed for {url}: {error}");
                    None
                }
            }
        })
    }

    /// A challenge or a transport failure is a site condition, not a code
    /// failure. An empty body counts as a challenge because the WAF sometimes
    /// answers `200` with nothing.
    fn live_probe_challenged(probe: Option<(bool, u16, usize)>) -> bool {
        match probe {
            None => true,
            Some((success, status, bytes)) => !success || status == 202 || bytes == 0,
        }
    }

    /// What a registry run did with one entry.
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum RegistryStep {
        /// The page did not answer usefully, so the entry was never fetched.
        /// Carries the probe so the run can report the status and size.
        SkipBeforeFetch(Option<(bool, u16, usize)>),
        /// The entry fetched successfully; only storage can confirm the rest.
        Verified,
        /// The fetch failed and a retry probe was challenged: a site
        /// condition, not a regression.
        SkipAfterFetch(Option<(bool, u16, usize)>),
        /// The page answered, yet the fetch failed. That is our problem.
        Broken,
    }

    /// Drive one registry entry: probe, fetch only a served page, and re-probe
    /// only to explain a failed fetch.
    ///
    /// Generic over the context so the sequencing is testable offline: the
    /// live test passes the app, the tests below pass a call recorder. The
    /// live test cannot run in CI, and the skip rules are the part most likely
    /// to drift silently, so they are proven without a network.
    fn registry_entry<C>(
        ctx: &mut C,
        probe: impl Fn(&mut C) -> Option<(bool, u16, usize)>,
        fetch: impl FnOnce(&mut C) -> bool,
    ) -> RegistryStep {
        let first = probe(&mut *ctx);
        if live_probe_challenged(first) {
            return RegistryStep::SkipBeforeFetch(first);
        }

        if fetch(&mut *ctx) {
            return RegistryStep::Verified;
        }

        let retry = probe(&mut *ctx);
        if live_probe_challenged(retry) {
            RegistryStep::SkipAfterFetch(retry)
        } else {
            RegistryStep::Broken
        }
    }

    #[test]
    fn a_challenged_registry_page_is_skipped_without_fetching() {
        // Every shape of "not a usable page": transport failure, server error,
        // the WAF's `202`, and a `200` with an empty body.
        for probe in [
            None,
            Some((false, 503, 40)),
            Some((true, 202, 0)),
            Some((true, 200, 0)),
        ] {
            let mut calls: Vec<&str> = Vec::new();
            let step = registry_entry(
                &mut calls,
                |_| probe,
                |calls| {
                    calls.push("fetch");
                    true
                },
            );

            assert_eq!(step, RegistryStep::SkipBeforeFetch(probe), "{probe:?}");
            assert!(
                calls.is_empty(),
                "a challenged page must not be fetched: {probe:?}"
            );
        }
    }

    #[test]
    fn a_served_registry_page_is_fetched_once_and_not_probed_again() {
        let mut calls: Vec<&str> = Vec::new();
        let step = registry_entry(
            &mut calls,
            |calls| {
                calls.push("probe");
                Some((true, 200, 4_096))
            },
            |calls| {
                calls.push("fetch");
                true
            },
        );

        assert_eq!(step, RegistryStep::Verified);
        assert_eq!(
            calls,
            vec!["probe", "fetch"],
            "a healthy fetch must not spend a second request on a probe"
        );
    }

    #[test]
    fn a_failed_registry_fetch_on_a_served_page_is_a_failure() {
        let mut calls: Vec<&str> = Vec::new();
        let step = registry_entry(
            &mut calls,
            |calls| {
                calls.push("probe");
                Some((true, 200, 4_096))
            },
            |calls| {
                calls.push("fetch");
                false
            },
        );

        assert_eq!(
            step,
            RegistryStep::Broken,
            "the page is served, so the failure is ours, not the site's"
        );
        assert_eq!(calls, vec!["probe", "fetch", "probe"]);
    }

    #[test]
    fn a_failed_registry_fetch_on_a_challenged_retry_is_skipped() {
        let mut calls: Vec<&str> = Vec::new();
        let step = registry_entry(
            &mut calls,
            |calls| {
                calls.push("probe");
                // Served first, challenged when asked again.
                if calls.len() == 1 {
                    Some((true, 200, 4_096))
                } else {
                    Some((true, 202, 0))
                }
            },
            |calls| {
                calls.push("fetch");
                false
            },
        );

        assert_eq!(step, RegistryStep::SkipAfterFetch(Some((true, 202, 0))));
        assert_eq!(calls, vec!["probe", "fetch", "probe"]);
    }

    #[test]
    #[ignore = "hits the network; run with --ignored"]
    fn live_fetch_with_pagination_and_details() {
        // Same guard as the other live test: the captured log is printed only
        // if something fails.
        let _logs = LogsOnFailure(career_ops_core::logging::handle());

        let _guard = KEYCHAIN_TEST_LOCK.lock().expect("keychain test lock");
        career_ops_test_support::install();

        let url = std::env::var("CAREER_OPS_LIVE_PAGED_PORTAL")
            .unwrap_or_else(|_| "https://careers.withwaymo.com/jobs/search".to_string());
        let config = std::env::var("CAREER_OPS_LIVE_SCRAPER_CONFIG")
            .ok()
            .filter(|raw| !raw.trim().is_empty())
            .unwrap_or_else(|| WAYMO_CONFIG.to_string());

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

        let name = live_portal_name(&url);

        // Waymo's WAF answers repeated traffic with `202` and an empty body.
        // That is a site condition, not a code failure: report it and stop,
        // because a red live run cannot distinguish it from a regression.
        // Other hosts are not probed.
        if url.contains("careers.withwaymo.com") {
            let probe = live_probe(&app, &url);
            if live_probe_challenged(probe) {
                eprintln!(
                    "live pagination: skipped, Waymo is challenging automated requests \
                     ({probe:?}). Retry later; see docs/manual-test-script.md §8."
                );
                return;
            }
        }

        app.handle_portals_action(PortalsAction::CreatePortal(PortalFormData {
            company_name: name.clone(),
            url: url.clone(),
            render_mode: "static".into(),
            scraper_config: Some(config.clone()),
        }));
        wait_until(&mut app, &ctx, |app| {
            app.portals.rows.len() == 1 && app.portals_task.is_none()
        });

        let started = Instant::now();
        app.handle_portals_action(PortalsAction::FetchOne(app.portals.rows[0].id));
        wait_until_within(&mut app, &ctx, Duration::from_secs(180), |app| {
            app.portals_task.is_none()
        });
        let elapsed = started.elapsed();

        let portal = &app.portals.rows[0];
        assert_eq!(
            portal.health,
            HealthBadge::Healthy,
            "{} was not fetched (hint: {:?}). A WAF challenge is a retry, not a code failure.",
            portal.url,
            portal.suggested_action
        );
        eprintln!("live pagination: {:?} in {elapsed:?}", app.portals_message);

        let pool = app.pool().expect("pool");
        let stored = app
            .runtime()
            .block_on(job_repo::list_recent(&pool, 10_000))
            .expect("list stored jobs");
        let jobs: Vec<_> = stored.iter().filter(|job| job.company == name).collect();

        // One Waymo page is 30 cards. If only one page arrived, probe page 2:
        // a served page with no extracted jobs is a pagination regression, a
        // challenge is the documented WAF limit.
        if jobs.len() <= 30 {
            let page_two = serde_json::from_str::<serde_json::Value>(&config)
                .ok()
                .and_then(|config| {
                    config
                        .pointer("/pagination/url_template")
                        .and_then(|value| value.as_str())
                        .map(|template| template.replace("{page}", "2"))
                });

            match page_two {
                Some(page_two) => {
                    let probe = live_probe(&app, &page_two);
                    if live_probe_challenged(probe) {
                        eprintln!(
                            "live pagination: skipped, page 2 is WAF-challenged ({probe:?}); \
                             page 1 stored {} jobs",
                            jobs.len()
                        );
                        return;
                    }
                    panic!(
                        "pagination stored {} jobs although page 2 was served ({probe:?})",
                        jobs.len()
                    );
                }
                None => panic!(
                    "pagination stored {} jobs; one page is 30 and the config has no url_template",
                    jobs.len()
                ),
            }
        }

        let longest = jobs.iter().map(|job| job.raw_text.len()).max().unwrap_or(0);
        let described = jobs.iter().filter(|job| job.raw_text.len() > 500).count();

        if described >= 1 {
            eprintln!(
                "live pagination: {} jobs, {described} with a description, longest {longest} bytes, fetched in {elapsed:?}",
                jobs.len()
            );
            return;
        }

        // No posting text. Either the WAF answered the detail request with a
        // challenge (operational, documented in the manual script) or extraction
        // broke. Probe one posting directly: a real document plus no extracted
        // text is a code failure and must fail the test.
        let probe_url = jobs[0].source_url.clone();
        let probe = live_probe(&app, probe_url.as_str());
        assert!(
            live_probe_challenged(probe),
            "the detail page was served ({probe:?}) but no description was extracted from \
             {probe_url}"
        );
        eprintln!(
            "live pagination: {} jobs, details WAF-challenged ({probe:?}), description \
             assertion skipped, fetched in {elapsed:?}",
            jobs.len()
        );
    }

    /// The registry, live: every `KnownSite` must fetch and store at least one
    /// posting with text. That is what "known" means — an entry ships only
    /// after this passes.
    ///
    /// It is a sibling of `live_fetch_reaches_the_pipeline` rather than a mode
    /// inside it because a WAF challenge must skip *one entry*, not abort the
    /// run, and that test is fail-fast over a single URL list. The environment
    /// variable is a second gate behind `#[ignore]`, so a broad `--ignored` run
    /// does not spend minutes on the network by accident.
    ///
    /// ```text
    /// CAREER_OPS_LIVE_REGISTRY=1 \
    ///     cargo test -p career-ops-app --features keychain -- --ignored \
    ///     live_registry_sites_reach_the_pipeline
    /// ```
    ///
    /// After a passing run, commit `verified_at` for the entries it verified in
    /// `crates/core/src/acquisition/known_sites.rs` and reflect them in
    /// `docs/known-sites.md`. The test never writes either file.
    #[test]
    #[ignore = "hits the network; run with --ignored and CAREER_OPS_LIVE_REGISTRY=1"]
    fn live_registry_sites_reach_the_pipeline() {
        if std::env::var("CAREER_OPS_LIVE_REGISTRY").as_deref() != Ok("1") {
            eprintln!("live registry: skipped, set CAREER_OPS_LIVE_REGISTRY=1 to run");
            return;
        }

        // Same guard as the other live tests: the captured log is printed only
        // if something fails.
        let _logs = LogsOnFailure(career_ops_core::logging::handle());

        let _guard = KEYCHAIN_TEST_LOCK.lock().expect("keychain test lock");
        career_ops_test_support::install();

        let sites = known_sites::all();
        assert!(!sites.is_empty(), "the known-site registry is empty");

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
        // The production HTTP source: the registry is a claim about the real
        // sites, so a stubbed transport would make the test meaningless.
        let mut app = CareerOpsApp::new(state, runtime);
        let ctx = egui::Context::default();

        app.handle_lock_action(LockAction::TryPassphrase(TEST_PASSPHRASE.to_string()));
        wait_until(&mut app, &ctx, |app| {
            app.state()
                .is_some_and(|state| state.current() == &Screen::Ready)
        });

        let mut verified: Vec<&str> = Vec::new();
        let mut skipped: Vec<&str> = Vec::new();

        for site in sites {
            let url = site.url();
            let expected_rows = app.portals.rows.len() + 1;
            let mut elapsed = Duration::ZERO;

            // The same sequencer the offline tests exercise: probe first, fetch
            // only a served page, and re-probe only to explain a failure.
            let step = registry_entry(
                &mut app,
                |app| live_probe(app, &url),
                |app| {
                    app.handle_portals_action(PortalsAction::CreatePortal(PortalFormData {
                        company_name: site.name.to_string(),
                        url: url.clone(),
                        render_mode: "static".into(),
                        scraper_config: Some(site.config.to_string()),
                    }));
                    wait_until_within(app, &ctx, Duration::from_secs(30), |app| {
                        app.portals.rows.len() == expected_rows && app.portals_task.is_none()
                    });

                    let portal_id = app
                        .portals
                        .rows
                        .iter()
                        .find(|row| row.name == site.name)
                        .unwrap_or_else(|| panic!("{} was not created", site.name))
                        .id;

                    let started = Instant::now();
                    app.handle_portals_action(PortalsAction::FetchOne(portal_id));
                    wait_until_within(app, &ctx, Duration::from_secs(180), |app| {
                        app.portals_task.is_none()
                    });
                    elapsed = started.elapsed();

                    app.portals
                        .rows
                        .iter()
                        .find(|row| row.name == site.name)
                        .is_some_and(|row| row.health == HealthBadge::Healthy)
                },
            );

            match step {
                RegistryStep::SkipBeforeFetch(probe) => {
                    eprintln!(
                        "live registry: {} skipped, the page is challenged or unreachable \
                         ({probe:?})",
                        site.name
                    );
                    skipped.push(site.name);
                }
                RegistryStep::SkipAfterFetch(probe) => {
                    eprintln!(
                        "live registry: {} skipped, the fetch was challenged ({probe:?})",
                        site.name
                    );
                    skipped.push(site.name);
                }
                RegistryStep::Broken => {
                    let hint = app
                        .portals
                        .rows
                        .iter()
                        .find(|row| row.name == site.name)
                        .and_then(|row| row.suggested_action.clone());
                    panic!(
                        "{} was served but the fetch failed (hint: {hint:?}); a challenge is a \
                         skip, this is a regression",
                        site.name
                    );
                }
                RegistryStep::Verified => {
                    eprintln!(
                        "live registry: {} healthy in {elapsed:?}: {:?}",
                        site.name, app.portals_message
                    );
                    verified.push(site.name);
                }
            }
        }

        let pool = app.pool().expect("pool");
        let stored = app
            .runtime()
            .block_on(job_repo::list_recent(&pool, 10_000))
            .expect("list stored jobs");

        for name in &verified {
            let jobs: Vec<_> = stored.iter().filter(|job| job.company == *name).collect();
            assert!(
                !jobs.is_empty(),
                "{name} reported a healthy fetch but stored no jobs"
            );
            let with_text = jobs
                .iter()
                .filter(|job| !job.raw_text.trim().is_empty())
                .count();
            assert!(
                with_text > 0,
                "{name} stored {} jobs but none with text; Phase 3 could not evaluate them",
                jobs.len()
            );
            for job in &jobs {
                assert!(
                    job.source_url.starts_with("https://"),
                    "{name} stored a row that is not an absolute posting URL: {}",
                    job.source_url
                );
            }
            let longest = jobs.iter().map(|job| job.raw_text.len()).max().unwrap_or(0);
            eprintln!(
                "live registry: {name} stored {} jobs, {with_text} with text, longest {longest} bytes",
                jobs.len()
            );
        }

        eprintln!(
            "live registry: verified {verified:?}; skipped {skipped:?}. Commit verified_at for \
             the verified entries only."
        );
        if verified.is_empty() {
            eprintln!(
                "live registry: nothing could be verified on this run; no entry may be marked \
                 verified from it"
            );
        }
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
