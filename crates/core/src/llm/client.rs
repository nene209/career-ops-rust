//! Building a provider client, verifying a key, and reporting readiness.
//!
//! This is the only module that names a `rig` type. Upgrading `rig` should
//! touch this file and [`super::error::LlmError::from_rig`], not the
//! application.
//!
//! DeepSeek is reached through its OpenAI-compatible dialect: the same client
//! type as OpenAI with a different base URL, auth header, and wire quirks
//! (which is why `rig` exposes it as `providers::deepseek`).

use futures_util::StreamExt;
use sqlx::SqlitePool;
use zeroize::Zeroizing;

use super::backend::Completion;
use super::capabilities::{ModelCapabilities, StructuredOutput};
use super::config::{LlmConfig, LlmProvider};
use super::error::LlmError;
use super::keys;
use super::usage::Usage;

/// Where a streamed reply's fragments go.
pub type DeltaSink = tokio::sync::mpsc::UnboundedSender<String>;

/// A built client. Holds the provider call and the chosen model, so callers
/// never have to thread both through.
#[derive(Debug)]
pub struct LlmClient {
    provider: LlmProvider,
    model: String,
    client: rig::providers::openai::OpenAI,
    temperature: f32,
    max_tokens: Option<u32>,
    /// Whether the model expects JSON mode. The evaluation pipeline asks for
    /// it; a chat completion without it does not.
    structured_output: StructuredOutput,
}

impl LlmConfig {
    /// Build a client for this configuration.
    ///
    /// A missing key is [`LlmError::MissingApiKey`] rather than an
    /// authentication failure later: the difference is between "configure
    /// this" and "something is wrong". A custom endpoint is the exception —
    /// a local server usually has no credential, so an empty key is expected
    /// there.
    ///
    /// The client is built from the sanitized form, so a value typed into the
    /// form and not yet saved cannot reach the wire unnormalized.
    pub fn build_client(&self, api_key: &Zeroizing<String>) -> Result<LlmClient, LlmError> {
        let config = self.clone().sanitized();
        let api_key = api_key.trim();

        if api_key.is_empty() && config.provider.requires_api_key() {
            return Err(LlmError::MissingApiKey {
                provider: config.provider,
            });
        }

        if !config.provider.is_supported() {
            return Err(LlmError::UnsupportedProvider {
                provider: config.provider,
            });
        }

        // A custom endpoint's URL is a user input, and an absent or invalid
        // one must not fall back to a provider's default host: that would
        // send the CV somewhere the user did not choose.
        config
            .validate_custom()
            .map_err(|problem| LlmError::InvalidConfiguration(problem.to_string()))?;

        // Construction is offline: `rig` builds the transport lazily, so this
        // does not touch the network.
        let client = match config.provider {
            LlmProvider::DeepSeek => rig::providers::openai::OpenAIConfig::with_key(
                &rig::providers::openai::wire::DEEPSEEK,
                api_key,
            )
            .client(),
            LlmProvider::Custom => rig::providers::openai::OpenAIConfig::with_key(
                &rig::providers::openai::wire::OPENAI,
                api_key,
            )
            .with_base_url(config.base_url.clone())
            // An empty key means "no Authorization header" rather than an
            // empty bearer token, which a local server may reject outright.
            .with_auth(rig::providers::openai::wire::Auth::OptionalBearer)
            // The OPENAI dialect defaults to the Responses API, which local
            // servers do not implement; every OpenAI-compatible server has
            // `/chat/completions`.
            .with_route(rig::providers::openai::wire::Route::Chat)
            .client(),
            other => {
                return Err(LlmError::UnsupportedProvider { provider: other });
            }
        };

        Ok(LlmClient {
            provider: config.provider,
            model: config.model.clone(),
            client,
            temperature: config.temperature,
            max_tokens: config.max_tokens,
            structured_output: config.capabilities().structured_output,
        })
    }
}

