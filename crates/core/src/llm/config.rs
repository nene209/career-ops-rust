//! What the user chose, and where it is stored.
//!
//! The choice lives in the `settings` table (never the key), one JSON value
//! per key, so adding a field later does not need a migration. A malformed
//! value reads as the default with a warning, for the same reason preferences
//! do: a hand-edited database must not make Settings unopenable.

use serde::de::DeserializeOwned;
use sqlx::SqlitePool;
use url::{Host, Url};

use crate::error::Result;
use crate::repo::settings;

/// The provider ids this build knows.
///
/// All of them parse so a choice written by a future build is not silently
/// replaced; only [`LlmProvider::is_supported`] ones can be used today.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LlmProvider {
    #[default]
    DeepSeek,
    OpenAI,
    Anthropic,
    Gemini,
    /// Any OpenAI-compatible endpoint the user runs or reaches themselves:
    /// Ollama, `llama-server`, vLLM, LM Studio, or a gateway. One id rather
    /// than one per engine, because they share a wire format and differ only
    /// in the base URL and model.
    Custom,
}

impl LlmProvider {
    pub const ALL: [LlmProvider; 5] = [
        LlmProvider::DeepSeek,
        LlmProvider::OpenAI,
        LlmProvider::Anthropic,
        LlmProvider::Gemini,
        LlmProvider::Custom,
    ];

    /// The stored form. Changing one of these invalidates saved settings, so
    /// they are part of the storage contract.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DeepSeek => "deepseek",
            Self::OpenAI => "openai",
            Self::Anthropic => "anthropic",
            Self::Gemini => "gemini",
            Self::Custom => "custom",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim().to_ascii_lowercase().as_str() {
            "deepseek" => Some(Self::DeepSeek),
            "openai" => Some(Self::OpenAI),
            "anthropic" => Some(Self::Anthropic),
            "gemini" => Some(Self::Gemini),
            "custom" => Some(Self::Custom),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::DeepSeek => "DeepSeek",
            Self::OpenAI => "OpenAI",
            Self::Anthropic => "Anthropic",
            Self::Gemini => "Gemini",
            Self::Custom => "Local / custom",
        }
    }

    /// Whether [`super::LlmConfig::build_client`] can build a client for this
    /// provider. The others are listed in Settings as unavailable rather than
    /// hidden, so the gap is visible instead of surprising.
    pub fn is_supported(self) -> bool {
        matches!(self, Self::DeepSeek)
    }

    /// Whether the provider refuses to work without a key.
    ///
    /// A local server usually has no authentication to offer, so requiring a
    /// key there would be a lie the user cannot satisfy. The key stays
    /// optional for `custom`; whether one is needed is the server's business.
    pub fn requires_api_key(self) -> bool {
        !matches!(self, Self::Custom)
    }

    /// The model DeepSeek's API serves as its general-purpose default.
    ///
    /// `deepseek-flash` is the active model in `rig`'s catalog, which is also
    /// where a price comes from when the provider does not report one. The
    /// older `deepseek-chat` alias is not listed there, so a configuration
    /// still holding it is preserved but cannot be priced.
    ///
    /// A custom endpoint has no default: model names are engine-specific, so
    /// guessing one would send a request that cannot work.
    pub fn default_model(self) -> Option<&'static str> {
        match self {
            Self::DeepSeek => Some("deepseek-flash"),
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
pub const KEY_THIN_DESCRIPTION_CHARS: &str = "llm.thin_description_chars";
pub const KEY_BASE_URL: &str = "llm.base_url";
pub const KEY_JSON_MODE: &str = "llm.json_mode";
pub const KEY_CUSTOM_CONTEXT_WINDOW: &str = "llm.custom_context_window";

/// Default character budget for the job text sent in a prompt.
///
/// A character proxy for tokens (~4 characters per token), so the budget can
/// be enforced without a tokenizer.
pub const DEFAULT_MAX_JOB_CHARS: usize = 12_000;

/// Below this many characters of normalized text, a row is "thin": a card
/// teaser rather than a description. The evaluation UI offers to fetch the
/// full text first.
pub const DEFAULT_THIN_DESCRIPTION_CHARS: usize = 400;

/// The smallest context window a custom endpoint may declare.
///
/// A CV and a posting do not fit in less, so a smaller number is far more
/// likely a units mistake (thousands) than a real window. Refusing it turns a
/// confusing refusal at call time into one at save time.
pub const MIN_CUSTOM_CONTEXT_WINDOW: usize = 1_024;

/// What is wrong with a custom endpoint's configuration.
///
/// One variant per field, so Settings can put the message beside the input
/// that caused it instead of in a banner.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConfigProblem {
    #[error("enter the endpoint's base URL, for example http://localhost:11434/v1")]
    BaseUrlMissing,
    #[error("\"{0}\" is not a valid URL")]
    BaseUrlInvalid(String),
    #[error("the base URL must start with http:// or https://")]
    BaseUrlScheme,
    #[error("the base URL must not embed a username or password; use the API key field")]
    BaseUrlCredentials,
    #[error("enter the model name the endpoint serves, for example llama3.2")]
    ModelMissing,
    #[error("the context window must be at least {MIN_CUSTOM_CONTEXT_WINDOW} tokens")]
    ContextWindowTooSmall,
}

