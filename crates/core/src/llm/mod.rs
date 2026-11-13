//! Embedded LLM integration (Phase 3).
//!
//! `rig` is the provider abstraction, but it is a private detail: nothing
//! outside this module names a `rig` type. The application talks to
//! [`LlmConfig`] (what the user chose), [`LlmClient`] (a built client), and
//! [`LlmStatus`] (whether the Pipeline's actions can run).
//!
//! The split is deliberate. `rig` is pre-1.0 and ships breaking changes; the
//! facade here means an upgrade touches one module rather than every call
//! site. Phase 3 workstreams 3–7 add completion and streaming behind the same
//! boundary.
//!
//! ## What lives where
//!
//! - [`config::LlmProvider`]: the provider ids and their stored form. Only
//!   DeepSeek is constructible so far; a custom OpenAI-compatible endpoint
//!   joins it later in Phase 3.5. The rest parse so a stored choice survives a
//!   build that cannot use it.
//! - [`config`]: `settings` keys, defaults, and the load/save pair.
//! - [`capabilities`]: what a model can do, consulted before a call.
//! - [`client`]: builds the `rig` client, verifies a key, and reports status.
//! - [`error`]: the application-level taxonomy `rig` errors map onto.
//! - [`retry`]: attempts and delays for transient failures.
//! - [`usage`]: what a call used and cost, and the [`Call`] recorded per
//!   provider call.
//! - [`keys`]: the OS-keychain namespace. API keys never touch the database.

pub mod backend;
pub mod cancel;
pub mod capabilities;
pub mod client;
pub mod config;
pub mod error;
pub mod keys;
pub mod retry;
pub mod usage;

pub use backend::{Completion, CompletionBackend};
pub use cancel::CancelToken;
pub use capabilities::{ModelCapabilities, StructuredOutput};
pub use client::{status, DeltaSink, LlmClient, LlmStatus};
pub use config::{ConfigProblem, LlmConfig, LlmProvider, MIN_CUSTOM_CONTEXT_WINDOW};
pub use error::LlmError;
pub use retry::{with_retry, RetryPolicy};
pub use usage::{Call, Outcome, Source, Task, Usage};
