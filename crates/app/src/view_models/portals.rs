//! Portals view-model.
//!
//! Framework-independent data shaping for the Portals screen. The view layer
//! renders these values; it does not recompute health states, suggested
//! actions, or sidecar status.
//!
//! The extra `render_mode` and `scraper_config` fields on `PortalRow` are not
//! shown in the list. They exist so the edit form can round-trip a portal
//! without a second database read and without dropping its scraper config.

use chrono::{DateTime, Utc};

use career_ops_core::acquisition::error::FailureCause;
use career_ops_core::acquisition::router::{FetchError, FetchOutcome};
use career_ops_core::model::{FetchTier, Portal};
use career_ops_core::portals::health::{suggested_action, HealthState, PortalHealth};
use career_ops_core::portals::service::{BatchEntry, FetchReport, PortalWithHealth};

#[derive(Debug, Default)]
pub struct PortalsViewModel {
    pub rows: Vec<PortalRow>,
    pub sidecar: SidecarStatus,
    pub batch: Option<BatchProgress>,
}

#[derive(Debug, Clone)]
pub struct PortalRow {
    pub id: i64,
    pub name: String,
    pub url: String,
    /// `"static"` or `"browser"`; used by the edit form.
    pub render_mode: String,
    /// Raw JSON; preserved on edit but not displayed in the list.
    pub scraper_config: Option<String>,
    pub health: HealthBadge,
    pub last_attempt: Option<String>,
    pub suggested_action: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HealthBadge {
    NeverFetched,
    Healthy,
    Degraded { cause: String },
    Broken { cause: String },
}

impl HealthBadge {
    pub fn cause(&self) -> Option<&str> {
        match self {
            Self::Degraded { cause } | Self::Broken { cause } => Some(cause),
            Self::NeverFetched | Self::Healthy => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SidecarStatus {
    #[default]
    NotStarted,
    Running,
    Stopped,
}

#[derive(Debug, Default)]
pub struct BatchProgress {
    pub total: usize,
    pub completed: Vec<BatchEntry>,
    pub in_flight: Option<i64>,
    pub cancelled: bool,
}

impl PortalsViewModel {
    pub fn from_entries(entries: Vec<PortalWithHealth>) -> Self {
        Self {
            rows: entries.into_iter().map(PortalRow::from_entry).collect(),
            sidecar: SidecarStatus::NotStarted,
            batch: None,
        }
    }

    /// Replace the rows after a reload while preserving screen-level state.
    pub fn replace_entries(&mut self, entries: Vec<PortalWithHealth>) {
        let sidecar = self.sidecar;
        let batch = self.batch.take();
        *self = Self::from_entries(entries);
        self.sidecar = sidecar;
        self.batch = batch;
    }

    pub fn row(&self, id: i64) -> Option<&PortalRow> {
        self.rows.iter().find(|row| row.id == id)
    }

    /// Apply the post-fetch portal row and update the sidecar indicator.
    pub fn apply_fetch_result(&mut self, portal_id: i64, report: &FetchReport) {
        if let Some(row) = self.rows.iter_mut().find(|row| row.id == portal_id) {
            row.apply_portal(&report.portal);
            let health = PortalHealth::from_portal(&report.portal);
            row.apply_health(&health);
        }

        self.update_sidecar(&report.portal, &report.outcome);
    }

    /// Apply one batch entry to the progress list and the matching row.
    ///
    /// `fetch_batch` reports only whether the fetch succeeded, not the tier
    /// that produced it, so a successful batch entry updates the row but can
    /// only infer the sidecar state when the portal is configured for browser
    /// mode. Failures carry the classified cause, which is enough to mark a
    /// sidecar as stopped.
    pub fn apply_batch_entry(&mut self, entry: &BatchEntry) {
        if let Some(batch) = &mut self.batch {
            batch.completed.push(entry.clone());
            batch.in_flight = None;
        }

        let Some(row) = self.rows.iter_mut().find(|row| row.id == entry.portal_id) else {
            return;
        };

        let sidecar = match &entry.result {
            Ok(_) => {
                row.health = HealthBadge::Healthy;
                row.last_attempt = Some("just now".into());
                row.suggested_action = None;

                if row.render_mode == "browser" {
                    Some(SidecarStatus::Running)
                } else {
                    None
                }
            }
            Err(error) => {
                let cause = error.cause.as_str().to_string();
                let had_success = matches!(
                    &row.health,
                    HealthBadge::Healthy | HealthBadge::Degraded { .. }
                );

                row.health = if had_success {
                    HealthBadge::Degraded { cause }
                } else {
                    HealthBadge::Broken { cause }
                };
                row.last_attempt = Some("just now".into());
                row.suggested_action = Some(suggested_action(error.cause));

                if row.render_mode == "browser" || is_tier2_error(error) {
                    Some(SidecarStatus::Stopped)
                } else {
                    None
                }
            }
        };

        if let Some(status) = sidecar {
            self.sidecar = status;
        }
    }

    fn update_sidecar(&mut self, portal: &Portal, outcome: &Result<FetchOutcome, FetchError>) {
        match outcome {
            Ok(outcome) if outcome.tier == FetchTier::Tier2Obscura => {
                self.sidecar = SidecarStatus::Running;
            }
            Err(error) if portal.render_mode == "browser" || is_tier2_error(error) => {
                self.sidecar = SidecarStatus::Stopped;
            }
            _ => {}
        }
    }
}

impl PortalRow {
    fn from_entry(entry: PortalWithHealth) -> Self {
        let mut row = Self::from_portal(&entry.portal);
        row.apply_health(&entry.health);
        row
    }

    fn from_portal(portal: &Portal) -> Self {
        Self {
            id: portal.id,
            name: portal.company_name.clone(),
            url: portal.url.clone(),
            render_mode: portal.render_mode.clone(),
            scraper_config: portal.scraper_config.clone(),
            health: HealthBadge::NeverFetched,
            last_attempt: None,
            suggested_action: None,
        }
    }

    fn apply_portal(&mut self, portal: &Portal) {
        self.name = portal.company_name.clone();
        self.url = portal.url.clone();
        self.render_mode = portal.render_mode.clone();
        self.scraper_config = portal.scraper_config.clone();
    }

    fn apply_health(&mut self, health: &PortalHealth) {
        self.health = health_badge(health.state, health.failure_cause);
        self.last_attempt = health.last_attempt_at.map(format_relative);
        self.suggested_action = health.suggested_action.clone();
    }
}

fn health_badge(state: HealthState, cause: Option<FailureCause>) -> HealthBadge {
    let cause = || {
        cause
            .map(|cause| cause.as_str().to_string())
            .unwrap_or_else(|| "unknown".into())
    };

    match state {
        HealthState::NeverFetched => HealthBadge::NeverFetched,
        HealthState::Healthy => HealthBadge::Healthy,
        HealthState::Degraded => HealthBadge::Degraded { cause: cause() },
        HealthState::Broken => HealthBadge::Broken { cause: cause() },
    }
}

fn is_tier2_error(error: &FetchError) -> bool {
    if error.cause == FailureCause::JsRequired {
        return true;
    }

    let detail = error.detail.to_lowercase();
    detail.contains("tier 2") || detail.contains("obscura")
}

fn format_relative(then: DateTime<Utc>) -> String {
    relative_time(then, Utc::now())
}

/// Coarse relative time for the list. Exposed for deterministic tests.
pub fn relative_time(then: DateTime<Utc>, now: DateTime<Utc>) -> String {
    let seconds = (now - then).num_seconds().max(0);

    if seconds < 60 {
        "just now".into()
    } else if seconds < 3_600 {
        format!("{}m ago", seconds / 60)
    } else if seconds < 86_400 {
        format!("{}h ago", seconds / 3_600)
    } else {
        format!("{}d ago", seconds / 86_400)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn portal(
        id: i64,
        name: &str,
        render_mode: &str,
        attempt: Option<DateTime<Utc>>,
        verified: Option<DateTime<Utc>>,
        failure: Option<&str>,
    ) -> Portal {
        Portal {
            id,
            company_name: name.into(),
            url: format!("https://example.com/{id}"),
            render_mode: render_mode.into(),
            scraper_config: None,
            last_verified_at: verified,
            last_fetch_attempt_at: attempt,
            last_failure_cause: failure.map(str::to_string),
        }
    }

    fn entry(portal: &Portal) -> PortalWithHealth {
        PortalWithHealth {
            health: PortalHealth::from_portal(portal),
            portal: portal.clone(),
        }
    }

    #[test]
    fn from_entries_maps_the_four_health_states() {
        let now = Utc::now();
        let entries = vec![
            entry(&portal(1, "Never", "static", None, None, None)),
            entry(&portal(2, "Healthy", "static", Some(now), Some(now), None)),
            entry(&portal(
                3,
                "Degraded",
                "browser",
                Some(now),
                Some(now),
                Some("blocked"),
            )),
            entry(&portal(
                4,
                "Broken",
                "static",
                Some(now),
                None,
                Some("timeout"),
            )),
        ];

        let view_model = PortalsViewModel::from_entries(entries);

        assert_eq!(view_model.rows[0].health, HealthBadge::NeverFetched);
        assert_eq!(view_model.rows[1].health, HealthBadge::Healthy);
        assert_eq!(
            view_model.rows[2].health,
            HealthBadge::Degraded {
                cause: "blocked".into()
            }
        );
        assert_eq!(
            view_model.rows[3].health,
            HealthBadge::Broken {
                cause: "timeout".into()
            }
        );
        assert!(view_model.rows[2].suggested_action.is_some());
        assert!(view_model.rows[1].suggested_action.is_none());
        assert!(view_model.rows[0].last_attempt.is_none());
        assert!(view_model.rows[1].last_attempt.is_some());
    }

    #[test]
    fn relative_time_is_coarse_and_never_negative() {
        let now = Utc::now();

        assert_eq!(
            relative_time(now - chrono::Duration::seconds(1), now),
            "just now"
        );
        assert_eq!(
            relative_time(now - chrono::Duration::minutes(5), now),
            "5m ago"
        );
        assert_eq!(
            relative_time(now - chrono::Duration::hours(3), now),
            "3h ago"
        );
        assert_eq!(
            relative_time(now - chrono::Duration::days(2), now),
            "2d ago"
        );
        assert_eq!(
            relative_time(now + chrono::Duration::minutes(1), now),
            "just now"
        );
    }

    #[test]
    fn apply_fetch_result_updates_the_row_and_marks_a_tier2_sidecar_running() {
        let now = Utc::now();
        let before = portal(1, "Acme", "static", None, None, None);
        let mut view_model = PortalsViewModel::from_entries(vec![entry(&before)]);

        let after = portal(1, "Acme", "static", Some(now), Some(now), None);
        let report = FetchReport {
            portal: after,
            outcome: Ok(FetchOutcome {
                jobs_fetched: 3,
                tier: FetchTier::Tier2Obscura,
            }),
        };

        view_model.apply_fetch_result(1, &report);

        assert_eq!(view_model.rows[0].health, HealthBadge::Healthy);
        assert_eq!(view_model.sidecar, SidecarStatus::Running);
        assert!(view_model.rows[0].suggested_action.is_none());
    }

    #[test]
    fn apply_fetch_result_marks_a_browser_failure_as_sidecar_stopped() {
        let now = Utc::now();
        let before = portal(1, "Acme", "browser", None, None, None);
        let mut view_model = PortalsViewModel::from_entries(vec![entry(&before)]);

        let after = portal(1, "Acme", "browser", Some(now), None, Some("js_required"));
        let report = FetchReport {
            portal: after,
            outcome: Err(FetchError {
                cause: FailureCause::JsRequired,
                detail: "Tier 2: rendered page had no jobs".into(),
            }),
        };

        view_model.apply_fetch_result(1, &report);

        assert_eq!(
            view_model.rows[0].health,
            HealthBadge::Broken {
                cause: "js_required".into()
            }
        );
        assert!(view_model.rows[0].suggested_action.is_some());
        assert_eq!(view_model.sidecar, SidecarStatus::Stopped);
    }

    #[test]
    fn apply_batch_entry_accumulates_progress_and_updates_health() {
        let never = portal(1, "Never", "static", None, None, None);
        let healthy = portal(
            2,
            "Healthy",
            "static",
            Some(Utc::now()),
            Some(Utc::now()),
            None,
        );

        let mut view_model = PortalsViewModel::from_entries(vec![entry(&never), entry(&healthy)]);
        view_model.batch = Some(BatchProgress {
            total: 2,
            ..Default::default()
        });

        view_model.apply_batch_entry(&BatchEntry {
            portal_id: 1,
            portal_name: "Never".into(),
            result: Err(FetchError {
                cause: FailureCause::Timeout,
                detail: "timed out".into(),
            }),
        });

        assert_eq!(
            view_model.rows[0].health,
            HealthBadge::Broken {
                cause: "timeout".into()
            }
        );
        assert!(view_model.rows[0].suggested_action.is_some());
        assert_eq!(view_model.batch.as_ref().unwrap().completed.len(), 1);

        view_model.apply_batch_entry(&BatchEntry {
            portal_id: 2,
            portal_name: "Healthy".into(),
            result: Ok(4),
        });

        assert_eq!(view_model.rows[1].health, HealthBadge::Healthy);
        assert_eq!(view_model.batch.as_ref().unwrap().completed.len(), 2);
        assert!(view_model.batch.as_ref().unwrap().in_flight.is_none());
    }

    #[test]
    fn a_failed_batch_entry_degrades_a_portal_that_had_succeeded() {
        let healthy = portal(
            1,
            "Healthy",
            "static",
            Some(Utc::now()),
            Some(Utc::now()),
            None,
        );

        let mut view_model = PortalsViewModel::from_entries(vec![entry(&healthy)]);
        view_model.apply_batch_entry(&BatchEntry {
            portal_id: 1,
            portal_name: "Healthy".into(),
            result: Err(FetchError {
                cause: FailureCause::Blocked,
                detail: "403".into(),
            }),
        });

        assert_eq!(
            view_model.rows[0].health,
            HealthBadge::Degraded {
                cause: "blocked".into()
            }
        );
    }
}
