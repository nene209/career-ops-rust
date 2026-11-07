//! Acquisition preferences: what to surface first, and what to hide.
//!
//! Stored as JSON in `profile.preferences`, which has existed since Phase 1 and
//! was unused until now. The type is deliberately independent of the app's view
//! models: matching takes the *facts* about a posting, so core can be tested
//! without egui and the Pipeline decides which row fields to feed in.
//!
//! Matching is **case-insensitive substring** on the specific field. Not regex,
//! not word boundaries: the Pipeline search behaves the same way, and someone
//! who types "rust" means "rust" anywhere in the title.
//!
//! The two halves have different jobs. Preferred companies, locations, and
//! keywords **boost** — they badge a row and offer a filter, but hide nothing.
//! Exclusions **filter**: an excluded company's rows are hidden in the `All`
//! view and reachable through an explicit `Excluded` filter. "Show me what I
//! want" is an ordering problem; "hide what I do not want" is a decision the
//! user made once.

use serde::{Deserialize, Serialize};

/// The JSON stored in `profile.preferences`.
///
/// Every field is optional in the stored form: `#[serde(default)]` makes a
/// missing key an empty list, so a minimal object like
/// `{"keywords":["rust"]}` parses.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Preferences {
    /// Companies whose rows get the **Preferred** badge.
    pub preferred_companies: Vec<String>,
    /// Locations matched against the posting's own location.
    pub preferred_locations: Vec<String>,
    /// Matched against the title and the description.
    pub keywords: Vec<String>,
    /// Companies whose rows the `All` Pipeline view hides.
    pub excluded_companies: Vec<String>,
    /// Free text ("staff", "senior"). Recorded and shown; no filter uses it.
    /// Phase 3.2's triage reads it.
    pub seniority: Option<String>,
}

/// The posting fields a preference can look at.
#[derive(Debug, Clone, Copy)]
pub struct PreferenceFacts<'a> {
    pub company: &'a str,
    pub location: Option<&'a str>,
    pub title: &'a str,
    pub description: &'a str,
}

impl<'a> PreferenceFacts<'a> {
    pub fn new(company: &'a str, location: Option<&'a str>, title: &'a str) -> Self {
        Self {
            company,
            location,
            title,
            description: "",
        }
    }

    /// Attach the posting text, for keyword matching.
    pub fn with_description(mut self, description: &'a str) -> Self {
        self.description = description;
        self
    }
}

/// What a posting is, relative to the saved preferences.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PreferenceMatch {
    /// A preferred company, location, or keyword matched.
    pub preferred: bool,
    /// The company is on the exclusion list.
    pub excluded: bool,
}

impl Preferences {
    /// `true` when nothing is configured, so the Pipeline can skip the work.
    pub fn is_empty(&self) -> bool {
        self.preferred_companies.is_empty()
            && self.preferred_locations.is_empty()
            && self.keywords.is_empty()
            && self.excluded_companies.is_empty()
            && self.seniority.is_none()
    }

    /// Parse the stored column.
    ///
    /// `NULL`, an empty string, or a malformed value reads as empty rather than
    /// as an error: the column predates this phase and is hand-editable, and a
    /// broken value must not make the Pipeline unopenable. Malformed input is
    /// logged, because silently discarding a user's configuration is worse than
    /// an empty Pipeline.
    pub fn from_json(raw: Option<&str>) -> Self {
        let Some(raw) = raw.map(str::trim).filter(|raw| !raw.is_empty()) else {
            return Self::default();
        };

        match serde_json::from_str(raw) {
            Ok(preferences) => preferences,
            Err(error) => {
                tracing::warn!(%error, "profile.preferences did not parse; using empty preferences");
                Self::default()
            }
        }
    }

    /// Classify a posting.
    pub fn matches(&self, facts: PreferenceFacts<'_>) -> PreferenceMatch {
        PreferenceMatch {
            preferred: self.is_preferred(&facts),
            excluded: contains_any(&self.excluded_companies, facts.company),
        }
    }

    fn is_preferred(&self, facts: &PreferenceFacts<'_>) -> bool {
        contains_any(&self.preferred_companies, facts.company)
            || facts
                .location
                .is_some_and(|location| contains_any(&self.preferred_locations, location))
            || contains_any(&self.keywords, facts.title)
            || contains_any(&self.keywords, facts.description)
    }
}

