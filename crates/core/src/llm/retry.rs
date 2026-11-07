//! Attempts and delays for transient LLM failures.
//!
//! Only [`LlmError::is_retryable`] errors are retried, and a provider's own
//! `Retry-After` wins over the computed backoff: the server knows when it will
//! answer again, and retrying earlier is what turns a rate limit into a ban.
//! Everything else is returned on the first attempt.

use std::future::Future;
use std::time::Duration;

use super::error::LlmError;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RetryPolicy {
    /// Total attempts, including the first.
    pub max_attempts: u32,
    pub base_delay: Duration,
    /// Upper bound on the computed delay. A provider's `Retry-After` is not
    /// capped: it is an instruction, not a suggestion.
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

impl RetryPolicy {
    /// The delay before `attempt` (1-based): `base * 2^(attempt-1)`, capped.
    pub fn delay(&self, attempt: u32) -> Duration {
        let exponent = attempt.saturating_sub(1).min(16);
        self.base_delay
            .saturating_mul(1u32 << exponent)
            .min(self.max_delay)
    }
}

/// Run `operation`, retrying retryable failures up to the policy's limit.
///
/// The closure receives the attempt number (1-based) so a caller can vary a
/// request if it wants to; `rig` does not need that, but it makes the retry
/// visible in a log line.
pub async fn with_retry<T, F, Fut>(policy: RetryPolicy, mut operation: F) -> Result<T, LlmError>
where
    F: FnMut(u32) -> Fut,
    Fut: Future<Output = Result<T, LlmError>>,
{
    let attempts = policy.max_attempts.max(1);
    let mut attempt = 1u32;

    loop {
        match operation(attempt).await {
            Ok(value) => return Ok(value),
            Err(error) if error.is_retryable() && attempt < attempts => {
                let wait = error.retry_after().unwrap_or_else(|| policy.delay(attempt));
                tracing::warn!(
                    attempt,
                    wait_ms = wait.as_millis() as u64,
                    error = %error,
                    "llm request failed; retrying"
                );
                if !wait.is_zero() {
                    tokio::time::sleep(wait).await;
                }
                attempt += 1;
            }
            Err(error) => return Err(error),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicU32, Ordering};

    fn instant() -> RetryPolicy {
        RetryPolicy {
            max_attempts: 3,
            base_delay: Duration::ZERO,
            max_delay: Duration::ZERO,
        }
    }

    #[test]
    fn delays_double_and_stop_at_the_cap() {
        let policy = RetryPolicy {
            max_attempts: 5,
            base_delay: Duration::from_millis(100),
            max_delay: Duration::from_millis(500),
        };

        assert_eq!(policy.delay(1), Duration::from_millis(100));
        assert_eq!(policy.delay(2), Duration::from_millis(200));
        assert_eq!(policy.delay(3), Duration::from_millis(400));
        assert_eq!(policy.delay(4), Duration::from_millis(500));
        assert_eq!(policy.delay(99), Duration::from_millis(500));
    }

    #[tokio::test]
    async fn a_transient_failure_is_retried_until_it_succeeds() {
        let calls = AtomicU32::new(0);
        let result = with_retry(instant(), |_attempt| {
            let call = calls.fetch_add(1, Ordering::Relaxed) + 1;
            async move {
                if call < 3 {
                    Err(LlmError::Transport("reset".into()))
                } else {
                    Ok(7)
                }
            }
        })
        .await;

        assert_eq!(result, Ok(7));
        assert_eq!(calls.load(Ordering::Relaxed), 3);
    }

    #[tokio::test]
    async fn the_attempt_limit_is_respected() {
        let calls = AtomicU32::new(0);
        let result = with_retry(instant(), |_attempt| {
            calls.fetch_add(1, Ordering::Relaxed);
            async { Err::<(), _>(LlmError::Transport("reset".into())) }
        })
        .await;

        assert!(result.is_err());
        assert_eq!(calls.load(Ordering::Relaxed), 3);
    }

    #[tokio::test]
    async fn a_permanent_failure_is_not_retried() {
        let calls = AtomicU32::new(0);
        let result = with_retry(instant(), |_attempt| {
            calls.fetch_add(1, Ordering::Relaxed);
            async {
                Err::<(), _>(LlmError::MissingApiKey {
                    provider: super::super::config::LlmProvider::DeepSeek,
                })
            }
        })
        .await;

        assert!(result.is_err());
        assert_eq!(calls.load(Ordering::Relaxed), 1);
    }
}
