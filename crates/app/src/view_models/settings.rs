//! Settings view-model: the acquisition preferences as editable text.
//!
//! Five comma-separated fields, because that is the cheapest thing to type and
//! the app already asks for lists elsewhere (the known-site filter). The parsed
//! [`Preferences`] is the source of truth for the dirty comparison, so
//! re-typing the same list with different spacing does not count as a change.

use career_ops_core::llm::{LlmConfig, LlmProvider};
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

/// The AI section as the form holds it.
///
/// The API key is write-only: it is typed, handed to the app, and cleared. The
/// stored key is represented by `has_api_key`, never by its text.
#[derive(Debug, Clone, PartialEq)]
pub struct LlmForm {
    pub provider: LlmProvider,
    pub model: String,
    pub api_key: String,
    pub has_api_key: bool,
    /// The custom endpoint's base URL, as typed. Kept for every provider so
    /// switching to DeepSeek and back does not lose it.
    pub base_url: String,
    /// Whether a custom endpoint is asked for `response_format`.
    pub json_mode: bool,
    /// The context window in tokens, as typed. Empty means "unknown", which
    /// disables the pre-flight token check.
    pub context_window: String,
}

impl Default for LlmForm {
    fn default() -> Self {
        let config = LlmConfig::default();
        Self {
            provider: config.provider,
            model: config.model,
            api_key: String::new(),
            has_api_key: false,
            base_url: config.base_url,
            json_mode: config.json_mode,
            context_window: String::new(),
        }
    }
}

impl LlmForm {
    fn adopt(&mut self, config: &LlmConfig) {
        self.provider = config.provider;
        self.model = config.model.clone();
        self.base_url = config.base_url.clone();
        self.json_mode = config.json_mode;
        self.context_window = config
            .custom_context_window
            .map(|window| window.to_string())
            .unwrap_or_default();
    }
}

/// The preferences screen's state.
#[derive(Debug, Clone, Default)]
pub struct SettingsViewModel {
    pub form: PreferencesForm,
    /// What storage currently holds, for the dirty comparison.
    saved: Preferences,
    pub llm: LlmForm,
    /// The whole LLM configuration as stored, so saving a provider or model
    /// does not reset the budget fields the form does not edit.
    saved_llm: LlmConfig,
}

