//! What a caller needs from a model: one completion.
//!
//! The trait exists so the evaluation and cover-letter pipelines can be tested
//! without a network or a cassette: a fake returns canned text and records the
//! prompts it was given. [`LlmClient`] is the production implementation, and
//! the only one that knows about `rig`.
//!
//! Usage travels with the text rather than beside it. A side channel would let
//! a caller forget to read it, and a fake could omit it without the compiler
//! noticing; returning both makes the ledger's input part of the contract.

use async_trait::async_trait;

use super::usage::Usage;
use super::{DeltaSink, LlmClient, LlmError};

/// A model reply and what it used.
#[derive(Debug, Clone, PartialEq)]
pub struct Completion {
    pub text: String,
    pub usage: Usage,
}

impl Completion {
    /// A reply with no usage. Fakes and cassettes use this; the pipeline then
    /// estimates the counters and records them as such.
    pub fn text_only(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            usage: Usage::unknown(),
        }
    }
}

#[async_trait]
pub trait CompletionBackend: Send + Sync {
    /// Send `prompt` and return the model's text and usage.
    ///
    /// `model` overrides the client's configured model for this call.
    /// `json_mode` asks the provider to constrain the reply to a JSON object;
    /// the caller still parses and validates it. With `deltas`, the same text
    /// is also sent fragment by fragment for display; the return value stays
    /// authoritative.
    async fn complete(
        &self,
        prompt: &str,
        model: Option<&str>,
        json_mode: bool,
        deltas: Option<&DeltaSink>,
    ) -> std::result::Result<Completion, LlmError>;
}

#[async_trait]
impl CompletionBackend for LlmClient {
    async fn complete(
        &self,
        prompt: &str,
        model: Option<&str>,
        json_mode: bool,
        deltas: Option<&DeltaSink>,
    ) -> std::result::Result<Completion, LlmError> {
        self.complete_text(prompt, model, json_mode, deltas).await
    }
}
