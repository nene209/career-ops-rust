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

use super::capabilities::{capabilities, ModelCapabilities, StructuredOutput};
use super::config::{LlmConfig, LlmProvider};
use super::error::LlmError;
use super::keys;

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
    /// A missing or blank key is [`LlmError::MissingApiKey`] rather than an
    /// authentication failure later: the difference is between "configure
    /// this" and "something is wrong".
    pub fn build_client(&self, api_key: &Zeroizing<String>) -> Result<LlmClient, LlmError> {
        if api_key.trim().is_empty() {
            return Err(LlmError::MissingApiKey {
                provider: self.provider,
            });
        }

        if !self.provider.is_supported() {
            return Err(LlmError::UnsupportedProvider {
                provider: self.provider,
            });
        }

        // Construction is offline: `rig` builds the transport lazily, so this
        // does not touch the network.
        let client = match self.provider {
            LlmProvider::DeepSeek => rig::providers::openai::OpenAIConfig::with_key(
                &rig::providers::openai::wire::DEEPSEEK,
                api_key.as_str(),
            )
            .client(),
            other => {
                return Err(LlmError::UnsupportedProvider { provider: other });
            }
        };

        Ok(LlmClient {
            provider: self.provider,
            model: self.model.clone(),
            client,
            temperature: self.temperature,
            max_tokens: self.max_tokens,
            structured_output: capabilities(self.provider, &self.model).structured_output,
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

    /// Send one prompt and return the model's text.
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
    ) -> Result<String, LlmError> {
        let model = self
            .client
            .chat(model_override.unwrap_or(self.model.as_str()));

        let mut request = rig::completion::CompletionRequest::new(prompt.to_string());
        request.temperature = Some(f64::from(self.temperature));
        if let Some(max_tokens) = self.max_tokens {
            request.max_tokens = Some(u64::from(max_tokens));
        }
        if json_mode && self.structured_output.is_usable() {
            // DeepSeek's dialect accepts `json_object` through additional
            // params rather than a `json_schema` response format; the same
            // shape works on the other OpenAI-compatible providers.
            request.additional_params = Some(serde_json::json!({
                "response_format": { "type": "json_object" }
            }));
        }

        let Some(sink) = deltas else {
            let response = model
                .call(request)
                .await
                .map_err(|error| LlmError::from_rig(self.provider, error))?;
            return Ok(response_text(&response));
        };

        let mut stream = model
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
        // fragments, and the assembled text is what call() would return.
        let response = stream
            .finish()
            .await
            .map_err(|error| LlmError::from_rig(self.provider, error))?;
        let assembled = response_text(&response);
        Ok(if assembled.is_empty() {
            text
        } else {
            assembled
        })
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
        if !self.has_api_key {
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
    let capabilities = capabilities(config.provider, &config.model);
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
        assert_eq!(client.model(), "deepseek-chat");
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
        let found = capabilities(config.provider, &config.model);

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
        let found = capabilities(config.provider, &config.model);

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
}
