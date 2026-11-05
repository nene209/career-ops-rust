//! Pipeline screen tests.
//!
//! The screen is rendered with `egui_kittest` against hand-built view-models.
//! App-level tests cover the database load and the manual-entry round trip;
//! these cover the controls, filters, search, delete gate, and action intents.

use career_ops_app::screens::pipeline::{self, PipelineAction, PipelineRenderState, PipelineView};
use career_ops_app::view_models::pipeline::{
    JobFilter, JobRow, PipelineViewModel, StatusLabel, TierLabel,
};
use egui::accesskit::Role;
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;

struct TestState {
    view: PipelineView,
    actions: Vec<PipelineAction>,
}

fn idle_state() -> PipelineRenderState<'static> {
    PipelineRenderState {
        loading: false,
        error: None,
        message: None,
        refreshing: None,
        marking: None,
        deleting: None,
    }
}

fn test_harness<'a>(
    view_model: &'a PipelineViewModel,
    view: PipelineView,
    render_state: PipelineRenderState<'a>,
) -> Harness<'a, TestState> {
    let state = TestState {
        view,
        actions: Vec::new(),
    };

    Harness::new_ui_state(
        move |ui, state: &mut TestState| {
            if let Some(action) = pipeline::show(ui, view_model, &mut state.view, render_state) {
                state.actions.push(action);
            }
        },
        state,
    )
}

fn row(id: i64, title: &str, status: Option<StatusLabel>, tier: TierLabel) -> JobRow {
    JobRow {
        id,
        title: title.into(),
        company: "Acme".into(),
        tier,
        fetched_at: "2h ago".into(),
        status,
        source_url: format!("https://example.com/jobs/{id}"),
        raw_text: "We are hiring.".into(),
        has_user_state: status.is_some_and(|status| status != StatusLabel::Discovered),
    }
}

fn selected(id: i64) -> PipelineView {
    PipelineView {
        selected: Some(id),
        ..PipelineView::default()
    }
}

fn view_model(rows: Vec<JobRow>, filter: JobFilter, search: &str) -> PipelineViewModel {
    PipelineViewModel {
        rows,
        filter,
        search: search.into(),
    }
}

#[test]
fn renders_rows_with_tier_and_status() {
    let view_model = view_model(
        vec![
            row(
                1,
                "Engineer",
                Some(StatusLabel::Interviewing),
                TierLabel::Ats,
            ),
            row(2, "Designer", None, TierLabel::Manual),
        ],
        JobFilter::All,
        "",
    );

    let mut harness = test_harness(&view_model, PipelineView::default(), idle_state());
    harness.run();

    for label in [
        "Engineer — Acme",
        "Designer — Acme",
        "ATS",
        "Manual",
        "Interviewing",
        "2 jobs",
    ] {
        let _ = harness.get_by_label(label);
    }
}

#[test]
fn empty_state_opens_the_add_job_action() {
    let view_model = PipelineViewModel::default();
    let mut harness = test_harness(&view_model, PipelineView::default(), idle_state());

    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Add your first job")
        .click();
    harness.run();

    assert!(harness.state().actions.contains(&PipelineAction::AddJob));
}

#[test]
fn header_add_job_emits_the_action() {
    let view_model = view_model(
        vec![row(1, "Engineer", None, TierLabel::Ats)],
        JobFilter::All,
        "",
    );
    let mut harness = test_harness(&view_model, PipelineView::default(), idle_state());

    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Add job")
        .click();
    harness.run();

    assert!(harness.state().actions.contains(&PipelineAction::AddJob));
}

#[test]
fn filter_buttons_emit_the_selected_filter() {
    let view_model = view_model(
        vec![row(1, "Engineer", None, TierLabel::Ats)],
        JobFilter::All,
        "",
    );
    let mut harness = test_harness(&view_model, selected(1), idle_state());

    harness.run();
    harness.get_by_label("Rejected").click();
    harness.run();

    assert_eq!(
        harness.state().actions.last(),
        Some(&PipelineAction::SetFilter(JobFilter::Rejected))
    );
    assert_eq!(
        harness.state().view.selected,
        None,
        "changing the filter returns to the list"
    );
}

#[test]
fn a_filter_with_no_matches_explains_itself() {
    let view_model = view_model(
        vec![row(1, "Engineer", None, TierLabel::Ats)],
        JobFilter::Rejected,
        "",
    );

    let mut harness = test_harness(&view_model, PipelineView::default(), idle_state());
    harness.run();

    let _ = harness.get_by_label("No jobs match this filter");
    assert!(
        harness.query_by_label("Engineer — Acme").is_none(),
        "the filtered-out row must not be rendered"
    );
}

#[test]
fn the_search_field_emits_the_query() {
    let view_model = view_model(
        vec![row(1, "Engineer", None, TierLabel::Ats)],
        JobFilter::All,
        "",
    );
    let mut harness = test_harness(&view_model, PipelineView::default(), idle_state());
    harness.run();

    let field = harness.get_by_label("Search");
    field.focus();
    field.type_text("engineer");
    harness.run();

    assert!(
        harness.state().actions.iter().any(
            |action| matches!(action, PipelineAction::SetSearch(query) if query == "engineer")
        ),
        "actions: {:?}",
        harness.state().actions
    );
}

