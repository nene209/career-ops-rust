//! The portal-suggestion service: assemble one call, validate the answer,
//! derive the boards.
//!
//! Order mirrors the other pipelines:
//!
//! 1. Everything local (the config, the prompt, the CV summary, the
//!    preferences, rendering, the context check) happens before the call, so a
//!    local failure costs nothing.
//! 2. The call's usage is recorded immediately afterwards, before the reply is
//!    parsed: a failed or unusable reply still spent tokens.
//! 3. The reply is validated and converted. The model never supplies a URL;
//!    each one is derived from the engine and the slug, then checked against
//!    the embedded registry so a known board carries its exact config.
//!
//! Nothing is written to `portals`. The report is a list of proposals the UI
//! shows; adding one is a separate, explicit approval.

use std::time::{Duration, Instant};

use sqlx::SqlitePool;
use url::Url;

use crate::acquisition::known_sites;
use crate::error::CoreError;
use crate::llm::usage::{catalog_cost, elapsed_ms, Call, Task};
use crate::llm::{CancelToken, CompletionBackend, LlmConfig, LlmError};
use crate::prompt_render::{estimated_tokens, take_chars, MAX_CV_CHARS};
use crate::repo::llm_usage;
use crate::repo::{profile, prompts};

use super::output::{parse, SuggestedPortal};
use super::prompt::{self, PROMPT_NAME};

/// The output estimate per proposal: one short JSON object plus a sentence.
pub const ESTIMATED_OUTPUT_TOKENS_PER_SUGGESTION: u64 = 100;

/// One proposal, ready for the review row and for `portals::create`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortalSuggestion {
    pub company: String,
    /// The derived URL's host, shown beside the engine.
    pub host: String,
    pub engine_label: String,
    pub open_roles: Option<u32>,
    /// One sentence, from the model.
    pub rationale: String,
    /// The board URL derived from the engine and the slug.
    pub url: String,
    /// The config to store: a known site's exact config, or the engine's
    /// fallback. `None` means an empty config, which is right for an ATS board.
    pub config: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SuggestReport {
    pub suggestions: Vec<PortalSuggestion>,
    pub prompt_version: i64,
    pub prompt_hash: String,
    pub model_used: String,
    pub input_chars: usize,
    /// A list-price estimate made before the call.
    pub estimated_cost_usd: Option<f64>,
    /// What the provider or the catalog priced for the call itself.
    pub actual_cost_usd: Option<f64>,
    pub elapsed: Duration,
}

#[derive(Debug, thiserror::Error)]
pub enum SuggestError {
    #[error("the model's reply was not usable: {0}")]
    InvalidResponse(String),

