//! Evaluation (Phase 3): one job, one structured report.
//!
//! The pipeline normalizes the stored text, resolves a prompt for use, renders
//! it strictly, asks the model for one JSON object, validates it, and stores it
//! with its provenance in one transaction. Streaming is optional and only
//! changes how the reply reaches the screen, never what is stored.

mod output;
mod service;

pub use crate::llm::CompletionBackend;
pub use output::{parse, EvaluationOutput, Recommendation};
pub use service::{
    run_batch, run_evaluation, EvaluationRequest, EvaluationResult, DEFAULT_PROMPT, MAX_BATCH,
    MAX_CV_CHARS,
};
