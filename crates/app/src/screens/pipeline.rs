//! Pipeline screen.
//!
//! The screen renders a `PipelineViewModel`, keeps only the selected job in
//! local view state, and returns a `PipelineAction`. The app owns the load,
//! the filter mutation, and the shared manual-entry modal.

use egui::{Align2, Button, Color32, RichText, Ui, Window};

use career_ops_core::evaluation::Recommendation;
use career_ops_core::llm::config::DEFAULT_THIN_DESCRIPTION_CHARS;
use career_ops_core::model::QueueState;
use career_ops_core::triage::{TriageChoice, TriageEstimate};

use crate::view_models::pipeline::{
    EvaluationRow, JobFilter, JobRow, PipelineDetail, PipelineViewModel, StatusLabel, TierLabel,
    PAGE_SIZES,
};

use super::components::{labelled_input, EmptyState, LoadingSpinner, StatusBadge};

#[derive(Default)]
pub struct PipelineView {
    pub selected: Option<i64>,
    /// Open only for a job the user has touched; a fetched-only row deletes
    /// without a dialog.
    pub delete: Option<DeleteJobModal>,
    /// The confirmation in front of a triage batch. The ids are captured when
    /// the modal opens so the estimate and the run cover the same set.
    pub triage: Option<TriageModal>,
    /// The user has read the override hint, so it does not return this session.
    pub triage_banner_dismissed: bool,
    /// Show the stored markup rather than the formatted text.
    pub show_raw: bool,
    /// The list's vertical scroll offset, so returning from the detail view
    /// restores where the user was.
    pub list_scroll: f32,
    /// Which detail tab is open.
    pub tab: DetailTab,
    /// The evaluation shown in the tab; `None` is the latest.
    pub selected_eval: Option<i64>,
    /// The cover letter shown in the tab; `None` is the latest.
    pub selected_letter: Option<i64>,
    /// Overrides for the next evaluation. Empty means the defaults.
    pub eval_prompt: String,
    pub eval_model: String,
    /// Overrides for the next cover letter.
    pub letter_prompt: String,
    pub letter_model: String,
    pub letter_tone: String,
}

/// The three parts of a job's detail view.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DetailTab {
    #[default]
    Description,
    Evaluation,
    CoverLetter,
}

impl DetailTab {
    pub fn all() -> &'static [DetailTab] {
        &[
            DetailTab::Description,
            DetailTab::Evaluation,
            DetailTab::CoverLetter,
        ]
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Description => "Description",
            Self::Evaluation => "Evaluation",
            Self::CoverLetter => "Cover letter",
        }
    }
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

