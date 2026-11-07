//! CV / Profile screen.
//!
//! The screen renders the editable profile form, the three CV entry groups, and
//! a preview of the canonical `cv_summary`. It returns a `CvProfileAction`;
//! `CareerOpsApp` owns the database and the async writes.
//!
//! The profile form lives in the view-model (it is persisted data in progress),
//! while the transient entry drafts live in `CvProfileView`. Only one draft is
//! open at a time: opening the add form closes the edit form and vice versa.
//!
//! Delete is a two-step inline confirm rather than a modal. An entry is
//! user-authored but small, and the extra click is enough friction without
//! another window.

use egui::{Button, RichText, Ui};

use career_ops_core::repo::profile::ProfileDetails;

use crate::view_models::cv_profile::{
    kind_add_label, CvEntryDraft, CvEntryGroup, CvEntryRow, CvProfileViewModel, MoveDirection,
};

use super::components::{labelled_input, labelled_multiline, LoadingSpinner};

#[derive(Default)]
pub struct CvProfileView {
    /// The add form that is open, carrying its own draft.
    pub adding: Option<CvEntryDraft>,
    /// The entry being edited, if any.
    pub editing: Option<EntryEditor>,
    /// The entry whose delete confirmation is showing.
    pub deleting: Option<i64>,
}

pub struct EntryEditor {
    pub id: i64,
    pub draft: CvEntryDraft,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CvProfileAction {
    Reload,
    SaveProfile(ProfileDetails),
    CreateEntry(CvEntryDraft),
    UpdateEntry { id: i64, draft: CvEntryDraft },
    DeleteEntry(i64),
    MoveEntry { id: i64, direction: MoveDirection },
}

#[derive(Default, Clone, Copy)]
pub struct CvProfileRenderState<'a> {
    pub loading: bool,
    pub error: Option<&'a str>,
    pub message: Option<&'a str>,
    /// A profile save or an entry write is running.
    pub saving: bool,
    /// The entry being deleted, so its row can show progress.
    pub deleting: Option<i64>,
}

pub fn show(
    ui: &mut Ui,
    view_model: &mut CvProfileViewModel,
    view: &mut CvProfileView,
    state: CvProfileRenderState<'_>,
) -> Option<CvProfileAction> {
    let mut action = None;

    header(ui, view_model, state, &mut action);

    if let Some(error) = state.error {
        ui.horizontal(|ui| {
            ui.colored_label(egui::Color32::from_rgb(0xc0, 0x39, 0x2b), error);
            if ui
                .add_enabled(!state.saving, Button::new("Retry"))
                .clicked()
            {
                action = Some(CvProfileAction::Reload);
            }
        });
    }

    ui.separator();

    if state.loading {
        ui.horizontal(|ui| {
            LoadingSpinner::show(ui);
            ui.label("Loading profile...");
        });
        return action;
    }

    egui::ScrollArea::vertical().show(ui, |ui| {
        identity_form(ui, view_model, state);
        ui.add_space(12.0);

        for group in view_model.groups() {
            group_ui(ui, &group, view, state, &mut action);
        }

        ui.add_space(12.0);
        preview(ui, view_model);
    });

    action
}

fn header(
    ui: &mut Ui,
    view_model: &CvProfileViewModel,
    state: CvProfileRenderState<'_>,
    action: &mut Option<CvProfileAction>,
) {
    ui.horizontal(|ui| {
        if ui
            .add_enabled(
                view_model.is_dirty() && !state.saving,
                Button::new("Save profile"),
            )
            .on_hover_text("Store the identity and summary fields")
            .clicked()
        {
            *action = Some(CvProfileAction::SaveProfile(view_model.details()));
        }
        if ui
            .add_enabled(!state.saving, Button::new("Reload"))
            .on_hover_text("Discard unsaved edits and load the stored profile again")
            .clicked()
        {
            *action = Some(CvProfileAction::Reload);
        }
        if state.saving {
            LoadingSpinner::show(ui);
        }

        ui.separator();
        let count = view_model.entries.len();
        let noun = if count == 1 { "entry" } else { "entries" };
        ui.label(RichText::new(format!("{count} CV {noun}")).weak());
        if view_model.is_dirty() && !state.saving {
            ui.label(RichText::new("Unsaved changes").weak());
        }
    });

    if let Some(message) = state.message {
        ui.label(RichText::new(message).weak());
    }
}

