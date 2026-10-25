//! Workstream 1 shell tests.
//!
//! These exercise the two non-obvious pieces of the shell contract:
//!
//! 1. `labelled_input` must produce an accessible name, or `get_by_label`
//!    (and a screen reader) cannot find the field.
//! 2. Sidebar navigation must change the rendered `MainScreen`.

use career_ops_app::navigation::{MainScreen, NavigationState};
use career_ops_app::screens::components::{labelled_input, EmptyState, StatusBadge};
use career_ops_app::screens::shell;
use egui::accesskit::Role;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;

#[derive(Default)]
struct FormState {
    value: String,
}

#[test]
fn labelled_input_is_queryable_by_its_label() {
    let mut harness = Harness::new_ui_state(
        |ui, state: &mut FormState| {
            labelled_input(ui, "Passphrase", &mut state.value, true);
        },
        FormState::default(),
    );

    harness.run();

    // `get_by_label` deliberately does not return the label node when the
    // widget points at it with `labelled_by`, so finding a node here proves
    // the association exists.
    let _input = harness.get_by_label("Passphrase");

    // The harness runs this automatically after `run`, but asserting it here
    // makes the intent explicit if the automatic check is ever disabled.
    assert!(
        harness.unnamed_widgets().is_empty(),
        "text inputs must have accessible names"
    );
}

#[test]
fn sidebar_navigation_switches_the_rendered_screen() {
    let mut harness = Harness::new_ui_state(
        |ui, nav: &mut NavigationState| shell::show(ui, nav),
        NavigationState::new(),
    );

    harness.run();
    assert_eq!(harness.state().current(), MainScreen::Pipeline);

    harness
        .get_by_role_and_label(Role::Button, "Portals")
        .click();
    harness.run();

    assert_eq!(
        harness.state().current(),
        MainScreen::Portals,
        "clicking the sidebar item should switch the current screen"
    );

    // The Portals placeholder is unique to that screen, so finding it proves
    // the central panel re-rendered, not just the sidebar selection.
    let _placeholder = harness.get_by_label("No portals configured yet");

    harness
        .get_by_role_and_label(Role::Button, "Settings")
        .click();
    harness.run();

    assert_eq!(harness.state().current(), MainScreen::Settings);
    let _settings_placeholder =
        harness.get_by_label("Application settings will be configured here.");
}

#[derive(Default)]
struct ClickState {
    clicks: usize,
}

#[test]
fn empty_state_action_button_reports_clicks() {
    let mut harness = Harness::new_ui_state(
        |ui, state: &mut ClickState| {
            let clicked = EmptyState::new("[ ]", "No jobs yet", "Nothing here yet.")
                .with_action("Add job")
                .show(ui);
            state.clicks += usize::from(clicked);
        },
        ClickState::default(),
    );

    harness.run();
    harness.get_by_label("Add job").click();
    harness.run();

    assert!(
        harness.state().clicks > 0,
        "the action button should report a click"
    );
}

#[test]
fn every_status_badge_renders_its_label() {
    let mut harness = Harness::new_ui(|ui| {
        for badge in [
            StatusBadge::NeverFetched,
            StatusBadge::Healthy,
            StatusBadge::Degraded,
            StatusBadge::Broken,
            StatusBadge::NotStarted,
            StatusBadge::Running,
            StatusBadge::Stopped,
        ] {
            badge.show(ui);
        }
    });

    harness.run();

    for label in [
        "Never fetched",
        "Healthy",
        "Degraded",
        "Broken",
        "Not started",
        "Running",
        "Stopped",
    ] {
        let _ = harness.get_by_label(label);
    }
}
