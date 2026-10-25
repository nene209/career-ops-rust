//! Portals screen tests.
//!
//! The screen is rendered with `egui_kittest` against hand-built view-models.
//! The app-level tests cover the async task orchestration; these cover the
//! controls, gates, and action intents.

use career_ops_app::screens::portals::{
    self, ManualEntryView, ManualTab, PortalForm, PortalModal, PortalsAction, PortalsRenderState,
    PortalsView, RenderModeChoice,
};
use career_ops_app::view_models::portals::{
    BatchProgress, HealthBadge, PortalRow, PortalsViewModel, SidecarStatus,
};
use egui::accesskit::Role;
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;

struct TestState {
    view: PortalsView,
    actions: Vec<PortalsAction>,
}

fn idle_state<'a>() -> PortalsRenderState<'a> {
    PortalsRenderState {
        loading: false,
        busy: false,
        manual_busy: false,
        fetching_portal: None,
        error: None,
        message: None,
    }
}

fn test_harness<'a>(
    view_model: &'a PortalsViewModel,
    view: PortalsView,
    render_state: PortalsRenderState<'a>,
) -> Harness<'a, TestState> {
    let state = TestState {
        view,
        actions: Vec::new(),
    };

    Harness::new_ui_state(
        move |ui, state: &mut TestState| {
            if let Some(action) = portals::show(ui, view_model, &mut state.view, render_state) {
                state.actions.push(action);
            }
        },
        state,
    )
}

fn row(id: i64, name: &str, health: HealthBadge) -> PortalRow {
    PortalRow {
        id,
        name: name.into(),
        url: format!("https://example.com/{id}"),
        render_mode: "static".into(),
        scraper_config: None,
        health,
        last_attempt: Some("5m ago".into()),
        suggested_action: None,
    }
}

#[test]
fn renders_rows_health_and_sidecar_status() {
    let view_model = PortalsViewModel {
        rows: vec![
            row(1, "Healthy Co", HealthBadge::Healthy),
            row(
                2,
                "Broken Co",
                HealthBadge::Broken {
                    cause: "timeout".into(),
                },
            ),
            row(3, "Never Co", HealthBadge::NeverFetched),
        ],
        sidecar: SidecarStatus::NotStarted,
        batch: None,
    };

    let mut harness = test_harness(&view_model, PortalsView::default(), idle_state());
    harness.run();

    for label in [
        "Healthy Co",
        "Broken Co",
        "Never Co",
        "Healthy",
        "Broken",
        "Never fetched",
        "Sidecar:",
        "Not started",
    ] {
        let _ = harness.get_by_label(label);
    }
}

#[test]
fn empty_state_opens_the_add_portal_action() {
    let view_model = PortalsViewModel::default();
    let mut harness = test_harness(&view_model, PortalsView::default(), idle_state());

    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Add your first portal")
        .click();
    harness.run();

    assert!(harness.state().actions.contains(&PortalsAction::OpenAdd));
}

#[test]
fn row_fetch_returns_the_portal_id() {
    let view_model = PortalsViewModel {
        rows: vec![row(1, "Acme", HealthBadge::NeverFetched)],
        ..Default::default()
    };
    let mut harness = test_harness(&view_model, PortalsView::default(), idle_state());

    harness.run();
    harness.get_by_role_and_label(Role::Button, "Fetch").click();
    harness.run();

    assert!(harness
        .state()
        .actions
        .contains(&PortalsAction::FetchOne(1)));
}

#[test]
fn add_form_requires_a_valid_http_url() {
    let view_model = PortalsViewModel::default();
    let view = PortalsView {
        modal: Some(PortalModal::Add(PortalForm {
            company_name: "Acme".into(),
            url: "not a url".into(),
            render_mode: RenderModeChoice::Static,
            scraper_config: None,
            error: None,
        })),
    };

    let mut harness = test_harness(&view_model, view, idle_state());
    harness.run();

    let save = harness.get_by_role_and_label(Role::Button, "Save portal");
    assert!(save.accesskit_node().is_disabled());
    let _hint = harness.get_by_label("Enter a valid http(s) URL.");

    harness.state_mut().view.modal = Some(PortalModal::Add(PortalForm {
        company_name: "Acme".into(),
        url: "https://example.com/jobs".into(),
        render_mode: RenderModeChoice::Static,
        scraper_config: None,
        error: None,
    }));
    harness.run();

    let save = harness.get_by_role_and_label(Role::Button, "Save portal");
    assert!(!save.accesskit_node().is_disabled());
}

