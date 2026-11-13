//! Settings screen tests.
//!
//! The screen is rendered against a hand-built view model. The app-level test in
//! `app.rs` covers the storage round trip; these cover the form, the gates, and
//! the actions.

use career_ops_app::screens::settings::{
    self, PrivacyPreview, SettingsAction, SettingsRenderState,
};
use career_ops_app::view_models::settings::SettingsViewModel;
use career_ops_core::llm::{LlmConfig, LlmProvider};
use career_ops_core::preferences::Preferences;
use career_ops_core::privacy::{PrivacyConfig, RedactionReport};
use egui::accesskit::Role;
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;

const FIELDS: [&str; 5] = [
    "Preferred companies",
    "Preferred locations",
    "Keywords",
    "Excluded companies",
    "Seniority",
];

struct TestState {
    view_model: SettingsViewModel,
    actions: Vec<SettingsAction>,
}

fn harness(
    view_model: SettingsViewModel,
    render: SettingsRenderState<'static>,
) -> Harness<'static, TestState> {
    // Tall enough that the AI section below the five preference fields is on
    // screen: a clipped widget accepts no click.
    Harness::builder()
        .with_size((900.0, 1_600.0))
        .build_ui_state(
            move |ui, state: &mut TestState| {
                if let Some(action) = settings::show(ui, &mut state.view_model, render) {
                    state.actions.push(action);
                }
            },
            TestState {
                view_model,
                actions: Vec::new(),
            },
        )
}

fn configured() -> Preferences {
    Preferences {
        preferred_companies: vec!["Anthropic".into(), "Figma".into()],
        preferred_locations: vec!["Remote".into()],
        keywords: vec!["rust".into()],
        excluded_companies: vec!["RecruiterCo".into()],
        seniority: Some("staff".into()),
    }
}

fn type_into(harness: &mut Harness<'static, TestState>, label: &str, text: &str) {
    let field = harness.get_by_label(label);
    field.focus();
    field.type_text(text);
    harness.run();
}

#[test]
fn the_five_fields_and_their_explanations_render() {
    let mut harness = harness(SettingsViewModel::default(), SettingsRenderState::default());
    harness.run();

    let _heading = harness.get_by_label("Acquisition preferences");
    for label in FIELDS {
        let _ = harness.get_by_label(label);
    }
    let _ = harness.get_by_label("No preferences yet");
    let _ = harness.get_by_label("Matched against the job title and the description.");
}

#[test]
fn a_saved_configuration_prefills_the_fields() {
    let mut harness = harness(
        SettingsViewModel::from_preferences(configured()),
        SettingsRenderState::default(),
    );
    harness.run();

    let _ = harness.get_by_label("Preferences saved");
    for label in FIELDS {
        let _ = harness.get_by_label(label);
    }
    let save = harness.get_by_role_and_label(Role::Button, "Save preferences");
    assert!(
        save.accesskit_node().is_disabled(),
        "an unchanged form must not offer a save"
    );
}

#[test]
fn saving_is_gated_on_a_change_and_sends_the_parsed_lists() {
    let mut harness = harness(SettingsViewModel::default(), SettingsRenderState::default());
    harness.run();

    let save = harness.get_by_role_and_label(Role::Button, "Save preferences");
    assert!(save.accesskit_node().is_disabled());

    type_into(&mut harness, "Keywords", "rust, compiler , rust");

    assert!(harness.state().view_model.is_dirty());
    let _ = harness.get_by_label("· unsaved changes");

    harness
        .get_by_role_and_label(Role::Button, "Save preferences")
        .click();
    harness.run();

    let Some(SettingsAction::Save(preferences)) = harness
        .state()
        .actions
        .iter()
        .find(|action| matches!(action, SettingsAction::Save(_)))
    else {
        panic!("actions: {:?}", harness.state().actions);
    };
    assert_eq!(
        preferences.keywords,
        vec!["rust".to_string(), "compiler".to_string()],
        "the duplicates collapse and the blanks disappear"
    );
}

#[test]
fn reload_is_always_offered() {
    let mut harness = harness(SettingsViewModel::default(), SettingsRenderState::default());
    harness.run();

    harness
        .get_by_role_and_label(Role::Button, "Reload")
        .click();
    harness.run();

    assert_eq!(harness.state().actions, vec![SettingsAction::Reload]);
}

