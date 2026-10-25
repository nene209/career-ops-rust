//! The post-unlock shell: sidebar plus content panel.
//!
//! The sidebar is the only navigation surface once `AppState` reaches
//! `Screen::Ready`. Each `MainScreen` renders through `screen_body`; Workstreams
//! 3 and 4 replace the placeholders with the Portals and Pipeline screens
//! without changing this frame.

use egui::Ui;

use crate::navigation::{MainScreen, NavigationState};

use super::components::EmptyState;

pub fn show(ui: &mut Ui, nav: &mut NavigationState) {
    // egui 0.37 renamed `SidePanel`/`TopBottomPanel` to `Panel` with
    // `Panel::left` and friends. See `docs/patterns.md`.
    egui::Panel::left("career_ops_sidebar")
        .resizable(false)
        .exact_size(190.0)
        .show(ui, |ui| sidebar(ui, nav));

    egui::CentralPanel::default().show(ui, |ui| {
        ui.heading(nav.current().label());
        ui.separator();
        screen_body(ui, nav.current());
    });
}

fn sidebar(ui: &mut Ui, nav: &mut NavigationState) {
    ui.add_space(8.0);
    ui.heading("Career-Ops");
    ui.add_space(4.0);
    ui.separator();
    ui.add_space(4.0);

    for &screen in MainScreen::all() {
        let selected = nav.current() == screen;
        if ui.selectable_label(selected, screen.label()).clicked() {
            nav.navigate_to(screen);
        }
    }
}

fn screen_body(ui: &mut Ui, screen: MainScreen) {
    match screen {
        MainScreen::Pipeline => {
            EmptyState::new(
                "|||",
                "No jobs yet",
                "Jobs you fetch from a portal, or add manually, will appear here.",
            )
            .show(ui);
        }
        MainScreen::Portals => {
            EmptyState::new(
                "(o)",
                "No portals configured yet",
                "Add a company job board to start fetching listings.",
            )
            .show(ui);
        }
        MainScreen::CvProfile => {
            EmptyState::new(
                "[CV]",
                "CV and profile",
                "Your CV entries and profile will be edited here.",
            )
            .show(ui);
        }
        MainScreen::Prompts => {
            EmptyState::new(
                "[>]",
                "Prompts",
                "System prompts and their versions will be managed here.",
            )
            .show(ui);
        }
        MainScreen::Settings => {
            EmptyState::new(
                "[*]",
                "Settings",
                "Application settings will be configured here.",
            )
            .show(ui);
        }
    }
}
