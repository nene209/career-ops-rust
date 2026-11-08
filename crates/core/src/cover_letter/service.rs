//! The cover-letter pipeline: assemble, ask, store.
//!
//! Order mirrors `evaluation::service`: every local step (normalization, CV,
//! prompt resolution, rendering, context check) runs before any write, so a
//! failure leaves nothing half-stored. The reply is free text, not JSON; the
//! prompt contract is the only constraint, and an empty reply is an error rather
//! than a stored blank letter.
//!
//! Regeneration is an insert, never an update: the history is the record of how
//! the letter changed.

use sqlx::SqlitePool;

use crate::error::{CoreError, Result};
use crate::llm::{capabilities, CancelToken, CompletionBackend, LlmConfig, LlmError};
use crate::prompt_render::{
    estimated_tokens, render, take_chars, untrusted_block, Variables, MAX_CV_CHARS,
};
use crate::repo::cover_letters::{self, NewCoverLetter};
use crate::repo::{applications, job_descriptions, profile, prompts};
use crate::text;

/// The prompt a request uses when it does not name one.
pub const DEFAULT_PROMPT: &str = "cover_letter";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoverLetterRequest {
    /// The Pipeline row to write for. An application is ensured for it.
    pub job_description_id: i64,
    /// Resolved through `prompts::active_for_use` when the run starts.
    pub prompt_name: String,
    /// Try a different model than the configured one, without rebuilding the
    /// client or changing the setting.
    pub model_override: Option<String>,
    /// A soft register for the letter; empty when the user did not choose one.
    pub tone: Option<String>,
    /// Stop the call when the UI asks. Nothing is stored on cancellation.
    pub cancel: CancelToken,
}

impl CoverLetterRequest {
    pub fn new(job_description_id: i64) -> Self {
        Self {
            job_description_id,
            prompt_name: DEFAULT_PROMPT.to_string(),
            model_override: None,
            tone: None,
            cancel: CancelToken::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct CoverLetterResult {
    pub cover_letter_id: i64,
    pub application_id: i64,
    /// The letter as stored, in markdown.
    pub content: String,
    pub prompt_version: i64,
    pub prompt_hash: String,
    /// The model string sent to the provider, verbatim.
    pub model_used: String,
    /// Characters of prompt input sent, after normalization and the budgets.
    pub input_chars: usize,
}

/// Generate one cover letter and store it.
pub async fn run_cover_letter(
    pool: &SqlitePool,
    request: &CoverLetterRequest,
    backend: &dyn CompletionBackend,
) -> Result<CoverLetterResult> {
    let job = job_descriptions::get(pool, request.job_description_id)
        .await?
        .ok_or_else(|| {
            CoreError::Message(format!("job {} was not found", request.job_description_id))
        })?;

    let mut config = LlmConfig::load(pool).await?;
    let model_override = request
        .model_override
        .as_deref()
        .map(str::trim)
        .filter(|model| !model.is_empty());
    if let Some(model) = model_override {
        config.model = model.to_string();
    }

    // The same normalization the evaluation uses: a letter must not see tags,
    // and the posting is untrusted input either way.
    let normalized = text::plain_text(&job.raw_text, config.max_job_chars);
    let untrusted = untrusted_block(&normalized.text);

    let cv_summary = profile::cv_summary(pool).await?.ok_or_else(|| {
        CoreError::Message("Add your CV in CV / Profile before writing a cover letter.".into())
    })?;
    let cv_summary = take_chars(&cv_summary, MAX_CV_CHARS);

    let tone = request.tone.as_deref().map(str::trim).unwrap_or("");

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
            tone,
        },
    )
    .map_err(|error| {
        CoreError::Message(format!(
            "the `{}` prompt cannot be rendered: {error}",
            prompt.name
        ))
    })?;

    let input_chars = assembled.chars().count();
    if let Some(window) = capabilities(config.provider, &config.model).context_window {
        let estimate = estimated_tokens(input_chars);
        if estimate > window {
            return Err(CoreError::from(LlmError::ContextLengthExceeded {
                input_chars: Some(input_chars),
                limit: Some(window),
                detail: format!("about {estimate} tokens for a {window}-token window"),
            }));
        }
    }

    let application = applications::ensure(pool, job.id).await?;

    let reply = {
        // `biased` so a token already set when the call starts wins without
        // polling the provider.
        tokio::select! {
            biased;
            _ = request.cancel.wait() => return Err(CoreError::from(LlmError::Cancelled)),
            result = backend.complete(&assembled, model_override, false, None) => {
                result.map_err(CoreError::from)?
            }
        }
    };
    let content = reply.trim();
    if content.is_empty() {
        return Err(CoreError::Message(
            "the model returned an empty cover letter".into(),
        ));
    }

    let cover_letter_id = cover_letters::insert(
        pool,
        NewCoverLetter {
            application_id: application.id,
            content,
            model_used: &config.model,
            prompt_version: Some(prompt.version),
            prompt_hash: Some(prompt.content_hash.as_str()),
            input_chars: Some(input_chars as i64),
        },
    )
    .await?;

    tracing::info!(
        job = job.id,
        cover_letter = cover_letter_id,
        model = %config.model,
        prompt_version = prompt.version,
        input_chars,
        "cover letter stored"
    );

    Ok(CoverLetterResult {
        cover_letter_id,
        application_id: application.id,
        content: content.to_string(),
        prompt_version: prompt.version,
        prompt_hash: prompt.content_hash,
        model_used: config.model,
        input_chars,
    })
}
