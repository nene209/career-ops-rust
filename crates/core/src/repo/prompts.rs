//! Prompt repository: versioning, resolution, and lookups.
//!
//! Every edit creates a new row with `supersedes_id` pointing at the prior
//! version **of the same owner**. Rows are never hard-deleted. Content is hashed
//! with SHA-256 so `prompt_hash` records in `evaluation_reports` remain stable
//! identifiers.
//!
//! ## Two independent version sequences
//!
//! `(name, is_system)` scopes versioning. A user copy of `evaluate` and the
//! shipped system prompt both start at version 1 and count up on their own, so a
//! later release seeding version 2 cannot collide with a user's version 2, and
//! neither can silently replace the other. Resolution for use goes through
//! [`active_for_use`]; `active` answers a different question and is documented
//! as such.

use chrono::Utc;
use sqlx::SqlitePool;

use crate::error::Result;
use crate::model::Prompt;

use super::content_hash;

/// Return the latest system prompt version with this name.
///
/// Ignores user prompts even if they share the name. This matters because
/// seeding must never modify a user's custom prompt, and `active()` alone
/// would return whichever prompt had the highest version number regardless
/// of `is_system`.
pub async fn active_system(pool: &SqlitePool, name: &str) -> Result<Option<Prompt>> {
    latest(pool, name, true).await
}

/// The highest-numbered version of `name` across **both** owners.
///
/// Kept for callers that want "any row with this name" (a diagnostic, an
/// export). It is not a resolution rule: a user copy and a system prompt have
/// independent version sequences, so the highest number does not say which one
/// should be used. Use [`active_for_use`] for that.
pub async fn active(pool: &SqlitePool, name: &str) -> Result<Option<Prompt>> {
    let row = sqlx::query_as::<_, Prompt>(
        "SELECT * FROM prompts
         WHERE name = ?
         ORDER BY version DESC
         LIMIT 1",
    )
    .bind(name)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// The latest version of `name` owned by `is_system` (true = shipped, false =
/// the user's copy).
async fn latest(pool: &SqlitePool, name: &str, is_system: bool) -> Result<Option<Prompt>> {
    let row = sqlx::query_as::<_, Prompt>(
        "SELECT * FROM prompts
         WHERE name = ? AND is_system = ?
         ORDER BY version DESC
         LIMIT 1",
    )
    .bind(name)
    .bind(is_system)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// The prompt to use for `name`: the user's latest copy when one exists, the
/// system prompt otherwise.
///
/// Evaluation and cover-letter assembly call this, never `active`. A malformed or
/// absent name resolves to `None`, which the caller turns into an actionable
/// "no prompt" error rather than an empty instruction.
pub async fn active_for_use(pool: &SqlitePool, name: &str) -> Result<Option<Prompt>> {
    if let Some(user) = latest(pool, name, false).await? {
        return Ok(Some(user));
    }
    latest(pool, name, true).await
}

/// Every version of `name` owned by `is_system`, newest first.
pub async fn list_versions(pool: &SqlitePool, name: &str, is_system: bool) -> Result<Vec<Prompt>> {
    let rows = sqlx::query_as::<_, Prompt>(
        "SELECT * FROM prompts
         WHERE name = ? AND is_system = ?
         ORDER BY version DESC",
    )
    .bind(name)
    .bind(is_system)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// The distinct categories in use, sorted, for a filter or a group heading.
pub async fn list_categories(pool: &SqlitePool) -> Result<Vec<String>> {
    let rows: Vec<(String,)> =
        sqlx::query_as("SELECT DISTINCT category FROM prompts ORDER BY category")
            .fetch_all(pool)
            .await?;
    Ok(rows.into_iter().map(|(category,)| category).collect())
}

/// The prompt whose content hashes to `content_hash`.
///
/// Reports store the hash rather than the text, so provenance is read back
/// through this lookup. A hash can in principle appear on several rows (the
/// same text seeded under two names); the oldest row wins so the answer is
/// stable.
pub async fn get_by_hash(pool: &SqlitePool, content_hash: &str) -> Result<Option<Prompt>> {
    let row = sqlx::query_as::<_, Prompt>(
        "SELECT * FROM prompts WHERE content_hash = ? ORDER BY id ASC LIMIT 1",
    )
    .bind(content_hash)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// Insert a new system prompt version with an explicitly provided version
/// number and content hash.
///
/// This differs from `create_version` in two ways:
///   - It forces `is_system = 1`.
///   - It does not compute the version internally; the caller (seeding)
///     knows what version it wants.
///
/// `supersedes_id` is looked up from the prior version of the same system
/// prompt, or left NULL for version 1.
pub async fn insert_system_prompt(
    pool: &SqlitePool,
    name: &str,
    category: &str,
    content: &str,
    content_hash: &str,
    version: i64,
) -> Result<Prompt> {
    let prior_id: Option<i64> = if version > 1 {
        sqlx::query_scalar(
            "SELECT id FROM prompts
             WHERE name = ? AND is_system = 1 AND version = ?",
        )
        .bind(name)
        .bind(version - 1)
        .fetch_optional(pool)
        .await?
    } else {
        None
    };

    let now = Utc::now();
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO prompts
         (name, category, content, is_system, version, content_hash,
          supersedes_id, created_at, updated_at)
         VALUES (?, ?, ?, 1, ?, ?, ?, ?, ?)
         RETURNING id",
    )
    .bind(name)
    .bind(category)
    .bind(content)
    .bind(version)
    .bind(content_hash)
    .bind(prior_id)
    .bind(now)
    .bind(now)
    .fetch_one(pool)
    .await?;

    Ok(Prompt {
        id,
        name: name.into(),
        category: category.into(),
        content: content.into(),
        is_system: true,
        version,
        content_hash: content_hash.into(),
        supersedes_id: prior_id,
        created_at: now,
        updated_at: now,
    })
}

pub async fn create_version(
    pool: &SqlitePool,
    name: &str,
    category: &str,
    content: &str,
    is_system: bool,
) -> Result<Prompt> {
    // Version within `(name, is_system)`: a user copy and a shipped system
    // prompt count independently, so neither can claim the other's next number.
    let prior: Option<Prompt> = latest(pool, name, is_system).await?;
    let next_version = prior.as_ref().map(|p| p.version + 1).unwrap_or(1);
    let now = Utc::now();
    let hash = content_hash(content);

    let id: i64 = sqlx::query_scalar(
        "INSERT INTO prompts
         (name, category, content, is_system, version, content_hash, supersedes_id, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
         RETURNING id",
    )
    .bind(name)
    .bind(category)
    .bind(content)
    .bind(is_system)
    .bind(next_version)
    .bind(&hash)
    .bind(prior.as_ref().map(|p| p.id))
    .bind(now)
    .bind(now)
    .fetch_one(pool)
    .await?;

    Ok(Prompt {
        id,
        name: name.into(),
        category: category.into(),
        content: content.into(),
        is_system,
        version: next_version,
        content_hash: hash,
        supersedes_id: prior.map(|p| p.id),
        created_at: now,
        updated_at: now,
    })
}
