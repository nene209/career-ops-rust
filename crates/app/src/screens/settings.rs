//! Settings screen.
//!
//! Phase 2.9 adds the acquisition preferences section and Phase 3 adds the AI
//! section beside it; the rest of Settings (log level, backends, key rotation)
//! arrives in Phase 5. The screen renders the forms and returns a
//! `SettingsAction`; the app owns the load, the save, and the keychain, the same
//! split every other screen uses.
//!
//! The five preference fields are comma-separated lists rather than add/remove
//! rows. This screen is visited rarely, so typing is cheaper than a widget per
//! entry, and the parsing (`view_models::settings`) is forgiving about spacing,
//! blanks, and duplicates.
//!
//! The AI section has its own save. Preferences live in `profile`, the AI
//! choice in `settings`, and the API key in the OS keychain; one button would
//! have to say which of the three failed.

use egui::{Button, Color32, RichText, Ui};

use career_ops_core::llm::{LlmConfig, LlmProvider};
use career_ops_core::preferences::Preferences;

use crate::view_models::settings::SettingsViewModel;

use super::components::{labelled_input, LoadingSpinner};

#[derive(Debug, Clone, PartialEq)]
pub enum SettingsAction {
    Reload,
    Save(Preferences),
    /// Provider and model. The key is saved separately.
    SaveLlm(LlmConfig),
    /// Store a key for the config's provider. The config travels with it so a
    /// provider change and its first key are one action.
    SaveApiKey {
        key: String,
        config: LlmConfig,
    },
    DeleteApiKey(LlmProvider),
    TestConnection(LlmConfig),
}

#[derive(Default, Clone, Copy)]
pub struct SettingsRenderState<'a> {
    pub loading: bool,
    pub saving: bool,
    /// A keychain write, a key delete, or a connection test is in flight.
    pub ai_busy: bool,
    /// One line from `llm.status()`: ready, or the first thing to fix.
    pub llm_status: Option<&'a str>,
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

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(8.0);

        ai_section(ui, view_model, state, &mut action);
    });

    action
}

/// The provider, the model, and the API key.
///
/// Everything here has a separate save from the preferences: the two live in
/// different places (`profile` and `settings`), and the key lives in the OS
/// keychain, so a single button could not name what failed.
fn ai_section(
    ui: &mut Ui,
    view_model: &mut SettingsViewModel,
    state: SettingsRenderState<'_>,
    action: &mut Option<SettingsAction>,
) {
    ui.label(RichText::new("AI evaluation").strong());
    ui.label(
        RichText::new(
            "Evaluations send your CV summary and the selected job's description to the \
             provider below. Nothing leaves this machine until you run one. DeepSeek is \
             hosted in the PRC; the API key is stored in the OS keychain, never the database.",
        )
        .weak(),
    );
    if let Some(status) = state.llm_status {
        ui.label(RichText::new(status).weak().small());
    }
    ui.add_space(8.0);

    ui.horizontal(|ui| {
        ui.label("Provider");
        for provider in LlmProvider::ALL {
            let selected = view_model.llm.provider == provider;
            let response = ui
                .add_enabled(
                    provider.is_supported(),
                    Button::selectable(selected, provider.label()),
                )
                .on_disabled_hover_text("Not supported by this build yet");
            if response.clicked() && !selected {
                // A model name belongs to one provider, so a switch replaces
                // it; the rest of the configuration, including a custom
                // endpoint, is kept (see `LlmConfig::switched_to`).
                let switched = view_model.llm_config().switched_to(provider);
                view_model.llm.provider = switched.provider;
                view_model.llm.model = switched.model;
            }
        }
    });

    labelled_input(ui, "Model", &mut view_model.llm.model, false);
    ui.label(
        RichText::new(match view_model.llm.provider {
            LlmProvider::Custom => {
                "The model name the endpoint serves, for example llama3.2 or qwen2.5:7b."
            }
            _ => "The model name the provider serves, for example deepseek-flash.",
        })
        .weak()
        .small(),
    );
    ui.add_space(6.0);

    let key_required = view_model.llm.provider.requires_api_key();
    labelled_input(
        ui,
        if key_required {
            "API key"
        } else {
            "API key (optional)"
        },
        &mut view_model.llm.api_key,
        true,
    );
    ui.horizontal(|ui| {
        let can_save = !view_model.llm.api_key.trim().is_empty() && !state.ai_busy;
        if ui
            .add_enabled(can_save, Button::new("Save key"))
            .on_hover_text("Store the key in the OS keychain; it is never written to the database")
            .clicked()
        {
            let key = view_model.take_api_key();
            *action = Some(SettingsAction::SaveApiKey {
                key,
                config: view_model.llm_config(),
            });
        }

        if view_model.llm.has_api_key {
            ui.label(RichText::new("A key is stored.").weak());
            if ui
                .add_enabled(!state.ai_busy, Button::new("Delete key"))
                .clicked()
            {
                *action = Some(SettingsAction::DeleteApiKey(view_model.llm.provider));
            }
        } else if key_required {
            ui.label(RichText::new("No key stored.").weak());
        } else {
            ui.label(RichText::new("No key stored (optional).").weak());
        }
    });

    ui.add_space(6.0);
    let config = view_model.llm_config();
    ui.horizontal(|ui| {
        if ui
            .add_enabled(
                view_model.is_llm_dirty() && !state.ai_busy,
                Button::new("Save AI settings"),
            )
            .clicked()
        {
            *action = Some(SettingsAction::SaveLlm(view_model.llm_config()));
        }

        // Testing spends no tokens, so a keyless local endpoint can be tried
        // before anything is saved. A custom endpoint that would fail its own
        // validation is left disabled and says why.
        let problem = config.validate_custom().err();
        let can_test =
            !state.ai_busy && problem.is_none() && (view_model.llm.has_api_key || !key_required);
        let test = ui.add_enabled(can_test, Button::new("Test connection"));
        let test = match problem {
            Some(problem) => test.on_disabled_hover_text(problem.to_string()),
            None if key_required => {
                test.on_hover_text("Ask DeepSeek to confirm the key; no tokens are spent")
            }
            None => test.on_hover_text(
                "Ask the endpoint for its model list; no tokens are spent. Some local \
                 servers do not implement it.",
            ),
        };
        if test.clicked() {
            *action = Some(SettingsAction::TestConnection(view_model.llm_config()));
        }

        if state.ai_busy {
            LoadingSpinner::show(ui);
        }
        if view_model.is_llm_dirty() && !state.ai_busy {
            ui.label(RichText::new("· unsaved changes").weak());
        }
    });
}

fn field(ui: &mut Ui, label: &str, value: &mut String, help: &str) {
    labelled_input(ui, label, value, false);
    ui.label(RichText::new(help).weak().small());
    ui.add_space(6.0);
}
