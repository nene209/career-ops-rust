//! CV entries — experience, education, and skill rows for the candidate.
//!
//! Entries are ordered by `sort_order`. New entries append to the end by
//! default. `reorder` accepts a full ordering in one transaction.

use chrono::Utc;
use sqlx::SqlitePool;

use crate::error::{CoreError, Result};
use crate::model::CvEntry;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CvEntryKind {
    Experience,
    Education,
    Skill,
}

impl CvEntryKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Experience => "experience",
            Self::Education => "education",
            Self::Skill => "skill",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "experience" => Self::Experience,
            "education" => Self::Education,
            "skill" => Self::Skill,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone)]
pub struct NewCvEntry {
    pub profile_id: i64,
    pub kind: CvEntryKind,
    pub title: String,
    pub organization: Option<String>,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
    pub description: Option<String>,
    pub metadata: Option<serde_json::Value>,
    /// `None` appends to the end.
    pub sort_order: Option<i64>,
}

pub async fn list_for_profile(pool: &SqlitePool, profile_id: i64) -> Result<Vec<CvEntry>> {
    let rows = sqlx::query_as::<_, CvEntry>(
        "SELECT * FROM cv_entries WHERE profile_id = ? ORDER BY sort_order ASC, id ASC",
    )
    .bind(profile_id)
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn get(pool: &SqlitePool, id: i64) -> Result<Option<CvEntry>> {
    let row = sqlx::query_as::<_, CvEntry>("SELECT * FROM cv_entries WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(row)
}

pub async fn create(pool: &SqlitePool, input: NewCvEntry) -> Result<CvEntry> {
    let sort_order = match input.sort_order {
        Some(n) => n,
        None => {
            let max: Option<i64> = sqlx::query_scalar(
                "SELECT MAX(sort_order) FROM cv_entries WHERE profile_id = ?",
            )
            .bind(input.profile_id)
            .fetch_one(pool)
            .await?;
            max.unwrap_or(0) + 1
        }
    };

    let metadata_json = input
        .metadata
        .as_ref()
        .map(serde_json::to_string)
        .transpose()?;

    let id: i64 = sqlx::query_scalar(
        "INSERT INTO cv_entries
         (profile_id, kind, title, organization, start_date, end_date, description, metadata, sort_order)
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
         RETURNING id",
    )
    .bind(input.profile_id)
    .bind(input.kind.as_str())
    .bind(&input.title)
    .bind(&input.organization)
    .bind(&input.start_date)
    .bind(&input.end_date)
    .bind(&input.description)
    .bind(metadata_json)
    .bind(sort_order)
    .fetch_one(pool)
    .await?;

    get(pool, id).await?.ok_or(CoreError::Internal("cv_entries::create"))
}

pub async fn update(pool: &SqlitePool, id: i64, input: NewCvEntry) -> Result<CvEntry> {
    let metadata_json = input
        .metadata
        .as_ref()
        .map(serde_json::to_string)
        .transpose()?;

    let result = sqlx::query(
        "UPDATE cv_entries
         SET kind = ?, title = ?, organization = ?, start_date = ?, end_date = ?,
             description = ?, metadata = ?
         WHERE id = ? AND profile_id = ?",
    )
    .bind(input.kind.as_str())
    .bind(&input.title)
    .bind(&input.organization)
    .bind(&input.start_date)
    .bind(&input.end_date)
    .bind(&input.description)
    .bind(metadata_json)
    .bind(id)
    .bind(input.profile_id)
    .execute(pool)
    .await?;

    if result.rows_affected() == 0 {
        return Err(CoreError::Internal("cv_entries::update: row not found"));
    }

    get(pool, id).await?.ok_or(CoreError::Internal("cv_entries::update"))
}

pub async fn delete(pool: &SqlitePool, id: i64) -> Result<()> {
    sqlx::query("DELETE FROM cv_entries WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Rewrite `sort_order` for the given entries, all belonging to `profile_id`.
/// Entries not listed are left untouched, so a caller that wants a full
/// reordering must pass the complete set.
pub async fn reorder(pool: &SqlitePool, profile_id: i64, ordered_ids: &[i64]) -> Result<()> {
    let mut tx = pool.begin().await?;
    for (idx, id) in ordered_ids.iter().enumerate() {
        let result = sqlx::query(
            "UPDATE cv_entries SET sort_order = ? WHERE id = ? AND profile_id = ?",
        )
        .bind(idx as i64)
        .bind(id)
        .bind(profile_id)
        .execute(&mut *tx)
        .await?;
        if result.rows_affected() == 0 {
            tx.rollback().await.ok();
            return Err(CoreError::Internal("cv_entries::reorder: unknown id"));
        }
    }
    tx.commit().await?;
    Ok(())
}

/// Sanity helper — unused in production but useful for tests.
pub async fn count(pool: &SqlitePool, profile_id: i64) -> Result<i64> {
    let n: i64 = sqlx::query_scalar("SELECT count(*) FROM cv_entries WHERE profile_id = ?")
        .bind(profile_id)
        .fetch_one(pool)
        .await?;
    Ok(n)
}