#[test]
fn an_error_offers_a_retry() {
    let mut harness = harness(
        SettingsViewModel::default(),
        SettingsRenderState {
            error: Some("Could not save the preferences: disk full"),
            ..Default::default()
        },
    );
    harness.run();

    let _ = harness.get_by_label("Could not save the preferences: disk full");
    harness.get_by_role_and_label(Role::Button, "Retry").click();
    harness.run();

    assert_eq!(harness.state().actions, vec![SettingsAction::Reload]);
}

#[test]
fn a_loading_screen_replaces_the_form() {
    let mut harness = harness(
        SettingsViewModel::default(),
        SettingsRenderState {
            loading: true,
            ..Default::default()
        },
    );
    // A spinner requests a repaint every frame, so `run` never settles.
    harness.run_steps(2);

    let _ = harness.get_by_label("Loading settings...");
    assert!(
        harness.query_by_label("Preferred companies").is_none(),
        "the form must not render before the stored value arrives"
    );
}

#[test]
fn a_saving_screen_disables_the_buttons() {
    let mut harness = harness(
        SettingsViewModel::from_preferences(configured()),
        SettingsRenderState {
            saving: true,
            ..Default::default()
        },
    );
    // The saving spinner repaints every frame.
    harness.run_steps(2);

    assert!(harness
        .get_by_role_and_label(Role::Button, "Reload")
        .accesskit_node()
        .is_disabled());
    assert!(harness
        .get_by_role_and_label(Role::Button, "Save preferences")
        .accesskit_node()
        .is_disabled());
}

// ---------------------------------------------------------------------
// AI section (Phase 3 WS1)
// ---------------------------------------------------------------------

#[test]
fn the_ai_section_renders_with_deepseek_and_nothing_configured() {
    let mut harness = harness(SettingsViewModel::default(), SettingsRenderState::default());
    harness.run();

    let _ = harness.get_by_label("AI evaluation");
    let _ = harness.get_by_label("Provider");
    let _ = harness.get_by_label("Model");
    let _ = harness.get_by_label("API key");
    let _ = harness.get_by_label("No key stored.");

    for label in ["Save AI settings", "Save key", "Test connection"] {
        assert!(
            harness
                .get_by_role_and_label(Role::Button, label)
                .accesskit_node()
                .is_disabled(),
            "{label} must be disabled with nothing to do"
        );
    }
}

#[test]
fn providers_this_build_cannot_reach_are_disabled() {
    let mut harness = harness(SettingsViewModel::default(), SettingsRenderState::default());
    harness.run();

    for label in ["DeepSeek", "Local / custom"] {
        assert!(
            !harness
                .get_by_role_and_label(Role::Button, label)
                .accesskit_node()
                .is_disabled(),
            "{label} is constructible"
        );
    }
    for label in ["OpenAI", "Anthropic", "Gemini"] {
        assert!(
            harness
                .get_by_role_and_label(Role::Button, label)
                .accesskit_node()
                .is_disabled(),
            "{label} is not constructible yet"
        );
    }
}

#[test]
fn a_custom_endpoint_needs_no_key_and_can_be_tested_without_one() {
    let mut view = SettingsViewModel::default();
    view.set_llm(
        LlmConfig {
            provider: LlmProvider::Custom,
            model: "llama3.2".into(),
            base_url: "http://localhost:11434/v1".into(),
            ..LlmConfig::default()
        },
        false,
    );
    let mut harness = harness(view, SettingsRenderState::default());
    harness.run();

    // The key is optional, and the test button does not wait for one.
    let _ = harness.get_by_label("API key (optional)");
    let _ = harness.get_by_label("No key stored (optional).");
    assert!(!harness
        .get_by_role_and_label(Role::Button, "Test connection")
        .accesskit_node()
        .is_disabled());

    harness
        .get_by_role_and_label(Role::Button, "Test connection")
        .click();
    harness.run();

    match harness.state().actions.as_slice() {
        [SettingsAction::TestConnection(config)] => {
            assert_eq!(config.provider, LlmProvider::Custom);
            assert_eq!(config.base_url, "http://localhost:11434/v1");
        }
        other => panic!("unexpected actions: {other:?}"),
    }
}

