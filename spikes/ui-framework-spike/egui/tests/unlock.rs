//! egui unlock screen test.
//!
//! The state (`UnlockApp`) is handed to the harness via
//! `build_ui_state`, so the test can read it back with
//! `harness.state()` without a borrow conflict.
//!
//! ## Query strategy
//!
//! - Text input: by role (`Role::TextInput`). egui's `hint_text` is
//!   visual only and is not exposed as an accessible label, so there
//!   is no label to query.
//! - Buttons: by label (`get_by_label`). Each button's label is its
//!   visible text, and no other widget shares that text.
//!
//! ## Import note
//!
//! `Role` is an AccessKit type, re-exported through `egui::accesskit`.
//! `egui_kittest` only re-exports the query traits (`Queryable`,
//! `NodeT`), not the AccessKit enums.

use egui::accesskit::Role;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;
use egui_unlock_spike::UnlockApp;

fn harness_with(app: UnlockApp) -> Harness<'static, UnlockApp> {
    Harness::builder()
        .with_size(egui::Vec2::new(480.0, 320.0))
        .build_ui_state(|ui, app: &mut UnlockApp| app.ui(ui), app)
}

#[test]
fn unlock_screen_wrong_passphrase_sets_error() {
    let mut harness = harness_with(UnlockApp::new());

    harness.get_by_role(Role::TextInput).focus();
    harness.run();

    harness.get_by_role(Role::TextInput).type_text("wrong");
    harness.run();

    harness.get_by_label("Unlock").click();
    harness.run();

    assert_eq!(
        harness.state().error.as_deref(),
        Some("Incorrect passphrase"),
        "wrong passphrase should set the error"
    );

    // The error text is rendered as a visible label.
    harness.get_by_label("Incorrect passphrase");
}

#[test]
fn unlock_screen_correct_passphrase_has_no_error() {
    let mut harness = harness_with(UnlockApp::new());

    harness.get_by_role(Role::TextInput).focus();
    harness.run();

    harness.get_by_role(Role::TextInput).type_text("correct");
    harness.run();

    harness.get_by_label("Unlock").click();
    harness.run();

    assert!(
        harness.state().error.is_none(),
        "correct passphrase should not set an error"
    );
}

#[test]
fn both_buttons_are_reachable_by_label() {
    let harness = harness_with(UnlockApp::new());

    harness.get_by_label("Unlock");
    harness.get_by_label("Switch to Recovery");
}