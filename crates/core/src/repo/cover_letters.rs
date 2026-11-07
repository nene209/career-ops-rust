//! Cover letters: the write path and the reads the UI needs.
//!
//! Regeneration inserts a new row rather than replacing one, so the history is
//! the list and the user can compare drafts. Text is small; keeping every
//! generation is cheap and the alternative loses work.

use chrono::Utc;
use sqlx::SqlitePool;

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
    .fetch_one(pool)
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
