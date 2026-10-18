//! Portals — job board configuration for the acquisition pipeline.

use chrono::{DateTime, Utc};
use sqlx::SqlitePool;

use crate::error::{CoreError, Result};
use crate::model::Portal;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderMode {
    Static,
    Browser,
}

impl RenderMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Static => "static",
            Self::Browser => "browser",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "static" => Self::Static,
            "browser" => Self::Browser,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone)]
pub struct NewPortal {
    pub company_name: String,
    pub url: String,
    pub render_mode: RenderMode,
    pub scraper_config: Option<serde_json::Value>,
}

pub async fn list(pool: &SqlitePool) -> Result<Vec<Portal>> {
    let rows = sqlx::query_as::<_, Portal>("SELECT * FROM portals ORDER BY company_name ASC")
        .fetch_all(pool)
        .await?;
    Ok(rows)
}

pub async fn get(pool: &SqlitePool, id: i64) -> Result<Option<Portal>> {
    let row = sqlx::query_as::<_, Portal>("SELECT * FROM portals WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(row)
}

pub async fn find_by_url(pool: &SqlitePool, url: &str) -> Result<Option<Portal>> {
    let row = sqlx::query_as::<_, Portal>("SELECT * FROM portals WHERE url = ?")
        .bind(url)
        .fetch_optional(pool)
        .await?;
    Ok(row)
}

pub async fn create(pool: &SqlitePool, input: NewPortal) -> Result<Portal> {
    let config_json = input
        .scraper_config
        .as_ref()
        .map(serde_json::to_string)
        .transpose()?;

    let id: i64 = sqlx::query_scalar(
        "INSERT INTO portals (company_name, url, render_mode, scraper_config)
         VALUES (?, ?, ?, ?)
         RETURNING id",
    )
    .bind(&input.company_name)
    .bind(&input.url)
    .bind(input.render_mode.as_str())
    .bind(config_json)
    .fetch_one(pool)
    .await?;

    get(pool, id)
        .await?
        .ok_or(CoreError::Internal("portals::create"))
}

pub async fn update(pool: &SqlitePool, id: i64, input: NewPortal) -> Result<Portal> {
    let config_json = input
        .scraper_config
        .as_ref()
        .map(serde_json::to_string)
        .transpose()?;

    let result = sqlx::query(
        "UPDATE portals
         SET company_name = ?, url = ?, render_mode = ?, scraper_config = ?
         WHERE id = ?",
    )
    .bind(&input.company_name)
    .bind(&input.url)
    .bind(input.render_mode.as_str())
    .bind(config_json)
    .bind(id)
    .execute(pool)
    .await?;

    if result.rows_affected() == 0 {
        return Err(CoreError::Internal("portals::update: row not found"));
    }

    get(pool, id)
        .await?
        .ok_or(CoreError::Internal("portals::update"))
}

pub async fn delete(pool: &SqlitePool, id: i64) -> Result<()> {
    sqlx::query("DELETE FROM portals WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn mark_verified(pool: &SqlitePool, id: i64) -> Result<()> {
    let now: DateTime<Utc> = Utc::now();
    sqlx::query("UPDATE portals SET last_verified_at = ? WHERE id = ?")
        .bind(now)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}
