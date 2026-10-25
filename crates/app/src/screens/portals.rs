//! Portals screen.
//!
//! The screen is a pure view: it renders a `PortalsViewModel` and returns a
//! `PortalsAction`. `CareerOpsApp` owns the database, fetch source, browser
//! backend, and async tasks.
//!
//! The add/edit/delete/manual dialogs live in `PortalsView` and render through
//! `show_modal`, which the app calls after the active screen. That keeps the
//! manual-entry dialog reachable from Pipeline without a second copy.
//!
//! The health badge uses `StatusBadge`; the drill-down text is
//! `PortalHealth::suggested_action`, rendered directly rather than rephrased in
//! the view.

use egui::{Align2, Button, Color32, RichText, Ui, Window};
use url::Url;

use crate::view_models::portals::{
    BatchProgress, HealthBadge, PortalRow, PortalsViewModel, SidecarStatus,
};

use super::components::{
    labelled_input, labelled_multiline, EmptyState, LoadingSpinner, StatusBadge,
};

#[derive(Default)]
pub struct PortalsView {
    pub modal: Option<PortalModal>,
}

impl PortalsView {
    /// Open the manual-entry dialog.
    ///
    /// Both Portals and Pipeline expose "Add job", so the dialog lives on the
    /// shared view state rather than on either screen.
    pub fn open_manual(&mut self) {
        self.modal = Some(PortalModal::Manual(ManualEntryView::default()));
    }

    pub fn manual_mut(&mut self) -> Option<&mut ManualEntryView> {
        match &mut self.modal {
            Some(PortalModal::Manual(manual)) => Some(manual),
            _ => None,
        }
    }

    pub fn manual_result(&self) -> Option<&ManualSuccess> {
        match &self.modal {
            Some(PortalModal::Manual(manual)) => manual.result.as_ref(),
            _ => None,
        }
    }

    pub fn set_form_error(&mut self, error: String) {
        match &mut self.modal {
            Some(PortalModal::Add(form)) | Some(PortalModal::Edit { form, .. }) => {
                form.error = Some(error);
            }
            _ => {}
        }
    }
}

pub enum PortalModal {
    Add(PortalForm),
    Edit { id: i64, form: PortalForm },
    Delete { id: i64, name: String },
    Manual(ManualEntryView),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RenderModeChoice {
    #[default]
    Static,
    Browser,
}

impl RenderModeChoice {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Static => "static",
            Self::Browser => "browser",
        }
    }

    fn from_str(value: &str) -> Self {
        if value == "browser" {
            Self::Browser
        } else {
            Self::Static
        }
    }

    fn label(self) -> &'static str {
        match self {
            Self::Static => "Static (Tier 1)",
            Self::Browser => "Browser (Tier 2)",
        }
    }
}

#[derive(Debug, Clone)]
pub struct PortalForm {
    pub company_name: String,
    pub url: String,
    pub render_mode: RenderModeChoice,
    pub scraper_config: Option<String>,
    pub error: Option<String>,
}

impl Default for PortalForm {
    fn default() -> Self {
        Self {
            company_name: String::new(),
            url: String::new(),
            render_mode: RenderModeChoice::Static,
            scraper_config: None,
            error: None,
        }
    }
}

impl PortalForm {
    pub fn from_row(row: &PortalRow) -> Self {
        Self {
            company_name: row.name.clone(),
            url: row.url.clone(),
            render_mode: RenderModeChoice::from_str(&row.render_mode),
            scraper_config: row.scraper_config.clone(),
            error: None,
        }
    }

    pub fn valid(&self) -> bool {
        !self.company_name.trim().is_empty() && valid_http_url(&self.url)
    }

    pub fn data(&self) -> PortalFormData {
        PortalFormData {
            company_name: self.company_name.trim().to_string(),
            url: self.url.trim().to_string(),
            render_mode: self.render_mode.as_str().to_string(),
            scraper_config: self.scraper_config.clone(),
        }
    }
}

