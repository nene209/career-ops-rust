//! Pipeline screen.
//!
//! The screen renders a `PipelineViewModel`, keeps only the selected job in
//! local view state, and returns a `PipelineAction`. The app owns the load,
//! the filter mutation, and the shared manual-entry modal.

use egui::{Align2, Button, Color32, RichText, Ui, Window};

use crate::view_models::pipeline::{JobFilter, JobRow, PipelineViewModel, StatusLabel, TierLabel};

use super::components::{labelled_input, EmptyState, LoadingSpinner, StatusBadge};

#[derive(Default)]
pub struct PipelineView {
    pub selected: Option<i64>,
    /// Open only for a job the user has touched; a fetched-only row deletes
    /// without a dialog.
    pub delete: Option<DeleteJobModal>,
    /// Show the stored markup rather than the formatted text.
    pub show_raw: bool,
    /// The list's vertical scroll offset, so returning from the detail view
    /// restores where the user was.
    pub list_scroll: f32,
}

/// The typed gate in front of a destructive delete.
#[derive(Default)]
pub struct DeleteJobModal {
    pub id: i64,
    pub title: String,
    pub company: String,
    pub phrase: String,
}

/// The phrase the user types to confirm deleting a touched job.
pub const DELETE_PHRASE: &str = "delete me";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PipelineAction {
    AddJob,
    Reload,
    SetFilter(JobFilter),
    SetSearch(String),
    /// Re-fetch one posting's description in place.
    RefreshJob(i64),
    /// Mark the job's application as not worth pursuing.
    MarkNotAMatch(i64),
    /// Delete the job, confirmed.
    DeleteJob(i64),
}

#[derive(Default, Clone, Copy)]
pub struct PipelineRenderState<'a> {
    pub loading: bool,
    pub error: Option<&'a str>,
    /// The outcome of the last refresh or status change, or a hint about one.
    pub message: Option<&'a str>,
    /// The job whose description is being re-fetched.
    pub refreshing: Option<i64>,
    /// The job whose status is being changed.
    pub marking: Option<i64>,
    /// The job being deleted.
    pub deleting: Option<i64>,
}

pub fn show(
    ui: &mut Ui,
    view_model: &PipelineViewModel,
    view: &mut PipelineView,
    state: PipelineRenderState<'_>,
) -> Option<PipelineAction> {
    let mut action = None;

    // The gate belongs to one row: if the selection moved on, drop it.
    if view
        .delete
        .as_ref()
        .is_some_and(|modal| view.selected != Some(modal.id))
    {
        view.delete = None;
    }

    header(ui, view_model, state, &mut action);

    // A filter or a query applies to the list. Return to it so the change is
    // visible instead of leaving the detail panel on screen.
    if matches!(
        &action,
        Some(PipelineAction::SetFilter(_) | PipelineAction::SetSearch(_))
    ) {
        view.selected = None;
        view.show_raw = false;
        view.list_scroll = 0.0;
    }

    if let Some(error) = state.error {
        ui.horizontal(|ui| {
            ui.colored_label(Color32::from_rgb(0xc0, 0x39, 0x2b), error);
            if ui.button("Retry").clicked() {
                action = Some(PipelineAction::Reload);
            }
        });
    }

    ui.separator();

    let mut showing_detail = false;
    if let Some(selected) = view.selected {
        if let Some(row) = view_model.row(selected) {
            detail(ui, row, view, state, &mut action);
            showing_detail = true;
        } else {
            // The row disappeared after a reload. Fall back to the list rather
            // than leaving the detail panel in an inconsistent state.
            view.selected = None;
            view.show_raw = false;
        }
    }

    if !showing_detail {
        list(ui, view_model, state, view, &mut action);
    }

    // The typed gate is chrome over the screen, like the portal dialogs.
    if let Some(gate_action) = delete_gate(ui, view, state) {
        action = Some(gate_action);
    }

    action
}

