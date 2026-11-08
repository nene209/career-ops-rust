//! Cooperative cancellation for a model call.
//!
//! A request carries a [`CancelToken`]. The service races the provider call
//! against the token; when the user presses Stop, the call is dropped and the
//! pipeline takes its failure path (restoring the application status, storing
//! nothing). The token is shared with the UI through an `Arc<AtomicBool>`.

use std::future::pending;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

/// A shared stop flag. `Default` is an uncancellable token, which is what tests
/// and a batch without a UI use.
#[derive(Clone, Debug, Default)]
pub struct CancelToken(Option<Arc<AtomicBool>>);

impl CancelToken {
    pub fn new(flag: Arc<AtomicBool>) -> Self {
        Self(Some(flag))
    }

    pub fn is_cancelled(&self) -> bool {
        self.0
            .as_ref()
            .is_some_and(|flag| flag.load(Ordering::Relaxed))
    }

    /// Set the shared flag. A no-op for the uncancellable token.
    pub fn cancel(&self) {
        if let Some(flag) = &self.0 {
            flag.store(true, Ordering::Relaxed);
        }
    }

    /// Resolve when the flag is set. Never resolves for an uncancellable token,
    /// so a `select!` around it always waits for the real call.
    ///
    /// Polling rather than a channel keeps the token dependency-free and lets a
    /// cancellation set before the call start win the biased race.
    pub(crate) async fn wait(&self) {
        let Some(flag) = &self.0 else {
            return pending().await;
        };
        while !flag.load(Ordering::Relaxed) {
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
}

impl PartialEq for CancelToken {
    fn eq(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            (None, None) => true,
            (Some(a), Some(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }
}

impl Eq for CancelToken {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_tokens_share_a_flag() {
        let flag = Arc::new(AtomicBool::new(false));
        let token = CancelToken::new(Arc::clone(&flag));
        assert!(!token.is_cancelled());

        token.cancel();
        assert!(flag.load(Ordering::Relaxed));
        assert!(token.is_cancelled());

        // A clone observes the same flag.
        assert!(token.clone().is_cancelled());
    }

    #[test]
    fn the_default_token_cannot_be_cancelled() {
        let token = CancelToken::default();
        token.cancel();
        assert!(!token.is_cancelled());
        assert_eq!(token, CancelToken::default());
    }
}
