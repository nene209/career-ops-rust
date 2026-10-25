//! Career-Ops core library.
//!
//! This crate implements the Phase 1 foundation: keystore, encrypted database,
//! domain models, repositories, and the browser/PDF backend traits.
//!
//! It has no GPUI dependency and is testable headlessly.

pub mod acquisition;
pub mod backend;
pub mod db;
pub mod error;
pub mod keystore;
pub mod model;
pub mod portals;
pub mod prompts_seed;
pub mod repo;

pub use error::{CoreError, Result};

/// Application-wide data directory resolver.
///
/// Phase 1 uses a single directory for the encrypted database and the
/// `dek.wrapped.json` file. The OS-specific app data directory is the
/// default; `CAREER_OPS_DATA_DIR` remains as an override for development
/// and tests. Tests that need an isolated directory pass one explicitly to
/// `AppState::bootstrap` rather than relying on this function.
pub fn data_dir() -> std::path::PathBuf {
    if let Some(dir) = std::env::var_os("CAREER_OPS_DATA_DIR") {
        return std::path::PathBuf::from(dir);
    }

    directories::ProjectDirs::from("", "", "career-ops")
        .map(|dirs| dirs.data_dir().to_path_buf())
        .unwrap_or_else(|| std::path::PathBuf::from("."))
}