#[test]
fn a_custom_endpoint_without_a_url_cannot_be_tested() {
    let mut view = SettingsViewModel::default();
    view.set_llm(
        LlmConfig {
            provider: LlmProvider::Custom,
            model: "llama3.2".into(),
            ..LlmConfig::default()
        },
        false,
    );
    let mut harness = harness(view, SettingsRenderState::default());
    harness.run();

    assert!(
        harness
            .get_by_role_and_label(Role::Button, "Test connection")
            .accesskit_node()
            .is_disabled(),
        "a missing base URL is a settings problem, not a connection problem"
    );
}

#[test]
fn a_hosted_provider_still_needs_a_key_before_testing() {
    let mut view = SettingsViewModel::default();
    view.set_llm(LlmConfig::default(), false);
    let mut harness = harness(view, SettingsRenderState::default());
    harness.run();

    let _ = harness.get_by_label("API key");
    let _ = harness.get_by_label("No key stored.");
    assert!(harness
        .get_by_role_and_label(Role::Button, "Test connection")
        .accesskit_node()
        .is_disabled());
}

fn custom_view() -> SettingsViewModel {
    let mut view = SettingsViewModel::default();
    view.set_llm(
        LlmConfig {
            provider: LlmProvider::Custom,
            model: "llama3.2".into(),
            base_url: "http://localhost:11434/v1".into(),
            ..LlmConfig::default()
        },
        false,
    );
    view
}

#[test]
fn the_endpoint_fields_belong_to_the_custom_provider_only() {
    let mut custom = harness(custom_view(), SettingsRenderState::default());
    custom.run();
    for label in ["Base URL", "Use JSON mode", "Context window (tokens)"] {
        let _ = custom.get_by_label(label);
    }

    let mut hosted = harness(SettingsViewModel::default(), SettingsRenderState::default());
    hosted.run();
    for label in ["Base URL", "Use JSON mode", "Context window (tokens)"] {
        assert!(
            hosted.query_by_label(label).is_none(),
            "{label} is not a DeepSeek setting"
        );
    }
}

#[test]
fn turning_json_mode_off_is_what_the_form_reports() {
    let mut harness = harness(custom_view(), SettingsRenderState::default());
    harness.run();

    let checkbox = harness.get_by_label("Use JSON mode");
    checkbox.click();
    harness.run();

    assert!(!harness.state().view_model.llm.json_mode);
    assert!(harness.state().view_model.is_llm_dirty());
    assert!(!harness
        .get_by_role_and_label(Role::Button, "Save AI settings")
        .accesskit_node()
        .is_disabled());
}

#[test]
fn a_typed_context_window_reaches_the_saved_configuration() {
    let mut harness = harness(custom_view(), SettingsRenderState::default());
    harness.run();

    type_into(&mut harness, "Context window (tokens)", "8192");
    harness
        .get_by_role_and_label(Role::Button, "Save AI settings")
        .click();
    harness.run();

    match harness.state().actions.as_slice() {
        [SettingsAction::SaveLlm(config)] => {
            assert_eq!(config.custom_context_window, Some(8_192));
            assert_eq!(config.base_url, "http://localhost:11434/v1");
            assert_eq!(config.model, "llama3.2");
        }
        other => panic!("unexpected actions: {other:?}"),
    }
}

#[test]
fn a_plain_http_url_away_from_this_machine_is_warned_about_and_still_savable() {
    let mut view = SettingsViewModel::default();
    view.set_llm(
        LlmConfig {
            provider: LlmProvider::Custom,
            model: "llama3.2".into(),
            base_url: "http://192.168.1.50:8080/v1".into(),
            ..LlmConfig::default()
        },
        false,
    );
    let mut harness = harness(view, SettingsRenderState::default());
    harness.run();

    let warning = harness.get_by_label("Plain http to 192.168.1.50 sends your CV and any API key across the network in clear text. Use https, or an address that stays on this machine.");
    let _ = warning;
    // A warning is not a refusal: the endpoint can still be tested.
    assert!(!harness
        .get_by_role_and_label(Role::Button, "Test connection")
        .accesskit_node()
        .is_disabled());
}

