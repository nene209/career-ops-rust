//! What the user chose, and where it is stored.
//!
//! The choice lives in the `settings` table (never the key), one JSON value
//! per key, so adding a field later does not need a migration. A malformed
//! value reads as the default with a warning, for the same reason preferences
//! do: a hand-edited database must not make Settings unopenable.

use serde::de::DeserializeOwned;
use sqlx::SqlitePool;

use crate::error::Result;
use crate::repo::settings;

/// The provider ids this build knows.
///
/// All four parse so a choice written by a future build is not silently
/// replaced; only [`LlmProvider::is_supported`] ones can be used today.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LlmProvider {
    #[default]
    DeepSeek,
    OpenAI,
    Anthropic,
    Gemini,
}

impl LlmProvider {
    pub const ALL: [LlmProvider; 4] = [
        LlmProvider::DeepSeek,
        LlmProvider::OpenAI,
        LlmProvider::Anthropic,
        LlmProvider::Gemini,
    ];

    /// The stored form. Changing one of these invalidates saved settings, so
    /// they are part of the storage contract.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DeepSeek => "deepseek",
            Self::OpenAI => "openai",
            Self::Anthropic => "anthropic",
            Self::Gemini => "gemini",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "deepseek" => Some(Self::DeepSeek),
            "openai" => Some(Self::OpenAI),
            "anthropic" => Some(Self::Anthropic),
            "gemini" => Some(Self::Gemini),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::DeepSeek => "DeepSeek",
            Self::OpenAI => "OpenAI",
            Self::Anthropic => "Anthropic",
            Self::Gemini => "Gemini",
        }
    }

    /// Whether [`super::LlmConfig::build_client`] can build a client for this
    /// provider. The others are listed in Settings as unavailable rather than
    /// hidden, so the gap is visible instead of surprising.
    pub fn is_supported(self) -> bool {
        matches!(self, Self::DeepSeek)
    }

    /// The model DeepSeek's API serves under its stable alias. `rig` also
    /// exposes `deepseek-flash` and the versioned names; the alias is the
    /// default because DeepSeek keeps it pointed at the current chat model.
    pub fn default_model(self) -> Option<&'static str> {
        match self {
            Self::DeepSeek => Some("deepseek-chat"),
            _ => None,
        }
    }
}

impl std::fmt::Display for LlmProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

pub const KEY_PROVIDER: &str = "llm.provider";
pub const KEY_MODEL: &str = "llm.model";
pub const KEY_TEMPERATURE: &str = "llm.temperature";
pub const KEY_MAX_TOKENS: &str = "llm.max_tokens";
pub const KEY_MAX_JOB_CHARS: &str = "llm.max_job_chars";

/// Default character budget for the job text sent in a prompt.
///
/// A character proxy for tokens (~4 characters per token), so the budget can
/// be enforced without a tokenizer.
pub const DEFAULT_MAX_JOB_CHARS: usize = 12_000;

#[derive(Debug, Clone, PartialEq)]
pub struct LlmConfig {
    pub provider: LlmProvider,
    pub model: String,
    pub temperature: f32,
    /// `None` lets the provider choose.
    pub max_tokens: Option<u32>,
    pub max_job_chars: usize,
}

impl Default for LlmConfig {
    fn default() -> Self {
        Self {
            provider: LlmProvider::DeepSeek,
            model: LlmProvider::DeepSeek
                .default_model()
                .unwrap_or_default()
                .to_string(),
            temperature: 0.2,
            max_tokens: None,
            max_job_chars: DEFAULT_MAX_JOB_CHARS,
        }
    }
}

