//! Cover letters (Phase 3): one job, one markdown letter.
//!
//! The pipeline is the evaluation's sibling and shares its primitives: the same
//! normalization, the same strict rendering, the same CV budget, and the same
//! context-window refusal. The differences are deliberate: the reply is free
//! text rather than JSON, there is no status change, and regeneration adds a
//! row instead of replacing one.

mod service;

pub use service::{run_cover_letter, CoverLetterRequest, CoverLetterResult, DEFAULT_PROMPT};
