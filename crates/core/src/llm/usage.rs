//! What a provider call used, and what it cost.
//!
//! The ledger in [`crate::repo::llm_usage`] stores one row per call. This
//! module is the vocabulary in front of it: the task a call served, whether the
//! counters came from the provider or from an estimate, how the call ended, and
//! the [`Call`] the pipelines hand over for recording.
//!
//! Two rules keep the numbers honest:
//!
//! - A counter the provider reported is stored verbatim (`Source::Reported`).
//!   Everything else is a four-characters-per-token estimate and is labelled
//!   as one (`Source::Estimated`).
//! - A cost is stored only when the provider sent one or [`rig`'s catalog]
//!   prices the reported counters. Otherwise it is `None`, never zero.
//!
//! [`rig`'s catalog]: rig::catalog::Catalog

use super::config::{LlmConfig, LlmProvider};
use super::error::LlmError;
use crate::prompt_render::estimated_tokens;

/// What a call was for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Task {
    Evaluation,
    /// The second, non-streamed call after an unusable evaluation reply.
    EvaluationRepair,
    CoverLetter,
    /// Reserved for Phase 3.2; nothing writes it yet.
    Triage,
    /// A task a newer build recorded. Kept as text rather than dropped.
    Other(String),
}

impl Task {
    /// The stored form. Changing one of these invalidates history, so they are
    /// part of the storage contract.
    pub fn as_str(&self) -> &str {
        match self {
            Self::Evaluation => "evaluation",
            Self::EvaluationRepair => "evaluation_repair",
            Self::CoverLetter => "cover_letter",
            Self::Triage => "triage",
            Self::Other(raw) => raw,
        }
    }

    /// Read a stored task. A value this build does not know is kept, not
    /// mapped onto a known task.
    pub fn parse(raw: &str) -> Self {
        match raw {
            "evaluation" => Self::Evaluation,
            "evaluation_repair" => Self::EvaluationRepair,
            "cover_letter" => Self::CoverLetter,
            "triage" => Self::Triage,
            other => Self::Other(other.to_string()),
        }
    }
}

impl std::fmt::Display for Task {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Where a row's token counts came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Reported,
    Estimated,
}

impl Source {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Reported => "reported",
            Self::Estimated => "estimated",
        }
    }

    /// Read a stored source. An unknown value reads as `Estimated`, the
    /// weaker claim: never present a row as more precise than it is.
    pub fn parse(raw: &str) -> Self {
        match raw {
            "reported" => Self::Reported,
            _ => Self::Estimated,
        }
    }
}

/// How a call ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Ok,
    Cancelled,
    Error,
}

impl Outcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Ok => "ok",
            Self::Cancelled => "cancelled",
            Self::Error => "error",
        }
    }

    /// Read a stored outcome. An unknown value reads as `Error`, the weaker
    /// claim.
    pub fn parse(raw: &str) -> Self {
        match raw {
            "ok" => Self::Ok,
            "cancelled" => Self::Cancelled,
            _ => Self::Error,
        }
    }

    /// The outcome of a failed call. Cancellation is its own outcome because
    /// the provider may still have billed for what it generated.
    pub fn of(error: &LlmError) -> Self {
        match error {
            LlmError::Cancelled => Self::Cancelled,
            _ => Self::Error,
        }
    }
}

/// One call's counters, in the app's own vocabulary.
///
/// All counters are `Option` because a provider may report none of them. A
/// reported zero is `Some(0)`; a missing counter is `None`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Usage {
    pub input_tokens: Option<u64>,
    pub output_tokens: Option<u64>,
    pub total_tokens: Option<u64>,
    pub cached_input_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
    /// `None` when no price is known. Never `Some(0.0)` as a stand-in.
    pub cost_usd: Option<f64>,
    /// Whether [`Self::cost_usd`] is the whole charge. `Some(false)` is a
    /// lower bound.
    pub cost_complete: Option<bool>,
    /// The provider sent counters (or a cost). `false` means every counter is
    /// an estimate.
    pub reported: bool,
}

impl Usage {
    /// No counter and no cost: what a backend that returns no usage reports.
    pub const fn unknown() -> Self {
        Self {
            input_tokens: None,
            output_tokens: None,
            total_tokens: None,
            cached_input_tokens: None,
            reasoning_tokens: None,
            cost_usd: None,
            cost_complete: None,
            reported: false,
        }
    }

