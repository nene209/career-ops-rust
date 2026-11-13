//! Prompt Manager screen.
//!
//! Two panes: the threads on the left (one row per name and owner, grouped by
//! category), the selected thread on the right. System rows are read-only; the
//! only way to change one is to duplicate it, which is what keeps a shipped
//! update from colliding with a user's edits.
//!
//! Version content is immutable. A thread has one **current** version; making an
//! older version current moves the pointer and writes no row. Saving appends a
//! new version. An unreferenced version can be deleted; a referenced one can be
//! retired, which hides it from the picker while keeping its text resolvable.
//!
//! The screen only emits actions; `CareerOpsApp` owns the reads and writes.

use egui::{Button, Color32, RichText, Ui};

use career_ops_core::prompt_render::variables_for_category;

use crate::view_models::prompts::{
    PromptDetail, PromptSelection, PromptThreadRow, PromptsView, PromptsViewModel,
};

use super::components::{labelled_multiline, EmptyState, LoadingSpinner};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PromptsAction {
    Reload,
    Select(PromptSelection),
    SaveDraft(String),
    DuplicateAsUser,
    /// Show a version; `None` returns to the current one.
    ShowVersion(Option<i64>),
    /// Make an existing version current without adding a row.
    SetCurrentVersion(i64),
    /// Delete (`retire = false`) or hide (`retire = true`) one version.
    RemoveVersion {
        version_id: i64,
        retire: bool,
    },
    RestoreVersion(i64),
    SetCompare(bool),
}

#[derive(Default, Clone, Copy)]
pub struct PromptsRenderState<'a> {
    pub loading: bool,
    pub detail_loading: bool,
    pub writing: bool,
    pub error: Option<&'a str>,
    pub message: Option<&'a str>,
}

pub fn show(
    ui: &mut Ui,
    view_model: &PromptsViewModel,
    detail: Option<&PromptDetail>,
    view: &mut PromptsView,
    state: PromptsRenderState<'_>,
) -> Option<PromptsAction> {
    let mut action = None;

    header(ui, view_model, state, &mut action);
    if let Some(error) = state.error {
        ui.horizontal(|ui| {
            ui.colored_label(Color32::from_rgb(0xc0, 0x39, 0x2b), error);
            if ui
                .add_enabled(!state.loading, Button::new("Retry"))
                .clicked()
            {
                action = Some(PromptsAction::Reload);
            }
        });
    }
    ui.separator();

    if state.loading && view_model.is_empty() {
        ui.horizontal(|ui| {
            LoadingSpinner::show(ui);
            ui.label("Loading prompts...");
        });
        return action;
    }

    if view_model.is_empty() {
        EmptyState::new(
            "[>]",
            "No prompts yet",
            "Prompts are seeded when the vault is unlocked.",
        )
        .show(ui);
        return action;
    }

    egui::Panel::left("prompts_list_panel")
        .resizable(true)
        .default_size(300.0)
        .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .id_salt("prompts_list")
                .auto_shrink([false, false])
                .show(ui, |ui| list(ui, view_model, view, &mut action));
        });

    egui::CentralPanel::default().show(ui, |ui| {
        egui::ScrollArea::vertical()
            .id_salt("prompts_detail")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                detail_ui(ui, detail, view, state, &mut action);
            });
    });

    remove_dialog(ui.ctx(), view, &mut action);

    action
}

fn header(
    ui: &mut Ui,
    view_model: &PromptsViewModel,
    state: PromptsRenderState<'_>,
    action: &mut Option<PromptsAction>,
) {
    ui.horizontal(|ui| {
        if ui
            .add_enabled(!state.loading, Button::new("Reload"))
            .on_hover_text("Read the stored prompts again")
            .clicked()
        {
            *action = Some(PromptsAction::Reload);
        }
        if state.loading || state.writing {
            LoadingSpinner::show(ui);
        }

        ui.separator();
        let count = view_model.threads.len();
        let noun = if count == 1 { "thread" } else { "threads" };
        ui.label(RichText::new(format!("{count} prompt {noun}")).weak());
        if state.writing {
            ui.label(RichText::new("Writing...").weak());
        }
    });

    if let Some(message) = state.message {
        ui.label(RichText::new(message).weak());
    }
}

