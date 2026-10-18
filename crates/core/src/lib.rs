//! Career-Ops core library.
//!
//! This crate implements the Phase 1 foundation: keystore, encrypted database,
//! domain models, repositories, and the browser/PDF backend traits.
//!
//! It has no GPUI dependency and is testable headlessly.

pub mod backend;
pub mod db;
pub mod error;
pub mod keystore;
pub mod model;
pub mod prompts_seed;
pub mod repo;

pub use error::{CoreError, Result};

/// Application-wide data directory resolver.
///
/// Phase 1 uses a single directory for the encrypted database and the
/// `dek.wrapped.json` file. In production this would resolve to the OS-specific
/// app data directory; tests pass an explicit temporary directory.
pub fn data_dir() -> std::path::PathBuf {
    // Real implementation: use the `directories` crate or similar.
    // Left as a placeholder so Phase 1 does not add another dependency.
    std::env::var_os("CAREER_OPS_DATA_DIR")
        .map(std::path::PathBuf::from)
        .expect("CAREER_OPS_DATA_DIR must be set (Phase 1 placeholder)")
}
