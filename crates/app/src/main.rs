//! Career-Ops desktop application entry point.
//!
//! Phase 2.1 Workstream 1 replaces the Phase 1 state-bootstrap placeholder
//! with the `eframe::run_native` entry point. The framework-independent
//! pieces — `AppState`, `NavigationState`, `AsyncTask` — live in the library
//! and are tested without a window.

use anyhow::Result;
use career_ops_app::app::CareerOpsApp;
use career_ops_app::state::AppState;
use tracing_subscriber::EnvFilter;

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let state = AppState::bootstrap(career_ops_core::data_dir())?;

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
