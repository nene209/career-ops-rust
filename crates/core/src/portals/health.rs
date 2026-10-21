//! Portal health summary.
//!
//! Pure computation from a `Portal` row to a `PortalHealth` struct,
//! plus the mapping from `FailureCause` to a suggested user action.
//!
//! ## State derivation
//!
//! `last_fetch_attempt_at` distinguishes "never fetched" from "fetched
//! at least once". `last_failure_cause` distinguishes success from
//! failure. `last_verified_at` distinguishes "has ever succeeded" from
//! "has only ever failed". The four combinations produce four states:
//!
//! | attempt | cause | verified | state |
//! | :--- | :--- | :--- | :--- |
//! | NULL | — | — | NeverFetched |
//! | set | NULL | — | Healthy |
//! | set | set | set | Degraded |
//! | set | set | NULL | Broken |
//!
//! An unrecognized `last_failure_cause` string (from a downgrade, or a
//! hand-edited database) is treated as `FailureCause::Other`. The state
//! is still Degraded or Broken, because the raw column says a failure
//! was recorded; only the suggested action is generic.

use chrono::{DateTime, Utc};

use crate::acquisition::error::FailureCause;
use crate::model::Portal;

#[derive(Debug, Clone)]
pub struct PortalHealth {
    pub state: HealthState,
    pub last_attempt_at: Option<DateTime<Utc>>,
    pub last_success_at: Option<DateTime<Utc>>,
    pub failure_cause: Option<FailureCause>,
    pub suggested_action: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HealthState {
    /// Never fetched.
    NeverFetched,
    /// Last fetch succeeded.
    Healthy,
    /// Last fetch failed, but the portal has succeeded at least once.
    Degraded,
    /// Last fetch failed and the portal has never succeeded.
    Broken,
}

impl PortalHealth {
    pub fn from_portal(portal: &Portal) -> Self {
        // Distinguish "no failure recorded" from "failure recorded but
        // unrecognized". The raw Option answers the first question; the
        // parsed Option answers the second.
        let had_failure = portal.last_failure_cause.is_some();
        let failure_cause = portal
            .last_failure_cause
            .as_deref()
            .map(|s| FailureCause::parse(s).unwrap_or(FailureCause::Other));

        let state = match (
            portal.last_fetch_attempt_at.is_some(),
            had_failure,
            portal.last_verified_at.is_some(),
        ) {
            (false, _, _) => HealthState::NeverFetched,
            (true, false, _) => HealthState::Healthy,
            (true, true, true) => HealthState::Degraded,
            (true, true, false) => HealthState::Broken,
        };

        let suggested_action = failure_cause.map(suggested_action);

        Self {
            state,
            last_attempt_at: portal.last_fetch_attempt_at,
            last_success_at: portal.last_verified_at,
            failure_cause,
            suggested_action,
        }
    }
}

/// Map a failure cause to a user-facing action.
///
/// Every cause has exactly one action. The message should tell the user
/// what to try next, not explain what went wrong in technical terms —
/// the user can infer the technical cause from the cause name if they
/// care, but the action is what gets them unstuck.
pub fn suggested_action(cause: FailureCause) -> String {
    match cause {
        FailureCause::JsRequired => {
            "Switch this portal to browser mode in its settings.".into()
        }
        FailureCause::Blocked => {
            "The site may be blocking automated requests. Try again later, or switch to browser mode.".into()
        }
        FailureCause::Timeout => {
            "The request timed out. Check network connectivity, or retry.".into()
        }
        FailureCause::ParseError => {
            "The page structure may have changed. Check the portal URL, or configure scraper selectors.".into()
        }
        FailureCause::NotFound => {
            "The board was not found. Check the company slug in the URL.".into()
        }
        FailureCause::Transport => {
            "Could not reach the site. Check network connectivity.".into()
        }
        FailureCause::InvalidRequest => {
            "The portal configuration is invalid. Check the URL and scraper config.".into()
        }
        FailureCause::Other => {
            "The fetch failed for an unrecognized reason. Try again, or check the portal configuration.".into()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn portal_with(
        last_attempt: Option<DateTime<Utc>>,
        verified: Option<DateTime<Utc>>,
        cause: Option<&str>,
    ) -> Portal {
        Portal {
            id: 1,
            company_name: "Acme".into(),
            url: "https://example.com".into(),
            render_mode: "static".into(),
            scraper_config: None,
            last_verified_at: verified,
            last_fetch_attempt_at: last_attempt,
            last_failure_cause: cause.map(str::to_string),
        }
    }

    fn now() -> DateTime<Utc> {
        Utc::now()
    }

    #[test]
    fn never_fetched() {
        let h = PortalHealth::from_portal(&portal_with(None, None, None));
        assert_eq!(h.state, HealthState::NeverFetched);
        assert!(h.failure_cause.is_none());
        assert!(h.suggested_action.is_none());
    }

    #[test]
    fn healthy() {
        let h = PortalHealth::from_portal(&portal_with(Some(now()), Some(now()), None));
        assert_eq!(h.state, HealthState::Healthy);
        assert!(h.failure_cause.is_none());
    }

    #[test]
    fn degraded() {
        let h = PortalHealth::from_portal(&portal_with(Some(now()), Some(now()), Some("blocked")));
        assert_eq!(h.state, HealthState::Degraded);
        assert_eq!(h.failure_cause, Some(FailureCause::Blocked));
        assert!(h.suggested_action.is_some());
    }

    #[test]
    fn broken() {
        let h = PortalHealth::from_portal(&portal_with(Some(now()), None, Some("timeout")));
        assert_eq!(h.state, HealthState::Broken);
        assert_eq!(h.failure_cause, Some(FailureCause::Timeout));
    }

    #[test]
    fn unrecognized_cause_falls_back_to_other() {
        let h =
            PortalHealth::from_portal(&portal_with(Some(now()), None, Some("newfangled_cause")));
        assert_eq!(h.state, HealthState::Broken);
        assert_eq!(h.failure_cause, Some(FailureCause::Other));
    }

    #[test]
    fn every_cause_has_a_distinct_action() {
        let causes = [
            FailureCause::JsRequired,
            FailureCause::Blocked,
            FailureCause::Timeout,
            FailureCause::ParseError,
            FailureCause::NotFound,
            FailureCause::Transport,
            FailureCause::InvalidRequest,
            FailureCause::Other,
        ];
        let actions: Vec<String> = causes.iter().map(|&c| suggested_action(c)).collect();
        let unique: std::collections::HashSet<&String> = actions.iter().collect();
        assert_eq!(
            unique.len(),
            causes.len(),
            "each FailureCause should map to a distinct action, got {actions:?}"
        );
    }

    #[test]
    fn failure_cause_round_trips() {
        for cause in [
            FailureCause::JsRequired,
            FailureCause::Blocked,
            FailureCause::Timeout,
            FailureCause::ParseError,
            FailureCause::NotFound,
            FailureCause::Transport,
            FailureCause::InvalidRequest,
            FailureCause::Other,
        ] {
            assert_eq!(FailureCause::parse(cause.as_str()), Some(cause));
        }
    }
}
