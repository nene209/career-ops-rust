// crates/app/src/lib.rs

//! Career-Ops desktop application library target.
//!
//! The binary target (`src/main.rs`) depends on this library. The library
//! exists so integration tests can exercise the state machine without
//! spawning the full GUI.

pub mod screens;
pub mod state;