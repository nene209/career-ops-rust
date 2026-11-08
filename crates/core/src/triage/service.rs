//! The triage batch service: assemble one call, validate the answer, apply it.
//!
//! Order matters and mirrors the evaluation pipeline:
//!
//! 1. Everything local (loading the postings, the CV summary, the preferences,
//!    the prompt, rendering, the context check) happens before the provider
//!    call, so a local failure changes nothing.
//! 2. The call's usage is recorded immediately afterwards, before the reply is
//!    parsed: a failed or unusable reply still spent tokens.
//! 3. The parsed decisions are applied to the queue and the `triage_*` columns
//!    in one transaction. A stale id aborts the transaction rather than
//!    applying half the batch.
//!
//! Triage does not create applications, fetch descriptions, or evaluate. It
//! moves rows in the queue the user already controls.

use std::collections::HashSet;
use std::time::{Duration, Instant};

use sqlx::SqlitePool;

use crate::error::CoreError;
use crate::llm::usage::{catalog_cost, elapsed_ms, Call, Task};
use crate::llm::{capabilities, CancelToken, CompletionBackend, LlmConfig, LlmError};
use crate::prompt_render::{estimated_tokens, take_chars, MAX_CV_CHARS};
use crate::repo::job_descriptions::{self, TriageWrite};
use crate::repo::llm_usage;
use crate::repo::{profile, prompts};
use crate::text;

use super::output::{parse, TriageDecision};
use super::prompt::{self, TriageJob, BRIEF_CHARS, PROMPT_NAME};

/// The default number of postings one click covers.
pub const DEFAULT_BATCH_LIMIT: usize = 50;

/// The ceiling a caller may raise the limit to. A batch is one call; a click
/// should stay a click.
pub const MAX_BATCH_LIMIT: usize = 100;

/// The output estimate per posting: one short JSON object with a sentence.
pub const ESTIMATED_OUTPUT_TOKENS_PER_JOB: u64 = 60;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriageRequest {
    /// The postings to sort. Duplicates are ignored.
    pub job_ids: Vec<i64>,
    /// The most postings this run may cover; clamped to [`MAX_BATCH_LIMIT`].
    pub limit: usize,
    /// Stop before the call, or before the decisions are applied.
    pub cancel: CancelToken,
}

impl TriageRequest {
    pub fn new(job_ids: Vec<i64>) -> Self {
        Self {
            job_ids,
            limit: DEFAULT_BATCH_LIMIT,
            cancel: CancelToken::default(),
        }
    }
}

/// What the pre-call line shows.
#[derive(Debug, Clone, PartialEq)]
pub struct TriageEstimate {
    pub jobs: usize,
    pub model: String,
    pub input_chars: usize,
    pub input_tokens: usize,
    pub estimated_output_tokens: u64,
    /// A list-price estimate; `None` when the catalog does not price the model.
    pub estimated_cost_usd: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TriageReport {
    pub decisions: Vec<TriageDecision>,
    pub prompt_version: i64,
    pub prompt_hash: String,
    pub model_used: String,
    pub input_chars: usize,
    pub estimated_cost_usd: Option<f64>,
    /// What the provider or the catalog priced for the call itself.
    pub actual_cost_usd: Option<f64>,
    pub elapsed: Duration,
}

#[derive(Debug, thiserror::Error)]
pub enum TriageError {
    #[error("the batch has {requested} postings; this run is limited to {limit}")]
    BatchTooLarge { requested: usize, limit: usize },

    #[error("no postings to triage")]
    EmptyBatch,

    #[error("the model's reply was not usable: {0}")]
    InvalidResponse(String),

