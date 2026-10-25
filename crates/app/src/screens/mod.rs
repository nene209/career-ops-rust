//! egui view layer.
//!
//! Screens are thin: they render state produced by `crate::state` and
//! `crate::view_models`, and they emit events back to `CareerOpsApp`. Correct
//! behaviour belongs in the state machine or a view-model so it can be tested
//! without egui.
//!
//! This module starts with the Workstream 1 shell and its shared components.
//! Workstreams 2–4 add `lock` content and the Portals/Pipeline screens behind
//! the same shell.

pub mod components;
pub mod lock;
pub mod shell;

pub use components::{labelled_input, EmptyState, LoadingSpinner, StatusBadge};
