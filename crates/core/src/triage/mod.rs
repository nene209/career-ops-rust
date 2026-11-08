//! Triage (Phase 3.2): one advisory batch over freshly fetched postings.
//!
//! WS1 owns the prompt and the structured output. The service and the review UI
//! arrive in later workstreams, so this module deliberately has no database or
//! provider dependency yet.
//!
//! The contract is small: one JSON object with exactly one decision per posting
//! id, each decision one of four values and each carrying a one-sentence
//! reason. The decision is advisory; nothing here changes a row.

mod output;
mod prompt;

pub use output::{parse, TriageChoice, TriageDecision, TriageOutput};
pub use prompt::{
    assemble, render_jobs, render_preferences, TriageJob, BRIEF_CHARS, PROMPT_CATEGORY, PROMPT_NAME,
};
