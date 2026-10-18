//! Job descriptions — the acquisition pipeline's destination.
//!
//! Deduplication is the load-bearing behaviour. The lookup order is:
//!
//! 1. `(source_url, external_id)` — the source's own stable identifier.
//! 2. `(source_url, content_hash)` — catches reposts under a new external_id
//!    with identical title, company, and body.
//! 3. `(source_url, title, company)` — fallback for entries with no
//!    external_id (manual entries, some HTML extractions).
//!
//! The `content_hash` column was added during Phase 1 to implement the
//! "content-aware deduplication" requirement from the Phase 2 plan. It is
//! computed by this module, not provided by callers.
//!
//! **Preservation note:** when a row is found by content hash with a
//! *different* external_id, the existing row's `external_id` is preserved
//! rather than overwritten. Downstream code should treat `external_id` as
//! "the identifier this row was first seen under," not "the current
//! identifier."

use chrono::{DateTime, Utc};
use sqlx::SqlitePool;

use crate::error::{CoreError, Result};
use crate::model::{FetchTier, JobDescription};

use super::content_hash;

#[derive(Debug, Clone)]
pub struct NewJobDescription {
    pub title: String,
    pub company: String,
    pub source_url: String,
    pub raw_text: String,
    pub fetch_tier: FetchTier,
    pub external_id: Option<String>,
}

/// Compute the dedup hash for a job description.
///
/// Hashes `title \n company \n raw_text`. Title and company are included so
/// that a company posting the same boilerplate JD for two different roles
/// does not get merged. Raw_text is included so that reposts with the same
/// title and company are caught. Any change to any of the three produces a
/// different hash, which is the conservative choice: an edited posting is
/// treated as a new posting rather than silently overwriting the old one.
fn hash_job(input: &NewJobDescription) -> String {
    let mut combined = String::with_capacity(
        input.title.len() + input.company.len() + input.raw_text.len() + 2,
    );
    combined.push_str(&input.title);
    combined.push('\n');
    combined.push_str(&input.company);
    combined.push('\n');
    combined.push_str(&input.raw_text);
    content_hash(&combined)
}

pub async fn get(pool: &SqlitePool, id: i64) -> Result<Option<JobDescription>> {
    let row = sqlx::query_as::<_, JobDescription>(
        "SELECT * FROM job_descriptions WHERE id = ?",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn list_recent(pool: &SqlitePool, limit: i64) -> Result<Vec<JobDescription>> {
    let rows = sqlx::query_as::<_, JobDescription>(
        "SELECT * FROM job_descriptions ORDER BY fetched_at DESC LIMIT ?",
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Insert a new job description, or update the existing row if one matches
/// by the dedup rule. Returns the row that ends up in the database.
pub async fn upsert(pool: &SqlitePool, input: NewJobDescription) -> Result<JobDescription> {
    let hash = hash_job(&input);

    if let Some(existing) = find_existing(pool, &input, &hash).await? {
        return update(pool, existing.id, input, &hash).await;
    }

    match insert(pool, input.clone(), &hash).await {
        Ok(row) => Ok(row),
        Err(CoreError::Database(e)) if is_unique_violation(&e) => {
            // Lost a race against a concurrent insert, or hit one of the two
            // unique indexes in an unexpected way. Re-read and update.
            let existing = find_existing(pool, &input, &hash)
                .await?
                .ok_or(CoreError::Internal(
                    "upsert: unique violation but no existing row found",
                ))?;
            update(pool, existing.id, input, &hash).await
        }
        Err(e) => Err(e),
    }
}

async fn find_existing(
    pool: &SqlitePool,
    input: &NewJobDescription,
    hash: &str,
) -> Result<Option<JobDescription>> {
    // 1. Primary: external_id if present.
    if let Some(ext) = &input.external_id {
        let row = sqlx::query_as::<_, JobDescription>(
            "SELECT * FROM job_descriptions WHERE source_url = ? AND external_id = ?",
        )
        .bind(&input.source_url)
        .bind(ext)
        .fetch_optional(pool)
        .await?;
        if row.is_some() {
            return Ok(row);
        }
    }

    // 2. Secondary: content hash catches reposts under new external_ids.
    let by_hash = sqlx::query_as::<_, JobDescription>(
        "SELECT * FROM job_descriptions
         WHERE source_url = ? AND content_hash = ?
         LIMIT 1",
    )
    .bind(&input.source_url)
    .bind(hash)
    .fetch_optional(pool)
    .await?;
    if by_hash.is_some() {
        return Ok(by_hash);
    }

    // 3. Fallback: title/company for entries with no external_id.
    if input.external_id.is_none() {
        let row = sqlx::query_as::<_, JobDescription>(
            "SELECT * FROM job_descriptions
             WHERE source_url = ? AND title = ? AND company = ? AND external_id IS NULL
             ORDER BY id ASC LIMIT 1",
        )
        .bind(&input.source_url)
        .bind(&input.title)
        .bind(&input.company)
        .fetch_optional(pool)
        .await?;
        return Ok(row);
    }

    Ok(None)
}

async fn insert(
    pool: &SqlitePool,
    input: NewJobDescription,
    hash: &str,
) -> Result<JobDescription> {
    let now: DateTime<Utc> = Utc::now();
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO job_descriptions
         (title, company, source_url, raw_text, fetched_at, fetch_tier, external_id, content_hash)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?)
         RETURNING id",
    )
    .bind(&input.title)
    .bind(&input.company)
    .bind(&input.source_url)
    .bind(&input.raw_text)
    .bind(now)
    .bind(input.fetch_tier.as_str())
    .bind(&input.external_id)
    .bind(hash)
    .fetch_one(pool)
    .await?;

    get(pool, id).await?.ok_or(CoreError::Internal("job_descriptions::insert"))
}

async fn update(
    pool: &SqlitePool,
    id: i64,
    input: NewJobDescription,
    hash: &str,
) -> Result<JobDescription> {
    // external_id is deliberately NOT updated. If this row was found by
    // content hash with a different external_id, the original is preserved
    // to keep downstream references stable.
    let now: DateTime<Utc> = Utc::now();
    sqlx::query(
        "UPDATE job_descriptions
         SET title = ?, company = ?, raw_text = ?, fetched_at = ?,
             fetch_tier = ?, content_hash = ?
         WHERE id = ?",
    )
    .bind(&input.title)
    .bind(&input.company)
    .bind(&input.raw_text)
    .bind(now)
    .bind(input.fetch_tier.as_str())
    .bind(hash)
    .bind(id)
    .execute(pool)
    .await?;

    get(pool, id).await?.ok_or(CoreError::Internal("job_descriptions::update"))
}

fn is_unique_violation(e: &sqlx::Error) -> bool {
    matches!(e, sqlx::Error::Database(db) if db.is_unique_violation())
}

#[cfg(test)]
mod tests {
    // Tests to write once the shared test fixture harness exists:
    //   1. Insert then re-insert with same external_id -> one row, updated.
    //   2. Insert then re-insert with same content but different external_id
    //      -> one row, external_id preserved as the original.
    //   3. Insert two different postings at the same URL with different
    //      titles and no external_id -> two rows.
    //   4. Insert a manual entry, then a portal entry with matching title,
    //      company, and body but a new external_id -> one row, merged.
    //   5. Two concurrent upserts of the same new job -> one row.
    //   6. An edited posting (same external_id, changed body) -> one row,
    //      raw_text updated, content_hash updated.
}