fn valid_http_url(raw: &str) -> bool {
    Url::parse(raw.trim())
        .map(|url| matches!(url.scheme(), "http" | "https"))
        .unwrap_or(false)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PortalFormData {
    pub company_name: String,
    pub url: String,
    pub render_mode: String,
    pub scraper_config: Option<String>,
}

#[derive(Default)]
pub struct ManualEntryView {
    pub tab: ManualTab,
    pub url: String,
    pub title: String,
    pub company: String,
    pub body: String,
    pub error: Option<String>,
    pub result: Option<ManualSuccess>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ManualTab {
    #[default]
    FromUrl,
    PasteText,
}

#[derive(Debug, Clone)]
pub struct ManualSuccess {
    pub title: String,
    pub company: String,
    pub detail: String,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PortalsAction {
    Reload,
    OpenAdd,
    OpenEdit(i64),
    OpenDelete(i64),
    OpenManual,
    CloseModal,
    CreatePortal(PortalFormData),
    UpdatePortal {
        id: i64,
        data: PortalFormData,
    },
    DeletePortal(i64),
    FetchOne(i64),
    FetchAll,
    CancelBatch,
    DismissBatch,
    ManualFromUrl(String),
    ManualFromText {
        title: String,
        company: String,
        body: String,
    },
    ShowInPipeline,
}

#[derive(Debug, Clone, Copy)]
pub struct PortalsRenderState<'a> {
    pub loading: bool,
    pub busy: bool,
    pub manual_busy: bool,
    /// A single `Fetch now` request for this portal is in flight.
    pub fetching_portal: Option<i64>,
    pub error: Option<&'a str>,
    pub message: Option<&'a str>,
}

impl PortalsRenderState<'_> {
    /// The subset the shared modal needs.
    ///
    /// The modal is app-level chrome: it is reachable from Pipeline as well as
    /// Portals, so the app renders it after whichever screen is active.
    pub fn modal_state(self) -> ModalState {
        ModalState {
            busy: self.busy,
            manual_busy: self.manual_busy,
        }
    }
}

/// The render state the add/edit/delete/manual dialogs need.
#[derive(Debug, Clone, Copy, Default)]
pub struct ModalState {
    /// A portal task is in flight; form and delete actions are disabled.
    pub busy: bool,
    /// A manual add is in flight; the manual dialog's submit button is disabled.
    pub manual_busy: bool,
}

pub fn show(
    ui: &mut Ui,
    view_model: &PortalsViewModel,
    state: PortalsRenderState<'_>,
) -> Option<PortalsAction> {
    let mut action = None;

    header(ui, view_model, state, &mut action);

    if let Some(error) = state.error {
        ui.horizontal(|ui| {
            ui.colored_label(Color32::from_rgb(0xc0, 0x39, 0x2b), error);
            if ui.button("Retry").clicked() {
                action = Some(PortalsAction::Reload);
            }
        });
    }
    if let Some(message) = state.message {
        ui.label(RichText::new(message).weak());
    }

    ui.separator();
    body(ui, view_model, state, &mut action);

    // Modal rendering is deliberately not part of this screen. See
    // `show_modal`.
    action
}

/// Render the shared modal, if one is open.
///
/// `CareerOpsApp` calls this after the active screen so the manual-entry dialog
/// is available from Portals and Pipeline alike. Rendering it here rather than
/// inside `show` keeps the dialog reachable while another screen is selected.
pub fn show_modal(ui: &mut Ui, view: &mut PortalsView, state: ModalState) -> Option<PortalsAction> {
    let mut action = None;
    modals(ui, view, state, &mut action);
    action
}

fn header(
    ui: &mut Ui,
    view_model: &PortalsViewModel,
    state: PortalsRenderState<'_>,
    action: &mut Option<PortalsAction>,
) {
    ui.horizontal(|ui| {
        if ui
            .add_enabled(!state.busy, Button::new("Add portal"))
            .clicked()
        {
            *action = Some(PortalsAction::OpenAdd);
        }
        if ui
            .add_enabled(!state.busy, Button::new("Add job"))
            .clicked()
        {
            *action = Some(PortalsAction::OpenManual);
        }

        let can_fetch_all = !state.busy && !view_model.rows.is_empty();
        if ui
            .add_enabled(can_fetch_all, Button::new("Fetch all"))
            .clicked()
        {
            *action = Some(PortalsAction::FetchAll);
        }
        if ui
            .add_enabled(!state.busy, Button::new("Refresh"))
            .clicked()
        {
            *action = Some(PortalsAction::Reload);
        }

        ui.separator();
        ui.label("Sidecar:");
        sidecar_badge(view_model.sidecar).show(ui);

        if state.loading {
            LoadingSpinner::show(ui);
        }
    });

    if let Some(batch) = &view_model.batch {
        batch_bar(ui, batch, state, action);
    }
}

fn batch_bar(
    ui: &mut Ui,
    batch: &BatchProgress,
    state: PortalsRenderState<'_>,
    action: &mut Option<PortalsAction>,
) {
    let fraction = if batch.total == 0 {
        0.0
    } else {
        (batch.completed.len() as f32 / batch.total as f32).min(1.0)
    };

    let text = if batch.cancelled {
        format!(
            "Cancelled after {} / {}",
            batch.completed.len(),
            batch.total
        )
    } else {
        format!("{} / {}", batch.completed.len(), batch.total)
    };

    ui.horizontal(|ui| {
        ui.add(egui::ProgressBar::new(fraction).text(text));

        let running = state.busy && !batch.cancelled && batch.completed.len() < batch.total;
        if running {
            if ui.button("Cancel").clicked() {
                *action = Some(PortalsAction::CancelBatch);
            }
        } else if ui.button("Dismiss").clicked() {
            *action = Some(PortalsAction::DismissBatch);
        }
    });
}

fn body(
    ui: &mut Ui,
    view_model: &PortalsViewModel,
    state: PortalsRenderState<'_>,
    action: &mut Option<PortalsAction>,
) {
    if state.loading && view_model.rows.is_empty() {
        ui.horizontal(|ui| {
            LoadingSpinner::show(ui);
            ui.label("Loading portals...");
        });
        return;
    }

    if view_model.rows.is_empty() {
        if EmptyState::new(
            "(o)",
            "No portals configured yet",
            "Add a company job board to start fetching listings.",
        )
        .with_action("Add your first portal")
        .show(ui)
        {
            *action = Some(PortalsAction::OpenAdd);
        }
        return;
    }

    egui::ScrollArea::vertical().show(ui, |ui| {
        for row in &view_model.rows {
            row_ui(ui, row, view_model, state, action);
        }
    });
}

fn row_ui(
    ui: &mut Ui,
    row: &PortalRow,
    view_model: &PortalsViewModel,
    state: PortalsRenderState<'_>,
    action: &mut Option<PortalsAction>,
) {
    ui.horizontal(|ui| {
        ui.add_sized(
            [160.0, 18.0],
            egui::Label::new(RichText::new(&row.name).strong()).truncate(),
        );
        ui.add_sized(
            [240.0, 18.0],
            egui::Label::new(RichText::new(&row.url).weak()).truncate(),
        );

        let in_flight = state.fetching_portal == Some(row.id)
            || view_model.batch.as_ref().and_then(|batch| batch.in_flight) == Some(row.id);

        if in_flight {
            LoadingSpinner::show(ui);
        } else {
            health_badge(ui, &row.health);
        }

        match &row.last_attempt {
            Some(last_attempt) => ui.label(RichText::new(last_attempt).weak()),
            None => ui.label(RichText::new("Never fetched").weak()),
        };

        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.add_enabled(!state.busy, Button::new("Delete")).clicked() {
                *action = Some(PortalsAction::OpenDelete(row.id));
            }
            if ui.add_enabled(!state.busy, Button::new("Edit")).clicked() {
                *action = Some(PortalsAction::OpenEdit(row.id));
            }
            if ui.add_enabled(!state.busy, Button::new("Fetch")).clicked() {
                *action = Some(PortalsAction::FetchOne(row.id));
            }
        });
    });

    if let Some(suggested) = &row.suggested_action {
        ui.label(RichText::new(suggested).weak().small());
    }
    ui.separator();
}

