//! Settings view-model: the acquisition preferences as editable text.
//!
//! Five comma-separated fields, because that is the cheapest thing to type and
//! the app already asks for lists elsewhere (the known-site filter). The parsed
//! [`Preferences`] is the source of truth for the dirty comparison, so
//! re-typing the same list with different spacing does not count as a change.

use career_ops_core::preferences::Preferences;

/// The preferences as the form holds them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PreferencesForm {
    pub companies: String,
    pub locations: String,
    pub keywords: String,
    pub excluded_companies: String,
    pub seniority: String,
}

impl PreferencesForm {
    pub fn from_preferences(preferences: &Preferences) -> Self {
        Self {
            companies: preferences.preferred_companies.join(", "),
            locations: preferences.preferred_locations.join(", "),
            keywords: preferences.keywords.join(", "),
            excluded_companies: preferences.excluded_companies.join(", "),
            seniority: preferences.seniority.clone().unwrap_or_default(),
        }
    }

    /// Parse the fields back into preferences.
    ///
    /// Splitting is forgiving: whitespace is trimmed, empty entries are dropped,
    /// and duplicates are dropped case-insensitively, so "Rust, rust ," is one
    /// entry rather than three.
    pub fn to_preferences(&self) -> Preferences {
        Preferences {
            preferred_companies: split_list(&self.companies),
            preferred_locations: split_list(&self.locations),
            keywords: split_list(&self.keywords),
            excluded_companies: split_list(&self.excluded_companies),
            seniority: blank_to_none(&self.seniority),
        }
    }
}

/// The preferences screen's state.
#[derive(Debug, Clone, Default)]
pub struct SettingsViewModel {
    pub form: PreferencesForm,
    /// What storage currently holds, for the dirty comparison.
    saved: Preferences,
}

impl SettingsViewModel {
    pub fn from_preferences(saved: Preferences) -> Self {
        Self {
            form: PreferencesForm::from_preferences(&saved),
            saved,
        }
    }

    /// `true` when the parsed form differs from what is stored.
    pub fn is_dirty(&self) -> bool {
        self.form.to_preferences() != self.saved
    }

    pub fn preferences(&self) -> Preferences {
        self.form.to_preferences()
    }

    /// Adopt the value that was just stored.
    pub fn mark_saved(&mut self, saved: Preferences) {
        self.form = PreferencesForm::from_preferences(&saved);
        self.saved = saved;
    }

    /// `true` when anything is configured. The Pipeline uses this to decide
    /// whether the preference summary is worth showing.
    pub fn is_configured(&self) -> bool {
        !self.saved.is_empty()
    }
}

fn split_list(raw: &str) -> Vec<String> {
    let mut entries: Vec<String> = Vec::new();
    for part in raw.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        if entries
            .iter()
            .any(|kept| kept.to_lowercase() == part.to_lowercase())
        {
            continue;
        }
        entries.push(part.to_string());
    }
    entries
}

fn blank_to_none(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn preferences() -> Preferences {
        Preferences {
            preferred_companies: vec!["Anthropic".into(), "Figma".into()],
            preferred_locations: vec!["Remote".into()],
            keywords: vec!["rust".into(), "compiler".into()],
            excluded_companies: vec!["RecruiterCo".into()],
            seniority: Some("staff".into()),
        }
    }

    #[test]
    fn a_loaded_form_reparses_to_what_was_saved() {
        let view = SettingsViewModel::from_preferences(preferences());

        assert_eq!(view.form.companies, "Anthropic, Figma");
        assert_eq!(view.form.keywords, "rust, compiler");
        assert_eq!(view.form.seniority, "staff");
        assert!(!view.is_dirty());
        assert_eq!(view.preferences(), preferences());
    }

    #[test]
    fn splitting_trims_drops_empties_and_folds_duplicates() {
        let form = PreferencesForm {
            companies: "  Anthropic ,, anthropic , Figma  ,".into(),
            seniority: "   ".into(),
            ..Default::default()
        };

        let parsed = form.to_preferences();

        assert_eq!(
            parsed.preferred_companies,
            vec!["Anthropic".to_string(), "Figma".to_string()]
        );
        assert_eq!(parsed.seniority, None, "whitespace is not a value");
    }

    #[test]
    fn respacing_an_unchanged_list_is_not_a_change() {
        let mut view = SettingsViewModel::from_preferences(preferences());

        view.form.companies = "Anthropic,Figma".into();
        assert!(!view.is_dirty(), "the parsed value is what counts");

        view.form.companies = "Anthropic, Figma, Linear".into();
        assert!(view.is_dirty());
    }

    #[test]
    fn marking_saved_adopts_the_stored_value_and_clears_dirty() {
        let mut view = SettingsViewModel::from_preferences(Preferences::default());
        view.form.keywords = "rust".into();
        assert!(view.is_dirty());

        let stored = view.preferences();
        view.mark_saved(stored);

        assert!(!view.is_dirty());
        assert!(view.is_configured());
        assert_eq!(view.form.keywords, "rust");
    }

    #[test]
    fn an_empty_configuration_says_so() {
        let view = SettingsViewModel::default();

        assert!(!view.is_configured());
        assert!(!view.is_dirty());
    }
}
