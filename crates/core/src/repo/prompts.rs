//! Prompt repository: versioning, resolution, and lookups.
//!
//! Every edit creates a new row with `supersedes_id` pointing at the prior
//! version **of the same owner**. Content is never rewritten, and a thread has
//! exactly one current version (`is_current`). Reverting activates an existing
//! row; it does not append a copy.
//!
//! A version referenced by an evaluation, cover letter, or triage row is retired
//! rather than deleted. `get_by_hash` still resolves it, so historical review
//! keeps the text that produced an artifact. An unreferenced version may be
//! hard-deleted; its `supersedes_id` children are repointed first.
//!
//! Content is hashed with SHA-256 so `prompt_hash` records remain stable
//! identifiers.
//!
//! ## Two independent version sequences
//!
//! `(name, is_system)` scopes versioning. A user copy of `evaluate` and the
//! shipped system prompt both start at version 1 and count up on their own, so a
//! later release seeding version 2 cannot collide with a user's version 2, and
//! neither can silently replace the other. Resolution for use goes through
//! [`active_for_use`]; [`active`] answers a different question and is documented
//! as such.

use chrono::Utc;
use sqlx::{SqliteConnection, SqlitePool};

use crate::error::{CoreError, Result};
use crate::model::Prompt;

use super::content_hash;

/// Return the newest system prompt version with this name.
///
/// This is what seeding compares against and what it uses to choose the next
/// version number. It is **not** resolution: the current version may be older
/// after an explicit activation. Use [`active_for_use`] for the version a call
/// should send.
///
/// Ignores user prompts even if they share the name. This matters because
/// seeding must never modify a user's custom prompt.
pub async fn newest_system(pool: &SqlitePool, name: &str) -> Result<Option<Prompt>> {
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

/// The current version of `name` owned by `is_system`.
pub async fn current(pool: &SqlitePool, name: &str, is_system: bool) -> Result<Option<Prompt>> {
    let row = sqlx::query_as::<_, Prompt>(
        "SELECT * FROM prompts
         WHERE name = ? AND is_system = ? AND is_current = 1
         LIMIT 1",
    )
    .bind(name)
    .bind(is_system)
    .fetch_optional(pool)
    .await?;
    Ok(row)
}

/// The highest-numbered version of `name` owned by `is_system`.
///
/// Used by seeding and by version numbering, where "newest" matters even when an
/// older row is current.
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

/// The prompt to use for `name`: the user's current version when one exists,
/// the system's current version otherwise.
///
/// Evaluation, cover-letter, triage, and suggestion assembly call this, never
/// `active`. A malformed or absent name resolves to `None`, which the caller
/// turns into an actionable "no prompt" error rather than an empty instruction.
pub async fn active_for_use(pool: &SqlitePool, name: &str) -> Result<Option<Prompt>> {
    if let Some(user) = current(pool, name, false).await? {
        return Ok(Some(user));
    }
    current(pool, name, true).await
}

/// One owner's current version, with the numbers the list shows.
///
/// The Prompt Manager lists a row per `(name, is_system)`: a user copy and the
/// shipped system prompt are different threads with independent versions, so
/// rendering them as one row would hide which one `active_for_use` picks.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct PromptThread {
    pub name: String,
    pub category: String,
    pub is_system: bool,
    /// The id of the current version in this thread.
    pub current_id: i64,
    pub current_version: i64,
    /// The number of non-retired versions.
    pub version_count: i64,
    pub updated_at: chrono::DateTime<Utc>,
}

/// Which table a usage record came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptUsageKind {
    Evaluation,
    CoverLetter,
    Triage,
}

impl PromptUsageKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Evaluation => "Evaluation",
            Self::CoverLetter => "Cover letter",
            Self::Triage => "Triage",
        }
    }
}

/// One stored artifact that recorded a prompt version's content hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PromptUsageEntry {
    pub kind: PromptUsageKind,
    pub id: i64,
    pub generated_at: chrono::DateTime<Utc>,
    pub model_used: String,
    pub job_title: Option<String>,
    pub company: Option<String>,
}

