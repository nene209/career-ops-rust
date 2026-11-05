//! Job descriptions — the acquisition pipeline's destination.
//!
//! Deduplication is the load-bearing behaviour. A row is identified by its
//! **canonical URL** (`acquisition::identity`) plus the provider's id, so two
//! extractions of the same posting meet even when one carries tracking
//! parameters and the other does not. The lookup order is:
//!
//! 1. The provider-scoped `external_id` alone (`eightfold:{domain}:{id}`) — it
//!    is unique within its engine, so a renamed URL slug still finds the row.
//! 2. `(canonical_url, external_id)` — the source's own stable identifier.
//! 3. `(canonical_url, content_hash)` — catches reposts under a new external id
//!    with identical title, company, and body.
//! 4. `(canonical_url, title, company)` — the same URL with the same title and
//!    company is the same posting, whether or not an external id is present.
//!    (The earlier rule excluded the external-id case, which let a second
//!    extraction of one posting insert a duplicate.)
//!
//! The `content_hash` column was added during Phase 1 to implement the
//! "content-aware deduplication" requirement from the Phase 2 plan. It is
//! computed by this module, not provided by callers.
//!
//! **Update policy:** a re-fetch never degrades a row. Empty text, or text that
//! is a strict prefix of the stored description (a site truncating its own
//! teaser), keeps the stored text; the hash is recomputed from what is stored.
//! An explicit refresh may replace it.
//!
//! **Preservation note:** when a row is found by content hash with a
//! *different* external_id, the existing row's `external_id` is preserved
//! rather than overwritten. Downstream code should treat `external_id` as
//! "the identifier this row was first seen under," not "the current
//! identifier."

use chrono::{DateTime, Utc};
use sqlx::SqlitePool;
use url::Url;

use crate::acquisition::identity;
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
    hash_parts(&input.title, &input.company, &input.raw_text)
}

fn hash_parts(title: &str, company: &str, raw_text: &str) -> String {
    let mut combined = String::with_capacity(title.len() + company.len() + raw_text.len() + 2);
    combined.push_str(title);
    combined.push('\n');
    combined.push_str(company);
    combined.push('\n');
    combined.push_str(raw_text);
    content_hash(&combined)
}

/// The comparison form of a stored URL. Unparsable values fall back to the raw
/// string so a row is still covered by the identity indexes.
fn canonical_url_of(source_url: &str) -> String {
    Url::parse(source_url)
        .map(|url| identity::canonical_url(&url))
        .unwrap_or_else(|_| source_url.to_string())
}

/// Choose the text to store when an existing row is updated.
///
/// A re-fetch that produced nothing, or a card teaser that is a strict prefix
/// of the stored description, must not degrade a row. A genuinely different
/// text replaces the old one; only an explicit refresh can replace a longer
/// text with a shorter unrelated one.
fn merge_raw_text(existing: &str, incoming: &str) -> String {
    let incoming = incoming.trim();
    if incoming.is_empty() {
        return existing.to_string();
    }

    let existing = existing.trim();
    if incoming.len() < existing.len() && existing.starts_with(incoming) {
        return existing.to_string();
    }

    incoming.to_string()
}

/// Provider ids are globally unique within their engine (`eightfold:{domain}:{id}`),
/// so they identify a posting even when the site renames its URL slug.
fn is_provider_scoped_id(external_id: &str) -> bool {
    external_id.starts_with("eightfold:")
}

pub async fn get(pool: &SqlitePool, id: i64) -> Result<Option<JobDescription>> {
    let row = sqlx::query_as::<_, JobDescription>("SELECT * FROM job_descriptions WHERE id = ?")
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
    let canonical = canonical_url_of(&input.source_url);
    let hash = hash_job(&input);

    if let Some(existing) = find_existing(pool, &input, &canonical, &hash).await? {
        return update(pool, existing.id, &existing.raw_text, input, &canonical).await;
    }

    match insert(pool, input.clone(), &canonical, &hash).await {
        Ok(row) => Ok(row),
        Err(CoreError::Database(e)) if is_unique_violation(&e) => {
            // Lost a race against a concurrent insert, or hit one of the two
            // unique indexes in an unexpected way. Re-read and update.
            let existing = find_existing(pool, &input, &canonical, &hash)
                .await?
                .ok_or(CoreError::Internal(
                    "upsert: unique violation but no existing row found",
                ))?;
            update(pool, existing.id, &existing.raw_text, input, &canonical).await
        }
        Err(e) => Err(e),
    }
}