/// Case-insensitive substring search over a list of needles.
///
/// Blank needles are skipped, so a trailing comma in the UI field cannot make
/// every row match.
fn contains_any(needles: &[String], haystack: &str) -> bool {
    let haystack = haystack.to_lowercase();
    needles
        .iter()
        .map(|needle| needle.trim())
        .filter(|needle| !needle.is_empty())
        .any(|needle| haystack.contains(&needle.to_lowercase()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn facts<'a>(
        company: &'a str,
        location: Option<&'a str>,
        title: &'a str,
    ) -> PreferenceFacts<'a> {
        // No description by default: a posting that matches on nothing must not
        // match by accident. The keyword-in-description case passes its own.
        PreferenceFacts::new(company, location, title)
    }

    fn preferences() -> Preferences {
        Preferences {
            preferred_companies: vec!["Anthropic".into(), "Figma".into()],
            preferred_locations: vec!["Remote".into(), "Berlin".into()],
            keywords: vec!["compiler".into(), "Rust".into()],
            excluded_companies: vec!["RecruiterCo".into()],
            seniority: Some("staff".into()),
        }
    }

    #[test]
    fn a_preferred_company_matches_case_insensitively() {
        let prefs = preferences();

        let hit = prefs.matches(facts("Anthropic", Some("London"), "Engineer"));
        assert!(hit.preferred);
        assert!(!hit.excluded);

        assert!(
            prefs
                .matches(facts("anthropic labs", None, "Engineer"))
                .preferred,
            "a substring is enough: the company field is a name, not an id"
        );
        assert!(!prefs.matches(facts("Figma", None, "Designer")).excluded);
    }

    #[test]
    fn a_preferred_location_matches_remote_and_a_city_with_region() {
        let prefs = preferences();

        assert!(
            prefs
                .matches(facts("Acme", Some("Remote"), "Engineer"))
                .preferred
        );
        assert!(
            prefs
                .matches(facts("Acme", Some("Berlin, Germany"), "Engineer"))
                .preferred
        );
        assert!(
            !prefs
                .matches(facts("Acme", Some("London, UK"), "Engineer"))
                .preferred
        );
        assert!(
            !prefs.matches(facts("Acme", None, "Engineer")).preferred,
            "a posting with no location cannot match a location preference"
        );
    }

    #[test]
    fn an_exclusion_matches_a_substring_of_the_company() {
        let prefs = preferences();

        let hit = prefs.matches(facts("RecruiterCo Ltd", None, "Engineer"));
        assert!(hit.excluded);
        assert!(!hit.preferred);
    }

    #[test]
    fn a_keyword_matches_the_title_or_the_description() {
        let prefs = preferences();

        assert!(
            prefs
                .matches(facts("Acme", None, "Compiler Engineer"))
                .preferred,
            "the title matches"
        );
        assert!(
            prefs
                .matches(
                    facts("Acme", None, "Backend Engineer")
                        .with_description("We build developer tools in Rust.")
                )
                .preferred,
            "the description matches"
        );
        assert!(
            !prefs
                .matches(
                    facts("Acme", None, "Office Manager")
                        .with_description("Scheduling and travel.")
                )
                .preferred
        );
    }

    #[test]
    fn empty_preferences_badge_and_hide_nothing() {
        let prefs = Preferences::default();

        let hit = prefs.matches(facts("RecruiterCo", Some("Remote"), "Rust Engineer"));

        assert!(!hit.preferred);
        assert!(!hit.excluded);
        assert!(prefs.is_empty());
    }

    #[test]
    fn blank_entries_are_ignored() {
        let prefs = Preferences {
            preferred_companies: vec!["  ".into(), String::new()],
            keywords: vec![" ".into()],
            ..Default::default()
        };

        let hit = prefs.matches(facts("Acme", Some("Remote"), "Engineer"));

        assert!(
            !hit.preferred,
            "a blank list entry must not match every posting"
        );
    }

    #[test]
    fn seniority_is_recorded_but_never_matches() {
        let prefs = Preferences {
            seniority: Some("staff".into()),
            ..Default::default()
        };

        let hit = prefs.matches(facts("Acme", Some("Stafford"), "Staff Engineer"));

        assert!(
            !hit.preferred,
            "seniority is a hint for triage, not a filter"
        );
        assert!(!prefs.is_empty(), "it is still configured");
    }

    #[test]
    fn the_stored_json_round_trips_and_tolerates_absences() {
        let prefs = preferences();
        let raw = serde_json::to_string(&prefs).expect("serialize");

        assert_eq!(Preferences::from_json(Some(&raw)), prefs);
        assert_eq!(Preferences::from_json(None), Preferences::default());
        assert_eq!(Preferences::from_json(Some("   ")), Preferences::default());
    }

    #[test]
    fn a_minimal_object_parses_and_a_malformed_one_reads_as_empty() {
        let minimal = Preferences::from_json(Some(r#"{"keywords":["rust"]}"#));

        assert_eq!(minimal.keywords, vec!["rust".to_string()]);
        assert!(minimal.preferred_companies.is_empty());

        assert_eq!(
            Preferences::from_json(Some("{not json")),
            Preferences::default(),
            "a broken value must not make the Pipeline unopenable"
        );
    }
}
