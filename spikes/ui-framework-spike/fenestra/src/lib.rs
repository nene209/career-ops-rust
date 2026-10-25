//! Unlock screen prototype built with fenestra's `App` / `update` /
//! `view` model.
//!
//! The text input is wrapped in a `field("Passphrase")` so the label is
//! visible, and carries `.id("passphrase")` so the headless harness can
//! click it by key — `by::id` is documented as the test-id escape hatch,
//! used here because `type_text` targets the focused element.

use fenestra::kit;
use fenestra::prelude::*;

#[derive(Clone)]
pub enum Msg {
    PassphraseChanged(String),
    UnlockClicked,
    SwitchToRecovery,
}

pub struct UnlockApp {
    pub passphrase: String,
    pub error: Option<String>,
    pub recovery_requested: bool,
}

impl UnlockApp {
    pub fn new() -> Self {
        Self {
            passphrase: String::new(),
            error: None,
            recovery_requested: false,
        }
    }
}

impl App for UnlockApp {
    type Msg = Msg;

    fn update(&mut self, msg: Msg) {
        match msg {
            Msg::PassphraseChanged(p) => self.passphrase = p,
            Msg::UnlockClicked => {
                if self.passphrase == "correct" {
                    self.error = None;
                } else {
                    self.error = Some("Incorrect passphrase".into());
                }
            }
            Msg::SwitchToRecovery => self.recovery_requested = true,
        }
    }

    fn view(&self) -> Element<Msg> {
        col()
            .p(SP6)
            .gap(SP4)
            // Named "Career-Ops", not "Unlock", so it cannot collide with
            // the button's accessible label.
            .child(
                text("Career-Ops")
                    .size(TextSize::Xl2)
                    .weight(Weight::Semibold),
            )
            .child(
                kit::field("Passphrase").child(
                    kit::text_input(self.passphrase.as_str())
                        .id("passphrase")
                        .on_input(Msg::PassphraseChanged),
                ),
            )
            .child(button("Unlock").on_click(Msg::UnlockClicked))
            .child(button("Switch to Recovery").on_click(Msg::SwitchToRecovery))
            .child(text(self.error.clone().unwrap_or_default()))
    }
}
