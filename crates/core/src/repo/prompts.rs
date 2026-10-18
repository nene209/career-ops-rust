//! Prompt repository, demonstrating the versioning pattern.
//!
//! Every edit creates a new row with `supersedes_id` pointing at the prior
//! version. Rows are never hard-deleted. Content is hashed with SHA-256 so
//! `prompt_hash` records in `evaluation_reports` remain stable identifiers.

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
    let row = sqlx::query_as::<_, Prompt>(
        "SELECT * FROM prompts
         WHERE name = ? AND is_system = 1
         ORDER BY version DESC
         LIMIT 1",
    )
    .bind(name)
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

pub async fn create_version(
    pool: &SqlitePool,
    name: &str,
    category: &str,
    content: &str,
    is_system: bool,
) -> Result<Prompt> {
    let prior: Option<Prompt> = active(pool, name).await?;
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