/// The latest current version of every `(name, is_system)` thread, grouped for a
/// list.
///
/// Ordered by category, then name, then system before user, so the caller does
/// not have to sort and two owners of the same name sit together.
pub async fn list_threads(pool: &SqlitePool) -> Result<Vec<PromptThread>> {
    let rows = sqlx::query_as::<_, PromptThread>(
        "SELECT p.name AS name,
                p.category AS category,
                p.is_system AS is_system,
                p.id AS current_id,
                p.version AS current_version,
                (SELECT COUNT(*) FROM prompts q
                  WHERE q.name = p.name AND q.is_system = p.is_system
                    AND q.retired_at IS NULL) AS version_count,
                p.updated_at AS updated_at
         FROM prompts p
         WHERE p.is_current = 1
         ORDER BY p.category ASC, p.name ASC, p.is_system DESC",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Every evaluation report, cover letter, and triage run that used
/// `content_hash`.
///
/// Empty for a prompt version that has never been sent. The job title and
/// company are joined in so the UI can say *which* posting, not only an id.
pub async fn usage(pool: &SqlitePool, content_hash: &str) -> Result<Vec<PromptUsageEntry>> {
    type Row = (
        i64,
        chrono::DateTime<Utc>,
        String,
        Option<String>,
        Option<String>,
    );

    let reports = sqlx::query_as::<_, Row>(
        "SELECT e.id, e.generated_at, e.model_used, j.title, j.company
         FROM evaluation_reports e
         JOIN applications a ON a.id = e.application_id
         JOIN job_descriptions j ON j.id = a.job_description_id
         WHERE e.prompt_hash = ?
         ORDER BY e.generated_at DESC, e.id DESC",
    )
    .bind(content_hash)
    .fetch_all(pool)
    .await?;

    let letters = sqlx::query_as::<_, Row>(
        "SELECT c.id, c.generated_at, c.model_used, j.title, j.company
         FROM cover_letters c
         JOIN applications a ON a.id = c.application_id
         JOIN job_descriptions j ON j.id = a.job_description_id
         WHERE c.prompt_hash = ?
         ORDER BY c.generated_at DESC, c.id DESC",
    )
    .bind(content_hash)
    .fetch_all(pool)
    .await?;

    // A triage row records its prompt hash on the job itself. `triaged_at` and
    // `triage_model` are written together with the hash; the fallbacks keep the
    // entry well-formed if a hand-edited row is incomplete.
    let triage = sqlx::query_as::<_, Row>(
        "SELECT j.id,
                COALESCE(j.triaged_at, j.queued_at, j.fetched_at) AS generated_at,
                COALESCE(j.triage_model, 'unknown') AS model_used,
                j.title,
                j.company
         FROM job_descriptions j
         WHERE j.triage_prompt_hash = ?
         ORDER BY generated_at DESC, j.id DESC",
    )
    .bind(content_hash)
    .fetch_all(pool)
    .await?;

    let to_entry = |kind: PromptUsageKind, (id, generated_at, model_used, title, company): Row| {
        PromptUsageEntry {
            kind,
            id,
            generated_at,
            model_used,
            job_title: title,
            company,
        }
    };

    Ok(reports
        .into_iter()
        .map(|row| to_entry(PromptUsageKind::Evaluation, row))
        .chain(
            letters
                .into_iter()
                .map(|row| to_entry(PromptUsageKind::CoverLetter, row)),
        )
        .chain(
            triage
                .into_iter()
                .map(|row| to_entry(PromptUsageKind::Triage, row)),
        )
        .collect())
}

/// How many stored artifacts reference a content hash.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PromptReferences {
    pub reports: i64,
    pub letters: i64,
    pub triage: i64,
}

impl PromptReferences {
    pub fn total(self) -> i64 {
        self.reports + self.letters + self.triage
    }
}

/// Count the stored artifacts that reference `content_hash`.
///
/// Triage is part of the count: a version used only by the sorter would
/// otherwise look deletable and its `triage_prompt_hash` would stop resolving.
pub async fn references(pool: &SqlitePool, content_hash: &str) -> Result<PromptReferences> {
    let mut conn = pool.acquire().await?;
    references_on(&mut conn, content_hash).await
}

async fn references_on(
    conn: &mut SqliteConnection,
    content_hash: &str,
) -> Result<PromptReferences> {
    let (reports, letters, triage): (i64, i64, i64) = sqlx::query_as(
        "SELECT
           (SELECT COUNT(*) FROM evaluation_reports WHERE prompt_hash = ?),
           (SELECT COUNT(*) FROM cover_letters WHERE prompt_hash = ?),
           (SELECT COUNT(*) FROM job_descriptions WHERE triage_prompt_hash = ?)",
    )
    .bind(content_hash)
    .bind(content_hash)
    .bind(content_hash)
    .fetch_one(&mut *conn)
    .await?;

    Ok(PromptReferences {
        reports,
        letters,
        triage,
    })
}

/// What a delete did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeleteOutcome {
    /// The row was removed. `promoted` is the version that became current when
    /// the deleted row was current, if one remained.
    Deleted { promoted: Option<i64> },
    /// The row is referenced by stored artifacts and no sibling row carries the
    /// same hash, so deleting it would break their provenance.
    InUse(PromptReferences),
}