/// The confirmation in front of a triage batch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TriageModal {
    /// The rows the batch will cover, fixed when the modal opened.
    pub job_ids: Vec<i64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PipelineAction {
    AddJob,
    Reload,
    SetFilter(JobFilter),
    SetSearch(String),
    /// Move to a page (0-based). The app clamps it to the last page.
    SetPage(usize),
    /// Change the page size and return to the first page.
    SetPageSize(usize),
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
    /// Open the triage confirmation and estimate the batch for it.
    RequestTriage {
        job_ids: Vec<i64>,
    },
    /// Run the batch the confirmation showed.
    RunTriage {
        job_ids: Vec<i64>,
    },
    /// Stop a running triage batch. Nothing partial is applied.
    CancelTriage,
    /// Delete the job, confirmed.
    DeleteJob(i64),
    /// Evaluate the job with the options the detail pane carries.
    Evaluate {
        job_id: i64,
        fetch_description_if_thin: bool,
        prompt_name: String,
        model_override: Option<String>,
    },
    /// Stop the running evaluation. The service restores the status.
    CancelEvaluation,
    /// Write a cover letter. `tone` is a soft register; empty leaves it to the
    /// prompt.
    GenerateCoverLetter {
        job_id: i64,
        tone: Option<String>,
        prompt_name: String,
        model_override: Option<String>,
    },
    /// Stop the running cover-letter call.
    CancelCoverLetter,
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
    /// A triage batch in flight, and how many postings it covers.
    pub triaging: bool,
    pub triaging_count: usize,
    /// The estimate shown in the triage confirmation.
    pub triage_estimate: Option<&'a TriageEstimate>,
    /// The job being deleted.
    pub deleting: Option<i64>,
    /// The selected job's evaluation and letter history.
    pub detail: Option<&'a PipelineDetail>,
    /// The history for the selection is still loading.
    pub detail_loading: bool,
    /// The job whose evaluation is running.
    pub evaluating: Option<i64>,
    /// The streamed reply so far, for the running evaluation.
    pub stream: &'a str,
    /// The job whose cover letter is being written.
    pub writing_letter: Option<i64>,
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

    header(ui, view_model, view, state, &mut action);

    // The override hint appears once the batch has touched something. It is a
    // hint, not a gate, and the user can put it away for the session.
    if !view.triage_banner_dismissed && view_model.rows.iter().any(|row| row.triage.is_some()) {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(
                    "You can override any triage decision. Skipped jobs stay visible under the Dismissed filter.",
                )
                .weak()
                .small(),
            );
            if ui.button("Got it").clicked() {
                view.triage_banner_dismissed = true;
            }
        });
    }

    // A filter or a query applies to the list. Return to it so the change is
    // visible instead of leaving the detail panel on screen. A page change
    // only resets the scroll: the user is already on the list.
    match &action {
        Some(PipelineAction::SetFilter(_) | PipelineAction::SetSearch(_)) => {
            view.selected = None;
            view.show_raw = false;
            view.list_scroll = 0.0;
        }
        Some(PipelineAction::SetPage(_) | PipelineAction::SetPageSize(_)) => {
            view.list_scroll = 0.0;
        }
        _ => {}
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
    if let Some(triage_action) = triage_modal(ui, view, state) {
        action = Some(triage_action);
    }

    action
}

