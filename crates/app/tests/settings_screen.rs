//! Settings screen tests.
//!
//! The screen is rendered against a hand-built view model. The app-level test in
//! `app.rs` covers the storage round trip; these cover the form, the gates, and
//! the actions.

use career_ops_app::screens::settings::{self, SettingsAction, SettingsRenderState};
use career_ops_app::view_models::settings::SettingsViewModel;
use career_ops_core::preferences::Preferences;
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
    Harness::new_ui_state(
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
