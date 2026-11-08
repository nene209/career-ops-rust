//! What a model can do, consulted before a call is built.
//!
//! The registry is ours, not the provider's: providers change capabilities
//! between model names and occasionally within one, so a table with an
//! explicit unknown-model fallback is more predictable than asking the wire
//! and hoping. An unknown model gets the provider's conservative defaults, and
//! the UI shows what was assumed.

use super::config::LlmProvider;

/// How a model can be made to return structured data.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StructuredOutput {
    /// The provider enforces a JSON schema (OpenAI-style strict tools or
    /// `json_schema`).
    Schema,
    /// JSON mode, with the schema validated on our side.
    ///
    /// DeepSeek's dialect accepts `response_format: json_object` but not a
    /// schema, so the reply is checked against [`EvaluationOutput`] after
    /// parsing and retried once. Recording this as a distinct value keeps the
    /// stronger promise from being claimed.
    ///
    /// [`EvaluationOutput`]: crate::llm
    JsonObject,
    /// No structured output: the caller must not build an evaluation on it.
    Unavailable,
}

impl StructuredOutput {
    pub fn label(self) -> &'static str {
        match self {
            Self::Schema => "JSON schema",
            Self::JsonObject => "JSON mode, validated locally",
            Self::Unavailable => "Not available",
        }
    }

    /// Whether an evaluation can rely on this mode.
    pub fn is_usable(self) -> bool {
        self != Self::Unavailable
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ModelCapabilities {
    pub structured_output: StructuredOutput,
    pub streaming: bool,
    pub tools: bool,
    /// The published context window, in tokens, when it is known. `None`
    /// means "do not enforce a token count"; the character budget still
    /// applies.
    pub context_window: Option<usize>,
}

impl ModelCapabilities {
    const UNSUPPORTED: Self = Self {
        structured_output: StructuredOutput::Unavailable,
        streaming: false,
        tools: false,
        context_window: None,
    };
}

/// The capabilities of `model` on `provider`.
///
/// Unknown models fall back to the provider's defaults rather than to
/// `UNSUPPORTED`: a renamed DeepSeek chat model still streams, still takes
/// tools, and still accepts JSON mode. Only a provider this build cannot
/// reach is wholly unavailable.
pub fn capabilities(provider: LlmProvider, model: &str) -> ModelCapabilities {
    match provider {
        LlmProvider::DeepSeek => deepseek(model),
        // Stored but not constructible in this build; see
        // `LlmProvider::is_supported`.
        LlmProvider::OpenAI | LlmProvider::Anthropic | LlmProvider::Gemini => {
            ModelCapabilities::UNSUPPORTED
        }
    }
}

fn deepseek(model: &str) -> ModelCapabilities {
    let model = model.trim().to_ascii_lowercase();

    // The reasoning models take `reasoning_content` back on every assistant
    // turn and, as of the V3 generation, did not offer function calling or
    // JSON mode. Erring unavailable means WS3 refuses rather than sending a
    // request the model cannot answer.
    //
    // `deepseek-flash` is the default since Phase 3.1 and is a reasoning
    // model too, but its catalog entry lists structured output, so it takes
    // the chat path below. `deepseek-chat` is the alias earlier builds
    // stored; both are named here so a future rename is a deliberate change.
    if model.contains("reasoner") {
        return ModelCapabilities {
            structured_output: StructuredOutput::Unavailable,
            streaming: true,
            tools: false,
            context_window: Some(65_536),
        };
    }

    ModelCapabilities {
        structured_output: StructuredOutput::JsonObject,
        streaming: true,
        tools: true,
        // DeepSeek publishes 64K for the chat models; the catalog lists 1M
        // for Flash. Keep the smaller number: the enforced limit is the
        // character budget in `LlmConfig`, so understating the window only
        // refuses a prompt the app never builds.
        context_window: Some(65_536),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_deepseek_chat_model_takes_json_mode_and_tools() {
        let found = capabilities(LlmProvider::DeepSeek, "deepseek-chat");
        assert_eq!(found.structured_output, StructuredOutput::JsonObject);
        assert!(found.streaming);
        assert!(found.tools);
        assert_eq!(found.context_window, Some(65_536));
    }

    #[test]
    fn a_reasoning_model_is_not_used_for_structured_output() {
        let found = capabilities(LlmProvider::DeepSeek, "deepseek-reasoner");
        assert_eq!(found.structured_output, StructuredOutput::Unavailable);
        assert!(found.streaming, "reasoning models stream");
        assert!(!found.tools);
        assert!(!found.structured_output.is_usable());
    }

    #[test]
    fn the_default_model_takes_json_mode_and_tools() {
        let found = capabilities(LlmProvider::DeepSeek, "deepseek-flash");
        assert_eq!(found.structured_output, StructuredOutput::JsonObject);
        assert!(found.streaming);
        assert!(found.tools);
        assert_eq!(found.context_window, Some(65_536));
    }

    #[test]
    fn an_unknown_model_gets_the_provider_defaults() {
        let found = capabilities(LlmProvider::DeepSeek, "deepseek-v9-unlisted");
        assert_eq!(found.structured_output, StructuredOutput::JsonObject);
        assert!(found.tools);
    }

    #[test]
    fn an_unsupported_provider_is_wholly_unavailable() {
        for provider in [
            LlmProvider::OpenAI,
            LlmProvider::Anthropic,
            LlmProvider::Gemini,
        ] {
            let found = capabilities(provider, "anything");
            assert_eq!(found, ModelCapabilities::UNSUPPORTED, "{provider:?}");
        }
    }
}
