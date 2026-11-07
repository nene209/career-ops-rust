//! Career-Ops desktop application entry point.
//!
//! Phase 2.1 Workstream 1 replaces the Phase 1 state-bootstrap placeholder
//! with the `eframe::run_native` entry point. The framework-independent
//! pieces — `AppState`, `NavigationState`, `AsyncTask` — live in the library
//! and are tested without a window.

use anyhow::Result;
use career_ops_app::app::CareerOpsApp;
use career_ops_app::state::AppState;
use career_ops_core::logging::{self, LogMode};

fn main() -> Result<()> {
    // The file sink and the in-process buffer, from one `tracing` seam. This
    // must run before anything else so the bootstrap is recorded too; stderr
    // alone was invisible for a windowed build.
    let logs = logging::init(LogMode::File);

    let data_dir = career_ops_core::data_dir();
    // One line that makes a bug report reproducible: which build, which data
    // directory, and where the log for it is.
    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        profile = if cfg!(debug_assertions) { "debug" } else { "release" },
        data_dir = %data_dir.display(),
        log = %logs
            .path()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| "<in-app buffer only>".to_string()),
        "career-ops starting"
    );

    let state = AppState::bootstrap(data_dir)?;

    // The runtime lives for the whole process. Screens spawn service-layer
    // work on it and poll the result from `eframe::App::ui`.
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
