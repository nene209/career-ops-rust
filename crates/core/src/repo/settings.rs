//! Settings — a key/value JSON store.
//!
//! API keys are NOT stored here. They live in the OS keychain (§7 of the
//! main plan). The settings table stores provider/model choices and other
//! non-sensitive configuration.
//!
//! Values are serialized as JSON so that structured config (provider maps,
//! user preferences) round-trips without a schema change per new field.

use serde::de::DeserializeOwned;
use serde::Serialize;
use sqlx::SqlitePool;

use crate::error::Result;

pub async fn get_raw(pool: &SqlitePool, key: &str) -> Result<Option<String>> {
    let value: Option<String> = sqlx::query_scalar("SELECT value FROM settings WHERE key = ?")
        .bind(key)
        .fetch_optional(pool)
        .await?;
    Ok(value)
}

pub async fn get_json<T: DeserializeOwned>(pool: &SqlitePool, key: &str) -> Result<Option<T>> {
    match get_raw(pool, key).await? {
        Some(raw) => Ok(Some(serde_json::from_str(&raw)?)),
        None => Ok(None),
    }
}

pub async fn get_or<T: DeserializeOwned>(pool: &SqlitePool, key: &str, default: T) -> Result<T> {
    Ok(get_json::<T>(pool, key).await?.unwrap_or(default))
}

pub async fn set_json<T: Serialize>(pool: &SqlitePool, key: &str, value: &T) -> Result<()> {
    let raw = serde_json::to_string(value)?;
    sqlx::query(
        "INSERT INTO settings (key, value) VALUES (?, ?)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
    )
    .bind(key)
    .bind(raw)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn delete(pool: &SqlitePool, key: &str) -> Result<()> {
    sqlx::query("DELETE FROM settings WHERE key = ?")
        .bind(key)
        .execute(pool)
        .await?;
    Ok(())
}
