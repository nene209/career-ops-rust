//! The LLM usage ledger: one row per provider call.
//!
//! This table is a record of what was asked of a provider, not a child of the
//! job. `application_id` and `job_description_id` are references without a
//! foreign key, so deleting a job cannot delete the record of the tokens it
//! cost — the same reason a bank statement outlives the purchase.
//!
//! The queries below shape the read-only **Usage** screen: totals by
//! provider/model, totals by task, and the most recent calls. Every one accepts
//! `since: None` for "all time"; the comparison is a string comparison because
//! every timestamp is stored in the same RFC 3339 UTC form.

use chrono::{DateTime, Utc};
use sqlx::{Sqlite, SqlitePool, Transaction};

use crate::error::Result;
use crate::llm::usage::{Call, Source};

#[derive(Debug, Clone)]
pub struct NewLlmUsage<'a> {
    pub provider: &'a str,
    pub model: &'a str,
    pub task: &'a str,
    pub application_id: Option<i64>,
    pub job_description_id: Option<i64>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    pub cached_input_tokens: Option<i64>,
    pub reasoning_tokens: Option<i64>,
    pub source: &'a str,
    pub cost_usd: Option<f64>,
    pub cost_complete: Option<bool>,
    pub outcome: &'a str,
    pub error_kind: Option<&'a str>,
    pub duration_ms: i64,
}

impl<'a> NewLlmUsage<'a> {
    /// The row for one call, attached to the application and job it concerned.
    pub fn from_call(
        call: &'a Call,
        application_id: Option<i64>,
        job_description_id: Option<i64>,
    ) -> Self {
        Self {
            provider: call.provider.as_str(),
            model: &call.model,
            task: call.task.as_str(),
            application_id,
            job_description_id,
            input_tokens: call.usage.input_tokens.map(as_i64),
            output_tokens: call.usage.output_tokens.map(as_i64),
            total_tokens: call.usage.total_tokens.map(as_i64),
            cached_input_tokens: call.usage.cached_input_tokens.map(as_i64),
            reasoning_tokens: call.usage.reasoning_tokens.map(as_i64),
            source: if call.usage.reported {
                Source::Reported.as_str()
            } else {
                Source::Estimated.as_str()
            },
            cost_usd: call.usage.cost_usd,
            cost_complete: call.usage.cost_complete,
            outcome: call.outcome.as_str(),
            error_kind: call.error_kind,
            duration_ms: call.duration_ms,
        }
    }
}