fn header(
    ui: &mut Ui,
    view_model: &PipelineViewModel,
    state: PipelineRenderState<'_>,
    action: &mut Option<PipelineAction>,
) {
    ui.horizontal(|ui| {
        if ui.button("Add job").clicked() {
            *action = Some(PipelineAction::AddJob);
        }
        if ui
            .add_enabled(!state.loading, Button::new("Refresh"))
            .clicked()
        {
            *action = Some(PipelineAction::Reload);
        }
        if state.loading {
            LoadingSpinner::show(ui);
        }

        ui.separator();
        let total = view_model.rows.len();
        if view_model.search.trim().is_empty() {
            ui.label(RichText::new(format!("{total} jobs")).weak());
        } else {
            // The count follows the visible list, so a search that intersects a
            // filter reads as what the user is actually looking at.
            let shown = view_model.apply_filter().count();
            ui.label(RichText::new(format!("{shown} of {total} jobs match")).weak());
        }
    });

    if let Some(message) = state.message {
        ui.label(RichText::new(message).weak());
    }

    ui.horizontal(|ui| {
        for &filter in JobFilter::all() {
            if ui
                .selectable_label(view_model.filter == filter, filter.label())
                .clicked()
            {
                *action = Some(PipelineAction::SetFilter(filter));
            }
        }
    });

    ui.horizontal(|ui| {
        // The screen does not own the view model, so the field edits a copy and
        // signals the app. The app applies it before the next frame.
        let mut query = view_model.search.clone();
        if labelled_input(ui, "Search", &mut query, false).changed() && query != view_model.search {
            *action = Some(PipelineAction::SetSearch(query));
        }

        if !view_model.search.is_empty() && ui.button("Clear").clicked() {
            *action = Some(PipelineAction::SetSearch(String::new()));
        }
    });
}

fn list(
    ui: &mut Ui,
    view_model: &PipelineViewModel,
    state: PipelineRenderState<'_>,
    view: &mut PipelineView,
    action: &mut Option<PipelineAction>,
) {
    if state.loading && view_model.rows.is_empty() {
        ui.horizontal(|ui| {
            LoadingSpinner::show(ui);
            ui.label("Loading jobs...");
        });
        return;
    }

    let rows: Vec<&JobRow> = view_model.apply_filter().collect();

    if rows.is_empty() {
        if view_model.rows.is_empty() {
            if EmptyState::new(
                "|||",
                "No jobs yet",
                "Fetch a job from a portal, or add one manually.",
            )
            .with_action("Add your first job")
            .show(ui)
            {
                *action = Some(PipelineAction::AddJob);
            }
        } else if !view_model.search.trim().is_empty() {
            if EmptyState::new(
                "|||",
                "No jobs match this search",
                "Clear the search or try a different title or company.",
            )
            .with_action("Clear search")
            .show(ui)
            {
                *action = Some(PipelineAction::SetSearch(String::new()));
            }
        } else {
            EmptyState::new(
                "|||",
                "No jobs match this filter",
                "Choose a different filter to see more listings.",
            )
            .show(ui);
        }
        return;
    }

    let output = egui::ScrollArea::vertical()
        .vertical_scroll_offset(view.list_scroll)
        .show(ui, |ui| {
            for row in rows {
                row_ui(ui, row, view);
            }
        });
    view.list_scroll = output.state.clamped_offset().y;
}

fn row_ui(ui: &mut Ui, row: &JobRow, view: &mut PipelineView) {
    ui.horizontal(|ui| {
        let selected = view.selected == Some(row.id);
        let label = format!("{} — {}", row.title, row.company);
        if ui.selectable_label(selected, label.as_str()).clicked() {
            view.selected = Some(row.id);
            view.show_raw = false;
        }

        tier_label(ui, row.tier);
        if let Some(status) = row.status {
            status_badge(ui, status);
        }
        ui.label(RichText::new(&row.fetched_at).weak());
    });
    ui.separator();
}

