//! Shared egui components.
//!
//! These are the reusable building blocks Phase 3, 4, and 5 add screens
//! against. Keeping them here means a behaviour fix (for example the
//! accessible-label workaround in `labelled_input`) lands once.

use egui::{Color32, Response, RichText, Ui};

/// A centered message for an empty list or an unconfigured screen.
///
/// The optional action returns `true` from `show` in the frame it is clicked.
/// Callers that do not want an action simply omit `with_action`.
pub struct EmptyState<'a> {
    icon: &'a str,
    title: &'a str,
    message: &'a str,
    action: Option<&'a str>,
}

impl<'a> EmptyState<'a> {
    pub fn new(icon: &'a str, title: &'a str, message: &'a str) -> Self {
        Self {
            icon,
            title,
            message,
            action: None,
        }
    }

    pub fn with_action(mut self, label: &'a str) -> Self {
        self.action = Some(label);
        self
    }

    pub fn show(self, ui: &mut Ui) -> bool {
        ui.vertical_centered(|ui| {
            ui.add_space(32.0);
            ui.label(RichText::new(self.icon).monospace().size(32.0));
            ui.add_space(4.0);
            ui.label(RichText::new(self.title).strong().size(16.0));
            ui.label(RichText::new(self.message).weak());

            if let Some(action) = self.action {
                ui.add_space(12.0);
                ui.button(action).clicked()
            } else {
                false
            }
        })
        .inner
    }
}

/// A compact status indicator.
///
/// The variants deliberately mirror the states the application has today:
/// portal health (`NeverFetched`, `Healthy`, `Degraded`, `Broken`) and the
/// coarse sidecar state (`NotStarted`, `Running`, `Stopped`). The view layer
/// maps its own enums onto this type rather than inventing a second palette.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusBadge {
    NeverFetched,
    Healthy,
    Degraded,
    Broken,
    NotStarted,
    Running,
    Stopped,
}

impl StatusBadge {
    pub fn label(self) -> &'static str {
        match self {
            Self::NeverFetched => "Never fetched",
            Self::Healthy => "Healthy",
            Self::Degraded => "Degraded",
            Self::Broken => "Broken",
            Self::NotStarted => "Not started",
            Self::Running => "Running",
            Self::Stopped => "Stopped",
        }
    }

    fn color(self) -> Color32 {
        match self {
            Self::Healthy | Self::Running => Color32::from_rgb(0x2e, 0xa0, 0x43),
            Self::Degraded => Color32::from_rgb(0xc9, 0x8a, 0x00),
            Self::Broken | Self::Stopped => Color32::from_rgb(0xc0, 0x39, 0x2b),
            Self::NeverFetched | Self::NotStarted => Color32::GRAY,
        }
    }

    pub fn show(self, ui: &mut Ui) -> Response {
        ui.horizontal(|ui| {
            ui.label(RichText::new("•").monospace().color(self.color()));
            ui.label(RichText::new(self.label()).color(self.color()));
        })
        .response
    }
}

/// A spinner with the project's spacing.
pub struct LoadingSpinner;

impl LoadingSpinner {
    pub fn show(ui: &mut Ui) -> Response {
        ui.add(egui::Spinner::new().size(16.0))
    }
}

/// A labelled text input.
///
/// `TextEdit::hint_text` is visual only and does not name the field in the
/// accessibility tree. Rendering a visible label and associating it with
/// `Response::labelled_by` gives screen readers (and `egui_kittest`'s
/// `get_by_label`) a name to query. All text inputs in the app should go
/// through this helper so the pattern exists in exactly one place.
///
/// The label is rendered above the field. Callers that need a horizontal
/// layout can wrap the field itself; the association does not depend on
/// placement.
pub fn labelled_input(ui: &mut Ui, label: &str, value: &mut String, password: bool) -> Response {
    let label_response = ui.label(label);

    let mut edit = egui::TextEdit::singleline(value);
    if password {
        edit = edit.password(true);
    }

    ui.add(edit).labelled_by(label_response.id)
}

/// A labelled multi-line text input, for job descriptions and other bodies.
pub fn labelled_multiline(ui: &mut Ui, label: &str, value: &mut String, rows: usize) -> Response {
    let label_response = ui.label(label);
    ui.add(egui::TextEdit::multiline(value).desired_rows(rows))
        .labelled_by(label_response.id)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_status_badge_has_a_unique_label() {
        let badges = [
            StatusBadge::NeverFetched,
            StatusBadge::Healthy,
            StatusBadge::Degraded,
            StatusBadge::Broken,
            StatusBadge::NotStarted,
            StatusBadge::Running,
            StatusBadge::Stopped,
        ];

        let labels: Vec<&str> = badges.iter().map(|badge| badge.label()).collect();
        let unique: std::collections::HashSet<&str> = labels.iter().copied().collect();
        assert_eq!(labels.len(), unique.len(), "duplicate labels: {labels:?}");
    }
}