impl SettingsViewModel {
    pub fn from_preferences(saved: Preferences) -> Self {
        Self {
            form: PreferencesForm::from_preferences(&saved),
            saved,
            llm: LlmForm::default(),
            saved_llm: LlmConfig::default(),
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

    /// Adopt the stored LLM configuration and whether a key is present.
    pub fn set_llm(&mut self, saved: LlmConfig, has_api_key: bool) {
        self.llm.adopt(&saved);
        self.llm.api_key.clear();
        self.llm.has_api_key = has_api_key;
        self.saved_llm = saved;
    }

    /// `true` when the form describes a different configuration than storage.
    pub fn is_llm_dirty(&self) -> bool {
        self.llm_config() != self.saved_llm
    }

    /// The configuration the form describes, with the stored temperature and
    /// budgets preserved, the model and URL sanitized, and the context window
    /// parsed.
    pub fn llm_config(&self) -> LlmConfig {
        LlmConfig {
            provider: self.llm.provider,
            model: self.llm.model.trim().to_string(),
            base_url: self.llm.base_url.trim().to_string(),
            json_mode: self.llm.json_mode,
            custom_context_window: parse_context_window(&self.llm.context_window),
            ..self.saved_llm.clone()
        }
    }

    /// Adopt the value that was just stored.
    pub fn mark_llm_saved(&mut self, saved: LlmConfig) {
        self.llm.adopt(&saved);
        self.saved_llm = saved;
    }

    /// Record that the keychain now holds (or no longer holds) a key. A saved
    /// key is never echoed back into the field.
    pub fn set_api_key_present(&mut self, present: bool) {
        self.llm.has_api_key = present;
        self.llm.api_key.clear();
    }

    /// Take the typed key for storage, leaving the field empty.
    pub fn take_api_key(&mut self) -> String {
        std::mem::take(&mut self.llm.api_key)
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

/// The typed context window.
///
/// Empty is `None`: the user does not know the window, so the pre-flight check
/// is skipped rather than guessed. Anything that is not a number becomes `0`,
/// which `validate_custom` refuses with a message about this field — silently
/// reading it as "unknown" would ignore what the user typed.
fn parse_context_window(raw: &str) -> Option<usize> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(trimmed.parse::<usize>().unwrap_or(0))
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

    #[test]
    fn an_llm_form_tracks_provider_and_model_changes() {
        let mut view = SettingsViewModel::default();
        view.set_llm(LlmConfig::default(), false);
        assert!(!view.is_llm_dirty());

        view.llm.model = "deepseek-v4-pro".into();
        assert!(view.is_llm_dirty());
        assert_eq!(view.llm_config().model, "deepseek-v4-pro");

        view.mark_llm_saved(view.llm_config());
        assert!(!view.is_llm_dirty());
        assert_eq!(view.llm.model, "deepseek-v4-pro");
    }

    #[test]
    fn saving_a_model_preserves_the_fields_the_form_does_not_edit() {
        let stored = LlmConfig {
            temperature: 0.9,
            max_tokens: Some(1_024),
            max_job_chars: 7_000,
            ..LlmConfig::default()
        };
        let mut view = SettingsViewModel::default();
        view.set_llm(stored.clone(), true);

        view.llm.model = "deepseek-v4-pro".into();
        let form = view.llm_config();

        assert_eq!(form.temperature, 0.9);
        assert_eq!(form.max_tokens, Some(1_024));
        assert_eq!(form.max_job_chars, 7_000);
    }

    #[test]
    fn a_stored_custom_endpoint_fills_the_form_and_round_trips() {
        let stored = LlmConfig {
            provider: LlmProvider::Custom,
            model: "qwen2.5:7b".into(),
            base_url: "http://localhost:11434/v1".into(),
            json_mode: false,
            custom_context_window: Some(32_768),
            ..LlmConfig::default()
        };
        let mut view = SettingsViewModel::default();
        view.set_llm(stored.clone(), false);

        assert_eq!(view.llm.base_url, "http://localhost:11434/v1");
        assert!(!view.llm.json_mode);
        assert_eq!(view.llm.context_window, "32768");
        assert!(!view.is_llm_dirty(), "an unchanged form is not a change");
        assert_eq!(view.llm_config(), stored);
    }

    #[test]
    fn the_context_window_is_optional_and_refuses_what_is_not_a_number() {
        let mut view = SettingsViewModel::default();
        view.set_llm(LlmConfig::default(), false);

        view.llm.context_window = "   ".into();
        assert_eq!(view.llm_config().custom_context_window, None, "blank");

        view.llm.context_window = "8192".into();
        assert_eq!(view.llm_config().custom_context_window, Some(8_192));

        // A typo must not silently disable the pre-flight check.
        view.llm.context_window = "8k".into();
        assert_eq!(view.llm_config().custom_context_window, Some(0));
    }

    #[test]
    fn toggling_json_mode_and_changing_the_endpoint_make_the_form_dirty() {
        let custom = LlmConfig {
            provider: LlmProvider::Custom,
            model: "llama3.2".into(),
            base_url: "http://localhost:11434/v1".into(),
            ..LlmConfig::default()
        };
        let mut view = SettingsViewModel::default();
        view.set_llm(custom, false);
        assert!(!view.is_llm_dirty());

        view.llm.json_mode = false;
        assert!(view.is_llm_dirty());
        assert!(!view.llm_config().json_mode);

        view.llm.json_mode = true;
        view.llm.base_url = "http://127.0.0.1:8080/v1".into();
        assert!(view.is_llm_dirty());
        assert_eq!(view.llm_config().base_url, "http://127.0.0.1:8080/v1");
    }

    #[test]
    fn a_saved_key_is_never_echoed_back_into_the_field() {
        let mut view = SettingsViewModel::default();
        view.llm.api_key = "sk-typed".into();

        assert_eq!(view.take_api_key(), "sk-typed");
        assert!(view.llm.api_key.is_empty(), "typed text is consumed");

        view.llm.api_key = "sk-typed-again".into();
        view.set_api_key_present(true);
        assert!(view.llm.has_api_key);
        assert!(view.llm.api_key.is_empty(), "a stored key is not shown");
    }
}
