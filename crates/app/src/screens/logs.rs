//! The Logs screen and the bottom drawer.
//!
//! Both render the same [`LogsView`] over the same snapshot; they differ only in
//! how much chrome they show. The screen is for reading and searching, the
//! drawer for watching a fetch while it runs — which is the whole reason the
//! drawer exists.
//!
//! The screen never reads the log file. It renders lines the app already
//! captured, so filtering and pausing work on structure instead of text.

use career_ops_core::logging::LogLine;
use egui::{Color32, RichText, ScrollArea, TextWrapMode, Ui};
use tracing::Level;

use super::components::{labelled_input, EmptyState};
use crate::view_models::logs::{LevelFilter, LogsMount, LogsView};

/// The offset that means "the end" without being infinite.
///
/// egui clamps a scroll offset to the content, so any value above the content
/// height lands at the bottom. `f32::MAX` is not usable: `content - MAX` is
/// `-inf`, and the resulting NaN rect panics the layout.
const JUMP_OFFSET: f32 = 1_000_000.0;

/// What the app has to do because a screen cannot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LogsAction {
    /// Open the log directory in the OS file manager.
    OpenFolder,
    /// Ask first: the file is the durable record.
    RequestDelete,
    ConfirmDelete,
    CancelDelete,
    /// The drawer's **Open screen**.
    OpenScreen,
    /// The drawer's **Close**.
    CloseDrawer,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct LogsRenderState<'a> {
    /// The file being written, when this session has one.
    pub path: Option<&'a str>,
    /// Where the capture level came from, so the screen can say so.
    pub policy: &'a str,
    /// The newest sequence number, for the pause watermark.
    pub latest_seq: u64,
    /// Lines the bounded buffer has evicted this session.
    pub dropped: u64,
    pub confirming_delete: bool,
    pub message: Option<&'a str>,
}

pub fn show(
    ui: &mut Ui,
    lines: &[LogLine],
    view: &mut LogsView,
    state: LogsRenderState<'_>,
) -> Option<LogsAction> {
    let mut action = None;

    filters(ui, view);
    actions(ui, lines, view, state, &mut action);

    ui.label(
        RichText::new(match state.path {
            Some(path) => format!("Capturing at {} · {path}", state.policy),
            None => format!(
                "Capturing at {} · this session is in memory only",
                state.policy
            ),
        })
        .weak()
        .small(),
    );

    if let Some(message) = state.message {
        ui.label(RichText::new(message).weak());
    }

    if state.confirming_delete {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new("Delete the log file and its archives? This cannot be undone.")
                    .color(Color32::from_rgb(0xc0, 0x39, 0x2b)),
            );
            if ui.button("Delete permanently").clicked() {
                action = Some(LogsAction::ConfirmDelete);
            }
            if ui.button("Cancel").clicked() {
                action = Some(LogsAction::CancelDelete);
            }
        });
    }

    ui.separator();
    dropped_marker(ui, state);
    list(ui, lines, view, LogsMount::Screen);

    action
}

/// The drawer: one row of the controls that matter while watching a walk.
pub fn drawer(
    ui: &mut Ui,
    lines: &[LogLine],
    view: &mut LogsView,
    state: LogsRenderState<'_>,
) -> Option<LogsAction> {
    let mut action = None;

    ui.horizontal(|ui| {
        ui.label(RichText::new("Logs").strong());
        for &level in LevelFilter::all() {
            if ui
                .selectable_label(view.level == level, level.label())
                .clicked()
            {
                view.level = level;
            }
        }
        ui.separator();
        pause_button(ui, view, state);
        if ui.selectable_label(view.follow(), "Follow").clicked() {
            view.toggle_follow();
        }
        ui.separator();
        if ui.button("Open screen").clicked() {
            action = Some(LogsAction::OpenScreen);
        }
        if ui.button("Close").clicked() {
            action = Some(LogsAction::CloseDrawer);
        }
    });

    dropped_marker(ui, state);
    list(ui, lines, view, LogsMount::Drawer);

    action
}

fn filters(ui: &mut Ui, view: &mut LogsView) {
    ui.horizontal(|ui| {
        for &level in LevelFilter::all() {
            if ui
                .selectable_label(view.level == level, level.label())
                .clicked()
            {
                view.level = level;
            }
        }

        ui.separator();
        // The screen owns this state, so the field edits in place; no action
        // round trip is needed.
        labelled_input(ui, "Filter", &mut view.query, false);
        if !view.query.is_empty() && ui.button("Clear filter").clicked() {
            view.query.clear();
        }
    });
}

