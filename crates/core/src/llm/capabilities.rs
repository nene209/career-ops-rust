//! What a model can do, consulted before a call is built.
//!
//! The registry is ours, not the provider's: providers change capabilities
//! between model names and occasionally within one, so a table with an
//! explicit unknown-model fallback is more predictable than asking the wire
//! and hoping. An unknown model gets the provider's conservative defaults, and
//! the UI shows what was assumed.
//!
//! The answer depends on the configuration, not only on the provider and
//! model: a custom endpoint's JSON mode and context window are settings the
//! user can change, so [`LlmConfig::capabilities`] is the entry point.

use super::config::{LlmConfig, LlmProvider};

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
    /// No provider-enforced JSON mode: the prompt asks for JSON and the reply
    /// is parsed and validated locally, with one repair call on a bad parse.
    ///
    /// A local model may reject `response_format`, so the request must not
    /// carry it. This is still usable for an evaluation — the prompt already
    /// states the JSON contract — which is why it is not [`Self::Unavailable`].
    PromptOnly,
    /// No structured output: the caller must not build an evaluation on it.
    Unavailable,
}

impl StructuredOutput {
    pub fn label(self) -> &'static str {
        match self {
            Self::Schema => "JSON schema",
            Self::JsonObject => "JSON mode, validated locally",
            Self::PromptOnly => "Asked in the prompt, validated locally",
            Self::Unavailable => "Not available",
        }
    }

    /// Whether an evaluation can rely on this mode.
    pub fn is_usable(self) -> bool {
        self != Self::Unavailable
    }

    /// Whether the request should carry `response_format: json_object`.
    ///
    /// Only [`Self::JsonObject`]: a prompt-only endpoint would reject it, and
    /// a schema-enforcing one needs a different body than this app sends.
    pub fn sends_json_object(self) -> bool {
        self == Self::JsonObject
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

/// The capabilities of the configured provider and model.
///
/// Unknown models fall back to the provider's defaults rather than to
/// `UNSUPPORTED`: a renamed DeepSeek chat model still streams, still takes
/// tools, and still accepts JSON mode. Only a provider this build cannot
/// reach is wholly unavailable.
impl LlmConfig {
    pub fn capabilities(&self) -> ModelCapabilities {
        match self.provider {
            LlmProvider::DeepSeek => deepseek(&self.model),
            LlmProvider::Custom => custom(self.json_mode, self.custom_context_window),
            // Stored but not constructible in this build; see
            // `LlmProvider::is_supported`.
            LlmProvider::OpenAI | LlmProvider::Anthropic | LlmProvider::Gemini => {
                ModelCapabilities::UNSUPPORTED
            }
        }
    }
}

/// A custom endpoint answers from its own settings.
///
/// Nothing here is known about the model itself, so the claim is only what the
/// user configured: JSON mode on or off, and a window if they named one. Tools
/// stay off — the pipelines do not use them, and a local server's support is
/// unknown.
fn custom(json_mode: bool, context_window: Option<usize>) -> ModelCapabilities {
    ModelCapabilities {
        structured_output: if json_mode {
            StructuredOutput::JsonObject
        } else {
            StructuredOutput::PromptOnly
        },
        streaming: true,
        tools: false,
        context_window,
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

    fn config(provider: LlmProvider, model: &str) -> LlmConfig {
        LlmConfig {
            provider,
            model: model.into(),
            ..LlmConfig::default()
        }
    }

    #[test]
    fn a_deepseek_chat_model_takes_json_mode_and_tools() {
        let found = config(LlmProvider::DeepSeek, "deepseek-chat").capabilities();
        assert_eq!(found.structured_output, StructuredOutput::JsonObject);
        assert!(found.streaming);
        assert!(found.tools);
        assert_eq!(found.context_window, Some(65_536));
    }

    #[test]
    fn a_reasoning_model_is_not_used_for_structured_output() {
        let found = config(LlmProvider::DeepSeek, "deepseek-reasoner").capabilities();
        assert_eq!(found.structured_output, StructuredOutput::Unavailable);
        assert!(found.streaming, "reasoning models stream");
        assert!(!found.tools);
        assert!(!found.structured_output.is_usable());
    }

    #[test]
    fn the_default_model_takes_json_mode_and_tools() {
        let found = config(LlmProvider::DeepSeek, "deepseek-flash").capabilities();
        assert_eq!(found.structured_output, StructuredOutput::JsonObject);
        assert!(found.streaming);
        assert!(found.tools);
        assert_eq!(found.context_window, Some(65_536));
    }

    #[test]
    fn an_unknown_model_gets_the_provider_defaults() {
        let found = config(LlmProvider::DeepSeek, "deepseek-v9-unlisted").capabilities();
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
            let found = config(provider, "anything").capabilities();
            assert_eq!(found, ModelCapabilities::UNSUPPORTED, "{provider:?}");
        }
    }

    #[test]
    fn a_custom_endpoint_answers_from_its_own_settings() {
        let mut endpoint = config(LlmProvider::Custom, "llama3.2");
        endpoint.json_mode = true;
        endpoint.custom_context_window = Some(32_768);

        let found = endpoint.capabilities();
        assert_eq!(found.structured_output, StructuredOutput::JsonObject);
        assert!(found.streaming);
        assert!(!found.tools, "local tool support is not assumed");
        assert_eq!(found.context_window, Some(32_768));
    }

    #[test]
    fn turning_json_mode_off_still_leaves_an_evaluation_possible() {
        let mut endpoint = config(LlmProvider::Custom, "llama3.2");
        endpoint.json_mode = false;

        let found = endpoint.capabilities();
        assert_eq!(found.structured_output, StructuredOutput::PromptOnly);
        assert!(
            found.structured_output.is_usable(),
            "the prompt carries the JSON contract"
        );
        assert!(
            !found.structured_output.sends_json_object(),
            "the request must not carry response_format"
        );
    }

    #[test]
    fn an_unknown_custom_window_disables_the_pre_flight_check() {
        let mut endpoint = config(LlmProvider::Custom, "llama3.2");
        endpoint.custom_context_window = None;

        assert_eq!(endpoint.capabilities().context_window, None);
    }

    #[test]
    fn only_json_mode_sends_response_format() {
        assert!(StructuredOutput::JsonObject.sends_json_object());
        assert!(!StructuredOutput::PromptOnly.sends_json_object());
        assert!(!StructuredOutput::Schema.sends_json_object());
        assert!(!StructuredOutput::Unavailable.sends_json_object());
    }
}
