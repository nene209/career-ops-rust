//! Scraper configuration: the provider discriminator and its payload.
//!
//! `portals.scraper_config` is one JSON object. `provider` names the
//! fetch-and-parse engine; omitting it means `selectors`, which is what every
//! Phase 2.2 row already is.
//!
//! A provider is an **engine**, not a company. `selectors` handles every
//! server-rendered HTML site (Waymo included); `eightfold` handles every
//! Eightfold-hosted JSON API. Site-specific profiles live in `known_sites.rs`
//! and merely name one of these engines.

use serde::Deserialize;
use serde_json::Value;

use super::html::detail::DetailConfig;
use super::html::selectors::{PaginationConfig, SelectorConfig};

/// The fetch-and-parse engine named by `scraper_config.provider`.
///
/// Adding a variant means adding an engine, not a company: it must have fetch
/// or parse behaviour the existing engines cannot express.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Provider {
    #[default]
    Selectors,
    Eightfold,
}

impl Provider {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Selectors => "selectors",
            Self::Eightfold => "eightfold",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Selectors => "Selectors",
            Self::Eightfold => "Eightfold",
        }
    }

    /// Parse the `provider` value. `None` for an unknown name.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "selectors" => Some(Self::Selectors),
            "eightfold" => Some(Self::Eightfold),
            _ => None,
        }
    }
}

/// A parsed `portals.scraper_config`.
#[derive(Debug, Clone)]
pub enum ScraperConfig {
    Selectors(SelectorConfig),
    Eightfold(EightfoldConfig),
}

/// Payload for the Eightfold JSON API.
///
/// The fetch path lands in WS2. `pagination` and `detail` reuse the Phase 2.2
/// knobs on purpose: a page cap and a description cap mean the same thing for
/// every provider.
#[derive(Debug, Clone, Deserialize)]
pub struct EightfoldConfig {
    pub domain: String,

    #[serde(default)]
    pub pagination: Option<PaginationConfig>,

    #[serde(default)]
    pub detail: Option<DetailConfig>,
}