    #[error(transparent)]
    Llm(#[from] LlmError),

    #[error(transparent)]
    Core(#[from] CoreError),
}

impl From<SuggestError> for CoreError {
    fn from(error: SuggestError) -> Self {
        CoreError::Message(error.to_string())
    }
}

/// Ask the model for portal proposals.
///
/// `cancel` lets the Stop button abandon the call; the usage row is recorded
/// either way, because the provider may still have billed for the attempt.
pub async fn suggest_portals(
    pool: &SqlitePool,
    cancel: &CancelToken,
    backend: &dyn CompletionBackend,
) -> Result<SuggestReport, SuggestError> {
    let prepared = prepare(pool).await?;

    let started = Instant::now();
    let result = tokio::select! {
        biased;
        _ = cancel.wait() => Err(LlmError::Cancelled),
        result = backend.complete(&prepared.assembled, None, true, None) => result,
    };
    let duration_ms = elapsed_ms(started);

    let call = match &result {
        Ok(completion) => Call::completed(
            &prepared.config,
            Task::SuggestPortals,
            &completion.usage,
            &prepared.assembled,
            &completion.text,
            duration_ms,
        ),
        Err(error) => Call::failed(
            &prepared.config,
            Task::SuggestPortals,
            &prepared.assembled,
            error,
            duration_ms,
        ),
    };

    // The tokens were spent whether or not the reply is usable.
    llm_usage::record_best_effort(pool, std::slice::from_ref(&call), None, None).await;

    let completion = result.map_err(SuggestError::Llm)?;
    if cancel.is_cancelled() {
        return Err(SuggestError::Llm(LlmError::Cancelled));
    }

    let output = parse(&completion.text).map_err(SuggestError::InvalidResponse)?;
    let suggestions = output
        .suggestions
        .iter()
        .map(to_portal)
        .collect::<Result<Vec<_>, _>>()?;

    Ok(SuggestReport {
        suggestions,
        prompt_version: prepared.prompt.version,
        prompt_hash: prepared.prompt.content_hash,
        model_used: prepared.config.model,
        input_chars: prepared.input_chars,
        estimated_cost_usd: prepared.estimated_cost_usd,
        actual_cost_usd: call.usage.cost_usd,
        elapsed: Duration::from_millis(duration_ms.max(0) as u64),
    })
}

/// Turn one validated proposal into a board the UI can add.
///
/// The registry lookup is what makes a known board keep its hand-written
/// config (Ubisoft's detail cap, for example) instead of a derived default.
fn to_portal(portal: &SuggestedPortal) -> Result<PortalSuggestion, SuggestError> {
    let url = portal.engine.url(&portal.slug).ok_or_else(|| {
        SuggestError::InvalidResponse(format!(
            "the {} slug `{}` cannot be turned into a URL",
            portal.engine.as_str(),
            portal.slug
        ))
    })?;
    let parsed = Url::parse(&url).map_err(|error| {
        SuggestError::InvalidResponse(format!("the derived URL `{url}` is invalid: {error}"))
    })?;
    let host = parsed
        .host_str()
        .ok_or_else(|| SuggestError::InvalidResponse(format!("`{url}` has no host")))?
        .to_string();

    let config = known_sites::lookup(&parsed)
        .map(|site| site.config.to_string())
        .or_else(|| portal.engine.fallback_config(&portal.slug));

    Ok(PortalSuggestion {
        company: portal.company.trim().to_string(),
        host,
        engine_label: portal.engine.label().to_string(),
        open_roles: portal.open_roles,
        rationale: portal.rationale.trim().to_string(),
        url,
        config,
    })
}

struct Prepared {
    config: LlmConfig,
    prompt: crate::model::Prompt,
    assembled: String,
    input_chars: usize,
    estimated_cost_usd: Option<f64>,
}

async fn prepare(pool: &SqlitePool) -> Result<Prepared, SuggestError> {
    let config = LlmConfig::load(pool).await?;
    if !config.capabilities().structured_output.is_usable() {
        return Err(SuggestError::Core(CoreError::Message(format!(
            "{} cannot return structured output; choose another model in Settings.",
            config.model
        ))));
    }

    let prompt = prompts::active_for_use(pool, PROMPT_NAME)
        .await?
        .ok_or_else(|| {
            SuggestError::Core(CoreError::Message(
                "no prompt named `suggest` is installed".into(),
            ))
        })?;

    let cv_summary = profile::cv_summary(pool).await?.ok_or_else(|| {
        SuggestError::Core(CoreError::Message(
            "Add your CV in CV / Profile before asking for suggestions.".into(),
        ))
    })?;
    let cv_summary = take_chars(&cv_summary, MAX_CV_CHARS);
    let preferences = profile::preferences(pool).await?;

    let assembled =
        prompt::assemble(&prompt.content, &cv_summary, &preferences).map_err(|error| {
            SuggestError::Core(CoreError::Message(format!(
                "the `suggest` prompt cannot be rendered: {error}"
            )))
        })?;

    let input_chars = assembled.chars().count();
    let input_tokens = estimated_tokens(input_chars);
    if let Some(window) = config.capabilities().context_window {
        if input_tokens > window {
            return Err(SuggestError::Llm(LlmError::ContextLengthExceeded {
                input_chars: Some(input_chars),
                limit: Some(window),
                detail: format!("about {input_tokens} tokens for a {window}-token window"),
            }));
        }
    }

    let estimated_cost_usd = catalog_cost(
        config.provider,
        &config.model,
        input_tokens as u64,
        ESTIMATED_OUTPUT_TOKENS_PER_SUGGESTION * super::output::MAX_SUGGESTIONS as u64,
    );

    Ok(Prepared {
        config,
        prompt,
        assembled,
        input_chars,
        estimated_cost_usd,
    })
}
