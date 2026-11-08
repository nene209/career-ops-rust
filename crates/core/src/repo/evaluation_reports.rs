//! Evaluation reports: the write path and the reads the UI needs.
//!
//! A report is written by the evaluation service inside the transaction that
//! also links it to the application and updates the job's queue state, so a
//! report never exists on a row that still says `evaluate`.
//!
//! The structured fields (`match_score`, `recommendation`, `input_chars`,
//! `source_chars`, `truncated`) are columns as of migration 0006, not JSON
//! inside `content`, so history and sorting do not re-parse markdown.

use chrono::Utc;
use sqlx::{Sqlite, SqlitePool, Transaction};

use crate::error::{CoreError, Result};
use crate::model::EvaluationReport;

#[derive(Debug, Clone)]
pub struct NewEvaluationReport<'a> {
    pub application_id: i64,
    /// The report itself, in markdown.
    pub content: &'a str,
    /// The model string sent to the provider, verbatim.
    pub model_used: &'a str,
    pub prompt_version: Option<i64>,
    pub prompt_hash: Option<&'a str>,
    pub match_score: Option<f64>,
    /// One of [`Recommendation`](crate::evaluation::Recommendation)'s stored
    /// values.
    pub recommendation: Option<&'a str>,
    /// Characters of job text sent, after normalization and truncation.
    pub input_chars: Option<i64>,
    /// The stored text size before normalization.
    pub source_chars: Option<i64>,
    /// The job text was cut to fit the budget.
    pub truncated: bool,
}

/// Insert a report inside a caller's transaction and return its id.
pub async fn insert_tx(
    tx: &mut Transaction<'_, Sqlite>,
    input: NewEvaluationReport<'_>,
) -> Result<i64> {
    let now: chrono::DateTime<Utc> = Utc::now();
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO evaluation_reports
         (application_id, content, generated_at, model_used, prompt_version,
          prompt_hash, match_score, recommendation, input_chars, source_chars,
          truncated)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
         RETURNING id",
    )
    .bind(input.application_id)
    .bind(input.content)
    .bind(now)
    .bind(input.model_used)
    .bind(input.prompt_version)
    .bind(input.prompt_hash)
    .bind(input.match_score)
    .bind(input.recommendation)
    .bind(input.input_chars)
    .bind(input.source_chars)
    .bind(input.truncated)
    .fetch_one(&mut **tx)
    .await?;

    Ok(id)
}

pub async fn get(pool: &SqlitePool, id: i64) -> Result<Option<EvaluationReport>> {
    let row =
        sqlx::query_as::<_, EvaluationReport>("SELECT * FROM evaluation_reports WHERE id = ?")
            .bind(id)
            .fetch_optional(pool)
            .await?;
    Ok(row)
}

/// Every report for an application, newest first.
///
/// Re-evaluation adds rows rather than replacing one, so the history is the
/// list and the application's `evaluation_report_id` is the latest pointer.
pub async fn list_for_application(
    pool: &SqlitePool,
    application_id: i64,
) -> Result<Vec<EvaluationReport>> {
    let rows = sqlx::query_as::<_, EvaluationReport>(
        "SELECT * FROM evaluation_reports
         WHERE application_id = ?
         ORDER BY generated_at DESC, id DESC",
    )
    .bind(application_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Every report for a job, newest first.
///
/// The Pipeline detail knows a job id, not an application id, so the join does
/// the resolution. A job has at most one application today; a future multi-CV
/// row would simply add to this list.
pub async fn list_for_job(
    pool: &SqlitePool,
    job_description_id: i64,
) -> Result<Vec<EvaluationReport>> {
    let rows = sqlx::query_as::<_, EvaluationReport>(
        "SELECT e.* FROM evaluation_reports e
         JOIN applications a ON a.id = e.application_id
         WHERE a.job_description_id = ?
         ORDER BY e.generated_at DESC, e.id DESC",
    )
    .bind(job_description_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// The newest report for an application.
pub async fn latest_for_application(
    pool: &SqlitePool,
    application_id: i64,
) -> Result<Option<EvaluationReport>> {
    let row = sqlx::query_as::<_, EvaluationReport>(
        "SELECT * FROM evaluation_reports
         WHERE application_id = ?
         ORDER BY generated_at DESC, id DESC
         LIMIT 1",
    )
    .bind(application_id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// Delete one report. The application's latest pointer is cleared when it
/// named this row, so the link never dangles.
pub async fn delete(pool: &SqlitePool, id: i64) -> Result<()> {
    let mut tx = pool.begin().await?;
    sqlx::query(
        "UPDATE applications SET evaluation_report_id = NULL
         WHERE evaluation_report_id = ?",
    )
    .bind(id)
    .execute(&mut *tx)
    .await?;
    let result = sqlx::query("DELETE FROM evaluation_reports WHERE id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await?;
    if result.rows_affected() == 0 {
        return Err(CoreError::Internal(
            "evaluation_reports::delete: row not found",
        ));
    }
    tx.commit().await?;
    Ok(())
}
