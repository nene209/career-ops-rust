//! Structured error types for acquisition.
//!
//! Two distinct concerns live here:
//!
//! - **`AtsError`** — what an adapter or HTML extractor returns on
//!   failure. Structured so the router can classify it without string
//!   matching, and so the adapter's callers do not have to know the
//!   HTTP status code that produced the failure.
//! - **`FailureCause`** — the coarse classification recorded in the
//!   database and surfaced in the Portals UI health drill-down.
//!   Deliberately a closed enum: adding a new cause requires a decision
//!   about whether the UI needs a new message.

use thiserror::Error;

/// An adapter or HTML extractor failure, classified at the point of
/// occurrence.
#[derive(Debug, Error)]
pub enum AtsError {
    #[error("request timed out: {0}")]
    Timeout(String),

    /// HTTP 403 or 429, or a 5xx that survived retries. The server
    /// answered but refused to serve the content.
    #[error("blocked by server (status {status})")]
    Blocked { status: u16 },

    #[error("response could not be parsed: {0}")]
    Parse(String),

    #[error("board not found: {0}")]
    NotFound(String),

    /// Transport-level failure that is not a timeout and not a status
    /// code — DNS, TLS, connection refused.
    #[error("transport error: {0}")]
    Transport(String),

    /// The request was malformed before it left the process, or the
    /// caller passed data the adapter cannot work with. Tier 2 cannot
    /// fix this.
    #[error("invalid request: {0}")]
    InvalidRequest(String),

    /// The extractor produced a routing signal rather than a data
    /// failure. `"js_required"` is the only value used today; the
    /// variant exists as a typed channel so the router does not have
    /// to pattern-match on error strings.
    #[error("extractor unsupported: {0}")]
    Unsupported(&'static str),
}

/// Result type for ATS adapter and HTML extractor failures.
///
/// Separate from `Result` because those failures carry `AtsError`
/// rather than `CoreError`. The router converts between them.
pub type AtsResult<T> = std::result::Result<T, AtsError>;

impl AtsError {
    /// Map a non-2xx HTTP status to an `AtsError`.
    ///
    /// Called by adapters after `error_for_status()` so a status code
    /// becomes a typed variant rather than a stringified error.
    pub fn from_status(status: reqwest::StatusCode) -> Self {
        match status.as_u16() {
            403 | 429 => AtsError::Blocked {
                status: status.as_u16(),
            },
            404 => AtsError::NotFound(status.to_string()),
            code if (500..600).contains(&code) => AtsError::Blocked { status: code },
            _ => AtsError::InvalidRequest(format!("unexpected status {status}")),
        }
    }

    /// Classify into the coarse `FailureCause` recorded in the
    /// database and displayed in the UI.
    pub fn cause(&self) -> FailureCause {
        match self {
            AtsError::Timeout(_) => FailureCause::Timeout,
            AtsError::Blocked { .. } => FailureCause::Blocked,
            AtsError::Parse(_) => FailureCause::ParseError,
            AtsError::NotFound(_) => FailureCause::NotFound,
            AtsError::Transport(_) => FailureCause::Transport,
            AtsError::InvalidRequest(_) => FailureCause::InvalidRequest,
            AtsError::Unsupported("js_required") => FailureCause::JsRequired,
            AtsError::Unsupported(_) => FailureCause::Other,
        }
    }

    /// Whether Tier 2 might succeed where this error occurred.
    ///
    /// `InvalidRequest` is the only non-escalatable cause: a bad config
    /// or a malformed URL will fail the same way in a headless browser.
    /// Everything else is worth one attempt at Tier 2 — a blocked
    /// request may pass with a real browser's TLS fingerprint, a
    /// timeout may not recur, and a parse failure may resolve once
    /// JavaScript has rendered the page.
    pub fn escalatable(&self) -> bool {
        !matches!(self, AtsError::InvalidRequest(_))
    }
}

/// Coarse failure classification. Recorded in logs and surfaced in the
/// Portals UI health drill-down. Additions require a decision about
/// the user-facing message, which is why this is a closed enum rather
/// than a string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FailureCause {
    /// Tier 1b detected an SPA shell with no static job content.
    JsRequired,
    /// HTTP 403, 429, or a 5xx that survived retries.
    Blocked,
    /// Network timeout.
    Timeout,
    /// JSON or HTML parse failure.
    ParseError,
    /// ATS board slug does not exist, or the API returned 404.
    NotFound,
    /// DNS, TLS, or connection refused.
    Transport,
    /// Bad config, malformed URL, or unexpected response shape.
    InvalidRequest,
    /// Any other cause.
    Other,
}

impl FailureCause {
    pub fn as_str(self) -> &'static str {
        match self {
            FailureCause::JsRequired => "js_required",
            FailureCause::Blocked => "blocked",
            FailureCause::Timeout => "timeout",
            FailureCause::ParseError => "parse_error",
            FailureCause::NotFound => "not_found",
            FailureCause::Transport => "transport",
            FailureCause::InvalidRequest => "invalid_request",
            FailureCause::Other => "other",
        }
    }

    /// Parse a string back into a `FailureCause`.
    ///
    /// Round-trips with `as_str`. Returns `None` for unrecognized values
    /// — which is a real case after a downgrade or if the database was
    /// modified externally. Callers that must always produce a value
    /// should use `.unwrap_or(FailureCause::Other)`.
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "js_required" => FailureCause::JsRequired,
            "blocked" => FailureCause::Blocked,
            "timeout" => FailureCause::Timeout,
            "parse_error" => FailureCause::ParseError,
            "not_found" => FailureCause::NotFound,
            "transport" => FailureCause::Transport,
            "invalid_request" => FailureCause::InvalidRequest,
            "other" => FailureCause::Other,
            _ => return None,
        })
    }
}