    #[error(transparent)]
    Llm(#[from] LlmError),

    #[error(transparent)]
    Core(#[from] CoreError),
}

impl From<TriageError> for CoreError {
    fn from(error: TriageError) -> Self {
        CoreError::Message(error.to_string())
    }
}

/// Estimate the batch without calling the provider.
pub async fn estimate(
    pool: &SqlitePool,
    request: &TriageRequest,
) -> Result<TriageEstimate, TriageError> {
    let prepared = prepare(pool, request).await?;

    Ok(TriageEstimate {
        jobs: prepared.ids.len(),
        model: prepared.config.model.clone(),
        input_chars: prepared.input_chars,
        input_tokens: estimated_tokens(prepared.input_chars),
        estimated_output_tokens: prepared.estimated_output_tokens,
        estimated_cost_usd: prepared.estimated_cost_usd,
    })
}

/// Sort a batch of postings and apply the decisions.
pub async fn run_triage(
    pool: &SqlitePool,
    request: &TriageRequest,
    backend: &dyn CompletionBackend,
) -> Result<TriageReport, TriageError> {
    let prepared = prepare(pool, request).await?;

    let started = Instant::now();
    let result = tokio::select! {
        biased;
        _ = request.cancel.wait() => Err(LlmError::Cancelled),
        result = backend.complete(&prepared.assembled, None, true, None) => result,
    };
    let duration_ms = elapsed_ms(started);

    let call = match &result {
        Ok(completion) => Call::completed(
            &prepared.config,
            Task::Triage,
            &completion.usage,
            &prepared.assembled,
            &completion.text,
            duration_ms,
        ),
        Err(error) => Call::failed(
            &prepared.config,
            Task::Triage,
            &prepared.assembled,
            error,
            duration_ms,
        ),
    };

    // The tokens were spent whether or not the reply is usable, and whether or
    // not the decisions are applied. Record the call first.
    llm_usage::record_best_effort(pool, std::slice::from_ref(&call), None, None).await;

    let completion = result.map_err(TriageError::Llm)?;
    let output = parse(&completion.text, &prepared.ids).map_err(TriageError::InvalidResponse)?;

    // A Stop pressed while the reply was arriving skips the write; the call is
    // already in the ledger.
    if request.cancel.is_cancelled() {
        return Err(TriageError::Llm(LlmError::Cancelled));
    }

    let writes: Vec<TriageWrite<'_>> = output
        .decisions
        .iter()
        .map(|decision| TriageWrite {
            job_id: decision.job_id,
            queue_state: decision.decision.queue_state(),
            decision: decision.decision.as_str(),
            reason: decision.reason.trim(),
            model: &prepared.config.model,
            prompt_hash: &prepared.prompt.content_hash,
        })
        .collect();

    let mut tx = pool.begin().await.map_err(CoreError::from)?;
    let updated = job_descriptions::apply_triage_tx(&mut tx, &writes).await?;
    if updated != writes.len() {
        return Err(TriageError::Core(CoreError::Message(format!(
            "only {updated} of {} postings could be updated; reload the list",
            writes.len()
        ))));
    }
    tx.commit().await.map_err(CoreError::from)?;

    tracing::info!(
        jobs = prepared.ids.len(),
        model = %prepared.config.model,
        prompt_version = prepared.prompt.version,
        elapsed_ms = duration_ms,
        "triage stored"
    );

    Ok(TriageReport {
        decisions: output.decisions,
        prompt_version: prepared.prompt.version,
        prompt_hash: prepared.prompt.content_hash,
        model_used: prepared.config.model,
        input_chars: prepared.input_chars,
        estimated_cost_usd: prepared.estimated_cost_usd,
        actual_cost_usd: call.usage.cost_usd,
        elapsed: Duration::from_millis(duration_ms.max(0) as u64),
    })
}

/// Everything the call needs, prepared once for both the estimate and the run.
struct Prepared {
    config: LlmConfig,
    prompt: crate::model::Prompt,
    ids: Vec<i64>,
    assembled: String,
    input_chars: usize,
    estimated_output_tokens: u64,
    estimated_cost_usd: Option<f64>,
}

async fn prepare(pool: &SqlitePool, request: &TriageRequest) -> Result<Prepared, TriageError> {
    let limit = request.limit.min(MAX_BATCH_LIMIT);
    if request.job_ids.len() > limit {
        return Err(TriageError::BatchTooLarge {
            requested: request.job_ids.len(),
            limit,
        });
    }

    let mut seen = HashSet::new();
    let requested: Vec<i64> = request
        .job_ids
        .iter()
        .copied()
        .filter(|id| seen.insert(*id))
        .collect();
    if requested.is_empty() {
        return Err(TriageError::EmptyBatch);
    }

    let mut jobs = Vec::with_capacity(requested.len());
    for id in &requested {
        match job_descriptions::get(pool, *id).await? {
            Some(job) => jobs.push(job),
            // A row that vanished between selection and this run is left out
            // rather than sending a stale id to the model.
            None => tracing::warn!(
                job = *id,
                "a requested posting is gone; left out of the batch"
            ),
        }
    }
    if jobs.is_empty() {
        return Err(TriageError::EmptyBatch);
    }

    let config = LlmConfig::load(pool).await?;
    if !capabilities(config.provider, &config.model)
        .structured_output
        .is_usable()
    {
        return Err(TriageError::Core(CoreError::Message(format!(
            "{} cannot return structured output; choose another model in Settings.",
            config.model
        ))));
    }

    let prompt = prompts::active_for_use(pool, PROMPT_NAME)
        .await?
        .ok_or_else(|| {
            TriageError::Core(CoreError::Message(
                "no prompt named `triage` is installed".into(),
            ))
        })?;

    let cv_summary = profile::cv_summary(pool).await?.ok_or_else(|| {
        TriageError::Core(CoreError::Message(
            "Add your CV in CV / Profile before triaging.".into(),
        ))
    })?;
    let cv_summary = take_chars(&cv_summary, MAX_CV_CHARS);
    let preferences = profile::preferences(pool).await?;

    let briefs: Vec<String> = jobs
        .iter()
        .map(|job| text::plain_text(&job.raw_text, BRIEF_CHARS).text)
        .collect();
    let triage_jobs: Vec<TriageJob<'_>> = jobs
        .iter()
        .zip(&briefs)
        .map(|(job, brief)| TriageJob {
            id: job.id,
            title: &job.title,
            company: &job.company,
            brief,
        })
        .collect();

    let assembled = prompt::assemble(&prompt.content, &cv_summary, &preferences, &triage_jobs)
        .map_err(|error| {
            TriageError::Core(CoreError::Message(format!(
                "the `triage` prompt cannot be rendered: {error}"
            )))
        })?;

    let input_chars = assembled.chars().count();
    let input_tokens = estimated_tokens(input_chars);
    if let Some(window) = capabilities(config.provider, &config.model).context_window {
        if input_tokens > window {
            return Err(TriageError::Llm(LlmError::ContextLengthExceeded {
                input_chars: Some(input_chars),
                limit: Some(window),
                detail: format!("about {input_tokens} tokens for a {window}-token window"),
            }));
        }
    }

    let ids: Vec<i64> = jobs.iter().map(|job| job.id).collect();
    let estimated_output_tokens =
        (ids.len() as u64).saturating_mul(ESTIMATED_OUTPUT_TOKENS_PER_JOB);
    let estimated_cost_usd = catalog_cost(
        config.provider,
        &config.model,
        input_tokens as u64,
        estimated_output_tokens,
    );

    Ok(Prepared {
        config,
        prompt,
        ids,
        assembled,
        input_chars,
        estimated_output_tokens,
        estimated_cost_usd,
    })
}