fn list(
    ui: &mut Ui,
    view_model: &PromptsViewModel,
    view: &PromptsView,
    action: &mut Option<PromptsAction>,
) {
    for category in view_model.categories() {
        ui.add_space(6.0);
        ui.label(RichText::new(category).strong());
        for row in view_model.threads_in(category) {
            thread_row(ui, row, view, action);
        }
    }
}

fn thread_row(
    ui: &mut Ui,
    row: &PromptThreadRow,
    view: &PromptsView,
    action: &mut Option<PromptsAction>,
) {
    let selected = view.selected.as_ref().is_some_and(|selection| {
        selection.name == row.name && selection.is_system == row.is_system
    });

    ui.vertical(|ui| {
        let label = if row.in_use {
            format!("{}  (in use)", row.name)
        } else {
            row.name.clone()
        };
        if ui.selectable_label(selected, label).clicked() {
            *action = Some(PromptsAction::Select(PromptSelection::new(
                row.name.clone(),
                row.is_system,
            )));
        }

        let version_label = if row.version_count == 1 {
            "1 version".to_string()
        } else {
            format!("{} versions", row.version_count)
        };
        ui.label(
            RichText::new(format!(
                "{} · v{} · {version_label} · {}",
                row.owner_label(),
                row.current_version,
                row.updated_at
            ))
            .weak()
            .small(),
        );
    });
}

fn detail_ui(
    ui: &mut Ui,
    detail: Option<&PromptDetail>,
    view: &mut PromptsView,
    state: PromptsRenderState<'_>,
    action: &mut Option<PromptsAction>,
) {
    let Some(selection) = view.selected.clone() else {
        EmptyState::new(
            "[>]",
            "No prompt selected",
            "Choose a prompt on the left to read its content and versions.",
        )
        .show(ui);
        return;
    };

    let Some(detail) = detail
        .filter(|detail| detail.name == selection.name && detail.is_system == selection.is_system)
    else {
        if state.detail_loading {
            ui.horizontal(|ui| {
                LoadingSpinner::show(ui);
                ui.label("Loading versions...");
            });
        } else {
            EmptyState::new("[!]", "Could not load this prompt", "Reload to try again.").show(ui);
        }
        return;
    };

    if detail.is_empty() {
        EmptyState::new("[!]", "No versions", "This thread has no stored versions.").show(ui);
        return;
    }

    ui.horizontal(|ui| {
        ui.heading(&detail.name);
        ui.label(RichText::new(&detail.category).weak());
        ui.label(
            RichText::new(if detail.is_system {
                "System · read-only"
            } else {
                "Your copy · editable"
            })
            .weak(),
        );
    });

    if detail.is_system {
        ui.label(
            RichText::new(
                "System prompts are read-only. Duplicate one to edit your own copy; the \
                 original stays as reference and future updates do not touch your copy.",
            )
            .weak()
            .small(),
        );
        ui.add_space(4.0);
        if ui
            .add_enabled(!state.writing, Button::new("Duplicate as my copy"))
            .on_hover_text("Create an editable copy at version 1")
            .clicked()
        {
            *action = Some(PromptsAction::DuplicateAsUser);
        }
    } else {
        ui.label(
            RichText::new(
                "Saving creates a new version. Earlier versions are never changed, so a \
                 report always points at what produced it. Making an older version current \
                 moves the marker without adding a row; a used version can be retired \
                 instead of deleted so its text stays available to those runs.",
            )
            .weak()
            .small(),
        );
    }

    ui.add_space(8.0);
    content_ui(ui, detail, view, state, action);

    ui.add_space(12.0);
    versions_ui(ui, detail, view, action);

    ui.add_space(12.0);
    usage_ui(ui, detail, view);

    ui.add_space(12.0);
    variables_ui(ui, detail);
}

