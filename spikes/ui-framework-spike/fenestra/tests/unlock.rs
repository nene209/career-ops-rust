//! fenestra unlock screen test.
//!
//! Drives the app headlessly through semantic queries. The harness is
//! deterministic by construction — scale 1.0, reduced motion, embedded
//! fonts, and an explicit clock — so animations only advance on `pump()`
//! and a render is byte-stable.
//!
//! ## Query strategy
//!
//! Follows fenestra's documented Testing Library priority:
//!
//! 1. `by::role(Semantics::Button).name("...")` for buttons. A button
//!    node carries the same name as its text child, so a bare
//!    `by::label` matches two nodes; the role filter excludes the child.
//! 2. `by::id("passphrase")` for the text input. `hint_text` is visual
//!    only and is not exposed as an accessible name, so there is nothing
//!    to query by role or label.
//! 3. `by::label("Incorrect passphrase")` for the error text. It is a
//!    bare text node with no wrapper, so the label query is unambiguous.
//!
//! `by::id` did not surface on buttons in this version — the input's id
//! appears in the tree as `#passphrase`, the buttons' ids do not appear
//! at all — which is why the buttons are queried by role.

use fenestra::prelude::*;
use fenestra::shell::Harness;
use fenestra_unlock_spike::UnlockApp;

fn harness() -> Harness<UnlockApp> {
    Harness::new(UnlockApp::new(), Theme::light(), (480, 320))
}

#[test]
fn unlock_screen_wrong_passphrase_sets_error() {
    let mut h = harness();

    h.click(&by::id("passphrase"));
    h.type_text("wrong");
    h.click(&by::role(Semantics::Button).name("Unlock"));

    assert_eq!(
        h.app().error.as_deref(),
        Some("Incorrect passphrase"),
        "wrong passphrase should set the error on the app"
    );
    assert!(
        h.query(&by::label("Incorrect passphrase")).is_some(),
        "error text should be present in the a11y tree"
    );
}

#[test]
fn unlock_screen_correct_passphrase_clears_error() {
    let mut h = harness();

    h.click(&by::id("passphrase"));
    h.type_text("wrong");
    h.click(&by::role(Semantics::Button).name("Unlock"));
    assert_eq!(h.app().error.as_deref(), Some("Incorrect passphrase"));

    h.click(&by::id("passphrase"));
    for _ in 0..5 {
        h.key(KeyInput::plain(Key::Backspace));
    }
    h.type_text("correct");
    h.click(&by::role(Semantics::Button).name("Unlock"));

    assert!(
        h.app().error.is_none(),
        "correct passphrase should clear the error"
    );
    assert!(
        h.query(&by::label("Incorrect passphrase")).is_none(),
        "error text should no longer be rendered"
    );
}

#[test]
fn both_buttons_are_reachable_by_role_and_name() {
    let h = harness();

    assert!(h
        .query(&by::role(Semantics::Button).name("Unlock"))
        .is_some());
    assert!(h
        .query(&by::role(Semantics::Button).name("Switch to Recovery"))
        .is_some());
}

#[test]
fn renders_to_pixels_without_a_display_server() {
    let mut h = harness();

    let png = h.render();
    assert!(!png.is_empty(), "headless render produced no bytes");
}