fn health_badge(ui: &mut Ui, health: &HealthBadge) {
    let badge = match health {
        HealthBadge::NeverFetched => StatusBadge::NeverFetched,
        HealthBadge::Healthy => StatusBadge::Healthy,
        HealthBadge::Degraded { .. } => StatusBadge::Degraded,
        HealthBadge::Broken { .. } => StatusBadge::Broken,
    };

    let response = badge.show(ui);
    if let Some(cause) = health.cause() {
        response.on_hover_text(format!("Cause: {cause}"));
    }
}

fn sidecar_badge(status: SidecarStatus) -> StatusBadge {
    match status {
        SidecarStatus::NotStarted => StatusBadge::NotStarted,
        SidecarStatus::Running => StatusBadge::Running,
        SidecarStatus::Stopped => StatusBadge::Stopped,
    }
}

fn modals(
    ui: &mut Ui,
    view: &mut PortalsView,
    state: ModalState,
    action: &mut Option<PortalsAction>,
) {
    match &mut view.modal {
        None => {}
        Some(PortalModal::Add(form)) => {
            portal_form_modal(ui, "New portal", form, None, state, action);
        }
        Some(PortalModal::Edit { id, form }) => {
            let id = *id;
            portal_form_modal(ui, "Edit portal", form, Some(id), state, action);
        }
        Some(PortalModal::Delete { id, name }) => {
            let id = *id;
            delete_modal(ui, id, name, state, action);
        }
        Some(PortalModal::Manual(manual)) => {
            manual_modal(ui, manual, state, action);
        }
    }
}

