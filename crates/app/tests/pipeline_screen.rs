//! Pipeline screen tests.
//!
//! The screen is rendered with `egui_kittest` against hand-built view-models.
//! App-level tests cover the database load and the manual-entry round trip;
//! these cover the controls, filters, and action intents.

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
    }
}

#[test]
fn renders_rows_with_tier_and_status() {
    let view_model = PipelineViewModel {
        rows: vec![
            row(
                1,
                "Engineer",
                Some(StatusLabel::Interviewing),
                TierLabel::Ats,
            ),
            row(2, "Designer", None, TierLabel::Manual),
        ],
        filter: JobFilter::All,
    };

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
    let view_model = PipelineViewModel {
        rows: vec![row(1, "Engineer", None, TierLabel::Ats)],
        filter: JobFilter::All,
    };
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
    let view_model = PipelineViewModel {
        rows: vec![row(1, "Engineer", None, TierLabel::Ats)],
        filter: JobFilter::All,
    };
    let view = PipelineView { selected: Some(1) };
    let mut harness = test_harness(&view_model, view, idle_state());

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
    let view_model = PipelineViewModel {
        rows: vec![row(1, "Engineer", None, TierLabel::Ats)],
        filter: JobFilter::Rejected,
    };

    let mut harness = test_harness(&view_model, PipelineView::default(), idle_state());
    harness.run();

    let _ = harness.get_by_label("No jobs match this filter");
    assert!(
        harness.query_by_label("Engineer — Acme").is_none(),
        "the filtered-out row must not be rendered"
    );
}

#[test]
fn selecting_a_row_shows_the_detail_and_placeholder_buttons() {
    let view_model = PipelineViewModel {
        rows: vec![row(
            1,
            "Engineer",
            Some(StatusLabel::Interviewing),
            TierLabel::Ats,
        )],
        filter: JobFilter::All,
    };
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
fn a_selection_that_disappeared_falls_back_to_the_list() {
    let view_model = PipelineViewModel {
        rows: Vec::new(),
        filter: JobFilter::All,
    };
    let view = PipelineView { selected: Some(7) };

    let mut harness = test_harness(&view_model, view, idle_state());
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
    };

    let mut harness = test_harness(&view_model, PipelineView::default(), render_state);
    harness.run();

    let _ = harness.get_by_label("database is locked");
    harness.get_by_role_and_label(Role::Button, "Retry").click();
    harness.run();

    assert!(harness.state().actions.contains(&PipelineAction::Reload));
}