fn identity_form(
    ui: &mut Ui,
    view_model: &mut CvProfileViewModel,
    state: CvProfileRenderState<'_>,
) {
    ui.heading("Profile");
    ui.label(
        RichText::new("These fields and your CV entries make up the candidate summary.")
            .weak()
            .small(),
    );
    ui.add_space(4.0);

    ui.add_enabled_ui(!state.saving, |ui| {
        egui::Grid::new("cv_profile_identity")
            .num_columns(2)
            .spacing([12.0, 6.0])
            .show(ui, |ui| {
                labelled_input(ui, "Name", &mut view_model.form.name, false);
                ui.end_row();
                labelled_input(ui, "Email", &mut view_model.form.email, false);
                ui.end_row();
                labelled_input(ui, "Phone", &mut view_model.form.phone, false);
                ui.end_row();
                labelled_input(ui, "Location", &mut view_model.form.location, false);
                ui.end_row();
            });

        ui.add_space(4.0);
        labelled_multiline(ui, "Summary", &mut view_model.form.summary, 4);
    });
}

fn group_ui(
    ui: &mut Ui,
    group: &CvEntryGroup<'_>,
    view: &mut CvProfileView,
    state: CvProfileRenderState<'_>,
    action: &mut Option<CvProfileAction>,
) {
    ui.add_space(8.0);
    let add_open = view
        .adding
        .as_ref()
        .is_some_and(|draft| draft.kind == group.kind);
    let can_open_add = !state.saving && view.editing.is_none() && !add_open;

    ui.horizontal(|ui| {
        ui.heading(group.title);
        ui.label(RichText::new(format!("({})", group.rows.len())).weak());
        if ui
            .add_enabled(can_open_add, Button::new(kind_add_label(group.kind)))
            .clicked()
        {
            view.adding = Some(CvEntryDraft::new(group.kind));
            view.editing = None;
            view.deleting = None;
        }
    });

    if group.rows.is_empty() && !add_open {
        ui.label(RichText::new("No entries yet.").weak().small());
    }

    for (index, row) in group.rows.iter().enumerate() {
        if view
            .editing
            .as_ref()
            .is_some_and(|editor| editor.id == row.id)
        {
            edit_row(ui, row, view, state, action);
        } else {
            row_ui(ui, group, row, index, view, state, action);
        }
    }

    if add_open {
        add_row(ui, view, state, action);
    }
}

fn add_row(
    ui: &mut Ui,
    view: &mut CvProfileView,
    state: CvProfileRenderState<'_>,
    action: &mut Option<CvProfileAction>,
) {
    let Some(draft) = view.adding.as_mut() else {
        return;
    };

    let outcome = entry_form(ui, "New entry", draft, state.saving);
    if outcome.cancel {
        view.adding = None;
        return;
    }
    if outcome.save {
        *action = Some(CvProfileAction::CreateEntry(draft.clone()));
    }
}

fn edit_row(
    ui: &mut Ui,
    row: &CvEntryRow,
    view: &mut CvProfileView,
    state: CvProfileRenderState<'_>,
    action: &mut Option<CvProfileAction>,
) {
    let Some(editor) = view.editing.as_mut() else {
        return;
    };

    let outcome = entry_form(ui, "Edit entry", &mut editor.draft, state.saving);
    if outcome.cancel {
        view.editing = None;
        return;
    }
    if outcome.save {
        *action = Some(CvProfileAction::UpdateEntry {
            id: row.id,
            draft: editor.draft.clone(),
        });
    }
}