    /// An estimate for a call the provider did not report on.
    ///
    /// Input comes from the assembled prompt; output from the reply when there
    /// is one. A failed or cancelled call has no reply, so its output stays
    /// `None` rather than zero.
    pub fn estimated(prompt: &str, reply: Option<&str>) -> Self {
        let input = estimated_tokens(prompt.chars().count()) as u64;
        let output = reply
            .map(|reply| estimated_tokens(reply.chars().count()) as u64)
            .filter(|tokens| *tokens > 0);

        Self {
            input_tokens: Some(input),
            output_tokens: output,
            total_tokens: output.map(|output| input + output),
            ..Self::unknown()
        }
    }

    /// Map `rig`'s usage, filling in a catalog price when the provider sent
    /// no cost of its own.
    pub(crate) fn from_rig(
        provider: LlmProvider,
        model: &str,
        usage: &rig::completion::Usage,
    ) -> Self {
        if !usage.is_reported() {
            return Self::unknown();
        }

        let cost = usage
            .cost
            .map(|cost| (cost.total, cost.is_complete()))
            .or_else(|| price(provider, model, usage).map(|cost| (cost.total, cost.is_complete())));

        Self {
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            total_tokens: usage.total_tokens,
            cached_input_tokens: usage.cached_input_tokens,
            reasoning_tokens: usage.reasoning_tokens,
            cost_usd: cost.map(|(total, _)| total),
            cost_complete: cost.map(|(_, complete)| complete),
            reported: true,
        }
    }
}

/// One provider call, ready to be recorded.
#[derive(Debug, Clone, PartialEq)]
pub struct Call {
    pub provider: LlmProvider,
    /// The model string actually sent, after any override.
    pub model: String,
    pub task: Task,
    pub usage: Usage,
    pub outcome: Outcome,
    /// The [`LlmError::kind`] of a failed call.
    pub error_kind: Option<&'static str>,
    /// Wall time from request to reply, or to failure.
    pub duration_ms: i64,
}

impl Call {
    /// A call that returned text. Reported counters are kept; otherwise the
    /// prompt and reply are estimated.
    pub fn completed(
        config: &LlmConfig,
        task: Task,
        reported: &Usage,
        prompt: &str,
        reply: &str,
        duration_ms: i64,
    ) -> Self {
        let usage = if reported.reported {
            *reported
        } else {
            Usage::estimated(prompt, Some(reply))
        };

        Self {
            provider: config.provider,
            model: config.model.clone(),
            task,
            usage,
            outcome: Outcome::Ok,
            error_kind: None,
            duration_ms,
        }
    }

    /// A call that failed or was cancelled. Its input is estimated from the
    /// prompt; there is no reply to estimate the output from.
    pub fn failed(
        config: &LlmConfig,
        task: Task,
        prompt: &str,
        error: &LlmError,
        duration_ms: i64,
    ) -> Self {
        Self {
            provider: config.provider,
            model: config.model.clone(),
            task,
            usage: Usage::estimated(prompt, None),
            outcome: Outcome::of(error),
            error_kind: Some(error.kind()),
            duration_ms,
        }
    }
}

/// Milliseconds since `started`, saturating at `i64::MAX`.
pub fn elapsed_ms(started: std::time::Instant) -> i64 {
    i64::try_from(started.elapsed().as_millis()).unwrap_or(i64::MAX)
}

/// Price `input_tokens` and `output_tokens` at the model's catalog rate.
///
/// For a pre-call estimate: the counters are our own estimate, not a provider
/// report. `None` when the catalog does not list the model. This is a list
/// price, so it is labelled as an estimate wherever it is shown.
pub fn catalog_cost(
    provider: LlmProvider,
    model: &str,
    input_tokens: u64,
    output_tokens: u64,
) -> Option<f64> {
    let usage = rig::completion::Usage::new()
        .input_tokens(input_tokens)
        .output_tokens(output_tokens)
        .total_tokens(input_tokens.saturating_add(output_tokens));
    price(provider, model, &usage).map(|cost| cost.total)
}