fn content_ui(
    ui: &mut Ui,
    detail: &PromptDetail,
    view: &mut PromptsView,
    state: PromptsRenderState<'_>,
    action: &mut Option<PromptsAction>,
) {
    let Some(version) = view.viewed_version(detail) else {
        return;
    };
    let showing_history = view.selected_version.is_some();
    let is_current = version.is_current;
    let is_retired = version.retired_at.is_some();
    let read_only = detail.is_system || showing_history || is_retired;

    ui.horizontal_wrapped(|ui| {
        let label = if is_retired {
            format!(
                "Retired v{} ({})",
                version.version,
                version.retired_at.as_deref().unwrap_or("retired")
            )
        } else if is_current {
            format!("Current version (v{})", version.version)
        } else {
            format!("Viewing v{}", version.version)
        };
        ui.label(RichText::new(label).strong());

        if showing_history && ui.button("Back to current").clicked() {
            *action = Some(PromptsAction::ShowVersion(None));
        }
        if !detail.is_system {
            if !is_current
                && !is_retired
                && ui
                    .add_enabled(!state.writing, Button::new("Make current"))
                    .on_hover_text("Use this version without creating a new one")
                    .clicked()
            {
                *action = Some(PromptsAction::SetCurrentVersion(version.id));
            }
            if is_retired
                && ui
                    .add_enabled(!state.writing, Button::new("Restore"))
                    .on_hover_text("Make this version selectable again")
                    .clicked()
            {
                *action = Some(PromptsAction::RestoreVersion(version.id));
            }

            let used = version.usage.len();
            let has_other = detail.versions.iter().any(|other| other.id != version.id);
            let active_sibling = detail
                .versions
                .iter()
                .any(|other| other.id != version.id && other.retired_at.is_none());
            // The repository refuses to leave a thread with rows but no current
            // version; disable the action instead of showing an error.
            let blocked = is_current && has_other && !active_sibling;
            let kept_for_history = is_retired && used > 0;
            let label = if used > 0 && !is_retired {
                "Retire…"
            } else {
                "Delete…"
            };
            let enabled = !blocked && !kept_for_history && !state.writing;
            let hover = if blocked {
                "Restore another version first"
            } else if kept_for_history {
                "Kept for the runs that used it"
            } else {
                ""
            };
            if ui
                .add_enabled(enabled, Button::new(label))
                .on_disabled_hover_text(hover)
                .clicked()
            {
                view.request_remove(detail, version.id);
            }
        }
    });

    if read_only {
        let content = view.viewed_content(detail).unwrap_or_default();
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.add(
                egui::Label::new(RichText::new(content).monospace().small())
                    .wrap()
                    .selectable(true),
            );
        });
        return;
    }

    let response = labelled_multiline(ui, "Prompt content", &mut view.editor, 18);
    if response.changed() {
        view.dirty = view.editor_is_changed(Some(detail));
    }

    let changed = view.editor_is_changed(Some(detail));
    ui.horizontal(|ui| {
        if ui
            .add_enabled(changed && !state.writing, Button::new("Save new version"))
            .clicked()
        {
            *action = Some(PromptsAction::SaveDraft(view.editor.clone()));
        }
        if view.dirty {
            ui.label(RichText::new("Unsaved changes").weak());
        }
    });
}

fn versions_ui(
    ui: &mut Ui,
    detail: &PromptDetail,
    view: &mut PromptsView,
    action: &mut Option<PromptsAction>,
) {
    ui.heading(RichText::new("Versions").size(15.0));
    let current_id = detail.current_id;
    let viewed_id = view.viewed_version(detail).map(|row| row.id);
    let retired = detail
        .versions
        .iter()
        .filter(|version| version.retired_at.is_some())
        .count();
    if retired > 0 {
        ui.checkbox(&mut view.show_retired, format!("Show retired ({retired})"));
    }

    for version in &detail.versions {
        if version.retired_at.is_some() && !view.show_retired {
            continue;
        }
        let selected = viewed_id == Some(version.id);
        ui.horizontal(|ui| {
            let label = if version.retired_at.is_some() {
                RichText::new(format!("v{} (retired)", version.version)).weak()
            } else {
                RichText::new(format!("v{}", version.version))
            };
            if ui.selectable_label(selected, label).clicked() {
                *action = Some(PromptsAction::ShowVersion(
                    if current_id == Some(version.id) {
                        None
                    } else {
                        Some(version.id)
                    },
                ));
            }
            let uses = match version.usage.len() {
                0 => "not used yet".to_string(),
                1 => "used once".to_string(),
                n => format!("used {n} times"),
            };
            ui.label(
                RichText::new(format!(
                    "{}… · {} · {uses}",
                    short_hash(&version.content_hash),
                    version.updated_at
                ))
                .weak()
                .small(),
            );
            if version.is_current {
                ui.label(RichText::new("current").weak().small());
            } else if version.retired_at.is_some() {
                ui.label(RichText::new("retired").weak().small());
            }
        });
    }
}

