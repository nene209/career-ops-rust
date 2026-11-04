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

use career_ops_core::acquisition::config::{Provider, ScraperConfig};
use career_ops_core::acquisition::known_sites::{self, KnownSite};

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

    /// The add/edit form, whichever kind of modal is open.
    pub fn form_mut(&mut self) -> Option<&mut PortalForm> {
        match &mut self.modal {
            Some(PortalModal::Add(form)) | Some(PortalModal::Edit { form, .. }) => Some(form),
            _ => None,
        }
    }

    pub fn form(&self) -> Option<&PortalForm> {
        match &self.modal {
            Some(PortalModal::Add(form)) | Some(PortalModal::Edit { form, .. }) => Some(form),
            _ => None,
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
    /// The fetch-and-parse engine. `Selectors` is the Phase 2.2 default.
    pub provider: Provider,
    /// Selector config JSON. Shown and stored when `provider == Selectors`.
    pub scraper_config: String,
    /// Eightfold API domain. Shown and stored when `provider == Eightfold`.
    pub domain: String,
    /// Extra Eightfold JSON (pagination/detail caps). The provider and domain
    /// come from the fields above and are written on save.
    pub eightfold_config: String,
    pub error: Option<String>,
    /// The result of the last "Test config" run for this form.
    pub preview: PreviewState,
    /// The known site whose config was already applied. Auto-prefill runs once
    /// per site, so it can never overwrite an edit.
    pub prefilled_for: Option<String>,
}

impl Default for PortalForm {
    fn default() -> Self {
        Self {
            company_name: String::new(),
            url: String::new(),
            render_mode: RenderModeChoice::Static,
            provider: Provider::Selectors,
            scraper_config: String::new(),
            domain: String::new(),
            eightfold_config: String::new(),
            error: None,
            preview: PreviewState::default(),
            prefilled_for: None,
        }
    }
}

impl PortalForm {
    pub fn from_row(row: &PortalRow) -> Self {
        let raw = row.scraper_config.clone().unwrap_or_default();
        let mut form = Self {
            company_name: row.name.clone(),
            url: row.url.clone(),
            render_mode: RenderModeChoice::from_str(&row.render_mode),
            scraper_config: raw.clone(),
            ..Self::default()
        };

        // An unparsable config stays in the selectors field so the user can fix
        // it; the fetch classifies it if they save without doing so.
        if let Ok(ScraperConfig::Eightfold(config)) = ScraperConfig::parse(&raw) {
            form.provider = Provider::Eightfold;
            form.domain = config.domain;
            form.eightfold_config = eightfold_extras(&raw);
            form.scraper_config.clear();
        }

        form
    }

    pub fn valid(&self) -> bool {
        !self.company_name.trim().is_empty() && valid_http_url(&self.url)
    }

    /// Whether the provider's own fields are usable. Gates Save and Test config
    /// in addition to `valid`, which stays the URL/company gate.
    pub fn provider_fields_valid(&self) -> bool {
        match self.provider {
            Provider::Selectors => {
                self.valid_selectors_config() && self.selectors_provider_conflict().is_none()
            }
            Provider::Eightfold => !self.domain.trim().is_empty() && self.valid_eightfold_config(),
        }
    }

    /// Empty is valid (no config). A non-empty config must be a JSON object;
    /// the app parses it again on submit and returns a detailed error if it fails.
    pub fn valid_scraper_config(&self) -> bool {
        match self.provider {
            Provider::Selectors => self.valid_selectors_config(),
            Provider::Eightfold => self.valid_eightfold_config(),
        }
    }

    pub fn valid_selectors_config(&self) -> bool {
        let raw = self.scraper_config.trim();
        raw.is_empty()
            || serde_json::from_str::<serde_json::Value>(raw)
                .map(|value| value.is_object())
                .unwrap_or(false)
    }

    pub fn valid_eightfold_config(&self) -> bool {
        let raw = self.eightfold_config.trim();
        raw.is_empty()
            || serde_json::from_str::<serde_json::Value>(raw)
                .map(|value| value.is_object())
                .unwrap_or(false)
    }

    /// A selectors config that names another provider would silently dispatch
    /// elsewhere, so the form refuses it and points at the provider combo.
    pub fn selectors_provider_conflict(&self) -> Option<&'static str> {
        if self.provider != Provider::Selectors {
            return None;
        }
        let raw = self.scraper_config.trim();
        if raw.is_empty() {
            return None;
        }
        let value: serde_json::Value = serde_json::from_str(raw).ok()?;
        match value.get("provider").and_then(|value| value.as_str()) {
            Some("eightfold") => Some(
                "This JSON names the eightfold provider. Choose Eightfold above, or remove the \
                 provider key.",
            ),
            _ => None,
        }
    }

    /// The inputs a preview result belongs to, derived from what a save would
    /// store so a provider or domain change invalidates the report.
    pub fn preview_inputs(&self) -> PreviewInputs {
        PreviewInputs {
            url: self.url.trim().to_string(),
            render_mode: self.render_mode.as_str().to_string(),
            scraper_config: self.data().scraper_config.unwrap_or_default(),
        }
    }

    pub fn data(&self) -> PortalFormData {
        let scraper_config = match self.provider {
            Provider::Selectors => {
                let raw = self.scraper_config.trim();
                if raw.is_empty() {
                    None
                } else {
                    Some(raw.to_string())
                }
            }
            Provider::Eightfold => {
                let mut object = if self.eightfold_config.trim().is_empty() {
                    serde_json::Map::new()
                } else {
                    serde_json::from_str::<serde_json::Value>(self.eightfold_config.trim())
                        .ok()
                        .and_then(|value| value.as_object().cloned())
                        .unwrap_or_default()
                };
                object.insert(
                    "provider".into(),
                    serde_json::Value::String("eightfold".into()),
                );
                object.insert(
                    "domain".into(),
                    serde_json::Value::String(self.domain.trim().to_string()),
                );
                Some(serde_json::Value::Object(object).to_string())
            }
        };

        PortalFormData {
            company_name: self.company_name.trim().to_string(),
            url: self.url.trim().to_string(),
            render_mode: self.render_mode.as_str().to_string(),
            scraper_config,
        }
    }

    /// True when the form's effective config is exactly the registry entry's.
    pub fn matches_known_site(&self, site: &KnownSite) -> bool {
        if self.provider != site.provider {
            return false;
        }
        let Some(current) = self.data().scraper_config else {
            return false;
        };
        match (
            serde_json::from_str::<serde_json::Value>(&current),
            serde_json::from_str::<serde_json::Value>(site.config),
        ) {
            (Ok(current), Ok(expected)) => current == expected,
            _ => false,
        }
    }

    /// Apply a registry entry. Called explicitly, or by auto-prefill while the
    /// provider fields are untouched.
    pub fn apply_known_site(&mut self, site: &KnownSite) {
        self.provider = site.provider;
        if self.company_name.trim().is_empty() {
            self.company_name = site.name.to_string();
        }

        match site.provider {
            Provider::Selectors => {
                self.scraper_config = site.config.to_string();
            }
            Provider::Eightfold => {
                if let Ok(ScraperConfig::Eightfold(config)) = ScraperConfig::parse(site.config) {
                    self.domain = config.domain;
                    self.eightfold_config = eightfold_extras(site.config);
                }
            }
        }

        self.prefilled_for = Some(site.name.to_string());
        self.error = None;
        self.preview = PreviewState::default();
    }

    /// Prefill a known site once, and only while the provider fields are empty.
    fn maybe_prefill_known_site(&mut self, site: Option<&'static KnownSite>) {
        let Some(site) = site else {
            return;
        };
        if self.prefilled_for.as_deref() == Some(site.name) {
            return;
        }
        if self.provider_fields_empty() {
            self.apply_known_site(site);
        }
    }

    fn provider_fields_empty(&self) -> bool {
        self.scraper_config.trim().is_empty()
            && self.domain.trim().is_empty()
            && self.eightfold_config.trim().is_empty()
    }
}

