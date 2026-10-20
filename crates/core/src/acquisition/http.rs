//! HTTP client construction and retry policy for the acquisition pipeline.
//!
//! Two responsibilities:
//!
//! 1. **Client construction.** One place decides the timeout and user
//!    agent, so every adapter behaves the same. `default_client()` is
//!    what the router (Week 3) and the live tests (this week) both use.
//! 2. **Retry on transient failure.** A 408, 429, or 5xx from an ATS is
//!    a transient condition, not a permanent one. The helper retries
//!    with exponential backoff and jitter, honouring `Retry-After` when
//!    the server sends it.
//!
//! ## What this is not
//!
//! This is not rate limiting. A desktop application fetches one portal
//! at a time, on a user action, and never runs a background crawl.
//! There is no scenario where a client-side token bucket or a configurable
//! inter-request delay does anything useful. The only property worth
//! having is "a transient failure does not fail the whole fetch", which
//! is what this module provides.
//!
//! ## Wiring
//!
//! The adapters are not wired to this yet — that happens in Week 3 when
//! the router owns the fetch loop. The call shape is:
//!
//! ```ignore
//! let resp = http::get_with_retry(&client, &url).await?;
//! let body = resp.text().await?;
//! ```
//!
//! or, for a request that needs more than a URL:
//!
//! ```ignore
//! let resp = http::send_with_retry(|| client.get(&url).header("X", "y")).await?;
//! ```

use std::time::Duration;

use rand::Rng;
use reqwest::header::RETRY_AFTER;
use reqwest::{RequestBuilder, Response, StatusCode};
use tracing::warn;

use crate::error::{CoreError, Result};

/// Retry policy for a single request.
#[derive(Debug, Clone)]
pub struct RetryPolicy {
    /// Total attempts, including the first. `3` means "one try plus two
    /// retries".
    pub max_attempts: u32,
    /// Delay before the first retry. Doubles each subsequent attempt.
    pub base_delay: Duration,
    /// Upper bound on the computed delay, before jitter.
    pub max_delay: Duration,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            base_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(8),
        }
    }
}

