//! Portals screen tests.
//!
//! The screen is rendered with `egui_kittest` against hand-built view-models.
//! The app-level tests cover the async task orchestration; these cover the
//! controls, gates, and action intents.

use career_ops_app::screens::portals::{
    self, ConfigPreview, ManualEntryView, ManualTab, PortalForm, PortalFormData, PortalModal,
    PortalsAction, PortalsRenderState, PortalsView, PreviewInputs, PreviewSample, PreviewState,
    RenderModeChoice, DETAIL_TOGGLE_LABEL,
};
use career_ops_app::view_models::portals::{
    BatchProgress, HealthBadge, PortalRow, PortalsViewModel, SidecarStatus,
};
use career_ops_core::acquisition::config::{Provider, ScraperConfig};
use career_ops_core::portals::service::FetchAllProgress;
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
        fetch_all_portal: None,
        fetch_all_progress: None,
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
fn row_refresh_returns_the_portal_id() {
    let view_model = PortalsViewModel {
        rows: vec![row(1, "Acme", HealthBadge::NeverFetched)],
        ..Default::default()
    };
    let mut harness = test_harness(&view_model, PortalsView::default(), idle_state());

    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Refresh")
        .click();
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
fn restore_known_config_replaces_a_divergent_config() {
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

    let _modified = harness.get_by_label("Modified from the known config.");

    harness
        .get_by_role_and_label(Role::Button, "Restore known config")
        .click();
    harness.run();

    let form = harness.state().view.form().expect("form");
    assert_eq!(form.domain, "nvidia.com");
    assert!(
        harness
            .query_by_role_and_label(Role::Button, "Restore known config")
            .is_none(),
        "the button disappears once the form matches the entry"
    );
    assert!(
        harness
            .query_by_label("Modified from the known config.")
            .is_none(),
        "restoring clears the modified marker"
    );
}

#[test]
fn a_matching_config_shows_no_modified_marker() {
    let view_model = PortalsViewModel::default();
    let view = PortalsView {
        modal: Some(PortalModal::Add(PortalForm {
            company_name: "NVIDIA".into(),
            url: "https://jobs.nvidia.com/careers".into(),
            provider: Provider::Eightfold,
            domain: "nvidia.com".into(),
            ..Default::default()
        })),
    };

    let mut harness = test_harness(&view_model, view, idle_state());
    harness.run();

    let _source = harness.get_by_label("Known site: NVIDIA (Eightfold) - verified 2026-11-03");
    assert!(harness
        .query_by_label("Modified from the known config.")
        .is_none());
    assert!(harness
        .query_by_role_and_label(Role::Button, "Restore known config")
        .is_none());
}

#[test]
fn editing_a_known_portal_shows_its_source_and_can_restore() {
    let view_model = PortalsViewModel {
        rows: vec![PortalRow {
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
        }],
        ..Default::default()
    };
    let view = PortalsView {
        modal: Some(PortalModal::Edit {
            id: 1,
            form: PortalForm::from_row(&view_model.rows[0]),
        }),
    };

    let mut harness = test_harness(&view_model, view, idle_state());
    harness.run();

    let _source = harness.get_by_label("Known site: NVIDIA (Eightfold) - verified 2026-11-03");
    let _modified = harness.get_by_label("Modified from the known config.");

    harness
        .get_by_role_and_label(Role::Button, "Restore known config")
        .click();
    harness.run();

    let form = harness.state().view.form().expect("form");
    assert_eq!(form.domain, "nvidia.com");
    assert!(
        form.eightfold_config.is_empty(),
        "restoring drops the saved extra caps"
    );
}

#[test]
fn an_unknown_site_has_no_known_site_affordances() {
    let view_model = PortalsViewModel::default();
    let view = PortalsView {
        modal: Some(PortalModal::Add(PortalForm {
            company_name: "Acme".into(),
            url: "https://acme.example/jobs".into(),
            ..Default::default()
        })),
    };

    let mut harness = test_harness(&view_model, view, idle_state());
    harness.run();

    for label in [
        "Known site: NVIDIA (Eightfold) - verified 2026-11-03",
        "Known site: Waymo (Selectors) - verified 2026-11-03",
        "Modified from the known config.",
    ] {
        assert!(
            harness.query_by_label(label).is_none(),
            "{label} must not appear for an unknown host"
        );
    }
    assert!(harness
        .query_by_role_and_label(Role::Button, "Restore known config")
        .is_none());
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

fn choose_site_view() -> PortalsView {
    PortalsView {
        modal: Some(PortalModal::ChooseSite {
            filter: String::new(),
        }),
    }
}

#[test]
fn the_add_chooser_lists_known_sites_and_other() {
    let view_model = PortalsViewModel::default();
    let view = choose_site_view();

    let mut harness = test_harness(&view_model, view, idle_state());
    harness.run();

    let _nvidia =
        harness.get_by_role_and_label(Role::Button, "NVIDIA (Eightfold) - verified 2026-11-03");
    let _waymo =
        harness.get_by_role_and_label(Role::Button, "Waymo (Selectors) - verified 2026-11-03");
    let _figma =
        harness.get_by_role_and_label(Role::Button, "Figma (Greenhouse) - verified 2026-11-05");
    let _spotify =
        harness.get_by_role_and_label(Role::Button, "Spotify (Lever) - verified 2026-11-05");
    let _linear =
        harness.get_by_role_and_label(Role::Button, "Linear (Ashby) - verified 2026-11-05");
    let _ubisoft = harness.get_by_role_and_label(
        Role::Button,
        "Ubisoft (SmartRecruiters) - verified 2026-11-05",
    );
    let _filter = harness.get_by_label("Filter sites");
    let _other = harness.get_by_role_and_label(Role::Button, "Other site");
    let _cancel = harness.get_by_role_and_label(Role::Button, "Cancel");
}

#[test]
fn the_site_filter_narrows_by_name_and_engine() {
    let view_model = PortalsViewModel::default();
    let mut harness = test_harness(&view_model, choose_site_view(), idle_state());
    harness.run();

    let field = harness.get_by_label("Filter sites");
    field.focus();
    field.type_text("lever");
    harness.run();

    let _spotify =
        harness.get_by_role_and_label(Role::Button, "Spotify (Lever) - verified 2026-11-05");
    assert!(
        harness
            .query_by_label("Figma (Greenhouse) - verified 2026-11-05")
            .is_none(),
        "a non-matching board must be hidden"
    );
    assert!(
        harness
            .query_by_label("Waymo (Selectors) - verified 2026-11-03")
            .is_none(),
        "the filter applies to the whole list"
    );
    let _clear = harness.get_by_role_and_label(Role::Button, "Clear");
}

#[test]
fn the_site_filter_matches_the_host() {
    let view_model = PortalsViewModel::default();
    let mut harness = test_harness(&view_model, choose_site_view(), idle_state());
    harness.run();

    let field = harness.get_by_label("Filter sites");
    field.focus();
    field.type_text("ashby");
    harness.run();

    let _linear =
        harness.get_by_role_and_label(Role::Button, "Linear (Ashby) - verified 2026-11-05");
    assert!(harness
        .query_by_label("Spotify (Lever) - verified 2026-11-05")
        .is_none());
}

#[test]
fn a_filter_with_no_matches_still_offers_other_site() {
    let view_model = PortalsViewModel::default();
    let mut harness = test_harness(&view_model, choose_site_view(), idle_state());
    harness.run();

    let field = harness.get_by_label("Filter sites");
    field.focus();
    field.type_text("zzzz");
    harness.run();

    let _empty = harness.get_by_label("No sites match this filter.");
    harness
        .get_by_role_and_label(Role::Button, "Other site")
        .click();
    harness.run();

    let form = harness.state().view.form().expect("the blank form is open");
    assert!(form.url.is_empty());
}

#[test]
fn clearing_the_site_filter_restores_the_list() {
    let view_model = PortalsViewModel::default();
    let mut harness = test_harness(&view_model, choose_site_view(), idle_state());
    harness.run();

    let field = harness.get_by_label("Filter sites");
    field.focus();
    field.type_text("lever");
    harness.run();

    harness.get_by_role_and_label(Role::Button, "Clear").click();
    harness.run();

    let _nvidia =
        harness.get_by_role_and_label(Role::Button, "NVIDIA (Eightfold) - verified 2026-11-03");
    let _waymo =
        harness.get_by_role_and_label(Role::Button, "Waymo (Selectors) - verified 2026-11-03");
}

#[test]
fn choosing_a_filtered_site_prefills_the_form() {
    let view_model = PortalsViewModel::default();
    let mut harness = test_harness(&view_model, choose_site_view(), idle_state());
    harness.run();

    let field = harness.get_by_label("Filter sites");
    field.focus();
    field.type_text("spot");
    harness.run();

    harness
        .get_by_role_and_label(Role::Button, "Spotify (Lever) - verified 2026-11-05")
        .click();
    harness.run();

    let form = harness.state().view.form().expect("the add form is open");
    assert_eq!(form.url, "https://jobs.lever.co/spotify");
    assert_eq!(form.company_name, "Spotify");
    assert_eq!(form.provider, Provider::Selectors);
    assert_eq!(form.scraper_config, r#"{"provider":"selectors"}"#);
    let _source = harness.get_by_label("Known site: Spotify (Lever) - verified 2026-11-05");
}

#[test]
fn choosing_a_known_site_prefills_the_form() {
    let view_model = PortalsViewModel::default();
    let view = choose_site_view();

    let mut harness = test_harness(&view_model, view, idle_state());
    harness.run();

    harness
        .get_by_role_and_label(Role::Button, "NVIDIA (Eightfold) - verified 2026-11-03")
        .click();
    harness.run();

    let form = harness.state().view.form().expect("the add form is open");
    assert_eq!(form.url, "https://jobs.nvidia.com/careers");
    assert_eq!(form.company_name, "NVIDIA");
    assert_eq!(form.provider, Provider::Eightfold);
    assert_eq!(form.domain, "nvidia.com");

    let _domain = harness.get_by_label("Eightfold domain");
    let _source = harness.get_by_label("Known site: NVIDIA (Eightfold) - verified 2026-11-03");
}

#[test]
fn choosing_other_site_opens_the_blank_form() {
    let view_model = PortalsViewModel::default();
    let view = choose_site_view();

    let mut harness = test_harness(&view_model, view, idle_state());
    harness.run();

    harness
        .get_by_role_and_label(Role::Button, "Other site")
        .click();
    harness.run();

    let form = harness.state().view.form().expect("the add form is open");
    assert!(form.url.is_empty());
    assert_eq!(form.provider, Provider::Selectors);
    assert!(
        harness
            .query_by_label("NVIDIA (Eightfold) - verified 2026-11-03")
            .is_none(),
        "the chooser is gone"
    );
}

#[test]
fn cancelling_the_chooser_closes_it() {
    let view_model = PortalsViewModel::default();
    let view = choose_site_view();

    let mut harness = test_harness(&view_model, view, idle_state());
    harness.run();

    harness
        .get_by_role_and_label(Role::Button, "Cancel")
        .click();
    harness.run();

    assert!(harness.state().view.modal.is_none());
}

#[test]
fn paginated_actions_are_offered_for_an_offset_provider() {
    let view_model = PortalsViewModel {
        rows: vec![PortalRow {
            scraper_config: Some(r#"{"provider":"eightfold","domain":"nvidia.com"}"#.into()),
            ..row(1, "NVIDIA", HealthBadge::NeverFetched)
        }],
        ..Default::default()
    };

    let mut harness = test_harness(&view_model, PortalsView::default(), idle_state());
    harness.run();

    let _ = harness.get_by_role_and_label(Role::Button, "Continue");
    let _ = harness.get_by_role_and_label(Role::Button, "Fetch all pages");
}

#[test]
fn paginated_actions_are_absent_for_a_next_link_portal() {
    let view_model = PortalsViewModel {
        rows: vec![PortalRow {
            scraper_config: Some(r#"{"next":"a[rel=next]"}"#.into()),
            ..row(1, "Acme", HealthBadge::NeverFetched)
        }],
        ..Default::default()
    };

    let mut harness = test_harness(&view_model, PortalsView::default(), idle_state());
    harness.run();

    for label in ["Continue", "Fetch all pages"] {
        assert!(
            harness
                .query_by_role_and_label(Role::Button, label)
                .is_none(),
            "a next-link chain cannot be jumped into, so {label} must not be offered"
        );
    }
    let _ = harness.get_by_role_and_label(Role::Button, "Refresh");
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
    let _fetch_all = harness.get_by_role_and_label(Role::Button, "Fetch all portals");
    let _reload = harness.get_by_role_and_label(Role::Button, "Reload list");
}

// -----------------------------------------------------------------
// Fetch controls (Phase 2.8)
// -----------------------------------------------------------------

fn eightfold_row(id: i64) -> PortalRow {
    PortalRow {
        scraper_config: Some(r#"{"provider":"eightfold","domain":"nvidia.com"}"#.into()),
        ..row(id, "NVIDIA", HealthBadge::Healthy)
    }
}

#[test]
fn a_paginated_row_offers_three_distinct_actions() {
    let view_model = PortalsViewModel {
        rows: vec![eightfold_row(1)],
        ..Default::default()
    };

    let mut harness = test_harness(&view_model, PortalsView::default(), idle_state());
    harness.run();

    for label in ["Refresh", "Continue", "Fetch all pages"] {
        let _ = harness.get_by_role_and_label(Role::Button, label);
    }
}

#[test]
fn fetch_all_pages_emits_the_portal_id() {
    let view_model = PortalsViewModel {
        rows: vec![eightfold_row(1)],
        ..Default::default()
    };

    let mut harness = test_harness(&view_model, PortalsView::default(), idle_state());
    harness.run();

    harness
        .get_by_role_and_label(Role::Button, "Fetch all pages")
        .click();
    harness.run();

    assert!(harness
        .state()
        .actions
        .contains(&PortalsAction::FetchAllPages(1)));
}

#[test]
fn a_running_walk_shows_cumulative_progress_and_stop() {
    let view_model = PortalsViewModel {
        rows: vec![eightfold_row(1)],
        ..Default::default()
    };
    let state = PortalsRenderState {
        busy: true,
        fetch_all_portal: Some(1),
        fetch_all_progress: Some(FetchAllProgress {
            pages_fetched: 10,
            jobs_stored: 200,
            budget: 50,
        }),
        ..idle_state()
    };

    let mut harness = test_harness(&view_model, PortalsView::default(), state);
    // The row shows a spinner while the walk runs, so the frame never settles.
    harness.run_steps(2);

    let _progress = harness.get_by_label("Page 10 of 50 (200 jobs)");

    // The row's other actions are replaced by the walk's own controls, so a
    // second click cannot start a second walk.
    for label in ["Edit", "Refresh", "Continue", "Fetch all pages"] {
        assert!(
            harness
                .query_by_role_and_label(Role::Button, label)
                .is_none(),
            "{label} must be hidden while the walk runs"
        );
    }

    harness.get_by_role_and_label(Role::Button, "Stop").click();
    harness.run_steps(2);

    assert!(harness
        .state()
        .actions
        .contains(&PortalsAction::CancelFetchAllPages));
}

#[test]
fn the_detail_toggle_is_off_and_usable_for_a_fresh_portal() {
    let view = PortalsView {
        modal: Some(PortalModal::Add(PortalForm {
            company_name: "Acme".into(),
            url: "https://example.com/jobs".into(),
            ..Default::default()
        })),
    };
    let view_model = PortalsViewModel::default();

    let mut harness = test_harness(&view_model, view, idle_state());
    harness.run();

    let toggle = harness.get_by_label(DETAIL_TOGGLE_LABEL);
    assert!(!toggle.accesskit_node().is_disabled());
    assert_eq!(
        toggle.accesskit_node().toggled(),
        Some(egui::accesskit::Toggled::False)
    );
}

#[test]
fn the_detail_toggle_is_on_for_an_old_config() {
    let view = PortalsView {
        modal: Some(PortalModal::Add(PortalForm {
            company_name: "Acme".into(),
            url: "https://example.com/jobs".into(),
            scraper_config: r#"{"detail":{"max_jobs":25}}"#.into(),
            ..Default::default()
        })),
    };
    let view_model = PortalsViewModel::default();

    let mut harness = test_harness(&view_model, view, idle_state());
    harness.run();

    let toggle = harness.get_by_label(DETAIL_TOGGLE_LABEL);
    assert_eq!(
        toggle.accesskit_node().toggled(),
        Some(egui::accesskit::Toggled::True)
    );
}

#[test]
fn a_hand_edited_config_disables_the_detail_toggle() {
    let view = PortalsView {
        modal: Some(PortalModal::Add(PortalForm {
            company_name: "Acme".into(),
            url: "https://example.com/jobs".into(),
            scraper_config: r#"{"list":"article.job"}"#.into(),
            detail_edited: true,
            ..Default::default()
        })),
    };
    let view_model = PortalsViewModel::default();

    let mut harness = test_harness(&view_model, view, idle_state());
    harness.run();

    let toggle = harness.get_by_label(DETAIL_TOGGLE_LABEL);
    assert!(
        toggle.accesskit_node().is_disabled(),
        "a hand-written config owns its own detail block"
    );
    let _ = harness.get_by_label(
        "This config was edited by hand; set `detail.enabled` in the JSON to change enrichment.",
    );
}