/// The Eightfold JSON without the provider/domain keys the form owns.
fn eightfold_extras(raw: &str) -> String {
    let Ok(mut value) = serde_json::from_str::<serde_json::Value>(raw) else {
        return String::new();
    };
    let Some(object) = value.as_object_mut() else {
        return String::new();
    };
    object.remove("provider");
    object.remove("domain");
    if object.is_empty() {
        return String::new();
    }
    serde_json::to_string_pretty(&value).unwrap_or_default()
}

fn known_site_for(form: &PortalForm) -> Option<&'static KnownSite> {
    let url = Url::parse(form.url.trim()).ok()?;
    known_sites::lookup(&url)
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

/// The form inputs a preview result describes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PreviewInputs {
    pub url: String,
    pub render_mode: String,
    pub scraper_config: String,
}

/// The last "Test config" run for the open form.
#[derive(Debug, Clone, Default)]
pub struct PreviewState {
    pub busy: bool,
    pub result: Option<ConfigPreview>,
    pub error: Option<String>,
    /// `None` until a run finishes; a mismatch with the current form clears the
    /// result during rendering.
    pub for_inputs: Option<PreviewInputs>,
}

/// A preview as the form renders it.
#[derive(Debug, Clone)]
pub struct ConfigPreview {
    pub jobs_found: usize,
    /// `0` means the board's adapter paginates internally, so the preview does
    /// not report a page count.
    pub pages_planned: usize,
    pub tier: String,
    pub sample: Vec<PreviewSample>,
    pub first_description_bytes: Option<usize>,
    pub details_attempted: usize,
}

