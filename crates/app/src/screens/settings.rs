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

use egui::{Align2, Button, Color32, RichText, Ui};

use career_ops_core::llm::{LlmConfig, LlmProvider};
use career_ops_core::preferences::Preferences;
use career_ops_core::privacy::{PrivacyConfig, RedactionReport};

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
    /// The redaction toggle. Its own save, like the AI section: a failure has to
    /// name what failed.
    SavePrivacy(PrivacyConfig),
    /// Ask the app for the CV summary as the model would see it, under the
    /// toggle's **current** (possibly unsaved) value, so the preview can be
    /// compared before committing.
    PreviewPrivacy {
        redact: bool,
    },
}

/// The summary the preview modal renders.
#[derive(Debug, Clone, Copy)]
pub struct PrivacyPreview<'a> {
    pub text: &'a str,
    /// Whether `text` was redacted. When false the modal says so.
    pub redacted: bool,
    /// Counts only, for the line above the text.
    pub report: RedactionReport,
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
    /// What the modal shows once the app has built it.
    pub privacy_preview: Option<PrivacyPreview<'a>>,
    pub privacy_preview_loading: bool,
    pub privacy_preview_error: Option<&'a str>,
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

        ui.add_space(8.0);
        ui.separator();
        ui.add_space(8.0);

        privacy_section(ui, view_model, state, &mut action);
    });

    // Outside the scroll area: a window clipped by it would be unusable.
    privacy_preview_modal(ui, view_model, state);

    action
}

/// The provider, the endpoint, the model, and the API key.
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
    let custom = view_model.llm.provider == LlmProvider::Custom;
    let config = view_model.llm_config();

    ui.label(RichText::new("AI evaluation").strong());
    if custom {
        // The data-sharing note names the endpoint, because "the provider" is
        // whatever the user typed: it may be this machine or a host on their
        // network, and the difference matters.
        let destination = config.base_url.trim();
        let destination = if destination.is_empty() {
            "the endpoint below"
        } else {
            destination
        };
        ui.label(
            RichText::new(format!(
                "Evaluations send your CV summary and the selected job's description to \
                 {destination}. Nothing leaves this machine until you run one. An API key is \
                 optional and stored in the OS keychain, never the database."
            ))
            .weak(),
        );
    } else {
        ui.label(
            RichText::new(
                "Evaluations send your CV summary and the selected job's description to the \
                 provider below. Nothing leaves this machine until you run one. DeepSeek is \
                 hosted in the PRC; the API key is stored in the OS keychain, never the \
                 database.",
            )
            .weak(),
        );
    }
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

    if custom {
        labelled_input(ui, "Base URL", &mut view_model.llm.base_url, false);
        ui.label(
            RichText::new(
                "For example http://localhost:11434/v1 (Ollama) or \
                 http://localhost:8080/v1 (llama-server). Include the /v1 suffix.",
            )
            .weak()
            .small(),
        );
        if let Some(warning) = config.custom_warning() {
            ui.label(RichText::new(warning).color(ui.visuals().warn_fg_color));
        }
        ui.add_space(6.0);
    }

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

    ui.add_space(6.0);
    if custom {
        ui.checkbox(&mut view_model.llm.json_mode, "Use JSON mode");
        ui.label(
            RichText::new(
                "Some local models reject `response_format`. Turning this off asks for JSON in \
                 the prompt instead; the reply is still validated here.",
            )
            .weak()
            .small(),
        );
        ui.add_space(6.0);

        labelled_input(
            ui,
            "Context window (tokens)",
            &mut view_model.llm.context_window,
            false,
        );
        ui.label(
            RichText::new(
                "Optional. Leave it blank if you do not know it: the pre-flight token check is \
                 then skipped and the server's own limit still applies.",
            )
            .weak()
            .small(),
        );
        // Only once something has been typed: a freshly switched provider is
        // an empty form, not a mistake to shout about.
        let touched =
            !view_model.llm.base_url.trim().is_empty() || !view_model.llm.model.trim().is_empty();
        if let (true, Err(problem)) = (touched, config.validate_custom()) {
            ui.label(RichText::new(problem.to_string()).color(ui.visuals().error_fg_color));
        }
        ui.add_space(6.0);
    }

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

