//! Pipeline screen tests.
//!
//! The screen is rendered with `egui_kittest` against hand-built view-models.
//! App-level tests cover the database load and the manual-entry round trip;
//! these cover the controls, filters, search, delete gate, and action intents.

use career_ops_app::screens::pipeline::{
    self, DetailTab, PipelineAction, PipelineRenderState, PipelineView,
};
use career_ops_app::view_models::pipeline::{
    CoverLetterRow, EvaluationRow, JobFilter, JobRow, PipelineDetail, PipelineViewModel,
    StatusLabel, TierLabel,
};
use career_ops_core::model::QueueState;
use career_ops_core::preferences::Preferences;
use egui::accesskit::Role;
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;

struct TestState {
    view: PipelineView,
    actions: Vec<PipelineAction>,
}

fn idle_state() -> PipelineRenderState<'static> {
    PipelineRenderState::default()
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
        plain_text: "We are hiring.".into(),
        stored_is_markup: false,
        has_user_state: status.is_some_and(|status| status != StatusLabel::Discovered),
        preferred: false,
        excluded: false,
        queue: QueueState::New,
        match_score: None,
    }
}

/// The same row, in a queue state the user already chose.
fn with_queue(mut row: JobRow, queue: QueueState) -> JobRow {
    row.queue = queue;
    row
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
        ..Default::default()
    }
}

