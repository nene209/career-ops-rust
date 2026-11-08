//! The Usage screen: what the LLM calls used, by model and by task.
//!
//! Read-only. The app loads the three queries and hands the view-model over;
//! this screen renders it and emits a range change or a reload. Everything is a
//! plain `ScrollArea` and `Grid`, as elsewhere in the app.

use egui::{Button, Color32, Grid, RichText, ScrollArea, Ui};

use crate::view_models::usage::{format_cost, format_tokens, Range, UsageViewModel};

use super::components::{EmptyState, LoadingSpinner};

const ERROR: Color32 = Color32::from_rgb(0xc0, 0x39, 0x2b);
const WARNING: Color32 = Color32::from_rgb(0xc9, 0x8a, 0x00);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UsageAction {
    SetRange(Range),
    Reload,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct UsageRenderState<'a> {
    pub loading: bool,
    pub error: Option<&'a str>,
}

pub fn show(
    ui: &mut Ui,
    view_model: &UsageViewModel,
    state: UsageRenderState<'_>,
) -> Option<UsageAction> {
    let mut action = None;

    ui.horizontal(|ui| {
        ui.heading("Usage");
        ui.add_space(12.0);
        for range in Range::all() {
            if ui
                .add_enabled(
                    !state.loading,
                    Button::selectable(view_model.range == *range, range.label()),
                )
                .clicked()
            {
                action = Some(UsageAction::SetRange(*range));
            }
        }
        if ui
            .add_enabled(!state.loading, Button::new("Reload"))
            .on_hover_text("Read the ledger again")
            .clicked()
        {
            action = Some(UsageAction::Reload);
        }
    });
    ui.separator();

    if let Some(error) = state.error {
        ui.horizontal(|ui| {
            ui.colored_label(ERROR, error);
            if ui
                .add_enabled(!state.loading, Button::new("Retry"))
                .clicked()
            {
                action = Some(UsageAction::Reload);
            }
        });
        ui.add_space(4.0);
    }

    if state.loading && view_model.is_empty() {
        ui.horizontal(|ui| {
            LoadingSpinner::show(ui);
            ui.label("Loading usage...");
        });
        return action;
    }

    if view_model.is_empty() {
        EmptyState::new(
            "[#]",
            "No LLM calls recorded yet",
            "Evaluation and cover-letter calls appear here with their tokens and cost.",
        )
        .show(ui);
        return action;
    }

    ui.label(RichText::new(view_model.totals.summary()).strong());
    ui.label(RichText::new(view_model.totals.provenance()).weak().small());
    ui.add_space(10.0);

    ScrollArea::vertical()
        .id_salt("usage_sections")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            section_title(ui, "By provider and model");
            provider_model_grid(ui, view_model);
            ui.add_space(14.0);

            section_title(ui, "By task");
            task_grid(ui, view_model);
            ui.add_space(14.0);

            section_title(ui, "Recent calls");
            recent_grid(ui, view_model);
        });

    action
}

fn section_title(ui: &mut Ui, title: &str) {
    ui.label(RichText::new(title).strong());
    ui.add_space(4.0);
}

fn header(ui: &mut Ui, labels: &[&str]) {
    for label in labels {
        ui.label(RichText::new(*label).strong().small());
    }
    ui.end_row();
}

fn provider_model_grid(ui: &mut Ui, view_model: &UsageViewModel) {
    Grid::new("usage_by_model")
        .num_columns(7)
        .striped(true)
        .spacing([12.0, 4.0])
        .show(ui, |ui| {
            header(
                ui,
                &[
                    "Provider", "Model", "Calls", "Input", "Output", "Total", "Cost",
                ],
            );
            for group in &view_model.by_provider_model {
                ui.label(&group.provider);
                ui.label(&group.model);
                ui.label(format_tokens(group.calls));
                ui.label(format_tokens(group.input_tokens));
                ui.label(format_tokens(group.output_tokens));
                ui.label(format_tokens(group.total_tokens));
                ui.label(format_cost(group.cost_usd, group.unpriced_calls > 0));
                ui.end_row();
            }
        });
}

fn task_grid(ui: &mut Ui, view_model: &UsageViewModel) {
    Grid::new("usage_by_task")
        .num_columns(6)
        .striped(true)
        .spacing([12.0, 4.0])
        .show(ui, |ui| {
            header(ui, &["Task", "Calls", "Input", "Output", "Total", "Cost"]);
            for group in &view_model.by_task {
                ui.label(&group.task);
                ui.label(format_tokens(group.calls));
                ui.label(format_tokens(group.input_tokens));
                ui.label(format_tokens(group.output_tokens));
                ui.label(format_tokens(group.total_tokens));
                ui.label(format_cost(group.cost_usd, group.unpriced_calls > 0));
                ui.end_row();
            }
        });
}

fn recent_grid(ui: &mut Ui, view_model: &UsageViewModel) {
    Grid::new("usage_recent")
        .num_columns(8)
        .striped(true)
        .spacing([12.0, 4.0])
        .show(ui, |ui| {
            header(
                ui,
                &[
                    "When", "Model", "Task", "Job", "Tokens", "Source", "Cost", "Outcome",
                ],
            );
            for row in &view_model.recent {
                ui.label(&row.when);
                ui.label(&row.model);
                ui.label(&row.task);
                ui.label(&row.job);
                ui.label(&row.tokens);
                ui.label(&row.source);
                ui.label(&row.cost);
                match row.outcome.as_str() {
                    "ok" => {
                        ui.label(&row.outcome);
                    }
                    "cancelled" => {
                        ui.colored_label(WARNING, &row.outcome);
                    }
                    _ => {
                        ui.colored_label(ERROR, &row.outcome);
                    }
                }
                ui.end_row();
            }
        });
}
