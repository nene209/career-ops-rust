//! Triage (Phase 3.2): one advisory batch over freshly fetched postings.
//!
//! WS1 owns the prompt and the structured output; WS2 adds the service that
//! assembles one call, validates the answer, and applies the decisions to the
//! queue in one transaction. The review UI arrives in WS3.
//!
//! The decision is advisory: triage writes `queue_state` and `queue_reason`
//! (the same fields the user's own actions write) plus the `triage_*`
//! provenance columns. It never creates an application, fetches a description,
//! evaluates a posting, or deletes anything.

mod output;
mod prompt;
mod service;

pub use output::{parse, TriageChoice, TriageDecision, TriageOutput};
pub use prompt::{
    assemble, render_jobs, render_preferences, TriageJob, BRIEF_CHARS, PROMPT_CATEGORY, PROMPT_NAME,
};
pub use service::{
    estimate, run_triage, TriageError, TriageEstimate, TriageReport, TriageRequest,
    DEFAULT_BATCH_LIMIT, ESTIMATED_OUTPUT_TOKENS_PER_JOB, MAX_BATCH_LIMIT,
};