impl LlmClient {
    pub fn provider(&self) -> LlmProvider {
        self.provider
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    /// Check that the provider accepts the key.
    ///
    /// DeepSeek's dialect answers this with `GET /user/balance`, which costs
    /// no tokens and returns no content — exactly what a **Test connection**
    /// button should send.
    pub async fn verify(&self) -> Result<(), LlmError> {
        self.client
            .verify()
            .await
            .map_err(|error| LlmError::from_rig(self.provider, error))
    }

    /// Send one prompt and return the model's text and its usage.
    ///
    /// `model_override` comes from a request, so a re-evaluation can try a
    /// different model without rebuilding the client. `json_mode` asks the
    /// provider to constrain its reply to a JSON object; the caller still
    /// parses and validates it, because DeepSeek offers JSON mode but not a
    /// schema.
    ///
    /// With `deltas`, the text is also sent fragment by fragment as it
    /// arrives. The returned string stays authoritative: a streamed reply is
    /// assembled again at the end, and a provider that re-sends a fragment
    /// must not double it in what we store.
    pub async fn complete_text(
        &self,
        prompt: &str,
        model_override: Option<&str>,
        json_mode: bool,
        deltas: Option<&DeltaSink>,
    ) -> Result<Completion, LlmError> {
        let model = model_override.unwrap_or(self.model.as_str());
        let chat = self.client.chat(model);

        let mut request = rig::completion::CompletionRequest::new(prompt.to_string());
        request.temperature = Some(f64::from(self.temperature));
        if let Some(max_tokens) = self.max_tokens {
            request.max_tokens = Some(u64::from(max_tokens));
        }
        if json_mode && self.structured_output.sends_json_object() {
            // DeepSeek's dialect accepts `json_object` through additional
            // params rather than a `json_schema` response format; the same
            // shape works on the other OpenAI-compatible providers. A
            // prompt-only endpoint is asked in the prompt instead, because
            // sending this is exactly what some local servers reject.
            request.additional_params = Some(serde_json::json!({
                "response_format": { "type": "json_object" }
            }));
        }

        let Some(sink) = deltas else {
            let response = chat
                .call(request)
                .await
                .map_err(|error| LlmError::from_rig(self.provider, error))?;
            return Ok(completion(&response, self.provider, model));
        };

        let mut stream = chat
            .stream(request)
            .map_err(|error| LlmError::from_rig(self.provider, error))?;

        let mut text = String::new();
        while let Some(item) = stream.next().await {
            if let rig::streaming::Item::Event(rig::streaming::StreamEvent::Text {
                text: fragment,
                ..
            }) = item.map_err(|error| LlmError::from_rig(self.provider, error))?
            {
                text.push_str(&fragment);
                // A closed receiver means the UI stopped listening; the
                // completion still finishes so the report is not lost.
                let _ = sink.send(fragment);
            }
        }

        // The folded reply is authoritative: a provider may split or resend
        // fragments, and the assembled text is what call() would return. The
        // folded response also carries the usage the stream reported.
        let response = stream
            .finish()
            .await
            .map_err(|error| LlmError::from_rig(self.provider, error))?;
        let assembled = response_text(&response);
        Ok(Completion {
            text: if assembled.is_empty() {
                text
            } else {
                assembled
            },
            usage: Usage::from_rig(self.provider, model, &response.usage),
        })
    }
}

/// A response as the application's [`Completion`], usage included.
fn completion(
    response: &rig::completion::CompletionResponse,
    provider: LlmProvider,
    model: &str,
) -> Completion {
    Completion {
        text: response_text(response),
        usage: Usage::from_rig(provider, model, &response.usage),
    }
}

/// The answer text of a response, concatenated across content blocks.
/// Reasoning, tool calls, and images are not answer text.
fn response_text(response: &rig::completion::CompletionResponse) -> String {
    response
        .choice
        .iter()
        .filter_map(|content| match content {
            rig::completion::AssistantContent::Text(text) => Some(text.text.as_str()),
            _ => None,
        })
        .collect()
}

/// Whether the Pipeline's evaluation actions can run, and why not when they
/// cannot.
#[derive(Debug, Clone, PartialEq)]
pub struct LlmStatus {
    pub config: LlmConfig,
    pub capabilities: ModelCapabilities,
    pub has_api_key: bool,
}

impl LlmStatus {
    /// Every condition the Evaluate button needs.
    pub fn ready(&self) -> bool {
        self.blocked_reason().is_none()
    }