/// Delete one unreferenced version.
///
/// A system row is refused. A referenced row is refused unless another row with
/// the same `content_hash` still resolves it. The transaction repoints
/// `supersedes_id` children to the deleted row's parent (foreign keys are on and
/// the column has no `ON DELETE` action), then deletes the row and promotes the
/// newest remaining non-retired version when the deleted row was current.
pub async fn delete_version(pool: &SqlitePool, version_id: i64) -> Result<DeleteOutcome> {
    let row = load(pool, version_id).await?;
    if row.is_system {
        return Err(CoreError::Message(
            "System prompts cannot be deleted.".into(),
        ));
    }

    let mut tx = pool.begin().await?;
    let refs = references_on(&mut tx, &row.content_hash).await?;
    if refs.total() > 0 {
        let sibling: Option<i64> =
            sqlx::query_scalar("SELECT id FROM prompts WHERE content_hash = ? AND id <> ? LIMIT 1")
                .bind(&row.content_hash)
                .bind(version_id)
                .fetch_optional(&mut *tx)
                .await?;
        if sibling.is_none() {
            tx.rollback().await?;
            return Ok(DeleteOutcome::InUse(refs));
        }
    }

    // The splice must run before the delete: foreign keys are on and
    // `supersedes_id` has no `ON DELETE` action.
    let next = if row.is_current {
        let next: Option<(i64, i64)> = sqlx::query_as(
            "SELECT id, version FROM prompts
             WHERE name = ? AND is_system = ? AND retired_at IS NULL AND id <> ?
             ORDER BY version DESC LIMIT 1",
        )
        .bind(&row.name)
        .bind(row.is_system)
        .bind(version_id)
        .fetch_optional(&mut *tx)
        .await?;
        if next.is_none() {
            // A remaining retired row would leave the thread with no current
            // version. Make the user restore it explicitly instead of silently
            // resurrecting a version they retired.
            let other: Option<i64> = sqlx::query_scalar(
                "SELECT id FROM prompts WHERE name = ? AND is_system = ? AND id <> ? LIMIT 1",
            )
            .bind(&row.name)
            .bind(row.is_system)
            .bind(version_id)
            .fetch_optional(&mut *tx)
            .await?;
            if other.is_some() {
                tx.rollback().await?;
                return Err(CoreError::Message(
                    "Restore another version before deleting the current one.".into(),
                ));
            }
        }
        next
    } else {
        None
    };

    sqlx::query("UPDATE prompts SET supersedes_id = ? WHERE supersedes_id = ?")
        .bind(row.supersedes_id)
        .bind(version_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM prompts WHERE id = ?")
        .bind(version_id)
        .execute(&mut *tx)
        .await?;

    if let Some((id, _)) = next {
        sqlx::query("UPDATE prompts SET is_current = 1 WHERE id = ?")
            .bind(id)
            .execute(&mut *tx)
            .await?;
    }

    tx.commit().await?;
    Ok(DeleteOutcome::Deleted {
        promoted: next.map(|(_, version)| version),
    })
}

/// Hide a version from the picker without removing its text.
///
/// Retiring the current version promotes the newest remaining non-retired
/// version. The last active version cannot be retired; the user must keep it or,
/// when nothing used it, delete it.
pub async fn retire_version(pool: &SqlitePool, version_id: i64) -> Result<()> {
    let row = load(pool, version_id).await?;
    if row.is_system {
        return Err(CoreError::Message(
            "System prompts cannot be retired.".into(),
        ));
    }
    if row.retired_at.is_some() {
        return Ok(());
    }

    let now = Utc::now();
    if row.is_current {
        let mut tx = pool.begin().await?;
        let next: Option<i64> = sqlx::query_scalar(
            "SELECT id FROM prompts
             WHERE name = ? AND is_system = ? AND retired_at IS NULL AND id <> ?
             ORDER BY version DESC LIMIT 1",
        )
        .bind(&row.name)
        .bind(row.is_system)
        .bind(version_id)
        .fetch_optional(&mut *tx)
        .await?;
        let Some(next) = next else {
            tx.rollback().await?;
            return Err(CoreError::Message(
                "The last active version cannot be retired. Delete it instead if nothing used it."
                    .into(),
            ));
        };
        sqlx::query("UPDATE prompts SET is_current = 0 WHERE name = ? AND is_system = ?")
            .bind(&row.name)
            .bind(row.is_system)
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE prompts SET is_current = 1 WHERE id = ?")
            .bind(next)
            .execute(&mut *tx)
            .await?;
        sqlx::query("UPDATE prompts SET retired_at = ? WHERE id = ?")
            .bind(now)
            .bind(version_id)
            .execute(&mut *tx)
            .await?;
        tx.commit().await?;
    } else {
        sqlx::query("UPDATE prompts SET retired_at = ? WHERE id = ?")
            .bind(now)
            .bind(version_id)
            .execute(pool)
            .await?;
    }
    Ok(())
}

/// Make a retired version selectable again. It does not become current.
pub async fn restore_version(pool: &SqlitePool, version_id: i64) -> Result<()> {
    let row = load(pool, version_id).await?;
    if row.is_system {
        return Err(CoreError::Message(
            "System prompts cannot be restored.".into(),
        ));
    }
    sqlx::query("UPDATE prompts SET retired_at = NULL WHERE id = ?")
        .bind(version_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Make an existing version the one `active_for_use` returns.
///
/// This is the "revert" action: no row is inserted, so no content hash is
/// duplicated and the version count is unchanged. A retired version must be
/// restored first, and a system row is read-only.
pub async fn set_current(pool: &SqlitePool, version_id: i64) -> Result<Prompt> {
    let row = load(pool, version_id).await?;
    if row.is_system {
        return Err(CoreError::Message("System prompts are read-only.".into()));
    }
    if row.retired_at.is_some() {
        return Err(CoreError::Message(
            "Restore this version before making it current.".into(),
        ));
    }

    let mut tx = pool.begin().await?;
    sqlx::query("UPDATE prompts SET is_current = 0 WHERE name = ? AND is_system = ?")
        .bind(&row.name)
        .bind(row.is_system)
        .execute(&mut *tx)
        .await?;
    sqlx::query("UPDATE prompts SET is_current = 1 WHERE id = ?")
        .bind(version_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;

    Ok(Prompt {
        is_current: true,
        ..row
    })
}

async fn load(pool: &SqlitePool, id: i64) -> Result<Prompt> {
    sqlx::query_as::<_, Prompt>("SELECT * FROM prompts WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| CoreError::Message("Prompt version not found.".into()))
}

/// Every version of `name` owned by `is_system`, newest first.
///
/// Retired rows are included; the screen filters them. The detail pane needs
/// them to offer **Show retired** and **Restore**.
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
/// through this lookup. A retired row still resolves: that is the point of
/// retiring instead of deleting a referenced version. A hash can in principle
/// appear on several rows (the same text seeded under two names); the oldest row
/// wins so the answer is stable.
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
/// number and content hash, and make it the system thread's current version.
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
    let mut tx = pool.begin().await?;

    let prior_id: Option<i64> = if version > 1 {
        sqlx::query_scalar(
            "SELECT id FROM prompts
             WHERE name = ? AND is_system = 1 AND version = ?",
        )
        .bind(name)
        .bind(version - 1)
        .fetch_optional(&mut *tx)
        .await?
    } else {
        None
    };

    let now = Utc::now();
    sqlx::query("UPDATE prompts SET is_current = 0 WHERE name = ? AND is_system = 1")
        .bind(name)
        .execute(&mut *tx)
        .await?;
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO prompts
         (name, category, content, is_system, version, content_hash,
          supersedes_id, is_current, created_at, updated_at)
         VALUES (?, ?, ?, 1, ?, ?, ?, 1, ?, ?)
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
    .fetch_one(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(Prompt {
        id,
        name: name.into(),
        category: category.into(),
        content: content.into(),
        is_system: true,
        version,
        content_hash: content_hash.into(),
        supersedes_id: prior_id,
        is_current: true,
        retired_at: None,
        created_at: now,
        updated_at: now,
    })
}

/// Append a version and make it current.
pub async fn create_version(
    pool: &SqlitePool,
    name: &str,
    category: &str,
    content: &str,
    is_system: bool,
) -> Result<Prompt> {
    let mut tx = pool.begin().await?;

    // Version within `(name, is_system)`: a user copy and a shipped system
    // prompt count independently, so neither can claim the other's next number.
    let prior: Option<Prompt> = sqlx::query_as::<_, Prompt>(
        "SELECT * FROM prompts
         WHERE name = ? AND is_system = ?
         ORDER BY version DESC
         LIMIT 1",
    )
    .bind(name)
    .bind(is_system)
    .fetch_optional(&mut *tx)
    .await?;
    let next_version = prior.as_ref().map(|p| p.version + 1).unwrap_or(1);
    let now = Utc::now();
    let hash = content_hash(content);

    sqlx::query("UPDATE prompts SET is_current = 0 WHERE name = ? AND is_system = ?")
        .bind(name)
        .bind(is_system)
        .execute(&mut *tx)
        .await?;
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO prompts
         (name, category, content, is_system, version, content_hash,
          supersedes_id, is_current, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, 1, ?, ?)
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
    .fetch_one(&mut *tx)
    .await?;

    tx.commit().await?;

    Ok(Prompt {
        id,
        name: name.into(),
        category: category.into(),
        content: content.into(),
        is_system,
        version: next_version,
        content_hash: hash,
        supersedes_id: prior.map(|p| p.id),
        is_current: true,
        retired_at: None,
        created_at: now,
        updated_at: now,
    })
}
