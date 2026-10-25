//! Career-Ops desktop application — Phase 1 skeleton.
//!
//! This file bootstraps the application state and exits. The UI shell —
//! window, screens, navigation — is implemented in **Phase 2.1**. The
//! framework-independent pieces that Phase 2.1 will use (`AppState`,
//! `NavigationState`, `AsyncTask`, the view-models) already live in the
//! library and are tested.
//!
//! When Phase 2.1 lands, this file becomes the `eframe::run_native`
//! entry point for a `CareerOpsApp` that implements `eframe::App`.

use career_ops_app::state::AppState;

use anyhow::Result;
use tracing_subscriber::EnvFilter;

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let data_dir = career_ops_core::data_dir();
    let state = AppState::bootstrap(data_dir)?;

    tracing::info!(state = ?state.current(), "app state initialized");

    // Phase 2.1 attaches the egui/eframe event loop here. Until then,
    // the binary is a smoke test for the state machine: it exercises
    // bootstrap, reports the resulting screen, and exits.
    eprintln!(
        "Application state: {:?}. The UI shell is implemented in Phase 2.1.",
        state.current()
    );

    Ok(())
}
