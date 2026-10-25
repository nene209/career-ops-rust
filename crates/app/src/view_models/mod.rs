//! View-models. Pure Rust, no egui dependency.
//!
//! The view-model layer is where correctness lives. Each view-model is
//! constructed from service-layer types and updated by service-layer
//! results. It has unit tests that exercise the data-shaping logic.
//!
//! The view layer (in `crate::screens`) renders these view-models. The view
//! code is replaceable; the view-model code is the application.

pub mod pipeline;
pub mod portals;
