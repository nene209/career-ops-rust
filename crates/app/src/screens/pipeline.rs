//! Pipeline screen.
//!
//! The screen renders a `PipelineViewModel`, keeps only the selected job in
//! local view state, and returns a `PipelineAction`. The app owns the load,
//! the filter mutation, and the shared manual-entry modal.

use egui::{Align2, Button, Color32, RichText, Ui, Window};

use career_ops_core::model::QueueState;

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
    /// Queue states: the work the user wants done before an evaluation.
    MarkForDetails(i64),
    MarkForEvaluation(i64),
    Dismiss(i64),
    /// Fetch the full description for every job marked for details.
    FetchDescriptions,
    /// Stop a running description batch between jobs.
    CancelDescriptions,
    /// Delete the job, confirmed.
    DeleteJob(i64),
    /// Evaluate the job with the description storage already holds.
    Evaluate(i64),
    /// Fetch a full description first, then evaluate. WS6 emits this when the
    /// selected row is under the thin-description threshold.
    FetchDescriptionAndEvaluate(i64),
}

/// How far a running description batch has got.
///
/// Counts only: the header shows "3 of 12", and the per-job history is in the
/// Logs screen rather than in a transient line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DescriptionProgress {
    pub done: usize,
    pub total: usize,
    pub failed: usize,
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
    /// The job whose queue state is being changed.
    pub queueing: Option<i64>,
    /// A description batch in flight.
    pub descriptions: Option<DescriptionProgress>,
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

        // One click is a bounded batch, so the button says how much work it is
        // about to do and refuses to start with nothing flagged.
        let flagged = view_model.details_needed();
        if ui
            .add_enabled(
                !state.loading && state.descriptions.is_none() && flagged > 0,
                Button::new(format!("Fetch descriptions ({flagged})")),
            )
            .on_hover_text(
                "Fetch the full description for each job marked for details, up to 20 per click",
            )
            .on_disabled_hover_text(if flagged == 0 {
                "No jobs are marked for details"
            } else {
                "A description batch is already running"
            })
            .clicked()
        {
            *action = Some(PipelineAction::FetchDescriptions);
        }
        if let Some(progress) = state.descriptions {
            ui.label(
                RichText::new(format!(
                    "Descriptions: {} of {}…",
                    progress.done, progress.total
                ))
                .weak(),
            );
            if progress.failed > 0 {
                ui.label(
                    RichText::new(format!("{} failed", progress.failed))
                        .small()
                        .color(Color32::from_rgb(0xc9, 0x8a, 0x00)),
                );
            }
            if ui.button("Stop").clicked() {
                *action = Some(PipelineAction::CancelDescriptions);
            }
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

    // Preference summary: shown only when something is configured, and phrased
    // so "nothing matched" is distinguishable from "nothing set".
    if let Some(summary) = view_model.preference_summary() {
        ui.label(RichText::new(summary).weak().small());
    }

    // Seven chips no longer fit on one line at the minimum window width.
    ui.horizontal_wrapped(|ui| {
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
        if row.preferred {
            ui.label(
                RichText::new("Preferred")
                    .small()
                    .color(Color32::from_rgb(0x0e, 0x7c, 0x86)),
            );
        }
        if row.excluded {
            ui.label(
                RichText::new("Excluded")
                    .small()
                    .color(Color32::from_rgb(0x6b, 0x72, 0x80)),
            );
        }
        if let Some(status) = row.status {
            status_badge(ui, status);
        }
        queue_badge(ui, row.queue);
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
        let busy = state.refreshing == Some(row.id)
            || state.marking == Some(row.id)
            || state.queueing == Some(row.id)
            || deleting;

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
        queue_badge(ui, row.queue);
        ui.label(RichText::new(format!("Fetched {}", row.fetched_at)).weak());
    });

    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.label(RichText::new("Queue").weak());
        let busy = state.queueing == Some(row.id) || state.marking == Some(row.id);
        for (target, label, hover) in QUEUE_ACTIONS {
            queue_action(ui, row, target, label, hover, busy, action);
        }
        if state.queueing == Some(row.id) {
            LoadingSpinner::show(ui);
        }
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

/// The three queue actions the detail view offers, in order.
const QUEUE_ACTIONS: [(QueueState, &str, &str); 3] = [
    (
        QueueState::DetailsNeeded,
        "Mark for details",
        "Wants the full description before deciding",
    ),
    (
        QueueState::Evaluate,
        "Mark for evaluation",
        "Wants an evaluation in a later phase",
    ),
    (
        QueueState::Dismissed,
        "Dismiss",
        "Hide this posting; find it again under the Dismissed filter",
    ),
];

/// One queue action, e.g. **Mark for details**.
///
/// A row already in the target state shows the state's label instead of a
/// button: the action would write the same value, so offering it would be a
/// dead click. `New` is the default and `Evaluated` is written by an evaluation
/// report, so neither is offered here.
fn queue_action(
    ui: &mut Ui,
    row: &JobRow,
    target: QueueState,
    label: &str,
    hover: &str,
    busy: bool,
    action: &mut Option<PipelineAction>,
) {
    let intent = match target {
        QueueState::DetailsNeeded => PipelineAction::MarkForDetails(row.id),
        QueueState::Evaluate => PipelineAction::MarkForEvaluation(row.id),
        QueueState::Dismissed => PipelineAction::Dismiss(row.id),
        QueueState::New | QueueState::Evaluated => return,
    };

    if row.queue == target {
        ui.add_enabled(false, Button::new(target.label()))
            .on_disabled_hover_text("Already in this state");
        return;
    }

    if ui
        .add_enabled(!busy, Button::new(label))
        .on_hover_text(hover)
        .clicked()
    {
        *action = Some(intent);
    }
}

/// The queue state beside the status badge. `New` is the default and is not
/// worth a badge; the other states are a decision the user can see and reverse
/// from the detail view.
fn queue_badge(ui: &mut Ui, queue: QueueState) {
    if queue == QueueState::New {
        return;
    }

    let color = match queue {
        QueueState::New => Color32::GRAY,
        QueueState::DetailsNeeded => Color32::from_rgb(0x1f, 0x6f, 0xeb),
        QueueState::Evaluate => Color32::from_rgb(0x7c, 0x3a, 0xed),
        QueueState::Dismissed => Color32::from_rgb(0x6b, 0x72, 0x80),
        QueueState::Evaluated => Color32::from_rgb(0x2e, 0xa0, 0x43),
    };
    ui.label(RichText::new(queue.label()).small().color(color));
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