fn portal_form_modal(
    ui: &mut Ui,
    title: &str,
    form: &mut PortalForm,
    id: Option<i64>,
    state: ModalState,
    action: &mut Option<PortalsAction>,
) {
    Window::new(title)
        .collapsible(false)
        .resizable(false)
        .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
        .default_width(480.0)
        .show(ui.ctx(), |ui| {
            labelled_input(ui, "Company name", &mut form.company_name, false);
            labelled_input(ui, "Job board URL", &mut form.url, false);

            egui::ComboBox::from_label("Render mode")
                .selected_text(form.render_mode.label())
                .show_ui(ui, |ui| {
                    ui.selectable_value(
                        &mut form.render_mode,
                        RenderModeChoice::Static,
                        RenderModeChoice::Static.label(),
                    );
                    ui.selectable_value(
                        &mut form.render_mode,
                        RenderModeChoice::Browser,
                        RenderModeChoice::Browser.label(),
                    );
                });

            if !form.url.trim().is_empty() && !valid_http_url(&form.url) {
                ui.colored_label(
                    Color32::from_rgb(0xc0, 0x39, 0x2b),
                    "Enter a valid http(s) URL.",
                );
            }
            if let Some(error) = &form.error {
                ui.colored_label(Color32::from_rgb(0xc0, 0x39, 0x2b), error.as_str());
            }

            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let can_save = form.valid() && !state.busy;
                if ui
                    .add_enabled(can_save, Button::new("Save portal"))
                    .clicked()
                {
                    let data = form.data();
                    *action = Some(match id {
                        Some(id) => PortalsAction::UpdatePortal { id, data },
                        None => PortalsAction::CreatePortal(data),
                    });
                }
                if ui.add_enabled(!state.busy, Button::new("Cancel")).clicked() {
                    *action = Some(PortalsAction::CloseModal);
                }
                if state.busy {
                    LoadingSpinner::show(ui);
                }
            });
        });
}

