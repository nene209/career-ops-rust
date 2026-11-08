//! The evaluation pipeline: assemble, call, validate, store.
//!
//! Order matters here, and it is deliberate:
//!
//! 1. Everything local (the optional description fetch, the CV summary, the
//!    prompt, rendering) happens **before** the application's status changes,
//!    so a local failure cannot leave a row stuck in `evaluating`.
//! 2. The model call is the only step that can fail after the status moves, so
//!    that failure restores the previous status.
//! 3. The report, the application link, and the job's `evaluated` queue state
//!    are written in **one transaction**. A report must never exist on a row
//!    that still says `evaluate`.

use sqlx::SqlitePool;
use std::time::Instant;

use crate::acquisition::manual;
use crate::acquisition::router::FetchSource;
use crate::error::{CoreError, Result};
use crate::llm::usage::{elapsed_ms, Call, Task};
use crate::llm::{
    capabilities, CancelToken, Completion, CompletionBackend, DeltaSink, LlmConfig, LlmError,
};
use crate::model::{ApplicationStatus, QueueState};
use crate::prompt_render::{estimated_tokens, render, take_chars, untrusted_block, Variables};
use crate::repo::evaluation_reports::{self, NewEvaluationReport};
use crate::repo::llm_usage::{self, NewLlmUsage};
use crate::repo::{applications, job_descriptions, profile, prompts};
use crate::text;

use super::output::{parse as parse_output, EvaluationOutput, Recommendation};

/// The prompt a request uses when it does not name one.
pub const DEFAULT_PROMPT: &str = "evaluate";

/// The most jobs one batch call will evaluate. There is no batch UI in Phase 3;
/// this exists for tests and for a future UI, and a click should stay a click.
pub const MAX_BATCH: usize = 10;

/// The CV summary's own budget, shared with every prompt that sends one.
pub use crate::prompt_render::MAX_CV_CHARS;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvaluationRequest {
    /// The Pipeline row to evaluate. An application is ensured for it.
    pub job_description_id: i64,
    /// Resolved through `prompts::active_for_use` when the run starts.
    pub prompt_name: String,
    /// Try a different model than the configured one, without rebuilding the
    /// client or changing the setting.
    pub model_override: Option<String>,
    /// Ask the backend to stream fragments to the caller's sink as well.
    pub stream: bool,
    /// Fetch the posting first when its stored text is only a card teaser.
    pub fetch_description_if_thin: bool,
    /// Stop the call when the UI asks. The failure path restores the status
    /// and stores nothing.
    pub cancel: CancelToken,
}

