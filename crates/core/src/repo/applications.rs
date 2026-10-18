//! Applications — the pipeline. Each row is a `(job_description, status)`
//! pair moving through a state machine.
//!
//! ## Human-in-the-loop invariant
//!
//! `applied` is reachable through exactly one code path: `mark_applied`.
//! `transition` refuses to move a row into `applied`, regardless of the
//! current state. This encodes the product's core principle — the application
//! never marks itself as having submitted on the user's behalf — at the
//! repository layer, so any code path that would violate it fails to compile
//! or fails at runtime with a clear error.
//!
//! The invariant is tested in this module's `#[cfg(test)]` section.

use chrono::{DateTime, Utc};
use sqlx::SqlitePool;

use crate::error::{CoreError, Result};
use crate::model::{Application, ApplicationStatus};

pub async fn get(pool: &SqlitePool, id: i64) -> Result<Option<Application>> {
    let row = sqlx::query_as::<_, Application>("SELECT * FROM applications WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await?;
    Ok(row)
}

pub async fn list(pool: &SqlitePool) -> Result<Vec<Application>> {
    let rows = sqlx::query_as::<_, Application>(
        "SELECT * FROM applications ORDER BY updated_at DESC",
    )
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

pub async fn list_by_status(
    pool: &SqlitePool,
    status: ApplicationStatus,
) -> Result<Vec<Application>> {
    let rows = sqlx::query_as::<_, Application>(
        "SELECT * FROM applications WHERE status = ? ORDER BY updated_at DESC",
    )
    .bind(status.as_str())
    .fetch_all(pool)
    .await?;
    Ok(rows)
}

/// Create a new application in `discovered` state.
pub async fn create(pool: &SqlitePool, job_description_id: i64) -> Result<Application> {
    let now: DateTime<Utc> = Utc::now();
    let id: i64 = sqlx::query_scalar(
        "INSERT INTO applications
         (job_description_id, status, created_at, updated_at)
         VALUES (?, 'discovered', ?, ?)
         RETURNING id",
    )
    .bind(job_description_id)
    .bind(now)
    .bind(now)
    .fetch_one(pool)
    .await?;
    get(pool, id).await?.ok_or(CoreError::Internal("applications::create"))
}

/// Move an application to a new status.
///
/// Rejects `to == Applied` with a clear error — use `mark_applied` instead.
/// Uses compare-and-set on the current status to avoid a lost-update race.
pub async fn transition(
    pool: &SqlitePool,
    id: i64,
    to: ApplicationStatus,
) -> Result<Application> {
    if to == ApplicationStatus::Applied {
        return Err(CoreError::Internal(
            "applications::transition cannot reach 'applied'; use mark_applied",
        ));
    }

    let current = get(pool, id)
        .await?
        .ok_or(CoreError::Internal("applications::transition: row not found"))?;
    let current_status = ApplicationStatus::parse(&current.status)
        .ok_or(CoreError::Internal("applications::transition: unknown current status"))?;

    if !is_valid_transition(current_status, to) {
        return Err(CoreError::Internal(
            "applications::transition: invalid transition",
        ));
    }

    let now: DateTime<Utc> = Utc::now();
    let result = sqlx::query(
        "UPDATE applications SET status = ?, updated_at = ?
         WHERE id = ? AND status = ?",
    )
    .bind(to.as_str())
    .bind(now)
    .bind(id)
    .bind(&current.status)
    .execute(pool)
    .await?;

    if result.rows_affected() == 0 {
        return Err(CoreError::Internal(
            "applications::transition: status changed concurrently",
        ));
    }

    get(pool, id)
        .await?
        .ok_or(CoreError::Internal("applications::transition: row vanished"))
}

/// Mark an application as applied. The only path to `applied`.
/// Requires the current status to be `ready`, expressed as a CAS so a
/// concurrent status change cannot sneak past the precondition.
pub async fn mark_applied(pool: &SqlitePool, id: i64) -> Result<Application> {
    let now: DateTime<Utc> = Utc::now();
    let result = sqlx::query(
        "UPDATE applications
         SET status = 'applied', applied_at = ?, updated_at = ?
         WHERE id = ? AND status = 'ready'",
    )
    .bind(now)
    .bind(now)
    .bind(id)
    .execute(pool)
    .await?;

    if result.rows_affected() == 0 {
        return Err(CoreError::Internal(
            "applications::mark_applied requires current status = 'ready'",
        ));
    }

    get(pool, id)
        .await?
        .ok_or(CoreError::Internal("applications::mark_applied: row vanished"))
}

pub async fn set_match_score(pool: &SqlitePool, id: i64, score: f64) -> Result<()> {
    if !(0.0..=1.0).contains(&score) {
        return Err(CoreError::Internal("match_score must be in [0.0, 1.0]"));
    }
    let now: DateTime<Utc> = Utc::now();
    sqlx::query("UPDATE applications SET match_score = ?, updated_at = ? WHERE id = ?")
        .bind(score)
        .bind(now)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn attach_report(pool: &SqlitePool, id: i64, report_id: i64) -> Result<()> {
    let now: DateTime<Utc> = Utc::now();
    sqlx::query(
        "UPDATE applications SET evaluation_report_id = ?, updated_at = ? WHERE id = ?",
    )
    .bind(report_id)
    .bind(now)
    .bind(id)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn update_notes(pool: &SqlitePool, id: i64, notes: &str) -> Result<()> {
    let now: DateTime<Utc> = Utc::now();
    sqlx::query("UPDATE applications SET notes = ?, updated_at = ? WHERE id = ?")
        .bind(notes)
        .bind(now)
        .bind(id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Allowed status transitions.
///
/// Forward: discovery -> evaluation -> ready -> (via mark_applied) -> applied
/// -> interviewing -> offer.
///
/// Backward: every backward edge exists for correcting a mistake. `offer` is
/// terminal — reopening an accepted or declined offer is a decision, not a
/// transition, and belongs in a future explicit method.
///
/// `rejected` is reachable from any non-terminal state, and can be reopened
/// back to `discovered`.
fn is_valid_transition(from: ApplicationStatus, to: ApplicationStatus) -> bool {
    use ApplicationStatus::*;
    matches!(
        (from, to),
        (Discovered, Evaluating)
            | (Discovered, Ready)
            | (Discovered, Rejected)
            | (Evaluating, Discovered)
            | (Evaluating, Ready)
            | (Evaluating, Rejected)
            | (Ready, Discovered)
            | (Ready, Evaluating)
            | (Ready, Rejected)
            | (Applied, Ready)
            | (Applied, Interviewing)
            | (Applied, Rejected)
            | (Interviewing, Offer)
            | (Interviewing, Rejected)
            | (Rejected, Discovered)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: the real test suite lives in tests/ with a shared pool helper.
    // These unit tests exercise only the pure transition predicate so they
    // can run without a database.

    #[test]
    fn applied_is_not_reachable_via_transition() {
        // Every "from" state must be rejected when "to" is Applied.
        // This is the human-in-the-loop invariant expressed as a predicate.
        for from in [
            ApplicationStatus::Discovered,
            ApplicationStatus::Evaluating,
            ApplicationStatus::Ready,
            ApplicationStatus::Applied,
            ApplicationStatus::Interviewing,
            ApplicationStatus::Rejected,
            ApplicationStatus::Offer,
        ] {
            assert!(
                !is_valid_transition(from, ApplicationStatus::Applied),
                "transition predicate must not allow {from:?} -> Applied; \
                 use mark_applied instead"
            );
        }
    }

    #[test]
    fn forward_pipeline_is_valid() {
        use ApplicationStatus::*;
        assert!(is_valid_transition(Discovered, Evaluating));
        assert!(is_valid_transition(Evaluating, Ready));
        assert!(is_valid_transition(Applied, Interviewing));
        assert!(is_valid_transition(Interviewing, Offer));
    }

    #[test]
    fn offer_is_terminal() {
        use ApplicationStatus::*;
        for to in [
            Discovered,
            Evaluating,
            Ready,
            Applied,
            Interviewing,
            Rejected,
        ] {
            assert!(
                !is_valid_transition(Offer, to),
                "Offer must be terminal; {to:?} should not be reachable"
            );
        }
    }

    #[test]
    fn rejection_is_reversible() {
        use ApplicationStatus::*;
        assert!(is_valid_transition(Rejected, Discovered));
        // But not to other states — reopening lands the row back at the start
        // of the pipeline so it can be re-evaluated.
        assert!(!is_valid_transition(Rejected, Applied));
        assert!(!is_valid_transition(Rejected, Offer));
    }
}