/// What `usage` costs at the model's catalog price, in USD.
///
/// The catalog is `rig`'s, not a table of ours: it ships with the pinned
/// version, knows the models this build can reach, and leaves an unknown model
/// unpriced rather than guessing. The figure is the standard-tier list price,
/// not an invoice.
fn price(
    provider: LlmProvider,
    model: &str,
    usage: &rig::completion::Usage,
) -> Option<rig::completion::Cost> {
    let reference = format!("{}/{model}", provider.as_str());
    let catalog = rig::catalog::Catalog::builtin();
    let resolved = catalog.resolve(&reference).ok()?;
    resolved.spec.pricing?.cost(usage)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reported(input: u64, output: u64) -> rig::completion::Usage {
        rig::completion::Usage::new()
            .input_tokens(input)
            .output_tokens(output)
            .total_tokens(input + output)
    }

    #[test]
    fn the_catalog_prices_a_reported_deepseek_call() {
        let found = Usage::from_rig(
            LlmProvider::DeepSeek,
            "deepseek-flash",
            &reported(1_000_000, 1_000_000),
        );

        assert!(found.reported);
        assert_eq!(found.input_tokens, Some(1_000_000));
        assert_eq!(found.output_tokens, Some(1_000_000));
        // The catalog lists 0.15 input and 0.6 output per million.
        let cost = found.cost_usd.expect("the catalog prices Flash");
        assert!((cost - 0.75).abs() < 1e-9, "cost was {cost}");
        assert_eq!(found.cost_complete, Some(true));
    }

    #[test]
    fn a_provider_reported_cost_wins_over_the_catalog() {
        let usage = reported(1_000_000, 1_000_000).cost(rig::completion::Cost::from_total(0.5));

        let found = Usage::from_rig(LlmProvider::DeepSeek, "deepseek-flash", &usage);
        assert_eq!(found.cost_usd, Some(0.5));
    }

    #[test]
    fn an_unknown_model_keeps_the_counters_and_leaves_the_cost_empty() {
        let found = Usage::from_rig(
            LlmProvider::DeepSeek,
            "deepseek-not-in-the-catalog",
            &reported(10, 5),
        );

        assert_eq!(found.input_tokens, Some(10));
        assert_eq!(found.cost_usd, None);
        assert_eq!(found.cost_complete, None);
    }

    #[test]
    fn an_unreported_usage_is_unknown() {
        let found = Usage::from_rig(
            LlmProvider::DeepSeek,
            "deepseek-flash",
            &rig::completion::Usage::new(),
        );
        assert_eq!(found, Usage::unknown());
        assert!(!found.reported);
    }

    #[test]
    fn an_estimate_counts_four_characters_per_token() {
        let prompt = "x".repeat(400);
        let reply = "y".repeat(80);
        let found = Usage::estimated(&prompt, Some(&reply));

        assert_eq!(found.input_tokens, Some(100));
        assert_eq!(found.output_tokens, Some(20));
        assert_eq!(found.total_tokens, Some(120));
        assert_eq!(found.cost_usd, None);
        assert!(!found.reported);
    }

    #[test]
    fn a_failed_call_has_no_output_estimate() {
        let found = Usage::estimated(&"x".repeat(400), None);
        assert_eq!(found.input_tokens, Some(100));
        assert_eq!(found.output_tokens, None);
        assert_eq!(found.total_tokens, None);
    }

    #[test]
    fn tasks_sources_and_outcomes_round_trip() {
        for task in [
            Task::Evaluation,
            Task::EvaluationRepair,
            Task::CoverLetter,
            Task::Triage,
        ] {
            assert_eq!(Task::parse(task.as_str()), task);
        }
        assert_eq!(
            Task::parse("a_later_task"),
            Task::Other("a_later_task".into())
        );

        assert_eq!(Source::parse("reported"), Source::Reported);
        assert_eq!(Source::parse("estimated"), Source::Estimated);
        assert_eq!(Source::parse("nonsense"), Source::Estimated);

        assert_eq!(Outcome::parse("ok"), Outcome::Ok);
        assert_eq!(Outcome::parse("cancelled"), Outcome::Cancelled);
        assert_eq!(Outcome::parse("error"), Outcome::Error);
        assert_eq!(Outcome::parse("nonsense"), Outcome::Error);
    }

    #[test]
    fn an_error_outcome_names_its_kind() {
        let error = LlmError::Transport("offline".into());
        assert_eq!(Outcome::of(&error), Outcome::Error);
        assert_eq!(error.kind(), "transport");

        let cancelled = LlmError::Cancelled;
        assert_eq!(Outcome::of(&cancelled), Outcome::Cancelled);
        assert_eq!(cancelled.kind(), "cancelled");
    }
}