#[test]
fn delete_confirmation_returns_the_delete_action() {
    let view_model = PortalsViewModel::default();
    let view = PortalsView {
        modal: Some(PortalModal::Delete {
            id: 4,
            name: "Acme".into(),
        }),
    };

    let mut harness = test_harness(&view_model, view, idle_state());
    harness.run();

    let _title = harness.get_by_label("Delete portal?");
    harness
        .get_by_role_and_label(Role::Button, "Delete portal")
        .click();
    harness.run();

    assert!(harness
        .state()
        .actions
        .contains(&PortalsAction::DeletePortal(4)));
}

#[test]
fn single_fetch_replaces_the_health_badge_with_a_spinner() {
    let view_model = PortalsViewModel {
        rows: vec![row(1, "Acme", HealthBadge::Healthy)],
        ..Default::default()
    };
    let render_state = PortalsRenderState {
        busy: true,
        fetching_portal: Some(1),
        ..idle_state()
    };

    let mut harness = test_harness(&view_model, PortalsView::default(), render_state);
    // The spinner keeps requesting repaints; two steps lay out the row.
    harness.run_steps(2);

    assert!(
        harness.query_by_label("Healthy").is_none(),
        "the health badge should be replaced while the fetch is in flight"
    );
    let _name = harness.get_by_label("Acme");
}

#[test]
fn running_batch_offers_cancel() {
    let view_model = PortalsViewModel {
        rows: vec![row(1, "Acme", HealthBadge::NeverFetched)],
        batch: Some(BatchProgress {
            total: 1,
            in_flight: Some(1),
            ..Default::default()
        }),
        ..Default::default()
    };
    let render_state = PortalsRenderState {
        busy: true,
        ..idle_state()
    };

    let mut harness = test_harness(&view_model, PortalsView::default(), render_state);
    // The in-flight row renders a spinner, which keeps requesting repaints;
    // `run()` would never settle. Two steps are enough to lay out and query.
    harness.run_steps(2);

    let _cancel = harness.get_by_role_and_label(Role::Button, "Cancel");
}

#[test]
fn manual_url_requires_a_url() {
    let view_model = PortalsViewModel::default();
    let view = PortalsView {
        modal: Some(PortalModal::Manual(ManualEntryView::default())),
    };

    let mut harness = test_harness(&view_model, view, idle_state());
    harness.run();

    let fetch = harness.get_by_role_and_label(Role::Button, "Fetch and add");
    assert!(fetch.accesskit_node().is_disabled());

    harness
        .state_mut()
        .view
        .manual_mut()
        .expect("manual modal")
        .url = "https://example.com/job/1".into();
    harness.run();

    let fetch = harness.get_by_role_and_label(Role::Button, "Fetch and add");
    assert!(!fetch.accesskit_node().is_disabled());
}

#[test]
fn manual_paste_requires_a_title_and_body() {
    let view_model = PortalsViewModel::default();
    let manual = ManualEntryView {
        tab: ManualTab::PasteText,
        ..Default::default()
    };
    let view = PortalsView {
        modal: Some(PortalModal::Manual(manual)),
    };

    let mut harness = test_harness(&view_model, view, idle_state());
    harness.run();

    let add = harness.get_by_role_and_label(Role::Button, "Add pasted job");
    assert!(add.accesskit_node().is_disabled());

    {
        let manual = harness.state_mut().view.manual_mut().expect("manual modal");
        manual.title = "Engineer".into();
        manual.body = "Job body".into();
    }
    harness.run();

    let add = harness.get_by_role_and_label(Role::Button, "Add pasted job");
    assert!(!add.accesskit_node().is_disabled());
}

#[test]
fn header_offers_add_and_fetch_all_when_portals_exist() {
    let view_model = PortalsViewModel {
        rows: vec![row(1, "Acme", HealthBadge::Healthy)],
        ..Default::default()
    };

    let mut harness = test_harness(&view_model, PortalsView::default(), idle_state());
    harness.run();

    let _add_portal = harness.get_by_role_and_label(Role::Button, "Add portal");
    let _add_job = harness.get_by_role_and_label(Role::Button, "Add job");
    let _fetch_all = harness.get_by_role_and_label(Role::Button, "Fetch all");
    let _refresh = harness.get_by_role_and_label(Role::Button, "Refresh");
}
