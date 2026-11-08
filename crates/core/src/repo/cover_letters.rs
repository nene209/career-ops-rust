//! Cover letters: the write path and the reads the UI needs.
//!
//! Regeneration inserts a new row rather than replacing one, so the history is
//! the list and the user can compare drafts. Text is small; keeping every
//! generation is cheap and the alternative loses work.

use chrono::Utc;
use sqlx::{Sqlite, SqlitePool, Transaction};

use crate::error::{CoreError, Result};
use crate::model::CoverLetter;

#[derive(Debug, Clone)]
pub struct NewCoverLetter<'a> {
    pub application_id: i64,
    /// The letter, in markdown.
    pub content: &'a str,
    /// The model string sent to the provider, verbatim.
    pub model_used: &'a str,
    pub prompt_version: Option<i64>,
    pub prompt_hash: Option<&'a str>,
    /// Characters of prompt input sent.
    pub input_chars: Option<i64>,
}

pub async fn insert(pool: &SqlitePool, input: NewCoverLetter<'_>) -> Result<i64> {
    let mut tx = pool.begin().await?;
    let id = insert_tx(&mut tx, input).await?;
    tx.commit().await?;
    Ok(id)
}

/// Insert a letter inside a caller's transaction.
///
/// The service writes the letter and its usage row together, so a letter can
/// never be stored without the record of what it cost.
pub async fn insert_tx(tx: &mut Transaction<'_, Sqlite>, input: NewCoverLetter<'_>) -> Result<i64> {
    let now: chrono::DateTime<Utc> = Utc::now();
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO cover_letters
         (application_id, content, generated_at, model_used, prompt_version,
          prompt_hash, input_chars)
         VALUES (?, ?, ?, ?, ?, ?, ?)
         RETURNING id",
    )
    .bind(input.application_id)
    .bind(input.content)
    .bind(now)
    .bind(input.model_used)
    .bind(input.prompt_version)
    .bind(input.prompt_hash)
    .bind(input.input_chars)
    .fetch_one(&mut **tx)
    .await?;

    Ok(id)
}

pub async fn get(pool: &SqlitePool, id: i64) -> Result<Option<CoverLetter>> {
    let row = sqlx::query_as::<_, CoverLetter>("SELECT * FROM cover_letters WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(row)
}

/// Every letter for an application, newest first.
pub async fn list_for_application(
    pool: &SqlitePool,
    application_id: i64,
) -> Result<Vec<CoverLetter>> {
    let rows = sqlx::query_as::<_, CoverLetter>(
        "SELECT * FROM cover_letters
         WHERE application_id = ?
         ORDER BY generated_at DESC, id DESC",
    )
    .bind(application_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Every letter for a job, newest first.
///
/// The Pipeline detail has a job id; the join resolves its application.
pub async fn list_for_job(pool: &SqlitePool, job_description_id: i64) -> Result<Vec<CoverLetter>> {
    let rows = sqlx::query_as::<_, CoverLetter>(
        "SELECT c.* FROM cover_letters c
         JOIN applications a ON a.id = c.application_id
         WHERE a.job_description_id = ?
         ORDER BY c.generated_at DESC, c.id DESC",
    )
    .bind(job_description_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// The newest letter for an application.
pub async fn latest_for_application(
    pool: &SqlitePool,
    application_id: i64,
) -> Result<Option<CoverLetter>> {
    let row = sqlx::query_as::<_, CoverLetter>(
        "SELECT * FROM cover_letters
         WHERE application_id = ?
         ORDER BY generated_at DESC, id DESC
         LIMIT 1",
    )
    .bind(application_id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// Delete one letter, for a user who does not want it in the history.
pub async fn delete(pool: &SqlitePool, id: i64) -> Result<()> {
    let result = sqlx::query("DELETE FROM cover_letters WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    if result.rows_affected() == 0 {
        return Err(CoreError::Internal("cover_letters::delete: row not found"));
    }
    Ok(())
}
