//! Pre-`Ready` screens.
//!
//! Workstream 1 only needs a calm full-screen placeholder so the shell can
//! boot on every path. Workstream 2 replaces each branch with the real unlock,
//! first-launch, keychain-failure, and recovery views. The state machine
//! (`crate::state::Screen`) decides which branch is shown; this module never
//! stores navigation state of its own.

use egui::{RichText, Ui};

use crate::state::Screen;

pub fn show(ui: &mut Ui, screen: &Screen) {
    egui::CentralPanel::default().show(ui, |ui| {
        ui.vertical_centered(|ui| {
            ui.add_space(64.0);

            let (title, detail) = describe(screen);
            ui.heading(title);
            ui.add_space(8.0);
            ui.label(RichText::new(detail).weak());
            ui.add_space(24.0);
            ui.label(
                RichText::new("This screen is implemented in Phase 2.1 Workstream 2.")
                    .small()
                    .weak(),
            );
        });
    });
}

fn describe(screen: &Screen) -> (&'static str, String) {
    match screen {
        Screen::Booting => ("Starting up", "Loading application state.".into()),
        Screen::FirstLaunch => (
            "Welcome",
            "No existing data was found. The first-launch wizard arrives next.".into(),
        ),
        Screen::Unlocking => (
            "Unlock",
            "Existing data was found. The unlock screen arrives next.".into(),
        ),
        Screen::KeychainUnavailable { reason } => ("Keychain unavailable", reason.clone()),
        Screen::Recovery { reason } => ("Recovery required", reason.clone()),
        // `show` is called only for non-Ready states by `CareerOpsApp::ui`.
        Screen::Ready => (
            "Ready",
            "The main shell is rendered by `screens::shell`.".into(),
        ),
    }
}