#[derive(Debug, Clone)]
pub struct PreviewSample {
    pub title: String,
    pub url: String,
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
    /// Fetch the draft URL + config without saving, and report what it finds.
    TestConfig(PortalFormData),
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
            let known = known_site_for(form);
            form.maybe_prefill_known_site(known);

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

            if let Some(site) = known {
                ui.label(
                    RichText::new(format!("Known site: {}", site.label()))
                        .weak()
                        .small(),
                );
                if !form.matches_known_site(site) && ui.button("Use known config").clicked() {
                    form.apply_known_site(site);
                }
            }

            egui::ComboBox::from_label("Provider")
                .selected_text(form.provider.label())
                .show_ui(ui, |ui| {
                    for provider in [Provider::Selectors, Provider::Eightfold] {
                        ui.selectable_value(&mut form.provider, provider, provider.label());
                    }
                });

            ui.add_space(4.0);
            match form.provider {
                Provider::Selectors => {
                    labelled_multiline(
                        ui,
                        "Scraper config (JSON, optional)",
                        &mut form.scraper_config,
                        5,
                    );
                    ui.label(
                        RichText::new(
                            "CSS selectors for a career page that has no JSON-LD JobPosting. \
                             Leave empty for ATS boards. Shape: {\"list\": \"…\", \"fields\": \
                             {\"title\": \"…\"}}.",
                        )
                        .weak()
                        .small(),
                    );
                    if !form.valid_selectors_config() {
                        ui.colored_label(
                            Color32::from_rgb(0xc0, 0x39, 0x2b),
                            "Scraper config is not valid JSON.",
                        );
                    }
                    if let Some(conflict) = form.selectors_provider_conflict() {
                        ui.colored_label(Color32::from_rgb(0xc0, 0x39, 0x2b), conflict);
                    }
                }
                Provider::Eightfold => {
                    labelled_input(ui, "Eightfold domain", &mut form.domain, false);
                    ui.label(
                        RichText::new("The domain the site's API expects, for example nvidia.com.")
                            .weak()
                            .small(),
                    );
                    if form.domain.trim().is_empty() {
                        ui.colored_label(
                            Color32::from_rgb(0xc0, 0x39, 0x2b),
                            "Domain cannot be empty.",
                        );
                    }
                    labelled_multiline(
                        ui,
                        "Advanced options (JSON, optional)",
                        &mut form.eightfold_config,
                        3,
                    );
                    ui.label(
                        RichText::new(
                            "Optional pagination/detail caps. Provider and domain come from \
                             the fields above.",
                        )
                        .weak()
                        .small(),
                    );
                    if !form.valid_eightfold_config() {
                        ui.colored_label(
                            Color32::from_rgb(0xc0, 0x39, 0x2b),
                            "Advanced options are not valid JSON.",
                        );
                    }
                }
            }