fn detail(
    ui: &mut Ui,
    row: &JobRow,
    view: &mut PipelineView,
    state: PipelineRenderState<'_>,
    action: &mut Option<PipelineAction>,
) {
    ui.horizontal(|ui| {
        if ui.button("Back to list").clicked() {
            view.selected = None;
            view.show_raw = false;
        }

        ui.separator();

        ui.add_enabled(false, Button::new("Evaluate"))
            .on_disabled_hover_text("Available in a later phase");
        ui.add_enabled(false, Button::new("Generate cover letter"))
            .on_disabled_hover_text("Available in a later phase");

        let deleting = state.deleting == Some(row.id);
        let busy = state.refreshing == Some(row.id) || state.marking == Some(row.id) || deleting;

        if ui
            .add_enabled(
                state.refreshing.is_none() && state.deleting.is_none(),
                Button::new("Refresh description"),
            )
            .on_hover_text("Fetch this posting again and replace its text")
            .clicked()
        {
            *action = Some(PipelineAction::RefreshJob(row.id));
        }
        if state.refreshing == Some(row.id) {
            LoadingSpinner::show(ui);
        }

        let can_mark = match row.status {
            None => true,
            Some(status) => status.can_mark_not_a_match(),
        };
        if ui
            .add_enabled(can_mark && !busy, Button::new("Mark not a match"))
            .on_hover_text("Keep the posting but leave it out of the active pipeline")
            .clicked()
        {
            *action = Some(PipelineAction::MarkNotAMatch(row.id));
        }
        if state.marking == Some(row.id) {
            LoadingSpinner::show(ui);
        }

        // A fetched-only row has nothing to lose: the portal can fetch it
        // again. A row with an evaluation or a saved status is gated.
        let label = if row.has_user_state {
            "Delete job..."
        } else {
            "Delete job"
        };
        let hint = if row.has_user_state {
            "This job has an evaluation, cover letter, or saved status; you will be asked to confirm"
        } else {
            "Remove this posting. The next portal fetch can add it again"
        };
        if ui
            .add_enabled(!busy, Button::new(label))
            .on_hover_text(hint)
            .clicked()
        {
            if row.has_user_state {
                // The gate is view state, so the screen owns it. Only the
                // confirmed delete travels to the app.
                view.delete = Some(DeleteJobModal {
                    id: row.id,
                    title: row.title.clone(),
                    company: row.company.clone(),
                    phrase: String::new(),
                });
            } else {
                *action = Some(PipelineAction::DeleteJob(row.id));
            }
        }
        if deleting {
            LoadingSpinner::show(ui);
        }
    });

    ui.add_space(4.0);
    ui.heading(&row.title);
    ui.label(RichText::new(&row.company).strong());
    ui.horizontal(|ui| {
        tier_label(ui, row.tier);
        if let Some(status) = row.status {
            status_badge(ui, status);
        }
        ui.label(RichText::new(format!("Fetched {}", row.fetched_at)).weak());
    });

    ui.add_space(4.0);
    ui.label(RichText::new(&row.source_url).monospace().weak());
    ui.separator();

    egui::ScrollArea::vertical().show(ui, |ui| {
        if row.raw_text.trim().is_empty() {
            ui.label(RichText::new("No description stored.").weak());
        } else {
            if row.stored_is_markup {
                ui.checkbox(&mut view.show_raw, "Raw text");
                if view.show_raw {
                    ui.label(
                        RichText::new("Stored as HTML from the source.")
                            .weak()
                            .small(),
                    );
                }
            }

            if view.show_raw {
                ui.label(RichText::new(&row.raw_text).monospace());
            } else {
                ui.label(RichText::new(&row.plain_text));
            }
        }
    });
}

/// The typed gate in front of a destructive delete.
///
/// A fetched-only row deletes without this dialog, because nothing the user
/// authored is lost. This dialog exists for the rows where something is: an
/// evaluation costs tokens and cannot be re-created by fetching.
fn delete_gate(
    ui: &mut Ui,
    view: &mut PipelineView,
    state: PipelineRenderState<'_>,
) -> Option<PipelineAction> {
    let modal = view.delete.as_mut()?;

    let id = modal.id;
    let busy = state.deleting == Some(id);
    let confirmed = modal.phrase.trim() == DELETE_PHRASE;
    let mut decision = None;

    Window::new("Delete job?")
        .collapsible(false)
        .resizable(false)
        .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
        .default_width(460.0)
        .show(ui.ctx(), |ui| {
            ui.label(format!("Delete {} at {}?", modal.title, modal.company));
            ui.label(
                "The application, its evaluation reports, and any cover letters are deleted too.",
            );
            ui.label("An evaluation cannot be fetched again.");
            ui.add_space(8.0);
            labelled_input(ui, "Type delete me to confirm", &mut modal.phrase, false);
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(confirmed && !busy, Button::new("Delete job"))
                    .clicked()
                {
                    decision = Some(true);
                }
                if ui.add_enabled(!busy, Button::new("Cancel")).clicked() {
                    decision = Some(false);
                }
                if busy {
                    LoadingSpinner::show(ui);
                }
            });
        });

    match decision {
        // The modal stays open until the app reports the outcome, so a failure
        // leaves the confirmation on screen instead of losing it.
        Some(true) => Some(PipelineAction::DeleteJob(id)),
        Some(false) => {
            view.delete = None;
            None
        }
        None => None,
    }
}

fn tier_label(ui: &mut Ui, tier: TierLabel) {
    ui.label(RichText::new(tier.label()).monospace().small().weak());
}

fn status_badge(ui: &mut Ui, status: StatusLabel) {
    let badge = match status {
        StatusLabel::Discovered => StatusBadge::Discovered,
        StatusLabel::Evaluating => StatusBadge::Evaluating,
        StatusLabel::Ready => StatusBadge::Ready,
        StatusLabel::Applied => StatusBadge::Applied,
        StatusLabel::Interviewing => StatusBadge::Interviewing,
        StatusLabel::Rejected => StatusBadge::Rejected,
        StatusLabel::Offer => StatusBadge::Offer,
        StatusLabel::NotAMatch => StatusBadge::NotAMatch,
    };
    badge.show(ui);
}
