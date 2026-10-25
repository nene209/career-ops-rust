//! View-models. Pure Rust, no GPUI dependency.
//!
//! The view-model layer is where correctness lives. Each view-model is
//! constructed from a service-layer type and updated by service-layer
//! results. It has unit tests that exercise the data-shaping logic.
//!
//! The view layer (in `crate::screens`) renders these view-models. The
//! view code is replaceable; the view-model code is the application.
//!
//! Modules are added as each screen is implemented. This file starts
//! empty because Phase 2.1 Workstream 1 is shell infrastructure; the
//! first view-model (`portals`) arrives in Workstream 3.

// pub mod portals;       // Workstream 3
// pub mod pipeline;      // Workstream 4
