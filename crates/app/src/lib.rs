//! Career-Ops desktop application library target.
//!
//! The binary target (`src/main.rs`) depends on this library. The
//! library exists so integration tests can exercise the state machine
//! and view-models without spawning the full GUI.

pub mod app;
pub mod async_bridge;
pub mod navigation;
pub mod recovery;
pub mod screens;
pub mod state;
pub mod view_models;
