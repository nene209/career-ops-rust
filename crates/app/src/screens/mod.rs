//! GPUI view layer. Everything in this module depends on `gpui`.
//!
//! Views are described and manually verified; their logic lives in
//! `crate::view_models` and `crate::navigation`, both of which are
//! testable without GPUI. See `crates/app/docs/patterns.md` for the
//! pattern contract that Phase 3, 4, and 5 add screens against.

// pub mod root;          // Step 2
// pub mod sidebar;       // Step 2