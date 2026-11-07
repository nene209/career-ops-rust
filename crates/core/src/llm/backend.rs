//! What a caller needs from a model: one completion.
//!
//! The trait exists so the evaluation and cover-letter pipelines can be tested
//! without a network or a cassette: a fake returns canned text and records the
//! prompts it was given. [`LlmClient`] is the production implementation, and
//! the only one that knows about `rig`.

use async_trait::async_trait;

use super::{DeltaSink, LlmClient, LlmError};

#[async_trait]
pub trait CompletionBackend: Send + Sync {
    /// Send `prompt` and return the model's text.
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
    ) -> Result<String, LlmError>;
}

#[async_trait]
impl CompletionBackend for LlmClient {
    async fn complete(
        &self,
        prompt: &str,
        model: Option<&str>,
        json_mode: bool,
        deltas: Option<&DeltaSink>,
    ) -> Result<String, LlmError> {
        self.complete_text(prompt, model, json_mode, deltas).await
    }
}
