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

use crate::acquisition::manual;
use crate::acquisition::router::FetchSource;
use crate::error::{CoreError, Result};
use crate::llm::{capabilities, DeltaSink, LlmConfig, LlmError};
use crate::model::{ApplicationStatus, QueueState};
use crate::prompt_render::{render, Variables};
use crate::repo::evaluation_reports::{self, NewEvaluationReport};
use crate::repo::{applications, job_descriptions, profile, prompts};
use crate::text;

use super::backend::CompletionBackend;
use super::output::{parse as parse_output, EvaluationOutput, Recommendation};

/// The prompt a request uses when it does not name one.
pub const DEFAULT_PROMPT: &str = "evaluate";

/// The most jobs one batch call will evaluate. There is no batch UI in Phase 3;
/// this exists for tests and for a future UI, and a click should stay a click.
pub const MAX_BATCH: usize = 10;

/// The CV summary's own budget, in characters (about 1.5k tokens). The job text
/// has `llm.max_job_chars`; the CV is the other half of the prompt.
pub const MAX_CV_CHARS: usize = 6_000;

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
}

impl EvaluationRequest {
    pub fn new(job_description_id: i64) -> Self {
        Self {
            job_description_id,
            prompt_name: DEFAULT_PROMPT.to_string(),
            model_override: None,
            stream: false,
            fetch_description_if_thin: false,
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
            job_description: &normalized.text,
            company_name: &job.company,
            job_title: &job.title,
            tone: "",
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

    let output = match ask(backend, &assembled, request, deltas).await {
        Ok(output) => output,
        Err(error) => {
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
/// doubled.
async fn ask(
    backend: &dyn CompletionBackend,
    prompt: &str,
    request: &EvaluationRequest,
    deltas: Option<&DeltaSink>,
) -> Result<EvaluationOutput> {
    let model = request.model_override.as_deref();
    let deltas = if request.stream { deltas } else { None };

    let first = backend
        .complete(prompt, model, true, deltas)
        .await
        .map_err(CoreError::from)?;

    match parse_output(&first) {
        Ok(output) => Ok(output),
        Err(reason) => {
            tracing::warn!(%reason, "the evaluation reply was not usable; asking once more");
            let repair = format!(
                "{prompt}\n\nYour previous reply could not be used: {reason}. Reply with only the \
                 JSON object described above, with no prose and no code fences."
            );
            let second = backend
                .complete(&repair, model, true, None)
                .await
                .map_err(CoreError::from)?;
            parse_output(&second).map_err(|reason| {
                CoreError::Message(format!("the model's reply was not usable: {reason}"))
            })
        }
    }
}

fn normalized_chars(raw: &str) -> usize {
    text::plain_text(raw, usize::MAX).text.chars().count()
}

/// A character-based token estimate, using the same ~4 characters per token
/// proxy as the budgets. It is deliberately conservative on the low side: the
/// point is to refuse an obviously oversized prompt, not to price a call.
fn estimated_tokens(chars: usize) -> usize {
    chars.div_ceil(4)
}

fn take_chars(text: &str, max: usize) -> String {
    if text.chars().count() <= max {
        return text.to_string();
    }
    let mut cut: String = text.chars().take(max).collect();
    cut.push('…');
    cut
}