impl LlmConfig {
    /// Read the stored choice. A missing value is the default; each key is
    /// read independently so one bad value does not discard the rest.
    pub async fn load(pool: &SqlitePool) -> Result<Self> {
        let default = Self::default();

        let provider = read::<String>(pool, KEY_PROVIDER)
            .await
            .and_then(|raw| LlmProvider::parse(&raw))
            .unwrap_or(default.provider);
        let model = read::<String>(pool, KEY_MODEL)
            .await
            .unwrap_or(default.model);
        let temperature = read::<f32>(pool, KEY_TEMPERATURE)
            .await
            .unwrap_or(default.temperature);
        let max_tokens = read::<Option<u32>>(pool, KEY_MAX_TOKENS)
            .await
            .unwrap_or(default.max_tokens);
        let max_job_chars = read::<usize>(pool, KEY_MAX_JOB_CHARS)
            .await
            .unwrap_or(default.max_job_chars);

        Ok(Self {
            provider,
            model,
            temperature,
            max_tokens,
            max_job_chars,
        }
        .sanitized())
    }

    /// Write every key. A partial write is not possible per key, but a caller
    /// that fails midway leaves a mix of old and new values; the values are
    /// independent, so the result is still a coherent configuration.
    pub async fn save(&self, pool: &SqlitePool) -> Result<Self> {
        let config = self.clone().sanitized();
        settings::set_json(pool, KEY_PROVIDER, &config.provider.as_str()).await?;
        settings::set_json(pool, KEY_MODEL, &config.model).await?;
        settings::set_json(pool, KEY_TEMPERATURE, &config.temperature).await?;
        settings::set_json(pool, KEY_MAX_TOKENS, &config.max_tokens).await?;
        settings::set_json(pool, KEY_MAX_JOB_CHARS, &config.max_job_chars).await?;
        Ok(config)
    }

    /// An empty model falls back to the provider's default, and the numeric
    /// fields are clamped to values the API accepts.
    pub fn sanitized(mut self) -> Self {
        if self.model.trim().is_empty() {
            if let Some(model) = self.provider.default_model() {
                self.model = model.to_string();
            }
        }
        self.model = self.model.trim().to_string();
        self.temperature = self.temperature.clamp(0.0, 2.0);
        self.max_job_chars = self.max_job_chars.max(1_000);
        self
    }
}

async fn read<T: DeserializeOwned>(pool: &SqlitePool, key: &str) -> Option<T> {
    match settings::get_json::<T>(pool, key).await {
        Ok(value) => value,
        Err(error) => {
            tracing::warn!(key, %error, "llm setting unreadable; using the default");
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn providers_round_trip_through_their_stored_form() {
        for provider in LlmProvider::ALL {
            assert_eq!(LlmProvider::parse(provider.as_str()), Some(provider));
            assert!(!provider.label().is_empty());
        }
        assert_eq!(
            LlmProvider::parse("  DeepSeek "),
            Some(LlmProvider::DeepSeek)
        );
        assert_eq!(LlmProvider::parse("bedrock"), None);
    }

    #[test]
    fn only_deepseek_is_configurable_today() {
        assert!(LlmProvider::DeepSeek.is_supported());
        assert!(LlmProvider::DeepSeek.default_model().is_some());
        for provider in [
            LlmProvider::OpenAI,
            LlmProvider::Anthropic,
            LlmProvider::Gemini,
        ] {
            assert!(!provider.is_supported(), "{provider:?}");
            assert!(provider.default_model().is_none(), "{provider:?}");
        }
    }

    #[test]
    fn the_default_is_deepseek_chat() {
        let config = LlmConfig::default();
        assert_eq!(config.provider, LlmProvider::DeepSeek);
        assert_eq!(config.model, "deepseek-chat");
        assert!(config.max_tokens.is_none());
        assert_eq!(config.max_job_chars, DEFAULT_MAX_JOB_CHARS);
    }

    #[test]
    fn sanitizing_fills_and_clamps() {
        let config = LlmConfig {
            provider: LlmProvider::DeepSeek,
            model: "  ".into(),
            temperature: 9.0,
            max_tokens: None,
            max_job_chars: 5,
        }
        .sanitized();

        assert_eq!(config.model, "deepseek-chat");
        assert_eq!(config.temperature, 2.0);
        assert_eq!(config.max_job_chars, 1_000);
    }
}