fn delete_modal(
    ui: &mut Ui,
    id: i64,
    name: &str,
    state: ModalState,
    action: &mut Option<PortalsAction>,
) {
    Window::new("Delete portal?")
        .collapsible(false)
        .resizable(false)
        .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
        .default_width(420.0)
        .show(ui.ctx(), |ui| {
            ui.label(format!("Delete {name}?"));
            ui.label(
                "This removes the portal configuration. Job listings already fetched from it \
                 are not deleted.",
            );
            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(!state.busy, Button::new("Delete portal"))
                    .clicked()
                {
                    *action = Some(PortalsAction::DeletePortal(id));
                }
                if ui.add_enabled(!state.busy, Button::new("Cancel")).clicked() {
                    *action = Some(PortalsAction::CloseModal);
                }
                if state.busy {
                    LoadingSpinner::show(ui);
                }
            });
        });
}

fn manual_modal(
    ui: &mut Ui,
    manual: &mut ManualEntryView,
    state: ModalState,
    action: &mut Option<PortalsAction>,
) {
    Window::new("Add a job")
        .collapsible(false)
        .resizable(false)
        .anchor(Align2::CENTER_CENTER, [0.0, 0.0])
        .default_width(520.0)
        .show(ui.ctx(), |ui| {
            ui.horizontal(|ui| {
                if ui
                    .selectable_label(manual.tab == ManualTab::FromUrl, "From URL")
                    .clicked()
                {
                    manual.tab = ManualTab::FromUrl;
                    manual.error = None;
                    manual.result = None;
                }
                if ui
                    .selectable_label(manual.tab == ManualTab::PasteText, "Paste text")
                    .clicked()
                {
                    manual.tab = ManualTab::PasteText;
                    manual.error = None;
                    manual.result = None;
                }
            });
            ui.separator();

            match manual.tab {
                ManualTab::FromUrl => {
                    labelled_input(ui, "Job URL", &mut manual.url, false);
                    let can_fetch = !manual.url.trim().is_empty() && !state.manual_busy;
                    if ui
                        .add_enabled(can_fetch, Button::new("Fetch and add"))
                        .clicked()
                    {
                        *action = Some(PortalsAction::ManualFromUrl(manual.url.trim().to_string()));
                    }
                }
                ManualTab::PasteText => {
                    labelled_input(ui, "Job title", &mut manual.title, false);
                    labelled_input(ui, "Company", &mut manual.company, false);
                    labelled_multiline(ui, "Job description", &mut manual.body, 8);

                    let can_add = !manual.title.trim().is_empty()
                        && !manual.body.trim().is_empty()
                        && !state.manual_busy;
                    if ui
                        .add_enabled(can_add, Button::new("Add pasted job"))
                        .clicked()
                    {
                        *action = Some(PortalsAction::ManualFromText {
                            title: manual.title.trim().to_string(),
                            company: manual.company.trim().to_string(),
                            body: manual.body.clone(),
                        });
                    }
                }
            }

            if let Some(error) = &manual.error {
                ui.add_space(8.0);
                ui.colored_label(Color32::from_rgb(0xc0, 0x39, 0x2b), error.as_str());
            }

            if let Some(result) = &manual.result {
                ui.add_space(8.0);
                ui.heading("Job added");
                ui.label(RichText::new(format!("{} at {}", result.title, result.company)).strong());
                ui.label(RichText::new(&result.detail).weak());
                for warning in &result.warnings {
                    ui.label(RichText::new(warning).weak().small());
                }
            }

            ui.add_space(8.0);
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(!state.manual_busy, Button::new("Close"))
                    .clicked()
                {
                    *action = Some(PortalsAction::CloseModal);
                }
                if manual.result.is_some() && ui.button("View in Pipeline").clicked() {
                    *action = Some(PortalsAction::ShowInPipeline);
                }
                if state.manual_busy {
                    LoadingSpinner::show(ui);
                }
            });
        });
}