fn actions(
    ui: &mut Ui,
    lines: &[LogLine],
    view: &mut LogsView,
    state: LogsRenderState<'_>,
    action: &mut Option<LogsAction>,
) {
    ui.horizontal(|ui| {
        pause_button(ui, view, state);

        if ui.selectable_label(view.follow(), "Follow").clicked() {
            view.toggle_follow();
        }

        if ui.button("Copy visible").clicked() {
            ui.ctx().copy_text(view.to_plain_text(lines));
        }

        ui.separator();
        if ui
            .add_enabled(state.path.is_some(), egui::Button::new("Open folder"))
            .on_hover_text("Show the log file in the file manager")
            .clicked()
        {
            *action = Some(LogsAction::OpenFolder);
        }
        if ui
            .add_enabled(!state.confirming_delete, egui::Button::new("Delete logs"))
            .clicked()
        {
            *action = Some(LogsAction::RequestDelete);
        }
    });
}

fn pause_button(ui: &mut Ui, view: &mut LogsView, state: LogsRenderState<'_>) {
    if ui
        .button(if view.is_paused() { "Resume" } else { "Pause" })
        .clicked()
    {
        view.toggle_pause(state.latest_seq);
    }
    if view.is_paused() {
        let new = view.new_since(state.latest_seq);
        let label = match new {
            1 => "1 new line".to_string(),
            0 => "paused".to_string(),
            count => format!("{count} new lines"),
        };
        ui.label(RichText::new(label).weak());
    }
}

/// The buffer is bounded, so it has to say what it lost.
fn dropped_marker(ui: &mut Ui, state: LogsRenderState<'_>) {
    if state.dropped > 0 {
        ui.label(
            RichText::new(format!(
                "{} earlier line(s) dropped from the in-memory buffer",
                state.dropped
            ))
            .weak()
            .small(),
        );
    }
}

fn list(ui: &mut Ui, lines: &[LogLine], view: &mut LogsView, mount: LogsMount) {
    let visible = view.visible(lines);
    if visible.is_empty() {
        if lines.is_empty() {
            EmptyState::new(
                ">_",
                "No log lines yet",
                "Lines appear here as the app runs: a fetch, a walk, an error.",
            )
            .show(ui);
        } else {
            ui.label(RichText::new("No lines match this filter.").weak());
        }
        return;
    }

    let jumped = view.take_follow_jump(mount);
    let mut area = ScrollArea::both().auto_shrink([false, false]);
    if view.follow() && !view.is_paused() {
        area = area.stick_to_bottom(true);
    }
    if jumped {
        // `stick_to_bottom` only keeps a view that is *already* at the end —
        // egui enters sticky mode from the current offset — so turning Follow
        // on has to ask for the jump. The offset is clamped to the content, and
        // it is a finite constant rather than `f32::MAX`: `content - MAX`
        // overflows to `-inf` and egui panics on the resulting NaN rect.
        area = area.vertical_scroll_offset(JUMP_OFFSET);
    }

    let output = area.show(ui, |ui| {
        for line in &visible {
            // One label per line, unwrapped: the horizontal scrollbar is the
            // reading surface, and a raw line must not be reflowed.
            ui.add(
                egui::Label::new(
                    RichText::new(line.compact())
                        .monospace()
                        .color(level_color(ui, line.level)),
                )
                .wrap_mode(TextWrapMode::Extend),
            );
        }
    });

    // Scrolling up is what takes the user out of follow mode; the Follow chip is
    // how they come back. A jump still in flight is not "the user scrolled
    // away", so `observe_scroll` settles it instead of ending follow mode.
    let at_bottom =
        output.state.clamped_offset().y + output.inner_rect.height() + 4.0 >= output.content_size.y;
    view.observe_scroll(mount, at_bottom);
}

fn level_color(ui: &Ui, level: Level) -> Color32 {
    match level {
        Level::ERROR => Color32::from_rgb(0xc0, 0x39, 0x2b),
        Level::WARN => Color32::from_rgb(0xc9, 0x8a, 0x00),
        Level::INFO => ui.visuals().text_color(),
        _ => ui.visuals().weak_text_color(),
    }
}