impl ConfigProblem {
    /// The field the message belongs beside.
    pub fn field(&self) -> &'static str {
        match self {
            Self::BaseUrlMissing
            | Self::BaseUrlInvalid(_)
            | Self::BaseUrlScheme
            | Self::BaseUrlCredentials => "Base URL",
            Self::ModelMissing => "Model",
            Self::ContextWindowTooSmall => "Context window",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct LlmConfig {
    pub provider: LlmProvider,
    pub model: String,
    pub temperature: f32,
    /// `None` lets the provider choose.
    pub max_tokens: Option<u32>,
    pub max_job_chars: usize,
    /// Below this many normalized characters, the UI offers to fetch a full
    /// description before evaluating.
    pub thin_description_chars: usize,
    /// The endpoint a `custom` provider talks to. Empty for the hosted
    /// providers, whose base URL is the dialect's own.
    pub base_url: String,
    /// Whether a `custom` endpoint is asked for `response_format: json_object`.
    /// A local model may not accept it; turning it off moves the requirement
    /// into the prompt, which the pipelines already ask for and validate.
    pub json_mode: bool,
    /// The model's context window, when the user knows it. `None` disables the
    /// pre-flight token check for a custom endpoint; the character budget
    /// still applies and the provider's own refusal is still classified.
    pub custom_context_window: Option<usize>,
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
            thin_description_chars: DEFAULT_THIN_DESCRIPTION_CHARS,
            base_url: String::new(),
            // The safer default is to constrain the reply. A server that
            // rejects `response_format` is a per-endpoint surprise, and the
            // toggle exists for exactly that case.
            json_mode: true,
            custom_context_window: None,
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
        let thin_description_chars = read::<usize>(pool, KEY_THIN_DESCRIPTION_CHARS)
            .await
            .unwrap_or(default.thin_description_chars);
        let base_url = read::<String>(pool, KEY_BASE_URL)
            .await
            .unwrap_or(default.base_url);
        let json_mode = read::<bool>(pool, KEY_JSON_MODE)
            .await
            .unwrap_or(default.json_mode);
        let custom_context_window = read::<Option<usize>>(pool, KEY_CUSTOM_CONTEXT_WINDOW)
            .await
            .unwrap_or(default.custom_context_window);

        Ok(Self {
            provider,
            model,
            temperature,
            max_tokens,
            max_job_chars,
            thin_description_chars,
            base_url,
            json_mode,
            custom_context_window,
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
        settings::set_json(
            pool,
            KEY_THIN_DESCRIPTION_CHARS,
            &config.thin_description_chars,
        )
        .await?;
        settings::set_json(pool, KEY_BASE_URL, &config.base_url).await?;
        settings::set_json(pool, KEY_JSON_MODE, &config.json_mode).await?;
        settings::set_json(
            pool,
            KEY_CUSTOM_CONTEXT_WINDOW,
            &config.custom_context_window,
        )
        .await?;
        Ok(config)
    }

    /// An empty model falls back to the provider's default, the base URL
    /// loses whitespace and any trailing slash, and the numeric fields are
    /// clamped to values the API accepts.
    ///
    /// A custom endpoint is deliberately *not* defaulted further: no model is
    /// invented, and a missing base URL stays empty for
    /// [`Self::validate_custom`] to refuse.
    pub fn sanitized(mut self) -> Self {
        if self.model.trim().is_empty() {
            if let Some(model) = self.provider.default_model() {
                self.model = model.to_string();
            }
        }
        self.model = self.model.trim().to_string();
        self.temperature = self.temperature.clamp(0.0, 2.0);
        self.max_job_chars = self.max_job_chars.max(1_000);
        self.thin_description_chars = self.thin_description_chars.max(1);
        self.base_url = trim_base_url(&self.base_url);
        self
    }

    /// The configuration after the user picks `provider`.
    ///
    /// A model name belongs to one provider, so a switch replaces it: a
    /// provider with a default gets that default, and one without keeps what
    /// is typed unless it is the previous provider's default (which would name
    /// a model the new endpoint cannot serve). The custom fields are untouched,
    /// so switching to DeepSeek and back does not lose the endpoint.
    pub fn switched_to(mut self, provider: LlmProvider) -> Self {
        if provider == self.provider {
            return self;
        }
        let model_was_the_default = self.provider.default_model() == Some(self.model.as_str());
        self.provider = provider;
        match provider.default_model() {
            Some(default) => self.model = default.to_string(),
            None if model_was_the_default => self.model.clear(),
            None => {}
        }
        self
    }

    /// Check what a `custom` endpoint needs before it can be saved or tested.
    ///
    /// Returns the first problem in the order the user would fix it. A hosted
    /// provider has nothing to check here, so it is always `Ok`.
    pub fn validate_custom(&self) -> std::result::Result<(), ConfigProblem> {
        if self.provider != LlmProvider::Custom {
            return Ok(());
        }
        parse_base_url(&self.base_url)?;
        if self.model.trim().is_empty() {
            return Err(ConfigProblem::ModelMissing);
        }
        if let Some(window) = self.custom_context_window {
            if window < MIN_CUSTOM_CONTEXT_WINDOW {
                return Err(ConfigProblem::ContextWindowTooSmall);
            }
        }
        Ok(())
    }

    /// A caution that does not block saving.
    ///
    /// Plain `http` is allowed for a loopback endpoint because the traffic
    /// never leaves the machine. Anywhere else it is visible on the network,
    /// so the user is told before they rely on it; refusing outright would
    /// block a home-lab server the user may be deliberately reaching.
    pub fn custom_warning(&self) -> Option<String> {
        if self.provider != LlmProvider::Custom {
            return None;
        }
        let url = parse_base_url(&self.base_url).ok()?;
        if url.scheme() != "http" || is_loopback(&url) {
            return None;
        }
        let host = url.host_str().unwrap_or("that host");
        Some(format!(
            "Plain http to {host} sends your CV and any API key across the network in clear \
             text. Use https, or an address that stays on this machine."
        ))
    }
}

/// Parse the base URL, refusing everything but a well-formed `http`/`https`
/// endpoint with no embedded credentials.
fn parse_base_url(raw: &str) -> std::result::Result<Url, ConfigProblem> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err(ConfigProblem::BaseUrlMissing);
    }
    let url =
        Url::parse(trimmed).map_err(|_| ConfigProblem::BaseUrlInvalid(trimmed.to_string()))?;
    if !matches!(url.scheme(), "http" | "https") {
        return Err(ConfigProblem::BaseUrlScheme);
    }
    // An endpoint without a host is not an endpoint, even when it parses.
    if url.host().is_none() {
        return Err(ConfigProblem::BaseUrlInvalid(trimmed.to_string()));
    }
    if !url.username().is_empty() || url.password().is_some() {
        return Err(ConfigProblem::BaseUrlCredentials);
    }
    Ok(url)
}

/// Whether the endpoint stays on this machine.
fn is_loopback(url: &Url) -> bool {
    match url.host() {
        Some(Host::Domain(domain)) => domain.eq_ignore_ascii_case("localhost"),
        Some(Host::Ipv4(address)) => address.is_loopback(),
        Some(Host::Ipv6(address)) => address.is_loopback(),
        None => false,
    }
}

/// Trim whitespace and one or more trailing slashes, but never the ones that
/// are part of the scheme.
fn trim_base_url(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.ends_with("://") {
        return trimmed.to_string();
    }
    trimmed.trim_end_matches('/').to_string()
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
            LlmProvider::Custom,
        ] {
            assert!(!provider.is_supported(), "{provider:?}");
            assert!(provider.default_model().is_none(), "{provider:?}");
        }
    }

    #[test]
    fn a_local_endpoint_needs_no_key_and_has_no_default_model() {
        assert!(!LlmProvider::Custom.requires_api_key());
        assert_eq!(LlmProvider::Custom.default_model(), None);
        for provider in [
            LlmProvider::DeepSeek,
            LlmProvider::OpenAI,
            LlmProvider::Anthropic,
            LlmProvider::Gemini,
        ] {
            assert!(provider.requires_api_key(), "{provider:?}");
        }
    }

    #[test]
    fn the_default_is_deepseek_flash() {
        let config = LlmConfig::default();
        assert_eq!(config.provider, LlmProvider::DeepSeek);
        assert_eq!(config.model, "deepseek-flash");
        assert!(config.max_tokens.is_none());
        assert_eq!(config.max_job_chars, DEFAULT_MAX_JOB_CHARS);
        assert!(config.base_url.is_empty());
        assert!(config.json_mode);
        assert_eq!(config.custom_context_window, None);
    }

    #[test]
    fn sanitizing_fills_and_clamps() {
        let config = LlmConfig {
            provider: LlmProvider::DeepSeek,
            model: "  ".into(),
            temperature: 9.0,
            max_tokens: None,
            max_job_chars: 5,
            thin_description_chars: 0,
            ..LlmConfig::default()
        }
        .sanitized();

        assert_eq!(config.model, "deepseek-flash");
        assert_eq!(config.temperature, 2.0);
        assert_eq!(config.max_job_chars, 1_000);
        assert_eq!(config.thin_description_chars, 1);
    }

    #[test]
    fn sanitizing_a_base_url_drops_whitespace_and_trailing_slashes() {
        for (stored, expected) in [
            (
                "  http://localhost:11434/v1/  ",
                "http://localhost:11434/v1",
            ),
            ("http://localhost:11434/v1///", "http://localhost:11434/v1"),
            ("https://gateway.example.com", "https://gateway.example.com"),
            // Incomplete input is left for validation to refuse, not for
            // sanitizing to mangle into `http:/`.
            ("http://", "http://"),
        ] {
            let config = LlmConfig {
                base_url: stored.into(),
                ..LlmConfig::default()
            }
            .sanitized();
            assert_eq!(config.base_url, expected, "{stored:?}");
        }
    }

    #[test]
    fn sanitizing_never_invents_a_custom_model() {
        let config = LlmConfig {
            provider: LlmProvider::Custom,
            model: "   ".into(),
            ..LlmConfig::default()
        }
        .sanitized();

        assert_eq!(config.model, "", "there is no safe default across engines");
    }

    fn custom(base_url: &str) -> LlmConfig {
        LlmConfig {
            provider: LlmProvider::Custom,
            model: "llama3.2".into(),
            base_url: base_url.into(),
            ..LlmConfig::default()
        }
    }

    #[test]
    fn a_loopback_or_https_endpoint_validates() {
        for url in [
            "http://localhost:11434/v1",
            "http://127.0.0.1:8080/v1",
            "http://[::1]:8080/v1",
            "https://llm.example.com/v1",
            "https://192.168.1.50:8443/v1",
        ] {
            assert_eq!(custom(url).validate_custom(), Ok(()), "{url}");
        }
    }

    #[test]
    fn a_missing_or_malformed_base_url_is_named_as_the_problem() {
        let cases = [
            ("", ConfigProblem::BaseUrlMissing),
            ("   ", ConfigProblem::BaseUrlMissing),
            // `url` reads `localhost:` as a scheme, so the message tells the
            // user what to add rather than calling the input unparseable.
            ("localhost:11434/v1", ConfigProblem::BaseUrlScheme),
            (
                "http://not a host/v1",
                ConfigProblem::BaseUrlInvalid("http://not a host/v1".into()),
            ),
            ("http://", ConfigProblem::BaseUrlInvalid("http://".into())),
            ("ftp://localhost/v1", ConfigProblem::BaseUrlScheme),
            ("file:///tmp/v1", ConfigProblem::BaseUrlScheme),
            (
                "http://user:secret@localhost:11434/v1",
                ConfigProblem::BaseUrlCredentials,
            ),
            (
                "http://user@localhost:11434/v1",
                ConfigProblem::BaseUrlCredentials,
            ),
        ];

        for (url, expected) in cases {
            let problem = custom(url).validate_custom().expect_err(url);
            assert_eq!(problem, expected, "{url}");
        }
    }

    #[test]
    fn each_problem_names_the_field_it_belongs_to() {
        assert_eq!(ConfigProblem::BaseUrlMissing.field(), "Base URL");
        assert_eq!(
            ConfigProblem::BaseUrlInvalid("x".into()).field(),
            "Base URL"
        );
        assert_eq!(ConfigProblem::BaseUrlScheme.field(), "Base URL");
        assert_eq!(ConfigProblem::BaseUrlCredentials.field(), "Base URL");
        assert_eq!(ConfigProblem::ModelMissing.field(), "Model");
        assert_eq!(
            ConfigProblem::ContextWindowTooSmall.field(),
            "Context window"
        );
    }

    #[test]
    fn a_custom_endpoint_needs_a_model_and_a_sane_context_window() {
        let mut config = custom("http://localhost:11434/v1");
        config.model = "   ".into();
        assert_eq!(config.validate_custom(), Err(ConfigProblem::ModelMissing));

        let mut config = custom("http://localhost:11434/v1");
        config.custom_context_window = Some(MIN_CUSTOM_CONTEXT_WINDOW - 1);
        assert_eq!(
            config.validate_custom(),
            Err(ConfigProblem::ContextWindowTooSmall)
        );

        config.custom_context_window = Some(MIN_CUSTOM_CONTEXT_WINDOW);
        assert_eq!(config.validate_custom(), Ok(()), "the bound is inclusive");

        config.custom_context_window = None;
        assert_eq!(config.validate_custom(), Ok(()), "blank disables the check");
    }

    #[test]
    fn a_hosted_provider_has_nothing_to_validate_or_warn_about() {
        let config = LlmConfig::default();
        assert_eq!(config.validate_custom(), Ok(()));
        assert_eq!(config.custom_warning(), None);
    }

    #[test]
    fn plain_http_away_from_this_machine_is_a_warning_not_a_refusal() {
        assert_eq!(
            custom("http://localhost:11434/v1").custom_warning(),
            None,
            "loopback traffic never leaves the machine"
        );
        assert_eq!(custom("http://127.0.0.1:8080/v1").custom_warning(), None);
        assert_eq!(
            custom("https://llm.example.com/v1").custom_warning(),
            None,
            "https is already private on the wire"
        );

        let warning = custom("http://192.168.1.50:8080/v1")
            .custom_warning()
            .expect("a plain-http LAN endpoint is worth a warning");
        assert!(warning.contains("192.168.1.50"), "{warning}");
        assert_eq!(
            custom("http://192.168.1.50:8080/v1").validate_custom(),
            Ok(()),
            "the warning must not block saving"
        );
    }

    #[test]
    fn switching_provider_replaces_a_borrowed_model_and_keeps_the_endpoint() {
        let configured = LlmConfig {
            provider: LlmProvider::Custom,
            model: "llama3.2".into(),
            base_url: "http://localhost:11434/v1".into(),
            json_mode: false,
            custom_context_window: Some(8_192),
            ..LlmConfig::default()
        };

        // Custom -> DeepSeek takes DeepSeek's default and leaves the endpoint.
        let hosted = configured.clone().switched_to(LlmProvider::DeepSeek);
        assert_eq!(hosted.provider, LlmProvider::DeepSeek);
        assert_eq!(hosted.model, "deepseek-flash");
        assert_eq!(hosted.base_url, "http://localhost:11434/v1");
        assert!(!hosted.json_mode);
        assert_eq!(hosted.custom_context_window, Some(8_192));

        // ...and back again drops the model that belongs to DeepSeek, because
        // the custom endpoint would not serve it.
        let custom_again = hosted.switched_to(LlmProvider::Custom);
        assert_eq!(custom_again.provider, LlmProvider::Custom);
        assert_eq!(custom_again.model, "");

        // A user-typed model survives a switch to another provider that has no
        // default of its own.
        let named = LlmConfig {
            provider: LlmProvider::OpenAI,
            model: "gpt-4o".into(),
            ..LlmConfig::default()
        };
        assert_eq!(
            named.switched_to(LlmProvider::Custom).model,
            "gpt-4o",
            "it was not the previous provider's default"
        );

        // Selecting the provider already in use changes nothing.
        let unchanged = LlmConfig::default().switched_to(LlmProvider::DeepSeek);
        assert_eq!(unchanged, LlmConfig::default());
    }
}