fn row_ui(
    ui: &mut Ui,
    group: &CvEntryGroup<'_>,
    row: &CvEntryRow,
    index: usize,
    view: &mut CvProfileView,
    state: CvProfileRenderState<'_>,
    action: &mut Option<CvProfileAction>,
) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(&row.title).strong());
        if let Some(organization) = &row.organization {
            ui.label(organization);
        }
        if let Some(dates) = row.dates() {
            ui.label(RichText::new(dates).weak());
        }
    });

    if let Some(description) = row.description.as_deref().map(str::trim) {
        if !description.is_empty() {
            // The full text is for the summary; the row shows its first line.
            let first = description.lines().next().unwrap_or("");
            ui.label(RichText::new(first).weak().small());
        }
    }

    let busy = state.saving && state.deleting == Some(row.id);
    ui.horizontal(|ui| {
        if view.deleting == Some(row.id) {
            ui.label(RichText::new("Delete this entry?").strong());
            if ui
                .add_enabled(!state.saving, Button::new("Confirm delete"))
                .clicked()
            {
                *action = Some(CvProfileAction::DeleteEntry(row.id));
            }
            if ui
                .add_enabled(!state.saving, Button::new("Cancel"))
                .clicked()
            {
                view.deleting = None;
            }
            if busy {
                LoadingSpinner::show(ui);
            }
            return;
        }

        if ui.add_enabled(!state.saving, Button::new("Edit")).clicked() {
            view.editing = Some(EntryEditor {
                id: row.id,
                draft: row.to_draft(),
            });
            view.adding = None;
            view.deleting = None;
        }
        if ui
            .add_enabled(!state.saving, Button::new("Delete"))
            .clicked()
        {
            view.deleting = Some(row.id);
        }

        let can_move_up = index > 0;
        let can_move_down = index + 1 < group.rows.len();
        if ui
            .add_enabled(can_move_up && !state.saving, Button::new("Move up"))
            .clicked()
        {
            *action = Some(CvProfileAction::MoveEntry {
                id: row.id,
                direction: MoveDirection::Up,
            });
        }
        if ui
            .add_enabled(can_move_down && !state.saving, Button::new("Move down"))
            .clicked()
        {
            *action = Some(CvProfileAction::MoveEntry {
                id: row.id,
                direction: MoveDirection::Down,
            });
        }
    });

    ui.separator();
}

/// The add/edit form. Returns what the user clicked; the caller owns the draft.
#[derive(Default)]
struct EntryFormOutcome {
    save: bool,
    cancel: bool,
}

fn entry_form(
    ui: &mut Ui,
    heading: &str,
    draft: &mut CvEntryDraft,
    busy: bool,
) -> EntryFormOutcome {
    let mut outcome = EntryFormOutcome::default();

    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.label(RichText::new(heading).strong());
        labelled_input(ui, "Title", &mut draft.title, false);
        labelled_input(ui, "Organization", &mut draft.organization, false);
        ui.horizontal(|ui| {
            labelled_input(ui, "Start date", &mut draft.start_date, false);
            labelled_input(ui, "End date", &mut draft.end_date, false);
        });
        labelled_multiline(ui, "Description", &mut draft.description, 3);

        ui.horizontal(|ui| {
            if ui
                .add_enabled(draft.is_valid() && !busy, Button::new("Save entry"))
                .clicked()
            {
                outcome.save = true;
            }
            if ui.add_enabled(!busy, Button::new("Cancel")).clicked() {
                outcome.cancel = true;
            }
            if !draft.is_valid() {
                ui.label(RichText::new("A title is required.").weak().small());
            }
            if busy {
                LoadingSpinner::show(ui);
            }
        });
    });

    outcome
}

fn preview(ui: &mut Ui, view_model: &CvProfileViewModel) {
    ui.heading("Preview");
    ui.label(
        RichText::new("The candidate summary Phase 3 sends with an evaluation.")
            .weak()
            .small(),
    );
    ui.add_space(4.0);

    match view_model.preview() {
        Some(text) => {
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.label(RichText::new(text).monospace());
            });
        }
        None => {
            ui.label(
                RichText::new(
                    "Add a name, a summary, or a CV entry and the summary will appear here.",
                )
                .weak(),
            );
        }
    }
}
