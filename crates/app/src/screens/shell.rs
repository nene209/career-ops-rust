//! The post-unlock shell: sidebar plus content panel.
//!
//! The sidebar is the only navigation surface once `AppState` reaches
//! `Screen::Ready`. `show_with` lets `CareerOpsApp` render the Portals screen
//! in the central panel; `show` keeps the placeholder path for tests and for
//! screens that are still placeholders.

use egui::Ui;

use crate::navigation::{MainScreen, NavigationState};

use super::components::EmptyState;

pub fn show(ui: &mut Ui, nav: &mut NavigationState) {
    // The placeholder path has no drawer to toggle.
    let mut unused = false;
    show_with(ui, nav, &mut unused, placeholder);
}

/// Render the sidebar and let the caller provide the central content.
///
/// Used by `CareerOpsApp` so the Portals screen can render against app state
/// without the shell knowing about it. The return value is whatever the
/// content closure returns.
///
/// `drawer_open` is the **Logs drawer** toggle in the sidebar footer. It lives
/// here rather than in the Logs screen because the drawer has to be reachable
/// from every screen — that is the point of a drawer.
pub fn show_with<R>(
    ui: &mut Ui,
    nav: &mut NavigationState,
    drawer_open: &mut bool,
    content: impl FnOnce(&mut Ui, MainScreen) -> R,
) -> R {
    // egui 0.37 renamed `SidePanel`/`TopBottomPanel` to `Panel` with
    // `Panel::left` and friends. See `docs/patterns.md`.
    egui::Panel::left("career_ops_sidebar")
        .resizable(false)
        .exact_size(190.0)
        .show(ui, |ui| sidebar(ui, nav, drawer_open));

    egui::CentralPanel::default()
        .show(ui, |ui| {
            ui.heading(nav.current().label());
            ui.separator();
            content(ui, nav.current())
        })
        .inner
}

fn sidebar(ui: &mut Ui, nav: &mut NavigationState, drawer_open: &mut bool) {
    ui.add_space(8.0);
    ui.heading("Career-Ops");
    ui.add_space(4.0);
    ui.separator();
    ui.add_space(4.0);

    for &screen in MainScreen::all() {
        if screen == MainScreen::Logs {
            ui.add_space(4.0);
            ui.separator();
            ui.add_space(4.0);
        }
        let selected = nav.current() == screen;
        if ui.selectable_label(selected, screen.label()).clicked() {
            nav.navigate_to(screen);
        }
    }

    // Footer: the drawer is a viewing mode, not a destination, so it belongs
    // next to the navigation rather than inside a screen.
    ui.with_layout(egui::Layout::bottom_up(egui::Align::LEFT), |ui| {
        ui.add_space(8.0);
        ui.checkbox(drawer_open, "Logs drawer")
            .on_hover_text("Show captured log lines below the screen (Cmd/Ctrl+L)");
        ui.separator();
    });
}

pub fn placeholder(ui: &mut Ui, screen: MainScreen) {
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
        MainScreen::Logs => {
            EmptyState::new(
                ">_",
                "Logs",
                "Captured log lines appear here once the app is running.",
            )
            .show(ui);
        }
    }
}
