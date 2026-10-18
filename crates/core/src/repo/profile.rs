use chrono::Utc;
use sqlx::SqlitePool;

use crate::error::Result;
use crate::model::Profile;

pub async fn ensure_singleton(pool: &SqlitePool) -> Result<Profile> {
    if let Some(p) = get(pool).await? {
        return Ok(p);
    }
    let now = Utc::now();
    sqlx::query(
        "INSERT INTO profile (id, name, email, created_at, updated_at)
         VALUES (1, '', '', ?, ?)",
    )
    .bind(now)
    .bind(now)
    .execute(pool)
    .await?;
    get(pool).await?.ok_or(crate::error::CoreError::Internal("profile insert"))
}

pub async fn get(pool: &SqlitePool) -> Result<Option<Profile>> {
    let row = sqlx::query_as::<_, Profile>("SELECT * FROM profile WHERE id = 1")
        .fetch_optional(pool)
        .await?;
    Ok(row)
}

pub async fn update_identity(
    pool: &SqlitePool,
    name: &str,
    email: &str,
    location: Option<&str>,
) -> Result<()> {
    let now = Utc::now();
    sqlx::query(
        "UPDATE profile SET name = ?, email = ?, location = ?, updated_at = ? WHERE id = 1",
    )
    .bind(name)
    .bind(email)
    .bind(location)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(())
}