#[derive(Debug, thiserror::Error)]
pub enum ConfigError {
    #[error("scraper config is not valid JSON: {0}")]
    Json(#[source] serde_json::Error),

    #[error("unknown provider `{0}`; expected `selectors` or `eightfold`")]
    UnknownProvider(String),

    #[error("scraper config `provider` must be a string")]
    ProviderNotString,

    #[error("selectors config is invalid: {0}")]
    Selectors(#[source] serde_json::Error),

    #[error("eightfold config is invalid: {0}")]
    Eightfold(#[source] serde_json::Error),
}

impl ScraperConfig {
    /// Parse the stored JSON. A missing (or null) `provider` means `selectors`.
    pub fn parse(raw: &str) -> std::result::Result<Self, ConfigError> {
        let value: Value = serde_json::from_str(raw).map_err(ConfigError::Json)?;

        let provider = match value.get("provider") {
            None | Some(Value::Null) => Provider::Selectors,
            Some(Value::String(name)) => Provider::from_name(name)
                .ok_or_else(|| ConfigError::UnknownProvider(name.clone()))?,
            Some(_) => return Err(ConfigError::ProviderNotString),
        };

        match provider {
            Provider::Selectors => serde_json::from_value(value)
                .map(Self::Selectors)
                .map_err(ConfigError::Selectors),
            Provider::Eightfold => serde_json::from_value(value)
                .map(Self::Eightfold)
                .map_err(ConfigError::Eightfold),
        }
    }

    pub fn provider(&self) -> Provider {
        match self {
            Self::Selectors(_) => Provider::Selectors,
            Self::Eightfold(_) => Provider::Eightfold,
        }
    }

    pub fn selectors(&self) -> Option<&SelectorConfig> {
        match self {
            Self::Selectors(config) => Some(config),
            Self::Eightfold(_) => None,
        }
    }

    pub fn eightfold(&self) -> Option<&EightfoldConfig> {
        match self {
            Self::Eightfold(config) => Some(config),
            Self::Selectors(_) => None,
        }
    }

    /// The description cap, whichever provider owns the payload.
    pub fn detail(&self) -> Option<&DetailConfig> {
        match self {
            Self::Selectors(config) => config.detail.as_ref(),
            Self::Eightfold(config) => config.detail.as_ref(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn selectors_config(raw: &str) -> SelectorConfig {
        match ScraperConfig::parse(raw).expect("parse") {
            ScraperConfig::Selectors(config) => config,
            other => panic!("expected selectors, got {:?}", other.provider()),
        }
    }

    #[test]
    fn a_missing_provider_means_selectors() {
        let config = selectors_config(r#"{"list":"article.job","fields":{"title":"h2"}}"#);

        assert_eq!(config.list.as_deref(), Some("article.job"));
        assert_eq!(
            config.fields.expect("fields").title,
            "h2",
            "the selector payload is unchanged"
        );
    }

    #[test]
    fn an_explicit_selectors_provider_is_accepted() {
        let config = selectors_config(
            r#"{"provider":"selectors","list":"article.job","fields":{"title":"h2"}}"#,
        );

        assert_eq!(config.list.as_deref(), Some("article.job"));
    }

    #[test]
    fn an_eightfold_config_carries_its_domain() {
        let parsed = ScraperConfig::parse(r#"{"provider":"eightfold","domain":"nvidia.com"}"#)
            .expect("parse");

        assert_eq!(parsed.provider(), Provider::Eightfold);
        assert_eq!(parsed.eightfold().expect("eightfold").domain, "nvidia.com");
        assert!(parsed.selectors().is_none());
    }

    #[test]
    fn eightfold_reuses_the_phase_2_2_bounds() {
        let parsed = ScraperConfig::parse(
            r#"{
                "provider": "eightfold",
                "domain": "nvidia.com",
                "pagination": { "max_pages": 3 },
                "detail": { "max_jobs": 5 }
            }"#,
        )
        .expect("parse");

        let config = parsed.eightfold().expect("eightfold");
        assert_eq!(
            config.pagination.as_ref().expect("pagination").page_limit(),
            3
        );
        assert_eq!(config.detail.as_ref().expect("detail").max_jobs(), 5);
    }

    #[test]
    fn an_unknown_provider_is_a_config_error() {
        let error =
            ScraperConfig::parse(r#"{"provider":"workday"}"#).expect_err("unknown provider");

        assert!(matches!(
            error,
            ConfigError::UnknownProvider(ref name) if name == "workday"
        ));
        assert!(error.to_string().contains("unknown provider `workday`"));
        assert!(error.to_string().contains("selectors"));
    }

    #[test]
    fn a_non_string_provider_is_a_config_error() {
        let error = ScraperConfig::parse(r#"{"provider": 7}"#).expect_err("not a string");

        assert!(matches!(error, ConfigError::ProviderNotString));
    }

    #[test]
    fn malformed_json_is_a_config_error() {
        let error = ScraperConfig::parse("not json").expect_err("malformed");

        assert!(matches!(error, ConfigError::Json(_)));
        assert!(error
            .to_string()
            .contains("scraper config is not valid JSON"));
    }

    #[test]
    fn eightfold_requires_a_domain() {
        let error =
            ScraperConfig::parse(r#"{"provider":"eightfold"}"#).expect_err("domain required");

        assert!(matches!(error, ConfigError::Eightfold(_)));
        assert!(error.to_string().contains("domain"));
    }

    #[test]
    fn a_null_provider_means_selectors() {
        let config = selectors_config(r#"{"provider":null,"list":"article.job"}"#);

        assert_eq!(config.list.as_deref(), Some("article.job"));
    }

    #[test]
    fn provider_names_round_trip() {
        for provider in [Provider::Selectors, Provider::Eightfold] {
            assert_eq!(Provider::from_name(provider.as_str()), Some(provider));
            assert!(!provider.label().is_empty());
        }
        assert_eq!(Provider::from_name("company-name"), None);
    }
}
