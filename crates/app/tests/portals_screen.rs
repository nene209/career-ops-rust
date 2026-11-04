//! Portals screen tests.
//!
//! The screen is rendered with `egui_kittest` against hand-built view-models.
//! The app-level tests cover the async task orchestration; these cover the
//! controls, gates, and action intents.

use career_ops_app::screens::portals::{
    self, ConfigPreview, ManualEntryView, ManualTab, PortalForm, PortalFormData, PortalModal,
    PortalsAction, PortalsRenderState, PortalsView, PreviewInputs, PreviewSample, PreviewState,
    RenderModeChoice,
};
use career_ops_app::view_models::portals::{
    BatchProgress, HealthBadge, PortalRow, PortalsViewModel, SidecarStatus,
};
use career_ops_core::acquisition::config::{Provider, ScraperConfig};
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
            if let Some(action) = portals::show(ui, view_model, render_state) {
                state.actions.push(action);
            }
            if let Some(action) =
                portals::show_modal(ui, &mut state.view, render_state.modal_state())
            {
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
            scraper_config: String::new(),
            error: None,
            ..Default::default()
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
        scraper_config: String::new(),
        error: None,
        ..Default::default()
    }));
    harness.run();

    let save = harness.get_by_role_and_label(Role::Button, "Save portal");
    assert!(!save.accesskit_node().is_disabled());
}

#[test]
fn scraper_config_is_optional_and_validated_as_json() {
    let view_model = PortalsViewModel::default();
    let view = PortalsView {
        modal: Some(PortalModal::Add(PortalForm {
            company_name: "Acme".into(),
            url: "https://example.com/jobs".into(),
            render_mode: RenderModeChoice::Static,
            scraper_config: "not json".into(),
            error: None,
            ..Default::default()
        })),
    };

    let mut harness = test_harness(&view_model, view, idle_state());
    harness.run();

    let _field = harness.get_by_label("Scraper config (JSON, optional)");
    let _hint = harness.get_by_label("Scraper config is not valid JSON.");
    // Invalid JSON is reported inline and blocks saving, the same gate the URL
    // field uses; the fetch would classify it anyway.
    let save = harness.get_by_role_and_label(Role::Button, "Save portal");
    assert!(save.accesskit_node().is_disabled());

    harness.state_mut().view.modal = Some(PortalModal::Add(PortalForm {
        company_name: "Acme".into(),
        url: "https://example.com/jobs".into(),
        render_mode: RenderModeChoice::Static,
        scraper_config: r#"{"list":"article.job"}"#.into(),
        error: None,
        ..Default::default()
    }));
    harness.run();

    assert!(
        harness
            .query_by_label("Scraper config is not valid JSON.")
            .is_none(),
        "valid JSON must clear the hint"
    );
    let save = harness.get_by_role_and_label(Role::Button, "Save portal");
    assert!(!save.accesskit_node().is_disabled());
}

#[test]
fn test_config_renders_counts_titles_and_description_length() {
    let view_model = PortalsViewModel::default();
    let view = PortalsView {
        modal: Some(PortalModal::Add(PortalForm {
            company_name: "Acme".into(),
            url: "https://example.com/jobs".into(),
            scraper_config: r#"{"list":"article.job","fields":{"title":"h2"}}"#.into(),
            preview: PreviewState {
                busy: false,
                result: Some(ConfigPreview {
                    jobs_found: 3,
                    pages_planned: 5,
                    tier: "tier1b:html".into(),
                    sample: vec![
                        PreviewSample {
                            title: "Engineer".into(),
                            url: "https://example.com/jobs/1".into(),
                        },
                        PreviewSample {
                            title: "Designer".into(),
                            url: "https://example.com/jobs/2".into(),
                        },
                    ],
                    first_description_bytes: Some(2048),
                    details_attempted: 1,
                }),
                error: None,
                for_inputs: None,
            },
            ..Default::default()
        })),
    };

    let mut harness = test_harness(&view_model, view, idle_state());
    harness.run();

    let _summary =
        harness.get_by_label("Found 3 jobs on page 1; the saved config follows up to 5 pages.");
    let _tier = harness.get_by_label("Tier: tier1b:html");
    let _first = harness.get_by_label("- Engineer - https://example.com/jobs/1");
    let _description = harness.get_by_label("First description: 2048 bytes.");
}

