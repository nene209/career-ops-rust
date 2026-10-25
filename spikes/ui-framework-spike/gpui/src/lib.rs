//! Unlock screen prototype built with gpui-kit components.
//!
//! The view holds an `InputState` for the passphrase field and a
//! `submitted` flag for the error message. The unlock action sets the
//! error when the passphrase does not match, which the test asserts on
//! via both the rendered accessibility tree and the app state.

use gpui_kit::TestSupportExt;

use gpui_kit::{
    component::{
        button::Button,
        input::{Input, InputState},
    },
    div,
    prelude::*,
    px, AppContext, Context, Entity, Render, SharedString, Window,
};

pub struct UnlockApp {
    pub passphrase: Entity<InputState>,
    pub error: Option<SharedString>,
    pub recovery_requested: bool,
}

impl UnlockApp {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            passphrase: cx.new(|cx| InputState::new(window, cx)),
            error: None,
            recovery_requested: false,
        }
    }

    fn unlock(&mut self, cx: &mut Context<Self>) {
        let value = self.passphrase.read(cx).value();
        if value == "correct" {
            self.error = None;
        } else {
            self.error = Some("Incorrect passphrase".into());
        }
        cx.notify();
    }

    fn request_recovery(&mut self, cx: &mut Context<Self>) {
        self.recovery_requested = true;
        cx.notify();
    }
}

impl Render for UnlockApp {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .flex_col()
            .p_6()
            .gap_4()
            .child("Unlock")
            .child(
                Input::new(&self.passphrase)
                    .id("passphrase")
                    .aria_label("Passphrase")
                    .w(px(240.)),
            )
            .child(
                Button::new("unlock")
                    .label("Unlock")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.unlock(cx);
                    })),
            )
            .child(
                Button::new("recovery")
                    .label("Switch to Recovery")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.request_recovery(cx);
                    })),
            )
            .child(
                div()
                    .id("error-status")
                    .role(gpui_kit::Role::Status)
                    .test_support()
                    .aria_label(self.error.clone().unwrap_or_else(|| SharedString::from("")))
                    .child(self.error.clone().unwrap_or_default()),
            )
    }
}