#[test]
fn a_search_with_no_matches_explains_itself() {
    let view_model = view_model(
        vec![row(1, "Engineer", None, TierLabel::Ats)],
        JobFilter::All,
        "zzz",
    );

    let mut harness = test_harness(&view_model, PipelineView::default(), idle_state());
    harness.run();

    let _ = harness.get_by_label("No jobs match this search");
    assert!(
        harness.query_by_label("Engineer — Acme").is_none(),
        "the non-matching row must not be rendered"
    );
}

#[test]
fn clearing_the_search_emits_an_empty_query() {
    let view_model = view_model(
        vec![row(1, "Engineer", None, TierLabel::Ats)],
        JobFilter::All,
        "zzz",
    );

    let mut harness = test_harness(&view_model, PipelineView::default(), idle_state());
    harness.run();
    harness.get_by_role_and_label(Role::Button, "Clear").click();
    harness.run();

    assert!(harness
        .state()
        .actions
        .contains(&PipelineAction::SetSearch(String::new())));
}

#[test]
fn selecting_a_row_shows_the_detail_and_placeholder_buttons() {
    let view_model = view_model(
        vec![row(
            1,
            "Engineer",
            Some(StatusLabel::Interviewing),
            TierLabel::Ats,
        )],
        JobFilter::All,
        "",
    );
    let mut harness = test_harness(&view_model, PipelineView::default(), idle_state());

    harness.run();
    harness.get_by_label("Engineer — Acme").click();
    harness.run();

    assert_eq!(harness.state().view.selected, Some(1));
    let _body = harness.get_by_label("We are hiring.");

    // Phase 3 owns these; the shell reserves their place.
    for label in ["Evaluate", "Generate cover letter"] {
        let button = harness.get_by_role_and_label(Role::Button, label);
        assert!(
            button.accesskit_node().is_disabled(),
            "{label} must stay disabled in Phase 2.1"
        );
    }

    harness
        .get_by_role_and_label(Role::Button, "Back to list")
        .click();
    harness.run();

    assert_eq!(harness.state().view.selected, None);
}

#[test]
fn refresh_description_emits_the_action() {
    let view_model = view_model(
        vec![row(1, "Engineer", None, TierLabel::Ats)],
        JobFilter::All,
        "",
    );
    let mut harness = test_harness(&view_model, selected(1), idle_state());

    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Refresh description")
        .click();
    harness.run();

    assert!(harness
        .state()
        .actions
        .contains(&PipelineAction::RefreshJob(1)));
}

#[test]
fn refreshing_disables_the_button_and_reports_the_result() {
    let view_model = view_model(
        vec![row(1, "Engineer", None, TierLabel::Ats)],
        JobFilter::All,
        "",
    );
    let render_state = PipelineRenderState {
        loading: false,
        error: None,
        message: Some("Description refreshed: 2048 bytes."),
        refreshing: Some(1),
        marking: None,
        deleting: None,
    };
    let mut harness = test_harness(&view_model, selected(1), render_state);

    // The spinner keeps requesting repaints; `run()` would never settle.
    harness.run_steps(2);

    let button = harness.get_by_role_and_label(Role::Button, "Refresh description");
    assert!(button.accesskit_node().is_disabled());
    let _ = harness.get_by_label("Description refreshed: 2048 bytes.");
}

#[test]
fn mark_not_a_match_emits_the_action() {
    let view_model = view_model(
        vec![row(1, "Engineer", None, TierLabel::Ats)],
        JobFilter::All,
        "",
    );
    let mut harness = test_harness(&view_model, selected(1), idle_state());

    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Mark not a match")
        .click();
    harness.run();

    assert!(harness
        .state()
        .actions
        .contains(&PipelineAction::MarkNotAMatch(1)));
}

#[test]
fn mark_not_a_match_is_disabled_once_the_application_is_decided() {
    for status in [
        StatusLabel::Applied,
        StatusLabel::Interviewing,
        StatusLabel::Offer,
        StatusLabel::Rejected,
        StatusLabel::NotAMatch,
    ] {
        let view_model = view_model(
            vec![row(1, "Engineer", Some(status), TierLabel::Ats)],
            JobFilter::All,
            "",
        );
        let mut harness = test_harness(&view_model, selected(1), idle_state());
        harness.run();

        let button = harness.get_by_role_and_label(Role::Button, "Mark not a match");
        assert!(button.accesskit_node().is_disabled(), "{status:?}");
    }
}

#[test]
fn the_not_a_match_filter_is_offered() {
    let view_model = view_model(
        vec![row(1, "Engineer", None, TierLabel::Ats)],
        JobFilter::All,
        "",
    );
    let mut harness = test_harness(&view_model, PipelineView::default(), idle_state());
    harness.run();

    let _ = harness.get_by_label("Not a match");
}