#[test]
fn an_unusable_base_url_is_explained_beside_the_endpoint() {
    let mut view = SettingsViewModel::default();
    view.set_llm(
        LlmConfig {
            provider: LlmProvider::Custom,
            model: "llama3.2".into(),
            base_url: "ftp://localhost/v1".into(),
            ..LlmConfig::default()
        },
        false,
    );
    let mut harness = harness(view, SettingsRenderState::default());
    harness.run();

    let _ = harness.get_by_label("the base URL must start with http:// or https://");
    assert!(harness
        .get_by_role_and_label(Role::Button, "Test connection")
        .accesskit_node()
        .is_disabled());
}

// ---------------------------------------------------------------------
// Privacy section (Phase 3.6 WS3)
// ---------------------------------------------------------------------

const REDACT_LABEL: &str = "Redact personal details before sending to the model";

#[test]
fn the_privacy_section_defaults_on_and_saves_a_change() {
    let mut harness = harness(SettingsViewModel::default(), SettingsRenderState::default());
    harness.run();

    let _ = harness.get_by_label("Privacy");
    let _ = harness.get_by_label("Removes: name, email, phone, street-address location.");
    assert!(
        harness.state().view_model.privacy.redact_cv,
        "redaction is on out of the box"
    );
    assert!(
        harness
            .get_by_role_and_label(Role::Button, "Save privacy settings")
            .accesskit_node()
            .is_disabled(),
        "nothing to save yet"
    );

    harness.get_by_label(REDACT_LABEL).click();
    harness.run();

    assert!(!harness.state().view_model.privacy.redact_cv);
    harness
        .get_by_role_and_label(Role::Button, "Save privacy settings")
        .click();
    harness.run();

    match harness.state().actions.as_slice() {
        [SettingsAction::SavePrivacy(config)] => assert!(!config.redact_cv),
        other => panic!("unexpected actions: {other:?}"),
    }
}

#[test]
fn a_stored_toggle_off_is_what_the_checkbox_shows() {
    let mut view = SettingsViewModel::default();
    view.set_privacy(PrivacyConfig { redact_cv: false });
    let mut harness = harness(view, SettingsRenderState::default());
    harness.run();

    assert!(!harness.state().view_model.privacy.redact_cv);
    assert!(!harness.state().view_model.is_privacy_dirty());
}

#[test]
fn the_preview_button_asks_for_the_summary_and_opens_the_window() {
    let mut harness = harness(SettingsViewModel::default(), SettingsRenderState::default());
    harness.run();

    harness
        .get_by_role_and_label(Role::Button, "Preview what the model sees")
        .click();
    harness.run();

    match harness.state().actions.as_slice() {
        [SettingsAction::PreviewPrivacy { redact }] => {
            assert!(*redact, "the form's current value is what is previewed")
        }
        other => panic!("unexpected actions: {other:?}"),
    }
    assert!(harness.state().view_model.privacy_preview_open);
}

#[test]
fn the_window_shows_the_redacted_line_and_its_counts() {
    let mut view = SettingsViewModel::default();
    view.privacy_preview_open = true;
    let render = SettingsRenderState {
        privacy_preview: Some(PrivacyPreview {
            text: "⟦NAME⟧ at ⟦EMAIL⟧",
            redacted: true,
            report: RedactionReport {
                names: 1,
                emails: 1,
                phones: 0,
                locations: 0,
            },
        }),
        ..Default::default()
    };
    let mut harness = harness(view, render);
    harness.run();

    let _ = harness.get_by_label("What the model sees");
    let _ = harness.get_by_label("2 replacements: 1 name, 1 email, 0 phones, 0 locations.");
    let _ = harness.get_by_label("⟦NAME⟧ at ⟦EMAIL⟧");
    assert!(
        harness.query_by_label("Ada Lovelace").is_none(),
        "the real name is not in the preview"
    );
}

#[test]
fn the_window_warns_when_redaction_is_off() {
    let mut view = SettingsViewModel::default();
    view.privacy_preview_open = true;
    let render = SettingsRenderState {
        privacy_preview: Some(PrivacyPreview {
            text: "Ada Lovelace <ada@example.com>",
            redacted: false,
            report: RedactionReport::default(),
        }),
        ..Default::default()
    };
    let mut harness = harness(view, render);
    harness.run();

    let _ =
        harness.get_by_label("Redaction is off: this is the summary exactly as it will be sent.");
    let _ = harness.get_by_label("Ada Lovelace <ada@example.com>");
}