#[test]
fn test_config_report_is_cleared_when_the_inputs_change() {
    let view_model = PortalsViewModel::default();
    let form = PortalForm {
        company_name: "Acme".into(),
        url: "https://example.com/jobs".into(),
        preview: PreviewState {
            busy: false,
            result: Some(ConfigPreview {
                jobs_found: 2,
                pages_planned: 1,
                tier: "tier1b:html".into(),
                sample: Vec::new(),
                first_description_bytes: None,
                details_attempted: 0,
            }),
            error: None,
            for_inputs: Some(PreviewInputs {
                url: "https://example.com/jobs".into(),
                render_mode: "static".into(),
                scraper_config: String::new(),
            }),
        },
        ..Default::default()
    };
    let view = PortalsView {
        modal: Some(PortalModal::Add(form)),
    };

    let mut harness = test_harness(&view_model, view, idle_state());
    harness.run();
    let _summary = harness.get_by_label("Found 2 jobs on page 1.");

    harness.state_mut().view.form_mut().expect("form").url = "https://example.com/other".into();
    harness.run();

    assert!(
        harness.query_by_label("Found 2 jobs on page 1.").is_none(),
        "a report about the old URL must not survive an edit"
    );
}

#[test]
fn test_config_renders_a_classified_error() {
    let view_model = PortalsViewModel::default();
    let view = PortalsView {
        modal: Some(PortalModal::Add(PortalForm {
            company_name: "Acme".into(),
            url: "https://example.com/jobs".into(),
            preview: PreviewState {
                busy: false,
                result: None,
                error: Some(
                    "Tier 1b: js_required The page structure may have changed. Check the portal \
                     URL, or configure scraper selectors."
                        .into(),
                ),
                for_inputs: None,
            },
            ..Default::default()
        })),
    };

    let mut harness = test_harness(&view_model, view, idle_state());
    harness.run();

    let _error = harness.get_by_label(
        "Tier 1b: js_required The page structure may have changed. Check the portal \
         URL, or configure scraper selectors.",
    );
}