impl EvaluationRequest {
    pub fn new(job_description_id: i64) -> Self {
        Self {
            job_description_id,
            prompt_name: DEFAULT_PROMPT.to_string(),
            model_override: None,
            stream: false,
            fetch_description_if_thin: false,
            cancel: CancelToken::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct EvaluationResult {
    pub report_id: i64,
    pub application_id: i64,
    /// `0.0..=1.0`; the rubric's global 1–5 score divided by five.
    pub match_score: f32,
    pub recommendation: Recommendation,
    /// The markdown report as stored.
    pub content: String,
    pub prompt_version: i64,
    pub prompt_hash: String,
    pub model_used: String,
    /// Characters of job text sent, after normalization and the budget.
    pub input_chars: usize,
    /// The stored text size before normalization.
    pub source_chars: usize,
    /// The job text was cut to fit the budget.
    pub truncated: bool,
}

/// Evaluate one job and store the report.
///
/// `fetch` is required only when `request.fetch_description_if_thin` is set and
/// the row is thin; the app passes its HTTP source. `deltas`, when set, receives
/// the streamed text for display.
pub async fn run_evaluation(
    pool: &SqlitePool,
    request: &EvaluationRequest,
    backend: &dyn CompletionBackend,
    fetch: Option<&dyn FetchSource>,
    deltas: Option<&DeltaSink>,
) -> Result<EvaluationResult> {
    let mut job = job_descriptions::get(pool, request.job_description_id)
        .await?
        .ok_or_else(|| {
            CoreError::Message(format!("job {} was not found", request.job_description_id))
        })?;

    let mut config = LlmConfig::load(pool).await?;
    if let Some(model) = request
        .model_override
        .as_deref()
        .map(str::trim)
        .filter(|model| !model.is_empty())
    {
        config.model = model.to_string();
    }

    if !capabilities(config.provider, &config.model)
        .structured_output
        .is_usable()
    {
        return Err(CoreError::Message(format!(
            "{} cannot return structured output; choose another model in Settings.",
            config.model
        )));
    }
    let context_window = capabilities(config.provider, &config.model).context_window;

    // A thin row is a card teaser: evaluating it would spend tokens to score a
    // headline. Fetching first is the only way the answer means anything.
    if request.fetch_description_if_thin
        && normalized_chars(&job.raw_text) < config.thin_description_chars
    {
        let Some(source) = fetch else {
            return Err(CoreError::Message(
                "the stored description is short and no fetch source is available".into(),
            ));
        };
        job = manual::refresh_job(pool, job.id, source)
            .await
            .map_err(|error| {
                CoreError::Message(format!("could not fetch the description: {error}"))
            })?;
    }

    let normalized = text::plain_text(&job.raw_text, config.max_job_chars);
    let input_chars = normalized.text.chars().count();
    let source_chars = job.raw_text.chars().count();
    // Fence the untrusted posting so the model can tell data from instructions.
    let untrusted = untrusted_block(&normalized.text);

    let cv_summary = profile::cv_summary(pool).await?.ok_or_else(|| {
        CoreError::Message("Add your CV in CV / Profile before evaluating.".into())
    })?;
    let cv_summary = take_chars(&cv_summary, MAX_CV_CHARS);

    let prompt = prompts::active_for_use(pool, &request.prompt_name)
        .await?
        .ok_or_else(|| {
            CoreError::Message(format!(
                "no prompt named `{}` is installed",
                request.prompt_name
            ))
        })?;

    let assembled = render(
        &prompt.content,
        &Variables {
            cv_summary: &cv_summary,
            job_description: &untrusted,
            company_name: &job.company,
            job_title: &job.title,
            tone: "",
            ..Variables::default()
        },
    )
    .map_err(|error| {
        CoreError::Message(format!(
            "the `{}` prompt cannot be rendered: {error}",
            prompt.name
        ))
    })?;

    // The character budgets keep the usual prompt small, but a stored job can
    // exceed the model's window anyway (a huge description, a large CV). The
    // check runs before any status change, so a refusal leaves the row alone.
    let assembled_chars = assembled.chars().count();
    if let Some(window) = context_window {
        let estimate = estimated_tokens(assembled_chars);
        if estimate > window {
            return Err(CoreError::from(LlmError::ContextLengthExceeded {
                input_chars: Some(assembled_chars),
                limit: Some(window),
                detail: format!("about {estimate} tokens for a {window}-token window"),
            }));
        }
    }

    let application = applications::ensure(pool, job.id).await?;
    let previous = ApplicationStatus::parse(&application.status).ok_or_else(|| {
        CoreError::Message(format!(
            "application {} has an unknown status",
            application.id
        ))
    })?;

    // Q-43: an evaluation is a visible step in the lifecycle. Only a row that
    // has not moved past discovery takes part; a submitted application is left
    // alone, and so is one already marked not a match.
    let moved_to_evaluating = matches!(
        previous,
        ApplicationStatus::Discovered | ApplicationStatus::Ready
    );
    if moved_to_evaluating {
        applications::transition(pool, application.id, ApplicationStatus::Evaluating).await?;
    }

    // The call's usage is recorded beside the report: one row per attempt,
    // including a repair. A failed call still spent tokens, so its rows are
    // written best-effort before the error is returned.
    let (asked, calls) = ask(backend, &assembled, request, deltas, &config).await;
    let output = match asked {
        Ok(output) => output,
        Err(error) => {
            llm_usage::record_best_effort(pool, &calls, Some(application.id), Some(job.id)).await;
            if moved_to_evaluating {
                if let Err(restore) = applications::transition(pool, application.id, previous).await
                {
                    tracing::warn!(
                        application = application.id,
                        %restore,
                        "could not restore the application status after a failed evaluation"
                    );
                }
            }
            return Err(error);
        }
    };

    let mut tx = pool.begin().await?;
    let report_id = evaluation_reports::insert_tx(
        &mut tx,
        NewEvaluationReport {
            application_id: application.id,
            content: &output.full_report_markdown,
            model_used: &config.model,
            prompt_version: Some(prompt.version),
            prompt_hash: Some(prompt.content_hash.as_str()),
            match_score: Some(f64::from(output.match_score)),
            recommendation: Some(output.recommendation.as_str()),
            input_chars: Some(input_chars as i64),
            source_chars: Some(source_chars as i64),
            truncated: normalized.truncated,
        },
    )
    .await?;

    let usage_rows: Vec<NewLlmUsage<'_>> = calls
        .iter()
        .map(|call| NewLlmUsage::from_call(call, Some(application.id), Some(job.id)))
        .collect();
    llm_usage::insert_many_tx(&mut tx, &usage_rows).await?;

    applications::finish_evaluation_tx(
        &mut tx,
        application.id,
        report_id,
        f64::from(output.match_score),
        moved_to_evaluating.then_some(ApplicationStatus::Ready),
    )
    .await?;
    job_descriptions::set_queue_state_tx(&mut tx, job.id, QueueState::Evaluated, None).await?;
    tx.commit().await?;

    // `NoMatch` is a status change the writer may refuse (a submitted
    // application); the report is already stored either way.
    if output.recommendation == Recommendation::NoMatch {
        if let Err(error) = applications::mark_not_a_match(pool, job.id).await {
            tracing::warn!(
                job = job.id,
                %error,
                "the report recommends no match but the status could not be changed"
            );
        }
    }

    tracing::info!(
        job = job.id,
        report = report_id,
        model = %config.model,
        score = f64::from(output.match_score),
        recommendation = output.recommendation.as_str(),
        input_chars,
        truncated = normalized.truncated,
        "evaluation stored"
    );

    Ok(EvaluationResult {
        report_id,
        application_id: application.id,
        match_score: output.match_score,
        recommendation: output.recommendation,
        content: output.full_report_markdown,
        prompt_version: prompt.version,
        prompt_hash: prompt.content_hash,
        model_used: config.model,
        input_chars,
        source_chars,
        truncated: normalized.truncated,
    })
}

/// Evaluate up to [`MAX_BATCH`] jobs in sequence.
///
/// Failures are returned per job rather than aborting the batch: one bad
/// posting should not cancel the other nine. There is no UI for this in
/// Phase 3.
pub async fn run_batch(
    pool: &SqlitePool,
    requests: &[EvaluationRequest],
    backend: &dyn CompletionBackend,
    fetch: Option<&dyn FetchSource>,
) -> Vec<Result<EvaluationResult>> {
    let mut results = Vec::with_capacity(requests.len().min(MAX_BATCH));
    for request in requests.iter().take(MAX_BATCH) {
        results.push(run_evaluation(pool, request, backend, fetch, None).await);
    }
    results
}

/// One model call, plus one repair attempt when the reply is usable JSON but
/// not a usable report.
///
/// The repair is a second call because a streamed reply cannot be parsed
/// before it ends; the second call does not stream, so the display is not
/// doubled. Every attempt, successful or not, is returned as a [`Call`] so the
/// caller can record it.
async fn ask(
    backend: &dyn CompletionBackend,
    prompt: &str,
    request: &EvaluationRequest,
    deltas: Option<&DeltaSink>,
    config: &LlmConfig,
) -> (Result<EvaluationOutput>, Vec<Call>) {
    let model = request.model_override.as_deref();
    let deltas = if request.stream { deltas } else { None };
    let mut calls = Vec::new();

    let (first, call) = timed_call(
        backend,
        prompt,
        model,
        true,
        deltas,
        request.cancel.clone(),
        config,
        Task::Evaluation,
    )
    .await;
    calls.push(call);

    let first = match first {
        Ok(completion) => completion,
        Err(error) => return (Err(CoreError::from(error)), calls),
    };

    match parse_output(&first.text) {
        Ok(output) => (Ok(output), calls),
        Err(reason) => {
            tracing::warn!(%reason, "the evaluation reply was not usable; asking once more");
            let repair = format!(
                "{prompt}\n\nYour previous reply could not be used: {reason}. Reply with only the \
                 JSON object described above, with no prose and no code fences."
            );
            let (second, call) = timed_call(
                backend,
                &repair,
                model,
                true,
                None,
                request.cancel.clone(),
                config,
                Task::EvaluationRepair,
            )
            .await;
            calls.push(call);

            let result = match second {
                Ok(completion) => parse_output(&completion.text).map_err(|reason| {
                    CoreError::Message(format!("the model's reply was not usable: {reason}"))
                }),
                Err(error) => Err(CoreError::from(error)),
            };
            (result, calls)
        }
    }
}

fn normalized_chars(raw: &str) -> usize {
    text::plain_text(raw, usize::MAX).text.chars().count()
}

/// One provider call, timed, with its usage turned into a recordable [`Call`].
///
/// The clock runs around the whole attempt, so a streamed reply's duration is
/// what the user waited for, and a failure has a duration too.
#[allow(clippy::too_many_arguments)]
async fn timed_call(
    backend: &dyn CompletionBackend,
    prompt: &str,
    model: Option<&str>,
    json_mode: bool,
    deltas: Option<&DeltaSink>,
    cancel: CancelToken,
    config: &LlmConfig,
    task: Task,
) -> (std::result::Result<Completion, LlmError>, Call) {
    let started = Instant::now();
    let result = complete_or_cancel(backend, prompt, model, json_mode, deltas, cancel).await;
    let duration_ms = elapsed_ms(started);

    let call = match &result {
        Ok(completion) => Call::completed(
            config,
            task,
            &completion.usage,
            prompt,
            &completion.text,
            duration_ms,
        ),
        Err(error) => Call::failed(config, task, prompt, error, duration_ms),
    };

    (result, call)
}

/// One provider call, raced against the request's stop token.
///
/// `biased` matters: a token already set when the call starts must win without
/// polling the provider, so a Stop pressed before the request leaves the app
/// never sends it.
async fn complete_or_cancel(
    backend: &dyn CompletionBackend,
    prompt: &str,
    model: Option<&str>,
    json_mode: bool,
    deltas: Option<&DeltaSink>,
    cancel: CancelToken,
) -> std::result::Result<Completion, LlmError> {
    tokio::select! {
        biased;
        _ = cancel.wait() => Err(LlmError::Cancelled),
        result = backend.complete(prompt, model, json_mode, deltas) => result,
    }
}