fn header(
    ui: &mut Ui,
    view_model: &PipelineViewModel,
    view: &mut PipelineView,
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
                !state.loading && state.descriptions.is_none() && !state.triaging && flagged > 0,
                Button::new(format!("Fetch descriptions ({flagged})")),
            )
            .on_hover_text(
                "Fetch the full description for each job marked for details, up to 20 per click",
            )
            .on_disabled_hover_text(if flagged == 0 {
                "No jobs are marked for details"
            } else {
                "A description batch or triage is already running"
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

        // One call, so the count is the batch's size and a model call or a
        // description batch in flight is enough to wait.
        let candidate_count = view_model.triage_candidate_count();
        let llm_busy =
            state.triaging || state.evaluating.is_some() || state.writing_letter.is_some();
        if ui
            .add_enabled(
                !state.loading && !llm_busy && state.descriptions.is_none() && candidate_count > 0,
                Button::new(format!("Triage ({candidate_count})")),
            )
            .on_hover_text(
                "Sort new postings into worth details, evaluate, skip, or unsure, in one call",
            )
            .on_disabled_hover_text(if candidate_count == 0 {
                "No new jobs to triage"
            } else if state.triaging {
                "A triage batch is already running"
            } else {
                "Wait for the running model call or description batch"
            })
            .clicked()
        {
            // The ids are captured here so the estimate and the run cover the
            // same rows even if the list reloads while the modal is open.
            let job_ids = view_model.triage_candidates();
            view.triage = Some(TriageModal {
                job_ids: job_ids.clone(),
            });
            *action = Some(PipelineAction::RequestTriage { job_ids });
        }
        if state.triaging {
            ui.label(RichText::new(format!("Triaging {} jobs…", state.triaging_count)).weak());
            if ui.button("Stop").clicked() {
                *action = Some(PipelineAction::CancelTriage);
            }
            LoadingSpinner::show(ui);
        }

        ui.separator();
        let total = view_model.rows.len();
        if view_model.search.trim().is_empty() {
            ui.label(RichText::new(format!("{total} jobs")).weak());
        } else {
            // The count follows the visible set, so a search that intersects a
            // filter reads as what the user is actually looking at.
            let shown = view_model.filtered_len();
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

    let total = view_model.filtered_len();

    if total == 0 {
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

    // The footer owns the bottom strip, so the page controls never scroll away.
    // It is shown for a single page too: the range and the page-size choice are
    // still information the user needs.
    egui::Panel::bottom("pipeline_pagination")
        .show(ui, |ui| pagination_bar(ui, view_model, action));

    let output = egui::ScrollArea::vertical()
        .vertical_scroll_offset(view.list_scroll)
        .show(ui, |ui| {
            for row in view_model.page_rows() {
                row_ui(ui, row, view);
            }
        });
    view.list_scroll = output.state.clamped_offset().y;
}

/// The page controls under the list.
///
/// The view model owns the page; this bar only reads it and emits the intent.
/// `First`/`Previous` are disabled on page 1 and `Next`/`Last` on the last page,
/// so a click can never be a no-op that still looks live.
fn pagination_bar(
    ui: &mut Ui,
    view_model: &PipelineViewModel,
    action: &mut Option<PipelineAction>,
) {
    let page = view_model.effective_page();
    let pages = view_model.page_count();
    let (start, end) = view_model.page_bounds();
    let total = view_model.filtered_len();

    ui.add_space(4.0);
    ui.horizontal_wrapped(|ui| {
        if ui.add_enabled(page > 0, Button::new("First")).clicked() {
            *action = Some(PipelineAction::SetPage(0));
        }
        if ui.add_enabled(page > 0, Button::new("Previous")).clicked() {
            *action = Some(PipelineAction::SetPage(page - 1));
        }

        ui.label(RichText::new(format!("{start}–{end} of {total}")).weak());
        ui.label(RichText::new(format!("Page {} of {pages}", page + 1)).weak());

        if ui
            .add_enabled(page + 1 < pages, Button::new("Next"))
            .clicked()
        {
            *action = Some(PipelineAction::SetPage(page + 1));
        }
        if ui
            .add_enabled(page + 1 < pages, Button::new("Last"))
            .clicked()
        {
            *action = Some(PipelineAction::SetPage(pages - 1));
        }

        ui.separator();
        ui.label(RichText::new("Rows per page:").weak().small());
        for size in PAGE_SIZES {
            if ui
                .selectable_label(view_model.page_size == size, size.to_string())
                .clicked()
                && view_model.page_size != size
            {
                *action = Some(PipelineAction::SetPageSize(size));
            }
        }
    });
}

fn row_ui(ui: &mut Ui, row: &JobRow, view: &mut PipelineView) {
    ui.horizontal(|ui| {
        let selected = view.selected == Some(row.id);
        let label = format!("{} — {}", row.title, row.company);
        if ui.selectable_label(selected, label.as_str()).clicked() {
            if view.selected != Some(row.id) {
                // A new job's history starts at its latest report.
                view.selected_eval = None;
                view.selected_letter = None;
            }
            view.selected = Some(row.id);
            view.show_raw = false;
        }

        tier_label(ui, row.tier);
        if let Some(score) = row.match_score {
            let band = Recommendation::from_score(score as f32);
            ui.label(RichText::new(format!("{score:.2}")).small().strong())
                .on_hover_text(band.label());
        }
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
        for tab in DetailTab::all() {
            if ui.selectable_label(view.tab == *tab, tab.label()).clicked() {
                view.tab = *tab;
            }
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
        if let Some(score) = row.match_score {
            let band = Recommendation::from_score(score as f32);
            ui.label(RichText::new(format!("{score:.2}")).small().strong())
                .on_hover_text(band.label());
        }
        ui.label(RichText::new(format!("Fetched {}", row.fetched_at)).weak());
    });

    // The decision and its reason: the queue badge says what the row became,
    // and this says why. `Unsure` shares `Evaluate`'s queue state, so the
    // choice itself has to be shown to tell them apart.
    if let Some(choice) = row.triage {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Triage").weak().small());
            triage_badge(ui, choice);
        });
    }
    if let Some(reason) = row.queue_reason.as_deref().filter(|r| !r.trim().is_empty()) {
        ui.label(RichText::new(reason).italics().small().weak());
    }

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
    action_row(ui, row, view, state, action);

    ui.add_space(4.0);
    ui.label(RichText::new(&row.source_url).monospace().weak());
    ui.separator();

    egui::ScrollArea::vertical()
        .id_salt("pipeline_detail")
        .show(ui, |ui| match view.tab {
            DetailTab::Description => description_tab(ui, row, view),
            DetailTab::Evaluation => evaluation_tab(ui, row, view, state, action),
            DetailTab::CoverLetter => cover_letter_tab(ui, row, view, state, action),
        });
}

/// The primary actions, plus the per-row maintenance ones.
fn action_row(
    ui: &mut Ui,
    row: &JobRow,
    view: &mut PipelineView,
    state: PipelineRenderState<'_>,
    action: &mut Option<PipelineAction>,
) {
    let thin = state
        .detail
        .map(|detail| detail.is_thin(row.plain_text.chars().count()))
        .unwrap_or_else(|| row.plain_text.chars().count() < DEFAULT_THIN_DESCRIPTION_CHARS);
    let evaluating = state.evaluating.is_some();
    let writing = state.writing_letter.is_some();
    let deleting = state.deleting == Some(row.id);
    let busy = state.refreshing == Some(row.id)
        || state.marking == Some(row.id)
        || state.queueing == Some(row.id)
        || deleting;

    ui.horizontal(|ui| {
        if ui
            .add_enabled(
                !evaluating,
                Button::new(if thin {
                    "Fetch description and evaluate"
                } else {
                    "Evaluate"
                }),
            )
            .on_hover_text("Send the posting and your CV to the configured model")
            .clicked()
        {
            *action = Some(PipelineAction::Evaluate {
                job_id: row.id,
                fetch_description_if_thin: thin,
                prompt_name: prompt_override(&view.eval_prompt, "evaluate"),
                model_override: model_override(&view.eval_model),
            });
        }
        if state.evaluating == Some(row.id) {
            LoadingSpinner::show(ui);
        }

        if ui
            .add_enabled(!writing, Button::new("Generate cover letter"))
            .on_hover_text("Write a letter from the posting and your CV")
            .clicked()
        {
            *action = Some(PipelineAction::GenerateCoverLetter {
                job_id: row.id,
                tone: non_empty(&view.letter_tone),
                prompt_name: prompt_override(&view.letter_prompt, "cover_letter"),
                model_override: model_override(&view.letter_model),
            });
        }
        if state.writing_letter == Some(row.id) {
            LoadingSpinner::show(ui);
        }

        ui.separator();

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
}

fn description_tab(ui: &mut Ui, row: &JobRow, view: &mut PipelineView) {
    if row.raw_text.trim().is_empty() {
        ui.label(RichText::new("No description stored.").weak());
        return;
    }

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

fn evaluation_tab(
    ui: &mut Ui,
    row: &JobRow,
    view: &mut PipelineView,
    state: PipelineRenderState<'_>,
    action: &mut Option<PipelineAction>,
) {
    if state.evaluating == Some(row.id) {
        ui.horizontal(|ui| {
            LoadingSpinner::show(ui);
            ui.label("Evaluating…");
            if ui.button("Stop").clicked() {
                *action = Some(PipelineAction::CancelEvaluation);
            }
        });
        if !state.stream.is_empty() {
            ui.add_space(4.0);
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.add(
                    egui::Label::new(RichText::new(state.stream).monospace().small())
                        .wrap()
                        .selectable(true),
                );
            });
        }
        return;
    }

    if state.evaluating.is_some() {
        ui.label(RichText::new("Another evaluation is running; wait for it or stop it.").weak());
    }

    // Re-evaluate options: blank means the configured defaults.
    labelled_input(
        ui,
        "Prompt override (blank = evaluate)",
        &mut view.eval_prompt,
        false,
    );
    labelled_input(
        ui,
        "Model override (blank = configured)",
        &mut view.eval_model,
        false,
    );

    let thin = state
        .detail
        .map(|detail| detail.is_thin(row.plain_text.chars().count()))
        .unwrap_or_else(|| row.plain_text.chars().count() < DEFAULT_THIN_DESCRIPTION_CHARS);
    if thin && ui.button("Evaluate without description").clicked() {
        *action = Some(PipelineAction::Evaluate {
            job_id: row.id,
            fetch_description_if_thin: false,
            prompt_name: prompt_override(&view.eval_prompt, "evaluate"),
            model_override: model_override(&view.eval_model),
        });
    }

    let Some(detail) = state.detail else {
        ui.add_space(8.0);
        if state.detail_loading {
            ui.horizontal(|ui| {
                LoadingSpinner::show(ui);
                ui.label("Loading evaluations…");
            });
        } else {
            ui.label(RichText::new("No evaluation yet.").weak());
        }
        return;
    };

    let selected = view
        .selected_eval
        .and_then(|id| detail.evaluation(id))
        .or_else(|| detail.latest_evaluation());

    ui.add_space(8.0);
    match selected {
        None => {
            ui.label(
                RichText::new("No evaluation yet. Evaluate this posting to see the report here.")
                    .weak(),
            );
        }
        Some(report) => {
            evaluation_summary(ui, report);
            ui.add_space(4.0);
            egui::ScrollArea::vertical()
                .id_salt("evaluation_report")
                .max_height(360.0)
                .show(ui, |ui| {
                    ui.add(
                        egui::Label::new(RichText::new(&report.content).monospace().small())
                            .wrap()
                            .selectable(true),
                    );
                });
        }
    }

    if detail.evaluations.len() > 1 || view.selected_eval.is_some() {
        ui.add_space(8.0);
        ui.label(RichText::new("History").strong());
        for report in &detail.evaluations {
            let current = selected.is_some_and(|s| s.id == report.id);
            let score = report
                .match_score
                .map(|score| format!("{score:.2}"))
                .unwrap_or_else(|| "—".into());
            let label = format!(
                "{} · {score} · {}",
                report.generated_at,
                report.recommendation.as_deref().unwrap_or("—")
            );
            if ui.selectable_label(current, label).clicked() {
                view.selected_eval = Some(report.id);
            }
            ui.label(
                RichText::new(format!(
                    "{} · prompt v{}",
                    report.model_used,
                    report.prompt_version.unwrap_or(0)
                ))
                .weak()
                .small(),
            );
        }
        if view.selected_eval.is_some() && ui.button("Show latest").clicked() {
            view.selected_eval = None;
        }
    }
}

fn evaluation_summary(ui: &mut Ui, report: &EvaluationRow) {
    ui.horizontal(|ui| {
        if let Some(score) = report.match_score {
            ui.label(RichText::new(format!("{score:.2}")).strong().size(20.0));
        }
        if let Some(recommendation) = &report.recommendation {
            ui.label(RichText::new(recommendation).strong());
        }
        ui.label(
            RichText::new(format!(
                "{} · {} · prompt v{}",
                report.model_used,
                report.generated_at,
                report.prompt_version.unwrap_or(0)
            ))
            .weak()
            .small(),
        );
    });
}

fn cover_letter_tab(
    ui: &mut Ui,
    row: &JobRow,
    view: &mut PipelineView,
    state: PipelineRenderState<'_>,
    action: &mut Option<PipelineAction>,
) {
    if state.writing_letter == Some(row.id) {
        ui.horizontal(|ui| {
            LoadingSpinner::show(ui);
            ui.label("Writing the cover letter…");
            if ui.button("Stop").clicked() {
                *action = Some(PipelineAction::CancelCoverLetter);
            }
        });
        return;
    }

    labelled_input(ui, "Tone (blank = direct)", &mut view.letter_tone, false);
    labelled_input(
        ui,
        "Prompt override (blank = cover_letter)",
        &mut view.letter_prompt,
        false,
    );
    labelled_input(
        ui,
        "Model override (blank = configured)",
        &mut view.letter_model,
        false,
    );
    if state.writing_letter.is_some() {
        ui.label(RichText::new("Another cover letter is being written.").weak());
    }

    let Some(detail) = state.detail else {
        ui.add_space(8.0);
        if state.detail_loading {
            ui.horizontal(|ui| {
                LoadingSpinner::show(ui);
                ui.label("Loading cover letters…");
            });
        } else {
            ui.label(RichText::new("No cover letter yet.").weak());
        }
        return;
    };

    let selected = view
        .selected_letter
        .and_then(|id| detail.letter(id))
        .or_else(|| detail.latest_letter());

    ui.add_space(8.0);
    match selected {
        None => {
            ui.label(
                RichText::new("No cover letter yet. Generate one from the action row above.")
                    .weak(),
            );
        }
        Some(letter) => {
            ui.label(
                RichText::new(format!(
                    "{} · {} · prompt v{}",
                    letter.model_used,
                    letter.generated_at,
                    letter.prompt_version.unwrap_or(0)
                ))
                .weak()
                .small(),
            );
            egui::Frame::group(ui.style()).show(ui, |ui| {
                ui.add(
                    egui::Label::new(RichText::new(&letter.content).monospace().small())
                        .wrap()
                        .selectable(true),
                );
            });
        }
    }

    if detail.cover_letters.len() > 1 || view.selected_letter.is_some() {
        ui.add_space(8.0);
        ui.label(RichText::new("Earlier letters").strong());
        for letter in &detail.cover_letters {
            let current = selected.is_some_and(|s| s.id == letter.id);
            let label = format!("{} · {}", letter.generated_at, letter.model_used);
            if ui.selectable_label(current, label).clicked() {
                view.selected_letter = Some(letter.id);
            }
        }
        if view.selected_letter.is_some() && ui.button("Show latest").clicked() {
            view.selected_letter = None;
        }
    }
}

/// A blank override means the pipeline default.
fn prompt_override(value: &str, default: &str) -> String {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        default.to_string()
    } else {
        trimmed.to_string()
    }
}

fn model_override(value: &str) -> Option<String> {
    non_empty(value)
}

fn non_empty(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
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

/// The confirmation in front of a triage batch.
///
/// The estimate is computed from the batch the modal captured, so the number
/// the user agrees to is the number the run sends. `Run triage` stays disabled
/// until the estimate lands.
fn triage_modal(
    ui: &mut Ui,
    view: &mut PipelineView,
    state: PipelineRenderState<'_>,
) -> Option<PipelineAction> {
    let job_ids = view.triage.as_ref()?.job_ids.clone();
    let mut decision = None;

    Window::new("Triage new jobs?")
        .collapsible(false)
        .resizable(false)
        .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
        .default_width(460.0)
        .show(ui.ctx(), |ui| {
            match state.triage_estimate {
                Some(estimate) => {
                    ui.label(format!(
                        "Triage {} jobs with {}.",
                        estimate.jobs, estimate.model
                    ));
                    ui.label(format!(
                        "About {} input tokens and {} output tokens; one call, a few seconds.",
                        estimate.input_tokens, estimate.estimated_output_tokens
                    ));
                    let cost = estimate
                        .estimated_cost_usd
                        .map(|cost| format!("Estimated cost: ~${cost:.4}."))
                        .unwrap_or_else(|| {
                            "Cost unknown: the catalog does not price this model.".into()
                        });
                    ui.label(cost);
                }
                None => {
                    ui.horizontal(|ui| {
                        LoadingSpinner::show(ui);
                        ui.label("Estimating the batch…");
                    });
                }
            }
            ui.add_space(4.0);
            ui.label(
                "Each decision moves a row in your queue. You can override any of them \
                 afterwards, and a skipped job stays visible under the Dismissed filter.",
            );
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(state.triage_estimate.is_some(), Button::new("Run triage"))
                    .clicked()
                {
                    decision = Some(true);
                }
                if ui.button("Cancel").clicked() {
                    decision = Some(false);
                }
            });
        });

    match decision {
        Some(true) => Some(PipelineAction::RunTriage { job_ids }),
        Some(false) => {
            view.triage = None;
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

/// The triage decision beside the queue badge.
fn triage_badge(ui: &mut Ui, choice: TriageChoice) {
    let color = match choice {
        TriageChoice::WorthDetails => Color32::from_rgb(0x1f, 0x6f, 0xeb),
        TriageChoice::Evaluate => Color32::from_rgb(0x7c, 0x3a, 0xed),
        TriageChoice::Skip => Color32::from_rgb(0x6b, 0x72, 0x80),
        TriageChoice::Unsure => Color32::from_rgb(0xc9, 0x8a, 0x00),
    };
    ui.label(RichText::new(choice.label()).small().color(color));
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