#[test]
fn test_config_button_is_gated_and_emits_the_action() {
    let view_model = PortalsViewModel::default();
    let view = PortalsView {
        modal: Some(PortalModal::Add(PortalForm {
            company_name: "Acme".into(),
            url: "https://example.com/jobs".into(),
            ..Default::default()
        })),
    };

    let mut harness = test_harness(&view_model, view, idle_state());
    harness.run();

    let button = harness.get_by_role_and_label(Role::Button, "Test config");
    assert!(!button.accesskit_node().is_disabled());

    harness
        .state_mut()
        .view
        .form_mut()
        .expect("form")
        .scraper_config = "not json".into();
    harness.run();
    let button = harness.get_by_role_and_label(Role::Button, "Test config");
    assert!(
        button.accesskit_node().is_disabled(),
        "invalid JSON must not be sent to the network"
    );

    harness
        .state_mut()
        .view
        .form_mut()
        .expect("form")
        .scraper_config = r#"{"list":"article.job"}"#.into();
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Test config")
        .click();
    harness.run();

    assert!(harness
        .state()
        .actions
        .contains(&PortalsAction::TestConfig(PortalFormData {
            company_name: "Acme".into(),
            url: "https://example.com/jobs".into(),
            render_mode: "static".into(),
            scraper_config: Some(r#"{"list":"article.job"}"#.into()),
        })));
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
fn a_known_site_shows_its_label_and_prefills_an_empty_form() {
    let view_model = PortalsViewModel::default();
    let view = PortalsView {
        modal: Some(PortalModal::Add(PortalForm {
            url: "https://jobs.nvidia.com/careers".into(),
            ..Default::default()
        })),
    };

    let mut harness = test_harness(&view_model, view, idle_state());
    harness.run();

    let _label = harness.get_by_label("Known site: NVIDIA (Eightfold) - verified 2026-11-03");
    let _domain = harness.get_by_label("Eightfold domain");
    let _advanced = harness.get_by_label("Advanced options (JSON, optional)");

    let form = harness.state().view.form().expect("form");
    assert_eq!(form.provider, Provider::Eightfold);
    assert_eq!(form.domain, "nvidia.com");
    assert_eq!(form.company_name, "NVIDIA", "the name is only a default");
    assert!(form.scraper_config.is_empty());
}

#[test]
fn a_selectors_known_site_takes_the_same_prefill_path() {
    // Waymo and NVIDIA differ in provider, not in workflow: both render a
    // known-site label and fill the fields that provider owns.
    let view_model = PortalsViewModel::default();
    let view = PortalsView {
        modal: Some(PortalModal::Add(PortalForm {
            url: "https://careers.withwaymo.com/jobs/search".into(),
            ..Default::default()
        })),
    };

    let mut harness = test_harness(&view_model, view, idle_state());
    harness.run();

    let _label = harness.get_by_label("Known site: Waymo (Selectors) - verified 2026-11-03");
    let _field = harness.get_by_label("Scraper config (JSON, optional)");

    let form = harness.state().view.form().expect("form");
    assert_eq!(form.provider, Provider::Selectors);
    assert_eq!(form.company_name, "Waymo");
    let parsed = ScraperConfig::parse(&form.scraper_config).expect("prefilled config parses");
    assert!(parsed.selectors().is_some());
}

#[test]
fn editing_after_prefill_is_preserved() {
    let view_model = PortalsViewModel::default();
    let view = PortalsView {
        modal: Some(PortalModal::Add(PortalForm {
            url: "https://jobs.nvidia.com/careers".into(),
            ..Default::default()
        })),
    };

    let mut harness = test_harness(&view_model, view, idle_state());
    harness.run();

    harness.state_mut().view.form_mut().expect("form").domain = "example.com".into();
    harness.run();
    harness.run();

    assert_eq!(
        harness.state().view.form().expect("form").domain,
        "example.com",
        "prefill runs once per site, so a later frame must not overwrite an edit"
    );
}

#[test]
fn use_known_config_replaces_a_divergent_config() {
    let view_model = PortalsViewModel::default();
    let view = PortalsView {
        modal: Some(PortalModal::Add(PortalForm {
            company_name: "NVIDIA".into(),
            url: "https://jobs.nvidia.com/careers".into(),
            provider: Provider::Eightfold,
            domain: "example.com".into(),
            prefilled_for: Some("NVIDIA".into()),
            ..Default::default()
        })),
    };

    let mut harness = test_harness(&view_model, view, idle_state());
    harness.run();

    harness
        .get_by_role_and_label(Role::Button, "Use known config")
        .click();
    harness.run();

    let form = harness.state().view.form().expect("form");
    assert_eq!(form.domain, "nvidia.com");
    assert!(
        harness
            .query_by_role_and_label(Role::Button, "Use known config")
            .is_none(),
        "the button disappears once the form matches the entry"
    );
}

#[test]
fn save_is_gated_on_an_eightfold_domain() {
    let view_model = PortalsViewModel::default();
    let view = PortalsView {
        modal: Some(PortalModal::Add(PortalForm {
            company_name: "Acme".into(),
            url: "https://acme.example/jobs".into(),
            provider: Provider::Eightfold,
            ..Default::default()
        })),
    };

    let mut harness = test_harness(&view_model, view, idle_state());
    harness.run();

    let _hint = harness.get_by_label("Domain cannot be empty.");
    let save = harness.get_by_role_and_label(Role::Button, "Save portal");
    assert!(save.accesskit_node().is_disabled());

    harness.state_mut().view.form_mut().expect("form").domain = "acme.com".into();
    harness.run();

    assert!(
        harness.query_by_label("Domain cannot be empty.").is_none(),
        "a domain clears the hint"
    );
    let save = harness.get_by_role_and_label(Role::Button, "Save portal");
    assert!(!save.accesskit_node().is_disabled());
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