/// A view model whose rows are classified by `preferences`.
fn preferred_view_model(rows: Vec<JobRow>, preferences: Preferences) -> PipelineViewModel {
    PipelineViewModel {
        rows,
        preferences,
        ..Default::default()
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
fn a_search_reports_how_many_rows_match() {
    let mut other = row(2, "Designer", None, TierLabel::Ats);
    other.company = "Globex".into();
    let view_model = view_model(
        vec![row(1, "Engineer", None, TierLabel::Ats), other],
        JobFilter::All,
        "engineer",
    );
    let mut harness = test_harness(&view_model, PipelineView::default(), idle_state());
    harness.run();

    let _ = harness.get_by_label("1 of 2 jobs match");
}

#[test]
fn the_list_scroll_offset_survives_a_detail_round_trip() {
    let rows: Vec<JobRow> = (0..100)
        .map(|id| row(id, &format!("Job {id}"), None, TierLabel::Ats))
        .collect();
    let view_model = view_model(rows, JobFilter::All, "");
    let view = PipelineView {
        list_scroll: 400.0,
        ..PipelineView::default()
    };
    let mut harness = test_harness(&view_model, view, idle_state());
    harness.run();

    let before = harness.state().view.list_scroll;
    assert!(
        before > 0.0,
        "the stored offset must be applied to a scrollable list"
    );

    // Enter and leave the detail view without touching the list.
    harness.state_mut().view.selected = Some(1);
    harness.run();
    harness.state_mut().view.selected = None;
    harness.run();

    assert_eq!(
        harness.state().view.list_scroll,
        before,
        "returning to the list must not jump to the top"
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

    // The fixture has a short body; the detail's threshold decides whether the
    // button offers a fetch first. With the default state (no detail yet) it
    // does, which is the safe default.
    let thin_label = harness.get_by_role_and_label(Role::Button, "Fetch description and evaluate");
    assert!(!thin_label.accesskit_node().is_disabled());
    assert!(harness.query_by_label("Evaluate").is_none());

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
        ..PipelineRenderState::default()
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
fn the_detail_renders_plain_text_and_offers_the_raw_toggle() {
    let mut html_row = row(1, "Engineer", None, TierLabel::Ats);
    html_row.raw_text = "<p>Hello <strong>world</strong>.</p>".into();
    html_row.plain_text = "Hello world.".into();
    html_row.stored_is_markup = true;
    let view_model = view_model(vec![html_row], JobFilter::All, "");
    let mut harness = test_harness(&view_model, selected(1), idle_state());
    harness.run();

    let _ = harness.get_by_label("Hello world.");
    assert!(
        harness
            .query_by_label("<p>Hello <strong>world</strong>.</p>")
            .is_none(),
        "the formatted view must not show markup"
    );
    let _ = harness.get_by_label("Raw text");
}

#[test]
fn the_raw_toggle_shows_the_stored_markup() {
    let mut html_row = row(1, "Engineer", None, TierLabel::Ats);
    html_row.raw_text = "<p>Hello <strong>world</strong>.</p>".into();
    html_row.plain_text = "Hello world.".into();
    html_row.stored_is_markup = true;
    let view_model = view_model(vec![html_row], JobFilter::All, "");
    let mut harness = test_harness(&view_model, selected(1), idle_state());
    harness.run();

    harness.get_by_label("Raw text").click();
    harness.run();

    assert!(harness.state().view.show_raw);
    let _ = harness.get_by_label("<p>Hello <strong>world</strong>.</p>");
    let _ = harness.get_by_label("Stored as HTML from the source.");
    assert!(
        harness.query_by_label("Hello world.").is_none(),
        "the formatted text is replaced, not duplicated"
    );
}

#[test]
fn a_plain_description_has_no_raw_toggle() {
    let view_model = view_model(
        vec![row(1, "Engineer", None, TierLabel::Ats)],
        JobFilter::All,
        "",
    );
    let mut harness = test_harness(&view_model, selected(1), idle_state());
    harness.run();

    let _ = harness.get_by_label("We are hiring.");
    assert!(
        harness.query_by_label("Raw text").is_none(),
        "there is nothing to reveal"
    );
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
        ..PipelineRenderState::default()
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
        error: Some("database is locked"),
        ..PipelineRenderState::default()
    };

    let mut harness = test_harness(&view_model, PipelineView::default(), render_state);
    harness.run();

    let _ = harness.get_by_label("database is locked");
    harness.get_by_role_and_label(Role::Button, "Retry").click();
    harness.run();

    assert!(harness.state().actions.contains(&PipelineAction::Reload));
}

fn preferences() -> Preferences {
    Preferences {
        preferred_companies: vec!["Acme".into()],
        excluded_companies: vec!["RecruiterCo".into()],
        ..Default::default()
    }
}

fn preferred_row(id: i64, title: &str) -> JobRow {
    let mut row = row(id, title, None, TierLabel::Ats);
    row.preferred = true;
    row
}

fn excluded_row(id: i64, title: &str) -> JobRow {
    let mut row = row(id, title, None, TierLabel::Ats);
    row.company = "RecruiterCo".into();
    row.excluded = true;
    row
}

/// The badge and the filter chip share the word "Preferred", so the badge is
/// queried by role: a label, not a button.
#[test]
fn a_preferred_row_shows_the_badge_and_others_do_not() {
    let view_model = preferred_view_model(
        vec![
            preferred_row(1, "Engineer"),
            row(2, "Designer", None, TierLabel::Ats),
        ],
        preferences(),
    );
    let mut harness = test_harness(&view_model, PipelineView::default(), idle_state());
    harness.run();

    // Exactly one preferred row, so exactly one badge: `get_by_role_and_label`
    // would panic if the second row carried one too.
    let _badge = harness.get_by_role_and_label(Role::Label, "Preferred");
    assert!(
        harness
            .query_by_role_and_label(Role::Label, "Excluded")
            .is_none(),
        "no row is excluded"
    );
}

#[test]
fn the_excluded_filter_is_the_only_view_that_shows_an_excluded_row() {
    let rows = || vec![preferred_row(1, "Engineer"), excluded_row(2, "Hidden role")];

    // All hides it...
    let all = view_model(rows(), JobFilter::All, "");
    let mut harness = test_harness(&all, PipelineView::default(), idle_state());
    harness.run();
    let _ = harness.get_by_label("Engineer — Acme");
    assert!(
        harness
            .query_by_label("Hidden role — RecruiterCo")
            .is_none(),
        "All hides an excluded row"
    );

    // ...and Excluded is the one view that shows it, so the exclusion can be
    // seen and reversed.
    let excluded = view_model(rows(), JobFilter::Excluded, "");
    let mut harness = test_harness(&excluded, PipelineView::default(), idle_state());
    harness.run();
    let _ = harness.get_by_label("Hidden role — RecruiterCo");
    assert!(
        harness.query_by_label("Engineer — Acme").is_none(),
        "the Excluded view shows only the excluded rows"
    );
}

#[test]
fn the_preferred_filter_shows_exactly_the_preferred_rows() {
    let view_model = view_model(
        vec![
            preferred_row(1, "Engineer"),
            row(2, "Designer", None, TierLabel::Ats),
        ],
        JobFilter::Preferred,
        "",
    );
    let mut harness = test_harness(&view_model, PipelineView::default(), idle_state());
    harness.run();

    let _ = harness.get_by_label("Engineer — Acme");
    assert!(harness.query_by_label("Designer — Acme").is_none());
}

#[test]
fn the_header_reports_the_preference_summary() {
    let view_model = preferred_view_model(
        vec![
            preferred_row(1, "Engineer"),
            excluded_row(2, "Hidden role"),
            row(3, "Designer", None, TierLabel::Ats),
        ],
        preferences(),
    );
    let mut harness = test_harness(&view_model, PipelineView::default(), idle_state());
    harness.run();

    let _ = harness.get_by_label("1 preferred of 2 shown; 1 excluded company hidden.");
}

#[test]
fn no_summary_without_preferences() {
    let view_model = view_model(
        vec![row(1, "Engineer", None, TierLabel::Ats)],
        JobFilter::All,
        "",
    );
    let mut harness = test_harness(&view_model, PipelineView::default(), idle_state());
    harness.run();

    assert!(
        harness.query_by_label("0 preferred of 1 shown.").is_none(),
        "nothing configured, nothing to say"
    );
}

#[test]
fn the_queue_actions_emit_their_states() {
    let cases = [
        ("Mark for details", PipelineAction::MarkForDetails(1)),
        ("Mark for evaluation", PipelineAction::MarkForEvaluation(1)),
        ("Dismiss", PipelineAction::Dismiss(1)),
    ];

    for (label, expected) in cases {
        let view_model = view_model(
            vec![row(1, "Engineer", None, TierLabel::Ats)],
            JobFilter::All,
            "",
        );
        let mut harness = test_harness(&view_model, selected(1), idle_state());
        harness.run();

        harness.get_by_role_and_label(Role::Button, label).click();
        harness.run();

        assert_eq!(harness.state().actions, vec![expected], "{label}");
    }
}

/// Once the state is applied, the action is replaced by the state label, so a
/// second click has nothing to emit.
#[test]
fn a_queue_action_becomes_the_state_label_once_applied() {
    let row = with_queue(
        row(1, "Engineer", None, TierLabel::Ats),
        QueueState::DetailsNeeded,
    );
    let view_model = view_model(vec![row], JobFilter::All, "");
    let mut harness = test_harness(&view_model, selected(1), idle_state());
    harness.run();

    assert!(
        harness
            .query_by_role_and_label(Role::Button, "Mark for details")
            .is_none(),
        "the applied action must not stay clickable"
    );
    let applied = harness.get_by_role_and_label(Role::Button, "Details needed");
    assert!(applied.accesskit_node().is_disabled());

    applied.click();
    harness.run();
    assert!(harness.state().actions.is_empty());
}

#[test]
fn a_queue_action_is_disabled_while_a_queue_write_is_in_flight() {
    let view_model = view_model(
        vec![row(1, "Engineer", None, TierLabel::Ats)],
        JobFilter::All,
        "",
    );
    let state = PipelineRenderState {
        queueing: Some(1),
        ..idle_state()
    };
    let mut harness = test_harness(&view_model, selected(1), state);
    // The in-flight spinner keeps requesting repaints; `run()` would never
    // settle.
    harness.run_steps(2);

    for label in ["Mark for details", "Mark for evaluation", "Dismiss"] {
        let button = harness.get_by_role_and_label(Role::Button, label);
        assert!(button.accesskit_node().is_disabled(), "{label}");
    }
}

#[test]
fn the_queue_badge_renders_beside_the_status_badge() {
    let row = with_queue(
        row(1, "Engineer", Some(StatusLabel::Ready), TierLabel::Ats),
        QueueState::Evaluate,
    );
    let view_model = view_model(vec![row], JobFilter::All, "");
    let mut harness = test_harness(&view_model, PipelineView::default(), idle_state());
    harness.run();

    // The filter chips do not include `Evaluate`, so the label is the badge.
    assert_eq!(
        harness
            .get_all_by_role_and_label(Role::Label, "Evaluate")
            .count(),
        1
    );
    assert_eq!(
        harness
            .get_all_by_role_and_label(Role::Label, "Ready")
            .count(),
        1
    );
}

#[test]
fn a_new_row_has_no_queue_badge() {
    let view_model = view_model(
        vec![row(1, "Engineer", None, TierLabel::Ats)],
        JobFilter::All,
        "",
    );
    let mut harness = test_harness(&view_model, PipelineView::default(), idle_state());
    harness.run();

    assert!(
        harness.query_by_label("New").is_none(),
        "the default state is not a badge"
    );
}

#[test]
fn fetch_descriptions_is_disabled_with_nothing_flagged() {
    let view_model = view_model(
        vec![row(1, "Engineer", None, TierLabel::Ats)],
        JobFilter::All,
        "",
    );
    let mut harness = test_harness(&view_model, PipelineView::default(), idle_state());
    harness.run();

    let button = harness.get_by_role_and_label(Role::Button, "Fetch descriptions (0)");
    assert!(
        button.accesskit_node().is_disabled(),
        "an empty queue offers nothing to fetch"
    );
}

#[test]
fn fetch_descriptions_counts_the_flagged_rows_and_emits_the_action() {
    let view_model = view_model(
        vec![
            with_queue(
                row(1, "Engineer", None, TierLabel::Ats),
                QueueState::DetailsNeeded,
            ),
            with_queue(
                row(2, "Designer", None, TierLabel::Ats),
                QueueState::DetailsNeeded,
            ),
            row(3, "Manager", None, TierLabel::Ats),
        ],
        JobFilter::All,
        "",
    );
    let mut harness = test_harness(&view_model, PipelineView::default(), idle_state());
    harness.run();

    harness
        .get_by_role_and_label(Role::Button, "Fetch descriptions (2)")
        .click();
    harness.run();

    assert_eq!(
        harness.state().actions,
        vec![PipelineAction::FetchDescriptions]
    );
}

#[test]
fn a_running_batch_shows_progress_and_offers_stop() {
    let view_model = view_model(
        vec![with_queue(
            row(1, "Engineer", None, TierLabel::Ats),
            QueueState::DetailsNeeded,
        )],
        JobFilter::All,
        "",
    );
    let state = PipelineRenderState {
        descriptions: Some(pipeline::DescriptionProgress {
            done: 1,
            total: 3,
            failed: 1,
        }),
        ..idle_state()
    };
    let mut harness = test_harness(&view_model, PipelineView::default(), state);
    // The in-flight spinner keeps requesting repaints; `run()` would never
    // settle.
    harness.run_steps(2);

    let _ = harness.get_by_label("Descriptions: 1 of 3…");
    let _ = harness.get_by_label("1 failed");
    assert!(
        harness
            .get_by_role_and_label(Role::Button, "Fetch descriptions (1)")
            .accesskit_node()
            .is_disabled(),
        "a second batch must not start while one runs"
    );

    harness.get_by_role_and_label(Role::Button, "Stop").click();
    harness.run_steps(2);
    assert_eq!(
        harness.state().actions,
        vec![PipelineAction::CancelDescriptions]
    );
}

/// Dismissing is a decision, not a delete: the row leaves the default view and
/// stays findable under `Dismissed`.
#[test]
fn dismissed_rows_are_hidden_from_all_and_shown_under_the_dismissed_filter() {
    let rows = || {
        vec![
            row(1, "Engineer", None, TierLabel::Ats),
            with_queue(
                row(2, "Dismissed role", None, TierLabel::Ats),
                QueueState::Dismissed,
            ),
        ]
    };

    let all = view_model(rows(), JobFilter::All, "");
    let mut harness = test_harness(&all, PipelineView::default(), idle_state());
    harness.run();
    let _ = harness.get_by_label("Engineer — Acme");
    assert!(
        harness.query_by_label("Dismissed role — Acme").is_none(),
        "All hides a dismissed row"
    );

    let dismissed = view_model(rows(), JobFilter::Dismissed, "");
    let mut harness = test_harness(&dismissed, PipelineView::default(), idle_state());
    harness.run();
    let _ = harness.get_by_label("Dismissed role — Acme");
    assert!(
        harness.query_by_label("Engineer — Acme").is_none(),
        "the Dismissed view shows only the dismissed rows"
    );
}

// ----------------------------------------------------------------------
// WS6: evaluation and cover-letter UI
// ----------------------------------------------------------------------

fn evaluation(id: i64, score: f32, recommendation: &str, content: &str) -> EvaluationRow {
    EvaluationRow {
        id,
        generated_at: format!("2026-01-0{id} 10:00"),
        model_used: "deepseek-flash".into(),
        prompt_version: Some(1),
        match_score: Some(score),
        recommendation: Some(recommendation.into()),
        content: content.into(),
    }
}

fn detail_with_thin(thin: usize) -> PipelineDetail {
    PipelineDetail {
        thin_description_chars: thin,
        ..PipelineDetail::default()
    }
}

#[test]
fn a_thin_row_offers_the_fetch_first() {
    let detail = detail_with_thin(10_000);
    let state = PipelineRenderState {
        detail: Some(&detail),
        ..PipelineRenderState::default()
    };
    let view_model = view_model(
        vec![row(1, "Engineer", None, TierLabel::Ats)],
        JobFilter::All,
        "",
    );
    let mut harness = test_harness(&view_model, selected(1), state);
    harness.run();

    harness
        .get_by_role_and_label(Role::Button, "Fetch description and evaluate")
        .click();
    harness.run();
    assert_eq!(
        harness.state().actions.last(),
        Some(&PipelineAction::Evaluate {
            job_id: 1,
            fetch_description_if_thin: true,
            prompt_name: "evaluate".into(),
            model_override: None,
        })
    );

    // The link lives in the Evaluation tab, which is where the overrides are.
    let mut view = selected(1);
    view.tab = DetailTab::Evaluation;
    let mut harness = test_harness(&view_model, view, state);
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Evaluate without description")
        .click();
    harness.run();
    assert_eq!(
        harness.state().actions.last(),
        Some(&PipelineAction::Evaluate {
            job_id: 1,
            fetch_description_if_thin: false,
            prompt_name: "evaluate".into(),
            model_override: None,
        })
    );
}

#[test]
fn a_full_row_evaluates_without_fetching() {
    let detail = detail_with_thin(0);
    let state = PipelineRenderState {
        detail: Some(&detail),
        ..PipelineRenderState::default()
    };
    let view_model = view_model(
        vec![row(1, "Engineer", None, TierLabel::Ats)],
        JobFilter::All,
        "",
    );
    let mut harness = test_harness(&view_model, selected(1), state);
    harness.run();

    harness
        .get_by_role_and_label(Role::Button, "Evaluate")
        .click();
    harness.run();
    assert_eq!(
        harness.state().actions.last(),
        Some(&PipelineAction::Evaluate {
            job_id: 1,
            fetch_description_if_thin: false,
            prompt_name: "evaluate".into(),
            model_override: None,
        })
    );
}

#[test]
fn an_evaluation_streams_and_can_be_stopped() {
    let detail = detail_with_thin(0);
    let state = PipelineRenderState {
        detail: Some(&detail),
        evaluating: Some(1),
        stream: "partial report text",
        ..PipelineRenderState::default()
    };
    let mut view = selected(1);
    view.tab = DetailTab::Evaluation;
    let view_model = view_model(
        vec![row(1, "Engineer", None, TierLabel::Ats)],
        JobFilter::All,
        "",
    );
    let mut harness = test_harness(&view_model, view, state);
    // A spinner keeps requesting repaints, so `run()` would never settle.
    harness.run_steps(2);

    let _ = harness.get_by_label("partial report text");
    harness.get_by_role_and_label(Role::Button, "Stop").click();
    harness.run_steps(2);
    assert_eq!(
        harness.state().actions.last(),
        Some(&PipelineAction::CancelEvaluation)
    );
}

#[test]
fn the_evaluation_tab_shows_the_report_and_history() {
    let detail = PipelineDetail {
        evaluations: vec![
            evaluation(2, 0.9, "Strong match", "latest report"),
            evaluation(1, 0.6, "Weak match", "older report"),
        ],
        thin_description_chars: 0,
        ..PipelineDetail::default()
    };
    let state = PipelineRenderState {
        detail: Some(&detail),
        ..PipelineRenderState::default()
    };
    let mut view = selected(1);
    view.tab = DetailTab::Evaluation;
    let view_model = view_model(
        vec![row(1, "Engineer", None, TierLabel::Ats)],
        JobFilter::All,
        "",
    );
    let mut harness = test_harness(&view_model, view, state);
    harness.run();

    let _ = harness.get_by_label("0.90");
    let _ = harness.get_by_label("latest report");
    let _ = harness.get_by_label("2026-01-02 10:00 · 0.90 · Strong match");

    harness
        .get_by_label("2026-01-01 10:00 · 0.60 · Weak match")
        .click();
    harness.run();

    assert_eq!(harness.state().view.selected_eval, Some(1));
    let _ = harness.get_by_label("older report");
    assert!(
        harness.query_by_label("latest report").is_none(),
        "the selected report replaces the latest"
    );
}

#[test]
fn the_cover_letter_tab_shows_the_latest_letter() {
    let detail = PipelineDetail {
        cover_letters: vec![CoverLetterRow {
            id: 4,
            generated_at: "2026-01-04 10:00".into(),
            model_used: "deepseek-flash".into(),
            prompt_version: Some(1),
            content: "Dear Hiring Manager".into(),
        }],
        thin_description_chars: 0,
        ..PipelineDetail::default()
    };
    let state = PipelineRenderState {
        detail: Some(&detail),
        ..PipelineRenderState::default()
    };
    let mut view = selected(1);
    view.tab = DetailTab::CoverLetter;
    let view_model = view_model(
        vec![row(1, "Engineer", None, TierLabel::Ats)],
        JobFilter::All,
        "",
    );
    let mut harness = test_harness(&view_model, view, state);
    harness.run();

    let _ = harness.get_by_label("Dear Hiring Manager");
}

#[test]
fn the_row_shows_the_match_score() {
    let mut scored = row(1, "Engineer", None, TierLabel::Ats);
    scored.match_score = Some(0.82);
    let view_model = view_model(vec![scored], JobFilter::All, "");
    let mut harness = test_harness(&view_model, PipelineView::default(), idle_state());
    harness.run();

    let _ = harness.get_by_label("0.82");
}