            // A report describes the inputs it was run with. Editing the URL,
            // mode, or config invalidates it rather than showing a stale claim.
            if !form.preview.busy
                && form
                    .preview
                    .for_inputs
                    .as_ref()
                    .is_some_and(|inputs| *inputs != form.preview_inputs())
            {
                form.preview.result = None;
                form.preview.error = None;
                form.preview.for_inputs = None;
            }

            ui.add_space(6.0);
            ui.horizontal(|ui| {
                let can_test =
                    valid_http_url(&form.url) && form.provider_fields_valid() && !state.busy;
                if ui
                    .add_enabled(can_test, Button::new("Test config"))
                    .clicked()
                {
                    *action = Some(PortalsAction::TestConfig(form.data()));
                }
                if form.preview.busy {
                    LoadingSpinner::show(ui);
                }
            });

            if let Some(error) = &form.preview.error {
                ui.colored_label(Color32::from_rgb(0xc0, 0x39, 0x2b), error.as_str());
            }
            if let Some(preview) = &form.preview.result {
                preview_panel(ui, preview);
            }

            if let Some(error) = &form.error {
                ui.colored_label(Color32::from_rgb(0xc0, 0x39, 0x2b), error.as_str());
            }

            ui.add_space(8.0);
            ui.horizontal(|ui| {
                let can_save = form.valid() && form.provider_fields_valid() && !state.busy;
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

/// Render the findings of a "Test config" run.
///
/// The wording keeps page 1 and the configured page count separate, because the
/// preview only fetches the first page.
fn preview_panel(ui: &mut Ui, preview: &ConfigPreview) {
    ui.group(|ui| {
        let summary = if preview.pages_planned == 0 {
            format!(
                "Found {} jobs on the board (the adapter paginates internally).",
                preview.jobs_found
            )
        } else if preview.pages_planned > 1 {
            format!(
                "Found {} jobs on page 1; the saved config follows up to {} pages.",
                preview.jobs_found, preview.pages_planned
            )
        } else {
            format!("Found {} jobs on page 1.", preview.jobs_found)
        };
        ui.label(RichText::new(summary).strong());
        ui.label(
            RichText::new(format!("Tier: {}", preview.tier))
                .weak()
                .small(),
        );

        for sample in &preview.sample {
            ui.label(format!("- {} - {}", sample.title, sample.url));
        }

        match (preview.details_attempted, preview.first_description_bytes) {
            (0, _) => {
                ui.label(
                    RichText::new("No detail enrichment configured.")
                        .weak()
                        .small(),
                );
            }
            (_, Some(bytes)) => {
                ui.label(format!("First description: {bytes} bytes."));
            }
            (_, None) => {
                ui.label(
                    RichText::new("Details were fetched, but no description text was found.")
                        .weak()
                        .small(),
                );
            }
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scraper_config_is_optional_and_trimmed() {
        let mut form = PortalForm {
            company_name: "Acme".into(),
            url: "https://example.com/jobs".into(),
            ..Default::default()
        };

        assert_eq!(form.data().scraper_config, None);
        assert!(form.valid_scraper_config());

        form.scraper_config = "  \n  ".into();
        assert_eq!(
            form.data().scraper_config,
            None,
            "a whitespace-only config means none"
        );
        assert!(form.valid_scraper_config());

        form.scraper_config = "  {\"list\":\"article.job\"}  ".into();
        assert_eq!(
            form.data().scraper_config.as_deref(),
            Some("{\"list\":\"article.job\"}"),
            "the stored config is trimmed"
        );
        assert!(form.valid_scraper_config());
    }

    #[test]
    fn an_invalid_scraper_config_does_not_block_the_url_gate() {
        let form = PortalForm {
            company_name: "Acme".into(),
            url: "https://example.com/jobs".into(),
            scraper_config: "not json".into(),
            ..Default::default()
        };

        assert!(!form.valid_scraper_config());
        // `valid` stays the URL/company gate; the Save button also consults
        // `provider_fields_valid`.
        assert!(form.valid());
        assert!(!form.provider_fields_valid());
    }

    #[test]
    fn eightfold_form_builds_the_provider_config() {
        let mut form = PortalForm {
            company_name: "NVIDIA".into(),
            url: "https://jobs.nvidia.com/careers".into(),
            provider: Provider::Eightfold,
            domain: " nvidia.com ".into(),
            ..Default::default()
        };
        assert!(form.provider_fields_valid());

        let data = form.data();
        let value: serde_json::Value =
            serde_json::from_str(data.scraper_config.as_deref().expect("config")).expect("json");
        assert_eq!(value["provider"], "eightfold");
        assert_eq!(value["domain"], "nvidia.com", "the domain is trimmed");

        form.domain = "  ".into();
        assert!(!form.provider_fields_valid(), "a domain is required");
    }

    #[test]
    fn eightfold_advanced_options_survive_the_form() {
        let form = PortalForm {
            company_name: "NVIDIA".into(),
            url: "https://jobs.nvidia.com/careers".into(),
            provider: Provider::Eightfold,
            domain: "nvidia.com".into(),
            eightfold_config: r#"{"pagination":{"max_pages":2}}"#.into(),
            ..Default::default()
        };

        let data = form.data();
        let value: serde_json::Value =
            serde_json::from_str(data.scraper_config.as_deref().expect("config")).expect("json");
        assert_eq!(value["provider"], "eightfold");
        assert_eq!(value["domain"], "nvidia.com");
        assert_eq!(
            value["pagination"]["max_pages"], 2,
            "caps survive the provider/domain rewrite"
        );
    }

    #[test]
    fn a_selectors_config_naming_eightfold_is_a_conflict() {
        let form = PortalForm {
            company_name: "Acme".into(),
            url: "https://example.com/jobs".into(),
            scraper_config: r#"{"provider":"eightfold","domain":"nvidia.com"}"#.into(),
            ..Default::default()
        };

        assert!(form.selectors_provider_conflict().is_some());
        assert!(!form.provider_fields_valid());
    }

    #[test]
    fn from_row_reads_an_eightfold_config_and_keeps_its_caps() {
        let row = PortalRow {
            id: 1,
            name: "NVIDIA".into(),
            url: "https://jobs.nvidia.com/careers".into(),
            render_mode: "static".into(),
            scraper_config: Some(
                r#"{"provider":"eightfold","domain":"nvidia.com","pagination":{"max_pages":2}}"#
                    .into(),
            ),
            health: HealthBadge::NeverFetched,
            last_attempt: None,
            suggested_action: None,
        };

        let form = PortalForm::from_row(&row);
        assert_eq!(form.provider, Provider::Eightfold);
        assert_eq!(form.domain, "nvidia.com");
        assert!(
            form.eightfold_config.contains("max_pages"),
            "the advanced JSON is preserved for editing"
        );
        assert!(form.scraper_config.is_empty());

        let data = form.data();
        let value: serde_json::Value =
            serde_json::from_str(data.scraper_config.as_deref().expect("config")).expect("json");
        assert_eq!(value["pagination"]["max_pages"], 2);
    }

    #[test]
    fn a_known_site_prefills_an_empty_form_once() {
        let mut form = PortalForm {
            url: "https://jobs.nvidia.com/careers".into(),
            ..Default::default()
        };
        let site = known_sites::lookup(&Url::parse(&form.url).expect("url")).expect("known");

        form.maybe_prefill_known_site(Some(site));
        assert_eq!(form.provider, Provider::Eightfold);
        assert_eq!(form.domain, "nvidia.com");
        assert_eq!(form.company_name, "NVIDIA");

        form.domain = "example.com".into();
        form.maybe_prefill_known_site(Some(site));
        assert_eq!(
            form.domain, "example.com",
            "a second call must not overwrite an edit"
        );
    }

    #[test]
    fn a_known_site_does_not_prefill_a_non_empty_form() {
        let mut form = PortalForm {
            url: "https://careers.withwaymo.com/jobs/search".into(),
            scraper_config: r#"{"list":"custom"}"#.into(),
            ..Default::default()
        };
        let site = known_sites::lookup(&Url::parse(&form.url).expect("url")).expect("known");

        form.maybe_prefill_known_site(Some(site));
        assert_eq!(form.scraper_config, r#"{"list":"custom"}"#);
        assert_eq!(form.provider, Provider::Selectors);
    }
}
