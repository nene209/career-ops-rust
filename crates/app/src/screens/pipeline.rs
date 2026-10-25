//! Pipeline screen.
//!
//! The screen renders a `PipelineViewModel`, keeps only the selected job in
//! local view state, and returns a `PipelineAction`. The app owns the load,
//! the filter mutation, and the shared manual-entry modal.

use egui::{Button, Color32, RichText, Ui};

use crate::view_models::pipeline::{JobFilter, JobRow, PipelineViewModel, StatusLabel, TierLabel};

use super::components::{EmptyState, LoadingSpinner, StatusBadge};

#[derive(Default)]
pub struct PipelineView {
    pub selected: Option<i64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipelineAction {
    AddJob,
    Reload,
    SetFilter(JobFilter),
}

#[derive(Debug, Clone, Copy)]
pub struct PipelineRenderState<'a> {
    pub loading: bool,
    pub error: Option<&'a str>,
}

pub fn show(
    ui: &mut Ui,
    view_model: &PipelineViewModel,
    view: &mut PipelineView,
    state: PipelineRenderState<'_>,
) -> Option<PipelineAction> {
    let mut action = None;

    header(ui, view_model, state, &mut action);

    // A filter applies to the list. Return to it so the change is visible
    // instead of leaving the detail panel on screen.
    if matches!(action, Some(PipelineAction::SetFilter(_))) {
        view.selected = None;
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

    if let Some(selected) = view.selected {
        if let Some(row) = view_model.row(selected) {
            detail(ui, row, view);
            return action;
        }

        // The row disappeared after a reload. Fall back to the list rather
        // than leaving the detail panel in an inconsistent state.
        view.selected = None;
    }

    list(ui, view_model, state, view, &mut action);
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
        ui.label(RichText::new(format!("{} jobs", view_model.rows.len())).weak());
    });

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

    egui::ScrollArea::vertical().show(ui, |ui| {
        for row in rows {
            row_ui(ui, row, view);
        }
    });
}

fn row_ui(ui: &mut Ui, row: &JobRow, view: &mut PipelineView) {
    ui.horizontal(|ui| {
        let selected = view.selected == Some(row.id);
        let label = format!("{} — {}", row.title, row.company);
        if ui.selectable_label(selected, label.as_str()).clicked() {
            view.selected = Some(row.id);
        }

        tier_label(ui, row.tier);
        if let Some(status) = row.status {
            status_badge(ui, status);
        }
        ui.label(RichText::new(&row.fetched_at).weak());
    });
    ui.separator();
}

fn detail(ui: &mut Ui, row: &JobRow, view: &mut PipelineView) {
    ui.horizontal(|ui| {
        if ui.button("Back to list").clicked() {
            view.selected = None;
        }

        ui.separator();

        ui.add_enabled(false, Button::new("Evaluate"))
            .on_disabled_hover_text("Available in a later phase");
        ui.add_enabled(false, Button::new("Generate cover letter"))
            .on_disabled_hover_text("Available in a later phase");
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
            ui.label(RichText::new(&row.raw_text).monospace());
        }
    });
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
    };
    badge.show(ui);
}
