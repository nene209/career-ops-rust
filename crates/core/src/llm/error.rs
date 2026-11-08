//! The application-level LLM error taxonomy.
//!
//! `rig`'s errors are precise but shaped for every provider it supports; the
//! UI needs a handful of outcomes it can explain and act on. [`LlmError`] is
//! that vocabulary, and [`LlmError::from_rig`] is the only place the two meet.

use std::time::Duration;

use thiserror::Error;

use super::config::LlmProvider;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum LlmError {
    #[error("no API key is configured for {provider}")]
    MissingApiKey { provider: LlmProvider },

    #[error("{provider} is not supported by this build yet")]
    UnsupportedProvider { provider: LlmProvider },

    #[error("the OS keychain is unavailable: {0}")]
    Keychain(String),

    #[error("{provider} rejected the API key: {detail}")]
    InvalidAuthentication {
        provider: LlmProvider,
        detail: String,
    },

    #[error("rate limited by {provider}: {detail}")]
    RateLimited {
        provider: LlmProvider,
        retry_after: Option<Duration>,
        detail: String,
    },

    /// The assembled prompt does not fit. `input_chars` and `limit` are set
    /// when the check ran on our side (WS3's pre-flight budget); a provider's
    /// own rejection leaves them `None`.
    #[error("the prompt exceeds the model's context window: {detail}")]
    ContextLengthExceeded {
        input_chars: Option<usize>,
        limit: Option<usize>,
        detail: String,
    },

    #[error("{provider} returned an error: {detail}")]
    Provider {
        provider: LlmProvider,
        detail: String,
        retryable: bool,
    },

    #[error("the request to the provider failed: {0}")]
    Transport(String),

    #[error("the model's response could not be read: {0}")]
    InvalidResponse(String),

    #[error("the request was cancelled")]
    Cancelled,
}