/// Insert rows inside a caller's transaction.
pub async fn insert_many_tx(
    tx: &mut Transaction<'_, Sqlite>,
    rows: &[NewLlmUsage<'_>],
) -> Result<()> {
    if rows.is_empty() {
        return Ok(());
    }

    let now: DateTime<Utc> = Utc::now();
    for row in rows {
        sqlx::query(
            "INSERT INTO llm_usage
             (provider, model, task, application_id, job_description_id,
              input_tokens, output_tokens, total_tokens, cached_input_tokens,
              reasoning_tokens, source, cost_usd, cost_complete, outcome,
              error_kind, duration_ms, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(row.provider)
        .bind(row.model)
        .bind(row.task)
        .bind(row.application_id)
        .bind(row.job_description_id)
        .bind(row.input_tokens)
        .bind(row.output_tokens)
        .bind(row.total_tokens)
        .bind(row.cached_input_tokens)
        .bind(row.reasoning_tokens)
        .bind(row.source)
        .bind(row.cost_usd)
        .bind(row.cost_complete)
        .bind(row.outcome)
        .bind(row.error_kind)
        .bind(row.duration_ms)
        .bind(now)
        .execute(&mut **tx)
        .await?;
    }

    Ok(())
}

/// Insert rows in a transaction of their own.
pub async fn insert_many(pool: &SqlitePool, rows: &[NewLlmUsage<'_>]) -> Result<()> {
    let mut tx = pool.begin().await?;
    insert_many_tx(&mut tx, rows).await?;
    tx.commit().await?;
    Ok(())
}

/// Record calls on a failure path.
///
/// A ledger write must never replace the error the caller is about to return,
/// so a failure here is logged and dropped. A failed or cancelled call still
/// spent tokens; the row is the only evidence of it.
pub async fn record_best_effort(
    pool: &SqlitePool,
    calls: &[Call],
    application_id: Option<i64>,
    job_description_id: Option<i64>,
) {
    if calls.is_empty() {
        return;
    }

    let rows: Vec<NewLlmUsage<'_>> = calls
        .iter()
        .map(|call| NewLlmUsage::from_call(call, application_id, job_description_id))
        .collect();
    if let Err(error) = insert_many(pool, &rows).await {
        tracing::warn!(%error, "a provider call's usage could not be recorded");
    }
}

/// Totals per provider and model.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct ProviderModelTotals {
    pub provider: String,
    pub model: String,
    pub calls: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub total_tokens: i64,
    /// `None` when every row in the group has no price.
    pub cost_usd: Option<f64>,
    /// Rows with no price, or with a price that is only a lower bound.
    pub unpriced_calls: i64,
    pub reported_calls: i64,
    pub estimated_calls: i64,
}

pub async fn totals_by_provider_model(
    pool: &SqlitePool,
    since: Option<DateTime<Utc>>,
) -> Result<Vec<ProviderModelTotals>> {
    let rows = sqlx::query_as::<_, ProviderModelTotals>(
        "SELECT provider, model,
                COUNT(*) AS calls,
                COALESCE(SUM(input_tokens), 0) AS input_tokens,
                COALESCE(SUM(output_tokens), 0) AS output_tokens,
                COALESCE(SUM(total_tokens), 0) AS total_tokens,
                SUM(cost_usd) AS cost_usd,
                SUM(CASE WHEN cost_usd IS NULL OR COALESCE(cost_complete, 0) = 0 THEN 1 ELSE 0 END)
                    AS unpriced_calls,
                SUM(CASE WHEN source = 'reported' THEN 1 ELSE 0 END) AS reported_calls,
                SUM(CASE WHEN source = 'estimated' THEN 1 ELSE 0 END) AS estimated_calls
         FROM llm_usage
         WHERE created_at >= COALESCE(?, '')
         GROUP BY provider, model
         ORDER BY total_tokens DESC, provider ASC, model ASC",
    )
    .bind(since)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Totals per task.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct TaskTotals {
    pub task: String,
    pub calls: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub total_tokens: i64,
    pub cost_usd: Option<f64>,
    pub unpriced_calls: i64,
}

pub async fn totals_by_task(
    pool: &SqlitePool,
    since: Option<DateTime<Utc>>,
) -> Result<Vec<TaskTotals>> {
    let rows = sqlx::query_as::<_, TaskTotals>(
        "SELECT task,
                COUNT(*) AS calls,
                COALESCE(SUM(input_tokens), 0) AS input_tokens,
                COALESCE(SUM(output_tokens), 0) AS output_tokens,
                COALESCE(SUM(total_tokens), 0) AS total_tokens,
                SUM(cost_usd) AS cost_usd,
                SUM(CASE WHEN cost_usd IS NULL OR COALESCE(cost_complete, 0) = 0 THEN 1 ELSE 0 END)
                    AS unpriced_calls
         FROM llm_usage
         WHERE created_at >= COALESCE(?, '')
         GROUP BY task
         ORDER BY total_tokens DESC, task ASC",
    )
    .bind(since)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// One recorded call, joined to the posting it concerned.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct RecentCall {
    pub id: i64,
    pub provider: String,
    pub model: String,
    pub task: String,
    pub job_title: Option<String>,
    pub company: Option<String>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    pub source: String,
    pub cost_usd: Option<f64>,
    pub cost_complete: Option<bool>,
    pub outcome: String,
    pub error_kind: Option<String>,
    pub duration_ms: i64,
    pub created_at: DateTime<Utc>,
}

pub async fn recent(
    pool: &SqlitePool,
    since: Option<DateTime<Utc>>,
    limit: i64,
) -> Result<Vec<RecentCall>> {
    let rows = sqlx::query_as::<_, RecentCall>(
        "SELECT u.id, u.provider, u.model, u.task,
                j.title AS job_title, j.company AS company,
                u.input_tokens, u.output_tokens, u.total_tokens, u.source,
                u.cost_usd, u.cost_complete, u.outcome, u.error_kind, u.duration_ms,
                u.created_at
         FROM llm_usage u
         LEFT JOIN job_descriptions j ON j.id = u.job_description_id
         WHERE u.created_at >= COALESCE(?, '')
         ORDER BY u.created_at DESC, u.id DESC
         LIMIT ?",
    )
    .bind(since)
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Token counters are `u64` on the way in and `i64` in SQLite. A count that
/// cannot be stored is clamped rather than wrapped; the value is reported, not
/// enforced.
fn as_i64(tokens: u64) -> i64 {
    i64::try_from(tokens).unwrap_or(i64::MAX)
}