#[test]
fn a_fetched_only_row_deletes_without_confirmation() {
    let view_model = view_model(
        vec![row(1, "Engineer", None, TierLabel::Ats)],
        JobFilter::All,
        "",
    );
    let mut harness = test_harness(&view_model, selected(1), idle_state());

    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Delete job")
        .click();
    harness.run();

    assert!(harness
        .state()
        .actions
        .contains(&PipelineAction::DeleteJob(1)));
    assert!(
        harness.state().view.delete.is_none(),
        "a fetched-only row must not open the gate"
    );
}

#[test]
fn a_touched_row_opens_the_typed_gate() {
    let mut touched = row(1, "Engineer", Some(StatusLabel::Discovered), TierLabel::Ats);
    touched.has_user_state = true;
    let view_model = view_model(vec![touched], JobFilter::All, "");
    let mut harness = test_harness(&view_model, selected(1), idle_state());

    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Delete job...")
        .click();
    harness.run();

    assert!(harness.state().view.delete.is_some());
    assert!(
        !harness
            .state()
            .actions
            .contains(&PipelineAction::DeleteJob(1)),
        "opening the gate must not delete anything"
    );

    let _ = harness.get_by_label("Delete Engineer at Acme?");
    let _ = harness.get_by_label(
        "The application, its evaluation reports, and any cover letters are deleted too.",
    );
    let _ = harness.get_by_label("An evaluation cannot be fetched again.");
    let _ = harness.get_by_label("Type delete me to confirm");
    assert!(harness
        .get_by_role_and_label(Role::Button, "Delete job")
        .accesskit_node()
        .is_disabled());
}

#[test]
fn a_wrong_phrase_keeps_the_delete_disabled() {
    let mut touched = row(1, "Engineer", None, TierLabel::Ats);
    touched.has_user_state = true;
    let view_model = view_model(vec![touched], JobFilter::All, "");
    let mut harness = test_harness(&view_model, selected(1), idle_state());

    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Delete job...")
        .click();
    harness.run();

    let field = harness.get_by_label("Type delete me to confirm");
    field.focus();
    field.type_text("delete");
    harness.run();

    let button = harness.get_by_role_and_label(Role::Button, "Delete job");
    assert!(button.accesskit_node().is_disabled());
    assert!(!harness
        .state()
        .actions
        .contains(&PipelineAction::DeleteJob(1)));
}

#[test]
fn the_typed_phrase_enables_the_delete() {
    let mut touched = row(1, "Engineer", None, TierLabel::Ats);
    touched.has_user_state = true;
    let view_model = view_model(vec![touched], JobFilter::All, "");
    let mut harness = test_harness(&view_model, selected(1), idle_state());

    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Delete job...")
        .click();
    harness.run();

    let field = harness.get_by_label("Type delete me to confirm");
    field.focus();
    field.type_text("delete me");
    harness.run();

    let button = harness.get_by_role_and_label(Role::Button, "Delete job");
    assert!(!button.accesskit_node().is_disabled());
    button.click();
    harness.run();

    assert!(harness
        .state()
        .actions
        .contains(&PipelineAction::DeleteJob(1)));
}

#[test]
fn cancelling_the_gate_leaves_the_job_alone() {
    let mut touched = row(1, "Engineer", None, TierLabel::Ats);
    touched.has_user_state = true;
    let view_model = view_model(vec![touched], JobFilter::All, "");
    let mut harness = test_harness(&view_model, selected(1), idle_state());

    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Delete job...")
        .click();
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Cancel")
        .click();
    harness.run();

    assert!(harness.state().view.delete.is_none());
    assert!(!harness
        .state()
        .actions
        .contains(&PipelineAction::DeleteJob(1)));
}

#[test]
fn a_selection_that_disappeared_falls_back_to_the_list() {
    let view_model = view_model(Vec::new(), JobFilter::All, "");

    let mut harness = test_harness(&view_model, selected(7), idle_state());
    harness.run();

    assert_eq!(
        harness.state().view.selected,
        None,
        "a stale selection must not stick after a reload"
    );
    let _ = harness.get_by_label("No jobs yet");
}

#[test]
fn first_load_shows_a_spinner_instead_of_the_empty_state() {
    let view_model = PipelineViewModel::default();
    let render_state = PipelineRenderState {
        loading: true,
        error: None,
        message: None,
        refreshing: None,
        marking: None,
        deleting: None,
    };

    let mut harness = test_harness(&view_model, PipelineView::default(), render_state);
    // The spinner keeps requesting repaints; `run()` would never settle.
    harness.run_steps(2);

    let _ = harness.get_by_label("Loading jobs...");
    assert!(harness.query_by_label("No jobs yet").is_none());
}

#[test]
fn an_error_offers_a_retry() {
    let view_model = PipelineViewModel::default();
    let render_state = PipelineRenderState {
        loading: false,
        error: Some("database is locked"),
        message: None,
        refreshing: None,
        marking: None,
        deleting: None,
    };

    let mut harness = test_harness(&view_model, PipelineView::default(), render_state);
    harness.run();

    let _ = harness.get_by_label("database is locked");
    harness.get_by_role_and_label(Role::Button, "Retry").click();
    harness.run();

    assert!(harness.state().actions.contains(&PipelineAction::Reload));
}
