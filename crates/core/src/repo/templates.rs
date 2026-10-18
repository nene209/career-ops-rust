//! Templates — HTML layouts for CV and cover letter rendering.
//!
//! Uses the same versioning pattern as prompts: every edit creates a new row,
//! rows are never hard-deleted, and `content_hash` identifies a specific
//! version independently of the numeric `version` field.

use chrono::Utc;
use sqlx::SqlitePool;

use crate::error::{CoreError, Result};
use crate::model::Template;

use super::content_hash;

/// Seed the templates table if it is empty.
///
/// `seeds` is `(name, kind, content)`. `kind` is `"cv"` or `"cover_letter"`.
pub async fn seed_if_empty(pool: &SqlitePool, seeds: &[(&str, &str, &str)]) -> Result<()> {
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM templates")
        .fetch_one(pool)
        .await?;
    if count > 0 {
        return Ok(());
    }

    let now = Utc::now();
    for (name, kind, content) in seeds {
        sqlx::query(
            "INSERT INTO templates
             (name, kind, content, version, content_hash, is_system, created_at, updated_at)
             VALUES (?, ?, ?, 1, ?, 1, ?, ?)",
        )
        .bind(name)
        .bind(kind)
        .bind(content)
        .bind(content_hash(content))
        .bind(now)
        .bind(now)
        .execute(pool)
        .await?;
    }
    Ok(())
}

pub async fn active(pool: &SqlitePool, name: &str, kind: &str) -> Result<Option<Template>> {
    let row = sqlx::query_as::<_, Template>(
        "SELECT * FROM templates
         WHERE name = ? AND kind = ?
         ORDER BY version DESC
         LIMIT 1",
    )
    .bind(name)
    .bind(kind)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn by_id(pool: &SqlitePool, id: i64) -> Result<Option<Template>> {
    let row = sqlx::query_as::<_, Template>("SELECT * FROM templates WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(row)
}

pub async fn by_hash(pool: &SqlitePool, hash: &str) -> Result<Option<Template>> {
    let row = sqlx::query_as::<_, Template>(
        "SELECT * FROM templates WHERE content_hash = ? ORDER BY id ASC LIMIT 1",
    )
    .bind(hash)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

pub async fn list_by_kind(pool: &SqlitePool, kind: &str) -> Result<Vec<Template>> {
    // Latest version per name.
    let rows = sqlx::query_as::<_, Template>(
        "SELECT t.* FROM templates t
         INNER JOIN (
             SELECT name, MAX(version) AS v
             FROM templates
             WHERE kind = ?
             GROUP BY name
         ) latest
         ON t.name = latest.name AND t.version = latest.v AND t.kind = ?
         ORDER BY t.name ASC",
    )
    .bind(kind)
    .bind(kind)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn create_version(
    pool: &SqlitePool,
    name: &str,
    kind: &str,
    content: &str,
    is_system: bool,
) -> Result<Template> {
    let prior = active(pool, name, kind).await?;
    let next_version = prior.as_ref().map(|t| t.version + 1).unwrap_or(1);
    let now = Utc::now();
    let hash = content_hash(content);

    let id: i64 = sqlx::query_scalar(
        "INSERT INTO templates
         (name, kind, content, version, content_hash, is_system, supersedes_id,
          created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
         RETURNING id",
    )
    .bind(name)
    .bind(kind)
    .bind(content)
    .bind(next_version)
    .bind(&hash)
    .bind(is_system)
    .bind(prior.as_ref().map(|t| t.id))
    .bind(now)
    .bind(now)
    .fetch_one(pool)
    .await?;

    by_id(pool, id)
        .await?
        .ok_or(CoreError::Internal("templates::create_version"))
}