#[test]
fn a_preview_that_could_not_be_built_says_why() {
    let mut view = SettingsViewModel::default();
    view.privacy_preview_open = true;
    let render = SettingsRenderState {
        privacy_preview_error: Some("Add your CV in CV / Profile first."),
        ..Default::default()
    };
    let mut harness = harness(view, render);
    harness.run();

    let _ = harness.get_by_label("Add your CV in CV / Profile first.");
}

#[test]
fn switching_providers_keeps_the_endpoint_and_replaces_the_model() {
    let mut harness = harness(custom_view(), SettingsRenderState::default());
    harness.run();

    harness
        .get_by_role_and_label(Role::Button, "DeepSeek")
        .click();
    harness.run();

    let switched = &harness.state().view_model;
    assert_eq!(switched.llm.provider, LlmProvider::DeepSeek);
    assert_eq!(switched.llm.model, "deepseek-flash");
    assert_eq!(
        switched.llm.base_url, "http://localhost:11434/v1",
        "the endpoint is kept for when the user switches back"
    );

    harness
        .get_by_role_and_label(Role::Button, "Local / custom")
        .click();
    harness.run();

    let back = &harness.state().view_model;
    assert_eq!(back.llm.provider, LlmProvider::Custom);
    assert_eq!(
        back.llm.model, "",
        "DeepSeek's model would not be served by the endpoint"
    );
    assert_eq!(back.llm.base_url, "http://localhost:11434/v1");
}

#[test]
fn a_model_change_enables_saving_and_emits_the_config() {
    let mut view = SettingsViewModel::default();
    view.set_llm(LlmConfig::default(), false);
    view.llm.model = "deepseek-v4-pro".into();
    let mut harness = harness(view, SettingsRenderState::default());
    harness.run();

    harness
        .get_by_role_and_label(Role::Button, "Save AI settings")
        .click();
    harness.run();

    match harness.state().actions.as_slice() {
        [SettingsAction::SaveLlm(config)] => {
            assert_eq!(config.provider, LlmProvider::DeepSeek);
            assert_eq!(config.model, "deepseek-v4-pro");
        }
        other => panic!("unexpected actions: {other:?}"),
    }
}

#[test]
fn a_stored_key_is_shown_as_a_fact_and_can_be_tested_or_deleted() {
    let mut view = SettingsViewModel::default();
    view.set_llm(LlmConfig::default(), true);
    let mut harness = harness(view, SettingsRenderState::default());
    harness.run();

    let _ = harness.get_by_label("A key is stored.");
    assert!(!harness
        .get_by_role_and_label(Role::Button, "Test connection")
        .accesskit_node()
        .is_disabled());

    harness
        .get_by_role_and_label(Role::Button, "Delete key")
        .click();
    harness.run();

    assert_eq!(
        harness.state().actions,
        vec![SettingsAction::DeleteApiKey(LlmProvider::DeepSeek)]
    );
}

#[test]
fn saving_a_typed_key_emits_it_and_clears_the_field() {
    let mut harness = harness(SettingsViewModel::default(), SettingsRenderState::default());
    harness.run();

    type_into(&mut harness, "API key", "sk-typed");
    harness
        .get_by_role_and_label(Role::Button, "Save key")
        .click();
    harness.run();

    match harness.state().actions.as_slice() {
        [SettingsAction::SaveApiKey { key, config }] => {
            assert_eq!(key, "sk-typed");
            assert_eq!(config.provider, LlmProvider::DeepSeek);
        }
        other => panic!("unexpected actions: {other:?}"),
    }
    assert!(
        harness.state().view_model.llm.api_key.is_empty(),
        "the typed key is handed to the app, not kept"
    );
}

#[test]
fn the_readiness_line_is_rendered_when_the_app_supplies_one() {
    let mut harness = harness(
        SettingsViewModel::default(),
        SettingsRenderState {
            llm_status: Some("Add a DeepSeek API key in Settings."),
            ..Default::default()
        },
    );
    harness.run();

    let _ = harness.get_by_label("Add a DeepSeek API key in Settings.");
}