/// The redaction toggle, its own save, and the preview.
///
/// The copy does two jobs: it says exactly what is removed, and it says plainly
/// that the matching is best-effort. An overstated promise would be worse than
/// no promise, because the user would stop thinking about it.
fn privacy_section(
    ui: &mut Ui,
    view_model: &mut SettingsViewModel,
    state: SettingsRenderState<'_>,
    action: &mut Option<SettingsAction>,
) {
    ui.label(RichText::new("Privacy").strong());
    ui.label(
        RichText::new(
            "Before an evaluation or a cover letter is sent, your name, email, phone, and a \
             street-address location are replaced with placeholders. The reply is put back \
             together here, and the log records counts only.",
        )
        .weak(),
    );
    ui.add_space(6.0);

    ui.checkbox(
        &mut view_model.privacy.redact_cv,
        "Redact personal details before sending to the model",
    );
    ui.label(
        RichText::new("Removes: name, email, phone, street-address location.")
            .weak()
            .small(),
    );
    ui.label(
        RichText::new(
            "Best-effort: a nickname or an unusual format can slip through. Local / custom \
             keeps everything on this machine.",
        )
        .weak()
        .small(),
    );

    ui.add_space(6.0);
    ui.horizontal(|ui| {
        if ui
            .add_enabled(
                view_model.is_privacy_dirty() && !state.saving,
                Button::new("Save privacy settings"),
            )
            .clicked()
        {
            *action = Some(SettingsAction::SavePrivacy(view_model.privacy_config()));
        }
        if ui
            .add_enabled(
                !state.privacy_preview_loading,
                Button::new("Preview what the model sees"),
            )
            .on_hover_text("The CV summary, with or without the placeholders")
            .clicked()
        {
            view_model.privacy_preview_open = true;
            *action = Some(SettingsAction::PreviewPrivacy {
                redact: view_model.privacy.redact_cv,
            });
        }
        if state.privacy_preview_loading {
            LoadingSpinner::show(ui);
        }
        if view_model.is_privacy_dirty() && !state.saving {
            ui.label(RichText::new("· unsaved changes").weak());
        }
    });
}

/// The preview window.
///
/// It renders from the app's prepared text, so the screen never touches the
/// database. A toggle the user has changed but not saved is what the preview
/// reflects: the point is comparing before committing.
fn privacy_preview_modal(
    ui: &mut Ui,
    view_model: &mut SettingsViewModel,
    state: SettingsRenderState<'_>,
) {
    if !view_model.privacy_preview_open {
        return;
    }

    let mut close = false;
    let mut window_open = true;

    egui::Window::new("What the model sees")
        .collapsible(false)
        .resizable(true)
        .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
        .default_width(680.0)
        .open(&mut window_open)
        .show(ui.ctx(), |ui| {
            if state.privacy_preview_loading {
                ui.horizontal(|ui| {
                    LoadingSpinner::show(ui);
                    ui.label("Preparing the preview...");
                });
            } else if let Some(error) = state.privacy_preview_error {
                ui.colored_label(Color32::from_rgb(0xc0, 0x39, 0x2b), error);
            } else if let Some(preview) = state.privacy_preview {
                if !preview.redacted {
                    ui.colored_label(
                        ui.visuals().warn_fg_color,
                        "Redaction is off: this is the summary exactly as it will be sent.",
                    );
                } else if preview.report.is_empty() {
                    ui.label(
                        "Nothing to redact: no name, email, phone, or street-address location \
                         was found in the summary.",
                    );
                } else {
                    let report = preview.report;
                    ui.label(format!(
                        "{} replacement{}: {} name{}, {} email{}, {} phone{}, {} location{}.",
                        report.total(),
                        plural(report.total()),
                        report.names,
                        plural(report.names),
                        report.emails,
                        plural(report.emails),
                        report.phones,
                        plural(report.phones),
                        report.locations,
                        plural(report.locations),
                    ));
                }
                ui.add_space(4.0);
                egui::ScrollArea::vertical()
                    .max_height(420.0)
                    .show(ui, |ui| {
                        ui.add(
                            egui::Label::new(RichText::new(preview.text).monospace())
                                .selectable(true)
                                .wrap(),
                        );
                    });
            }

            ui.add_space(8.0);
            if ui.button("Close").clicked() {
                close = true;
            }
        });

    if close || !window_open {
        view_model.privacy_preview_open = false;
    }
}

fn plural(count: usize) -> &'static str {
    if count == 1 {
        ""
    } else {
        "s"
    }
}

fn field(ui: &mut Ui, label: &str, value: &mut String, help: &str) {
    labelled_input(ui, label, value, false);
    ui.label(RichText::new(help).weak().small());
    ui.add_space(6.0);
}
