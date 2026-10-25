//! Workstream 2 lock-screen tests.
//!
//! These render the views directly with `egui_kittest`. The state machine and
//! the async orchestration are tested separately, so these tests can focus on
//! what the user can see and click.

use career_ops_app::screens::lock::{self, FirstLaunchStep, LockView, ACK_PHRASE, RESET_PHRASE};
use career_ops_app::state::Screen;
use egui::accesskit::Role;
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;

#[test]
fn unlock_screen_exposes_the_passphrase_and_both_actions() {
    let dir = tempfile::tempdir().expect("tempdir");
    let screen = Screen::Unlocking;

    let mut harness = Harness::new_ui_state(
        |ui, view: &mut LockView| {
            lock::show(ui, &screen, view, dir.path());
        },
        LockView::default(),
    );

    harness.run();

    let _passphrase = harness.get_by_label("Passphrase");
    let _unlock = harness.get_by_role_and_label(Role::Button, "Unlock");
    let _keychain = harness.get_by_role_and_label(Role::Button, "Try keychain");
}

#[test]
fn first_launch_acknowledgement_gates_continue() {
    let dir = tempfile::tempdir().expect("tempdir");
    let screen = Screen::FirstLaunch;

    let mut view = LockView::default();
    view.first_launch.step = FirstLaunchStep::Passphrase;
    view.first_launch.passphrase = "correct horse battery staple".into();
    view.first_launch.confirmation = "correct horse battery staple".into();

    let mut harness = Harness::new_ui_state(
        |ui, view: &mut LockView| {
            lock::show(ui, &screen, view, dir.path());
        },
        view,
    );

    harness.run();
    let button = harness.get_by_role_and_label(Role::Button, "Continue");
    assert!(
        button.accesskit_node().is_disabled(),
        "Continue must be disabled until the acknowledgement phrase matches"
    );

    harness.state_mut().first_launch.acknowledgement = ACK_PHRASE.into();
    harness.run();

    let button = harness.get_by_role_and_label(Role::Button, "Continue");
    assert!(
        !button.accesskit_node().is_disabled(),
        "Continue must enable once the passphrase matches and the phrase is typed"
    );
}

#[test]
fn first_launch_wizard_advances_from_welcome_to_passphrase() {
    let dir = tempfile::tempdir().expect("tempdir");
    let screen = Screen::FirstLaunch;

    let mut harness = Harness::new_ui_state(
        |ui, view: &mut LockView| {
            lock::show(ui, &screen, view, dir.path());
        },
        LockView::default(),
    );

    harness.run();
    let _welcome = harness.get_by_label("Step 1 of 3 — Welcome");

    harness
        .get_by_role_and_label(Role::Button, "Continue")
        .click();
    harness.run();

    assert_eq!(
        harness.state().first_launch.step,
        FirstLaunchStep::Passphrase
    );
    let _step_two = harness.get_by_label("Step 2 of 3 — Create a passphrase");
}

#[test]
fn backup_step_requires_a_passphrase_to_create_the_vault() {
    let dir = tempfile::tempdir().expect("tempdir");
    let screen = Screen::FirstLaunch;

    let mut view = LockView::default();
    view.first_launch.step = FirstLaunchStep::Backup;

    let mut harness = Harness::new_ui_state(
        |ui, view: &mut LockView| {
            lock::show(ui, &screen, view, dir.path());
        },
        view,
    );

    harness.run();
    let create = harness.get_by_role_and_label(Role::Button, "Create vault");
    assert!(
        create.accesskit_node().is_disabled(),
        "a retry must not be able to submit an empty passphrase"
    );

    harness.state_mut().first_launch.passphrase = "correct horse battery staple".into();
    harness.run();
    let create = harness.get_by_role_and_label(Role::Button, "Create vault");
    assert!(!create.accesskit_node().is_disabled());
}

#[test]
fn recovery_screen_offers_all_five_options_and_gates_start_fresh() {
    let dir = tempfile::tempdir().expect("tempdir");
    let screen = Screen::Recovery {
        reason: "passphrase is incorrect".into(),
    };

    let mut harness = Harness::new_ui_state(
        |ui, view: &mut LockView| {
            lock::show(ui, &screen, view, dir.path());
        },
        LockView::default(),
    );

    harness.run();

    for heading in [
        "1. Unlock with your passphrase",
        "2. Retry the keychain",
        "3. Locate your key file",
        "4. Export the encrypted database",
        "5. Start fresh (last resort)",
    ] {
        let _ = harness.get_by_label(heading);
    }

    let start_fresh = harness.get_by_role_and_label(Role::Button, "Delete my data and start fresh");
    assert!(
        start_fresh.accesskit_node().is_disabled(),
        "the destructive action must be disabled until the phrase is typed"
    );

    harness.state_mut().recovery.confirmation = RESET_PHRASE.into();
    harness.run();

    let start_fresh = harness.get_by_role_and_label(Role::Button, "Delete my data and start fresh");
    assert!(
        !start_fresh.accesskit_node().is_disabled(),
        "the destructive action must enable once the phrase matches"
    );
}