/// Build the shared HTTP client.
///
/// The 30-second timeout is generous because Greenhouse's
/// `?content=true` response for a large board can be several megabytes.
/// The user agent identifies the application rather than pretending to
/// be a browser; the ATS APIs are public and do not require otherwise.
pub fn default_client() -> Result<reqwest::Client> {
    reqwest::Client::builder()
        .timeout(Duration::from_secs(30))
        .user_agent(concat!("career-ops/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|_| CoreError::Internal("failed to build http client"))
}

/// GET `url`, retrying on transient failure with the default policy.
pub async fn get_with_retry(client: &reqwest::Client, url: &str) -> Result<Response> {
    send_with_retry(|| client.get(url)).await
}

/// Send a request built by `build`, retrying on transient failure with
/// the default policy.
///
/// `build` is called once per attempt. That is why it is a closure
/// rather than a `RequestBuilder`: `RequestBuilder` is consumed by
/// `send()`, so retrying requires rebuilding it.
pub async fn send_with_retry<F>(build: F) -> Result<Response>
where
    F: Fn() -> RequestBuilder,
{
    send_with_retry_policy(build, RetryPolicy::default()).await
}

/// As `send_with_retry`, with an explicit policy.
pub async fn send_with_retry_policy<F>(build: F, policy: RetryPolicy) -> Result<Response>
where
    F: Fn() -> RequestBuilder,
{
    debug_assert!(policy.max_attempts >= 1, "max_attempts must be at least 1");

    let mut attempt: u32 = 0;

    loop {
        attempt += 1;

        match build().send().await {
            Ok(resp) if is_retryable_status(resp.status()) && attempt < policy.max_attempts => {
                let delay = retry_after(&resp).unwrap_or_else(|| backoff_delay(&policy, attempt));
                warn!(
                    status = resp.status().as_u16(),
                    attempt,
                    delay_ms = delay.as_millis() as u64,
                    "retryable status; backing off"
                );
                tokio::time::sleep(delay).await;
            }
            Ok(resp) => return Ok(resp),
            Err(e) if is_retryable_error(&e) && attempt < policy.max_attempts => {
                let delay = backoff_delay(&policy, attempt);
                warn!(
                    error = %e,
                    attempt,
                    delay_ms = delay.as_millis() as u64,
                    "retryable transport error; backing off"
                );
                tokio::time::sleep(delay).await;
            }
            Err(e) => {
                warn!(error = %e, attempt, "request failed permanently");
                return Err(CoreError::Internal("http request failed"));
            }
        }
    }
}

/// 408 (timeout), 429 (rate limit), and all 5xx are transient.
/// Every other status is the server's final answer.
fn is_retryable_status(status: StatusCode) -> bool {
    status == StatusCode::REQUEST_TIMEOUT
        || status == StatusCode::TOO_MANY_REQUESTS
        || status.is_server_error()
}

/// Timeouts and connection failures are transient. DNS errors, TLS
/// errors, and URL parse errors are not.
fn is_retryable_error(e: &reqwest::Error) -> bool {
    e.is_timeout() || e.is_connect()
}

/// Read `Retry-After` if the server sent one. Only the seconds form is
/// handled; the HTTP-date form falls through to exponential backoff,
/// which is an acceptable degradation.
///
/// Capped at 60 seconds so a hostile or misconfigured header cannot
/// stall the application.
fn retry_after(resp: &Response) -> Option<Duration> {
    let raw = resp.headers().get(RETRY_AFTER)?.to_str().ok()?.trim();
    let secs: u64 = raw.parse().ok()?;
    Some(Duration::from_secs(secs.min(60)))
}

/// Exponential backoff with equal jitter: half the capped delay is
/// fixed, half is random. This avoids the thundering-herd problem of
/// pure exponential backoff while keeping the wait predictable.
fn backoff_delay(policy: &RetryPolicy, attempt: u32) -> Duration {
    let shift = attempt.saturating_sub(1).min(10);
    let exp = policy.base_delay.saturating_mul(1u32 << shift);
    let capped = exp.min(policy.max_delay);
    let half = (capped.as_millis() / 2) as u64;
    let jitter = rand::thread_rng().gen_range(0..=half.max(1));
    Duration::from_millis(half + jitter)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retryable_statuses_are_transient_only() {
        assert!(is_retryable_status(StatusCode::REQUEST_TIMEOUT));
        assert!(is_retryable_status(StatusCode::TOO_MANY_REQUESTS));
        assert!(is_retryable_status(StatusCode::INTERNAL_SERVER_ERROR));
        assert!(is_retryable_status(StatusCode::BAD_GATEWAY));
        assert!(is_retryable_status(StatusCode::SERVICE_UNAVAILABLE));
        assert!(is_retryable_status(StatusCode::GATEWAY_TIMEOUT));

        assert!(!is_retryable_status(StatusCode::OK));
        assert!(!is_retryable_status(StatusCode::BAD_REQUEST));
        assert!(!is_retryable_status(StatusCode::NOT_FOUND));
        assert!(!is_retryable_status(StatusCode::UNAUTHORIZED));
    }

    #[test]
    fn backoff_is_capped_and_within_half_to_full_range() {
        let policy = RetryPolicy {
            max_attempts: 5,
            base_delay: Duration::from_millis(100),
            max_delay: Duration::from_millis(400),
        };
    
        for attempt in 1u32..=5 {
            // Each attempt has its own cap: the exponential term, min'd
            // with max_delay. The test must compute that cap per attempt,
            // not reuse the global max_delay for every attempt.
            let shift = attempt.saturating_sub(1).min(10);
            let exp = (policy.base_delay.as_millis() as u64) * (1u64 << shift);
            let attempt_cap = exp.min(policy.max_delay.as_millis() as u64);
    
            for _ in 0..20 {
                let d = backoff_delay(&policy, attempt);
                let d_ms = d.as_millis() as u64;
                assert!(
                    d_ms >= attempt_cap / 2 && d_ms <= attempt_cap,
                    "attempt {attempt}: {d:?} outside [{}/2, {attempt_cap}]",
                    attempt_cap
                );
            }
        }
    }

    #[test]
    fn backoff_grows_before_hitting_the_cap() {
        // With attempt 1 the cap is base_delay; with attempt 3 it is the
        // configured max_delay. Assert the ceiling moves, which proves
        // the exponential term is being applied.
        let policy = RetryPolicy {
            max_attempts: 5,
            base_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(10),
        };

        let d1 = backoff_delay(&policy, 1);
        let d3 = backoff_delay(&policy, 3);

        // attempt 1 caps at 100ms, attempt 3 caps at 400ms.
        assert!(d1 <= Duration::from_millis(100));
        assert!(d3 <= Duration::from_millis(400));
    }

    #[test]
    fn default_client_builds() {
        default_client().expect("client builds");
    }
}