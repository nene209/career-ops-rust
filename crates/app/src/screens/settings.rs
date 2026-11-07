//! Settings screen.
//!
//! Phase 2.9 adds the acquisition preferences section; the rest of Settings
//! (log level, backends, key rotation) arrives in Phase 5. The screen renders
//! the form and returns a `SettingsAction`; the app owns the load and the save,
//! the same split every other screen uses.
//!
//! The five fields are comma-separated lists rather than add/remove rows. This
//! screen is visited rarely, so typing is cheaper than a widget per entry, and
//! the parsing (`view_models::settings`) is forgiving about spacing, blanks, and
//! duplicates.

use egui::{Button, Color32, RichText, Ui};

use career_ops_core::preferences::Preferences;

use crate::view_models::settings::SettingsViewModel;

use super::components::{labelled_input, LoadingSpinner};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SettingsAction {
    Reload,
    Save(Preferences),
}

#[derive(Default, Clone, Copy)]
pub struct SettingsRenderState<'a> {
    pub loading: bool,
    pub saving: bool,
    pub error: Option<&'a str>,
    pub message: Option<&'a str>,
}

pub fn show(
    ui: &mut Ui,
    view_model: &mut SettingsViewModel,
    state: SettingsRenderState<'_>,
) -> Option<SettingsAction> {
    let mut action = None;

    ui.horizontal(|ui| {
        if ui
            .add_enabled(
                view_model.is_dirty() && !state.saving,
                Button::new("Save preferences"),
            )
            .on_hover_text("Store these preferences; the Pipeline applies them on its next reload")
            .clicked()
        {
            action = Some(SettingsAction::Save(view_model.preferences()));
        }
        if ui
            .add_enabled(!state.saving, Button::new("Reload"))
            .on_hover_text("Discard unsaved edits and load the stored preferences again")
            .clicked()
        {
            action = Some(SettingsAction::Reload);
        }
        if state.saving {
            LoadingSpinner::show(ui);
        }

        ui.separator();
        let summary = if view_model.is_configured() {
            "Preferences saved"
        } else {
            "No preferences yet"
        };
        ui.label(RichText::new(summary).weak());
        if view_model.is_dirty() && !state.saving {
            ui.label(RichText::new("· unsaved changes").weak());
        }
    });

    if let Some(error) = state.error {
        ui.horizontal(|ui| {
            ui.colored_label(Color32::from_rgb(0xc0, 0x39, 0x2b), error);
            if ui
                .add_enabled(!state.saving, Button::new("Retry"))
                .clicked()
            {
                action = Some(SettingsAction::Reload);
            }
        });
    }
    if let Some(message) = state.message {
        ui.label(RichText::new(message).weak());
    }

    ui.separator();

    if state.loading {
        ui.horizontal(|ui| {
            LoadingSpinner::show(ui);
            ui.label("Loading settings...");
        });
        return action;
    }

    egui::ScrollArea::vertical().show(ui, |ui| {
        ui.label(RichText::new("Acquisition preferences").strong());
        ui.label(
            RichText::new(
                "Comma-separated, case does not matter. These badge and order the Pipeline; \
                 only an exclusion hides anything.",
            )
            .weak(),
        );
        ui.add_space(8.0);

        field(
            ui,
            "Preferred companies",
            &mut view_model.form.companies,
            "Rows from these companies get a Preferred badge.",
        );
        field(
            ui,
            "Preferred locations",
            &mut view_model.form.locations,
            "Matched against the posting's location, so \"Remote\" or \"Berlin\" works.",
        );
        field(
            ui,
            "Keywords",
            &mut view_model.form.keywords,
            "Matched against the job title and the description.",
        );
        field(
            ui,
            "Excluded companies",
            &mut view_model.form.excluded_companies,
            "Hidden from the Pipeline's All view. The Excluded filter still shows them.",
        );
        field(
            ui,
            "Seniority",
            &mut view_model.form.seniority,
            "A hint for Phase 3.2's triage. No filter uses it yet.",
        );
    });

    action
}

fn field(ui: &mut Ui, label: &str, value: &mut String, help: &str) {
    labelled_input(ui, label, value, false);
    ui.label(RichText::new(help).weak().small());
    ui.add_space(6.0);
}