fn usage_ui(ui: &mut Ui, detail: &PromptDetail, view: &PromptsView) {
    let Some(version) = view.viewed_version(detail) else {
        return;
    };
    ui.heading(RichText::new("Used by").size(15.0));
    ui.label(
        RichText::new(format!(
            "Evaluations, cover letters, and triage runs that recorded this version (v{}).",
            version.version
        ))
        .weak()
        .small(),
    );

    if version.usage.is_empty() {
        ui.label(RichText::new("Nothing has used this version yet.").weak());
        return;
    }

    for entry in &version.usage {
        ui.label(
            RichText::new(format!(
                "{} #{} · {} · {} · {}",
                entry.kind, entry.id, entry.posting, entry.generated_at, entry.model_used
            ))
            .small(),
        );
    }
}

fn remove_dialog(ctx: &egui::Context, view: &mut PromptsView, action: &mut Option<PromptsAction>) {
    let Some(confirm) = view.remove.clone() else {
        return;
    };

    let mut emitted = None;
    let mut close = false;
    let mut open = true;
    let title = if confirm.retired {
        "Retired prompt version"
    } else if confirm.retires() {
        "Retire prompt version"
    } else {
        "Delete prompt version"
    };

    egui::Window::new(title)
        .open(&mut open)
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            if confirm.retired {
                ui.label(format!(
                    "v{} is retired. Its text is still available to the runs that used it.",
                    confirm.version
                ));
                if confirm.usage_count == 0 {
                    ui.label("Nothing used it, so it can also be deleted permanently.");
                }
            } else if confirm.retires() {
                ui.label(format!(
                    "v{} has been used {} time{}. Deleting it would break the link from those \
                     runs to this text.",
                    confirm.version,
                    confirm.usage_count,
                    if confirm.usage_count == 1 { "" } else { "s" }
                ));
                ui.label("Retiring hides it from the picker and keeps the text available.");
            } else {
                ui.label(format!(
                    "v{} has not been used. Deleting it cannot be undone.",
                    confirm.version
                ));
                if confirm.blocked() {
                    ui.label("Restore another version before deleting the current one.");
                } else if let Some(promoted) = confirm.promoted {
                    ui.label(format!("v{promoted} becomes current."));
                } else if confirm.removes_thread {
                    ui.label("This removes your copy; the system prompt becomes active.");
                }
            }

            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if confirm.retired {
                    if ui.button("Restore").clicked() {
                        emitted = Some(PromptsAction::RestoreVersion(confirm.version_id));
                        close = true;
                    }
                    if confirm.usage_count == 0 && ui.button("Delete permanently").clicked() {
                        emitted = Some(PromptsAction::RemoveVersion {
                            version_id: confirm.version_id,
                            retire: false,
                        });
                        close = true;
                    }
                } else if confirm.retires() {
                    if ui.button("Retire").clicked() {
                        emitted = Some(PromptsAction::RemoveVersion {
                            version_id: confirm.version_id,
                            retire: true,
                        });
                        close = true;
                    }
                } else if !confirm.blocked() && ui.button("Delete permanently").clicked() {
                    emitted = Some(PromptsAction::RemoveVersion {
                        version_id: confirm.version_id,
                        retire: false,
                    });
                    close = true;
                }

                if ui.button("Cancel").clicked() {
                    close = true;
                }
            });
        });

    if !open {
        close = true;
    }
    if let Some(action_value) = emitted {
        *action = Some(action_value);
    }
    if close {
        view.remove = None;
    }
}

fn variables_ui(ui: &mut Ui, detail: &PromptDetail) {
    match variables_for_category(&detail.category) {
        Some(variables) => {
            egui::CollapsingHeader::new("Variables this category may use")
                .default_open(false)
                .show(ui, |ui| {
                    for variable in variables {
                        ui.label(
                            RichText::new(format!(
                                "{{{{{}}}}} — {}",
                                variable.name(),
                                variable.description()
                            ))
                            .small(),
                        );
                    }
                });
        }
        None => {
            ui.label(
                RichText::new(
                    "Reference prompt: the app does not render this category, so placeholder \
                     text is left as written.",
                )
                .weak()
                .small(),
            );
        }
    }
}

fn short_hash(hash: &str) -> &str {
    hash.get(..8).unwrap_or(hash)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_short_hash_is_bounded() {
        assert_eq!(short_hash("0123456789abcdef"), "01234567");
        assert_eq!(short_hash("short"), "short");
        assert_eq!(short_hash(""), "");
    }
}