async fn find_existing(
    pool: &SqlitePool,
    input: &NewJobDescription,
    canonical: &str,
    hash: &str,
) -> Result<Option<JobDescription>> {
    // 0. Primary for provider-scoped ids: the id alone. An Eightfold position
    //    keeps its id when the site renames the URL, so comparing the URL
    //    first would create a second row for the same posting.
    if let Some(ext) = &input.external_id {
        if is_provider_scoped_id(ext) {
            let row = sqlx::query_as::<_, JobDescription>(
                "SELECT * FROM job_descriptions WHERE external_id = ? ORDER BY id ASC LIMIT 1",
            )
            .bind(ext)
            .fetch_optional(pool)
            .await?;
            if row.is_some() {
                return Ok(row);
            }
        }
    }

    // 1. The provider's own identity within one canonical URL.
    if let Some(ext) = &input.external_id {
        let row = sqlx::query_as::<_, JobDescription>(
            "SELECT * FROM job_descriptions
             WHERE canonical_url = ? AND external_id = ?
             ORDER BY id ASC LIMIT 1",
        )
        .bind(canonical)
        .bind(ext)
        .fetch_optional(pool)
        .await?;
        if row.is_some() {
            return Ok(row);
        }
    }

    // 2. Content hash: a repost under a new external id.
    let by_hash = sqlx::query_as::<_, JobDescription>(
        "SELECT * FROM job_descriptions
         WHERE canonical_url = ? AND content_hash = ?
         LIMIT 1",
    )
    .bind(canonical)
    .bind(hash)
    .fetch_optional(pool)
    .await?;
    if by_hash.is_some() {
        return Ok(by_hash);
    }

    // 3. Title and company at the same URL, whether or not an external id is
    //    present. The same URL with the same title and company is the same
    //    posting; this is the rule that catches a second extraction whose
    //    external id differs from the first one.
    let row = sqlx::query_as::<_, JobDescription>(
        "SELECT * FROM job_descriptions
         WHERE canonical_url = ? AND title = ? AND company = ?
         ORDER BY id ASC LIMIT 1",
    )
    .bind(canonical)
    .bind(&input.title)
    .bind(&input.company)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// Replace the stored description, bypassing the merge rule.
///
/// Used by an explicit per-job refresh, where the user asked for the new text.
/// The discovery timestamp is kept: the row was first seen then, and reordering
/// the pipeline on a description refresh would be surprising.
pub async fn set_raw_text(pool: &SqlitePool, id: i64, raw_text: &str) -> Result<()> {
    let existing = get(pool, id)
        .await?
        .ok_or(CoreError::Internal("job_descriptions::set_raw_text"))?;
    let hash = hash_parts(&existing.title, &existing.company, raw_text);

    sqlx::query("UPDATE job_descriptions SET raw_text = ?, content_hash = ? WHERE id = ?")
        .bind(raw_text)
        .bind(hash)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Delete a job description. Its application, evaluations, and cover letters
/// cascade (`foreign_keys = ON`).
pub async fn delete(pool: &SqlitePool, id: i64) -> Result<()> {
    sqlx::query("DELETE FROM job_descriptions WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Job ids that carry anything the user would lose: a status beyond
/// `discovered`, an evaluation report, or a cover letter.
///
/// The Pipeline uses this to decide whether deleting a row needs the typed
/// confirmation. A fetched-only row has nothing in this set.
pub async fn user_touched_ids(pool: &SqlitePool) -> Result<std::collections::HashSet<i64>> {
    let ids: Vec<(i64,)> = sqlx::query_as(
        "SELECT DISTINCT job_description_id FROM applications
         WHERE status != 'discovered'
         UNION
         SELECT DISTINCT a.job_description_id FROM evaluation_reports e
         JOIN applications a ON a.id = e.application_id
         UNION
         SELECT DISTINCT a.job_description_id FROM cover_letters c
         JOIN applications a ON a.id = c.application_id",
    )
    .fetch_all(pool)
    .await?;

    Ok(ids.into_iter().map(|(id,)| id).collect())
}

async fn insert(
    pool: &SqlitePool,
    input: NewJobDescription,
    canonical: &str,
    hash: &str,
) -> Result<JobDescription> {
    let now: DateTime<Utc> = Utc::now();
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO job_descriptions
         (title, company, source_url, canonical_url, raw_text, fetched_at, fetch_tier,
          external_id, content_hash)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
         RETURNING id",
    )
    .bind(&input.title)
    .bind(&input.company)
    .bind(&input.source_url)
    .bind(canonical)
    .bind(&input.raw_text)
    .bind(now)
    .bind(input.fetch_tier.as_str())
    .bind(&input.external_id)
    .bind(hash)
    .fetch_one(pool)
    .await?;

    get(pool, id)
        .await?
        .ok_or(CoreError::Internal("job_descriptions::insert"))
}

async fn update(
    pool: &SqlitePool,
    id: i64,
    previous_text: &str,
    input: NewJobDescription,
    canonical: &str,
) -> Result<JobDescription> {
    // external_id is deliberately NOT updated. If this row was found by
    // content hash with a different external_id, the original is preserved
    // to keep downstream references stable.
    //
    // `source_url` is not updated either: it is the user-facing address the
    // row was first seen at, and a renamed slug should not rewrite history.
    let raw_text = merge_raw_text(previous_text, &input.raw_text);
    let hash = hash_parts(&input.title, &input.company, &raw_text);
    let now: DateTime<Utc> = Utc::now();
    sqlx::query(
        "UPDATE job_descriptions
         SET title = ?, company = ?, raw_text = ?, fetched_at = ?,
             fetch_tier = ?, content_hash = ?, canonical_url = ?
         WHERE id = ?",
    )
    .bind(&input.title)
    .bind(&input.company)
    .bind(&raw_text)
    .bind(now)
    .bind(input.fetch_tier.as_str())
    .bind(&hash)
    .bind(canonical)
    .bind(id)
    .execute(pool)
    .await?;

    get(pool, id)
        .await?
        .ok_or(CoreError::Internal("job_descriptions::update"))
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