    /// The first unmet condition, in the order the user would fix it.
    pub fn blocked_reason(&self) -> Option<String> {
        let provider = self.config.provider;
        if !provider.is_supported() {
            return Some(format!("{} is not supported yet.", provider.label()));
        }
        if let Err(problem) = self.config.validate_custom() {
            return Some(problem.to_string());
        }
        if provider.requires_api_key() && !self.has_api_key {
            return Some(format!("Add a {} API key in Settings.", provider.label()));
        }
        if !self.capabilities.structured_output.is_usable() {
            return Some(format!(
                "{} cannot return structured output; choose another model.",
                self.config.model
            ));
        }
        None
    }
}

/// Load the configuration and the key presence in one call.
///
/// Reads only: it does not build a client and does not touch the network, so
/// it is safe every time the Pipeline renders.
pub async fn status(pool: &SqlitePool) -> crate::Result<LlmStatus> {
    let config = LlmConfig::load(pool).await?;
    let capabilities = config.capabilities();
    // A keychain that cannot be read is reported as "no key": making Settings
    // unopenable over it would hide the preferences form too, and the actual
    // error surfaces when the user saves a key or tests the connection.
    let has_api_key = match keys::has_api_key(config.provider) {
        Ok(present) => present,
        Err(error) => {
            tracing::warn!(%error, "API key presence could not be read");
            false
        }
    };

    Ok(LlmStatus {
        config,
        capabilities,
        has_api_key,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deepseek() -> LlmConfig {
        LlmConfig::default()
    }

    #[test]
    fn a_blank_key_is_reported_before_anything_is_built() {
        let config = deepseek();
        let error = config
            .build_client(&Zeroizing::new(String::new()))
            .expect_err("blank key");

        assert!(matches!(error, LlmError::MissingApiKey { .. }));
        assert!(!error.is_retryable());
    }

    #[test]
    fn deepseek_builds_a_client_without_touching_the_network() {
        let client = deepseek()
            .build_client(&Zeroizing::new("sk-test".into()))
            .expect("client");

        assert_eq!(client.provider(), LlmProvider::DeepSeek);
        assert_eq!(client.model(), "deepseek-flash");
    }

    #[test]
    fn a_provider_this_build_cannot_reach_is_refused() {
        let config = LlmConfig {
            provider: LlmProvider::OpenAI,
            ..LlmConfig::default()
        };
        let error = config
            .build_client(&Zeroizing::new("sk-test".into()))
            .expect_err("unsupported");

        assert!(matches!(error, LlmError::UnsupportedProvider { .. }));
    }

    #[test]
    fn readiness_names_the_first_thing_to_fix() {
        let config = deepseek();
        let found = config.capabilities();

        let blocked = LlmStatus {
            config: config.clone(),
            capabilities: found,
            has_api_key: false,
        };
        assert!(!blocked.ready());
        assert_eq!(
            blocked.blocked_reason().as_deref(),
            Some("Add a DeepSeek API key in Settings.")
        );

        let ready = LlmStatus {
            config,
            capabilities: found,
            has_api_key: true,
        };
        assert!(ready.ready());
        assert!(ready.blocked_reason().is_none());
    }

    #[test]
    fn a_model_without_structured_output_is_not_ready() {
        let config = LlmConfig {
            model: "deepseek-reasoner".into(),
            ..LlmConfig::default()
        };
        let found = config.capabilities();

        let status = LlmStatus {
            config,
            capabilities: found,
            has_api_key: true,
        };
        assert!(!status.ready());
        assert!(status
            .blocked_reason()
            .is_some_and(|reason| reason.contains("structured output")));
    }

    fn local(base_url: &str) -> LlmConfig {
        LlmConfig {
            provider: LlmProvider::Custom,
            model: "llama3.2".into(),
            base_url: base_url.into(),
            ..LlmConfig::default()
        }
    }

    #[test]
    fn a_keyless_custom_endpoint_builds_a_client_offline() {
        let client = local("http://localhost:11434/v1")
            .build_client(&Zeroizing::new(String::new()))
            .expect("a local server needs no key");

        assert_eq!(client.provider(), LlmProvider::Custom);
        assert_eq!(client.model(), "llama3.2");
        assert_eq!(
            client.structured_output,
            StructuredOutput::JsonObject,
            "JSON mode is on by default"
        );
    }

    #[test]
    fn a_custom_endpoint_without_a_url_is_refused_before_a_call() {
        // The dangerous failure is falling back to a default host, which would
        // send the CV somewhere the user did not name.
        let error = local("")
            .build_client(&Zeroizing::new(String::new()))
            .expect_err("no endpoint");

        assert!(
            matches!(&error, LlmError::InvalidConfiguration(detail) if detail.contains("base URL")),
            "{error:?}"
        );
        assert!(!error.is_retryable());
    }

    #[test]
    fn a_missing_key_still_blocks_a_hosted_provider() {
        let config = deepseek();
        let status = LlmStatus {
            capabilities: config.capabilities(),
            config,
            has_api_key: false,
        };

        assert!(status
            .blocked_reason()
            .is_some_and(|reason| reason.contains("API key")));
    }

    #[test]
    fn a_configured_custom_endpoint_is_ready_without_a_key() {
        let config = local("http://localhost:11434/v1");
        let status = LlmStatus {
            capabilities: config.capabilities(),
            config,
            has_api_key: false,
        };

        assert!(status.ready(), "{:?}", status.blocked_reason());
    }

    #[test]
    fn an_unconfigured_custom_endpoint_says_what_is_missing() {
        for (config, expected) in [
            (local(""), "base URL"),
            (
                LlmConfig {
                    base_url: "http://localhost:11434/v1".into(),
                    model: "  ".into(),
                    ..local("http://localhost:11434/v1")
                },
                "model name",
            ),
            (
                LlmConfig {
                    custom_context_window: Some(512),
                    ..local("http://localhost:11434/v1")
                },
                "context window",
            ),
        ] {
            let status = LlmStatus {
                capabilities: config.capabilities(),
                config,
                has_api_key: false,
            };
            let reason = status.blocked_reason().expect("blocked");
            assert!(reason.contains(expected), "{reason:?} misses {expected:?}");
        }
    }

    #[test]
    fn prompt_only_mode_builds_a_client_and_stays_ready() {
        let config = LlmConfig {
            json_mode: false,
            ..local("http://localhost:11434/v1")
        };

        let client = config
            .build_client(&Zeroizing::new(String::new()))
            .expect("prompt-only is still usable");
        assert_eq!(client.structured_output, StructuredOutput::PromptOnly);

        let status = LlmStatus {
            capabilities: config.capabilities(),
            config,
            has_api_key: false,
        };
        assert!(status.ready());
    }
}
