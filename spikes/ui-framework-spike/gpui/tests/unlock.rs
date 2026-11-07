//! GPUI Kit unlock screen test.
//!
//! Uses `#[gpui_kit::test]` with `TestAppContext`. The test:
//!
//! 1. Opens a headless window with `Root` wrapping `UnlockApp`.
//! 2. Finds elements by `ElementId` and dispatches real GPUI events.
//! 3. Asserts on the rendered accessibility tree — the error-status
//!    node's label — rather than on private application fields.
//!
//! ## Why no `common::open_window` helper
//!
//! The gpui-kit repo has a `common::open_window` helper in its own test
//! suite, but it depends on `TestAppContext::update`, which is
//! `pub(crate)` in the gpui-pre version this spike pins. `cx.add_window`
//! is public in that version and does the same job for a single-window
//! test, so the helper is inlined per test.

use gpui_kit::component::Root;
use gpui_kit::test::{TestQueryExt, TestWindowExt};
use gpui_kit::{px, size, AppContext, Role, TestAppContext};
use gpui_kit_unlock_spike::UnlockApp;

#[gpui_kit::test]
fn unlock_screen_wrong_passphrase_sets_error(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);

    let handle = cx.open_window(size(px(480.), px(320.)), |window, cx| {
        let app = cx.new(|cx| UnlockApp::new(window, cx));
        Root::new(app, window, cx)
    });

    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);

        window.click("passphrase", cx);
        window.input("wrong", cx);
        window.click("unlock", cx);

        let status = window.find("error-status");
        assert_eq!(status.role(), Some(Role::Status));
        assert_eq!(status.label(), Some("Incorrect passphrase"));
    })
    .unwrap();
}

#[gpui_kit::test]
fn unlock_screen_correct_passphrase_clears_error(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);

    let handle = cx.open_window(size(px(480.), px(320.)), |window, cx| {
        let app = cx.new(|cx| UnlockApp::new(window, cx));
        Root::new(app, window, cx)
    });

    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);

        window.click("passphrase", cx);
        window.input("wrong", cx);
        window.click("unlock", cx);
        assert_eq!(
            window.find("error-status").label(),
            Some("Incorrect passphrase")
        );

        window.click("passphrase", cx);
        for _ in 0..5 {
            window.press("backspace", cx);
        }
        window.input("correct", cx);
        window.click("unlock", cx);

        assert_eq!(window.find("error-status").label(), Some(""));
    })
    .unwrap();
}

#[gpui_kit::test]
fn recovery_button_is_present_and_clickable(cx: &mut TestAppContext) {
    cx.update(gpui_kit::init);

    let handle = cx.open_window(size(px(480.), px(320.)), |window, cx| {
        let app = cx.new(|cx| UnlockApp::new(window, cx));
        Root::new(app, window, cx)
    });

    cx.update_window(handle.into(), |_, window, cx| {
        window.render_frame(cx);

        let buttons = window.find_all_by_role(Role::Button);
        assert_eq!(buttons.len(), 2, "expected Unlock and Recovery buttons");

        window.click("recovery", cx);
    })
    .unwrap();
}
