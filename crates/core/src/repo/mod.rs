//! Repository modules. One module per table.
//!
//! Phase 1 implements these in full:
//!   - profile       (singleton row)
//!   - cv_entries    (ordered list, profile-scoped)
//!   - job_descriptions (dedup-aware upsert)
//!   - applications  (status machine with human-in-the-loop invariant)
//!   - portals       (CRUD)
//!   - settings      (key/value JSON store)
//!   - prompts       (versioned)
//!   - templates     (versioned, same shape as prompts)

pub mod applications;
pub mod cv_entries;
pub mod job_descriptions;
pub mod portals;
pub mod profile;
pub mod prompts;
pub mod settings;
pub mod templates;

use sha2::{Digest, Sha256};

/// SHA-256 hex digest of content. Used by prompts and templates to identify
/// a specific version independently of the numeric `version` field.
pub fn content_hash(content: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(content.as_bytes());
    hex::encode(hasher.finalize())
}