impl LlmError {
    /// A stable, snake_case name for the failure, for a record that outlives
    /// this build (the usage ledger's `error_kind`). The `Display` text is for
    /// people and may be reworded; this must not be.
    pub fn kind(&self) -> &'static str {
        match self {
            Self::MissingApiKey { .. } => "missing_api_key",
            Self::UnsupportedProvider { .. } => "unsupported_provider",
            Self::Keychain(_) => "keychain",
            Self::InvalidAuthentication { .. } => "invalid_authentication",
            Self::RateLimited { .. } => "rate_limited",
            Self::ContextLengthExceeded { .. } => "context_length",
            Self::Provider { .. } => "provider",
            Self::Transport(_) => "transport",
            Self::InvalidResponse(_) => "invalid_response",
            Self::Cancelled => "cancelled",
        }
    }

    /// Whether the same request may reasonably be sent again. A missing key,
    /// a rejected key, and a prompt that is too long are not transient; a
    /// rate limit, a transport failure, and a provider's 5xx are.
    pub fn is_retryable(&self) -> bool {
        match self {
            Self::RateLimited { .. } | Self::Transport(_) => true,
            Self::Provider { retryable, .. } => *retryable,
            _ => false,
        }
    }

    /// How long the provider asked us to wait, when it said so.
    pub fn retry_after(&self) -> Option<Duration> {
        match self {
            Self::RateLimited { retry_after, .. } => *retry_after,
            _ => None,
        }
    }

    /// A one-line reason for the Settings button that disables a feature.
    pub fn short_reason(&self) -> String {
        match self {
            Self::MissingApiKey { .. } => "Add an API key in Settings.".into(),
            Self::InvalidAuthentication { .. } => {
                "The API key was rejected. Check it in Settings.".into()
            }
            Self::UnsupportedProvider { .. } => "Choose a supported provider in Settings.".into(),
            other => other.to_string(),
        }
    }

    /// Map a `rig` failure onto this vocabulary.
    ///
    /// The order matters: an authentication failure must not be reported as a
    /// generic provider error, and a rate limit must keep its retry hint.
    pub(crate) fn from_rig(provider: LlmProvider, error: rig::error::ProviderError) -> Self {
        use rig::error::ProviderError as Rig;

        match error {
            Rig::InvalidAuthentication(reply) => Self::InvalidAuthentication {
                provider,
                detail: describe(&reply),
            },
            Rig::ProviderResponse(reply) => Self::from_reply(provider, &reply),
            Rig::CacheExpired { response, .. } => Self::from_reply(provider, &response),
            Rig::Http(source) => Self::Transport(source.to_string()),
            Rig::Url(error) => Self::Transport(error.to_string()),
            Rig::Request(error) => Self::Transport(error.to_string()),
            Rig::Json(error) => Self::InvalidResponse(error.to_string()),
            Rig::Response(detail) => Self::InvalidResponse(detail),
            Rig::Truncated => {
                Self::InvalidResponse("the reply ended before the model finished".into())
            }
            Rig::Stalled(stalled) => {
                Self::Transport(format!("the provider sent nothing for {stalled:?}"))
            }
            Rig::Provider(detail) => Self::Provider {
                provider,
                detail,
                retryable: false,
            },
            Rig::Relayed(report) => Self::Provider {
                provider,
                detail: report.message.clone(),
                retryable: report.retryable,
            },
            other => Self::Provider {
                provider,
                detail: other.to_string(),
                retryable: false,
            },
        }
    }

    /// A preserved provider reply, classified by its status first.
    fn from_reply(provider: LlmProvider, reply: &rig::ProviderResponseError) -> Self {
        let status = reply.status.map(|status| status.as_u16());
        let detail = describe(reply);
        let lower = reply.body.to_ascii_lowercase();

        match status {
            Some(429) => Self::RateLimited {
                provider,
                retry_after: retry_after(reply),
                detail,
            },
            Some(401 | 403) => Self::InvalidAuthentication { provider, detail },
            Some(413) => Self::ContextLengthExceeded {
                input_chars: None,
                limit: None,
                detail,
            },
            _ if mentions_context_window(&lower) => Self::ContextLengthExceeded {
                input_chars: None,
                limit: None,
                detail,
            },
            Some(status) => Self::Provider {
                provider,
                detail,
                retryable: reply.is_retryable() || status >= 500,
            },
            None => Self::Provider {
                provider,
                detail,
                retryable: reply.is_retryable(),
            },
        }
    }
}

impl From<LlmError> for crate::error::CoreError {
    fn from(error: LlmError) -> Self {
        // The taxonomy stays typed at the LLM boundary; the rest of the app
        // carries a displayable message in its single error type.
        crate::error::CoreError::Message(error.to_string())
    }
}

/// The provider's own text, bounded so a 200 KB HTML error page cannot end up
/// in a header line. The body is never logged.
fn describe(reply: &rig::ProviderResponseError) -> String {
    let status = reply
        .status
        .map(|status| format!("HTTP {}", status.as_u16()));
    let body = reply.body.trim();

    let mut text = match (status, body.is_empty()) {
        (Some(status), true) => status,
        (Some(status), false) => format!("{status}: {body}"),
        (None, true) => "no response body".to_string(),
        (None, false) => body.to_string(),
    };
    if text.chars().count() > 300 {
        text = text.chars().take(300).collect::<String>() + "…";
    }
    text
}

fn retry_after(reply: &rig::ProviderResponseError) -> Option<Duration> {
    let headers = reply.headers.as_ref()?;
    let value = headers.get("retry-after")?.to_str().ok()?.trim();
    // The header is either delta-seconds or an HTTP date; only the numeric
    // form is worth acting on, and a date-shaped value falls back to the
    // policy's own backoff.
    value.parse::<u64>().ok().map(Duration::from_secs)
}

fn mentions_context_window(body_lower: &str) -> bool {
    const MARKERS: [&str; 5] = [
        "context length",
        "context window",
        "maximum context",
        "too many tokens",
        "prompt is too long",
    ];
    MARKERS.iter().any(|marker| body_lower.contains(marker))
}
