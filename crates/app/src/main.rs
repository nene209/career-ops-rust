//! Career-Ops desktop application — Phase 1 skeleton.
//!
//! The state machine (`state.rs`) is fully implemented and testable without
//! GPUI. The GPUI view layer (`screens.rs`) is a placeholder. When GPUI's
//! API stabilises, the views will be filled in against the same state.

use career_ops_app::state::AppState;

use anyhow::Result;
use tracing_subscriber::EnvFilter;

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env())
        .init();

    let data_dir = career_ops_core::data_dir();

    // Phase 1: construct the state machine. The GPUI event loop is where the
    // view rendering would attach — see `screens.rs`.
    let state = AppState::bootstrap(data_dir)?;

    // In the finished application, this would be:
    //     gpui::Application::new().run(move |cx| {
    //         cx.open_window(Default::default(), |_window, cx| {
    //             cx.new(|_cx| screens::RootView::new(state))
    //         });
    //     });
    //
    // Left commented out because GPUI's API is pre-1.0 and the exact call
    // signature is unstable.
    tracing::info!(state = ?state.current(), "app state initialized");

    Ok(())
}
