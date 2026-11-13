//! Usage screen tests.
//!
//! The screen is rendered against hand-built view-models, so every assertion is
//! about what it shows and which actions it emits. The repository reads and the
//! lazy load are covered by the app and core tests.

use career_ops_app::screens::usage::{self, UsageAction, UsageRenderState};
use career_ops_app::view_models::usage::{Range, UsageViewModel};
use career_ops_core::repo::llm_usage::{ProviderModelTotals, RecentCall, TaskTotals};
use chrono::{TimeZone, Utc};
use egui::accesskit::Role;
use egui_kittest::kittest::Queryable;
use egui_kittest::Harness;

struct TestState {
    view_model: UsageViewModel,
    state: UsageRenderState<'static>,
    actions: Vec<UsageAction>,
}

fn harness(view_model: UsageViewModel) -> Harness<'static, TestState> {
    harness_with(TestState {
        view_model,
        state: UsageRenderState::default(),
        actions: Vec::new(),
    })
}

fn harness_with(state: TestState) -> Harness<'static, TestState> {
    Harness::builder()
        // Tall enough that the section grids stay laid out; a clipped scroll
        // area would hide nodes the queries walk to.
        .with_size(egui::vec2(1280.0, 1600.0))
        .build_ui_state(
            move |ui, state: &mut TestState| {
                if let Some(action) = usage::show(ui, &state.view_model, state.state) {
                    state.actions.push(action);
                }
            },
            state,
        )
}

fn group(cost: Option<f64>, unpriced: i64, reported: i64, estimated: i64) -> ProviderModelTotals {
    ProviderModelTotals {
        provider: "deepseek".into(),
        model: "deepseek-flash".into(),
        calls: reported + estimated,
        input_tokens: 1_234,
        output_tokens: 567,
        total_tokens: 1_801,
        cost_usd: cost,
        unpriced_calls: unpriced,
        reported_calls: reported,
        estimated_calls: estimated,
    }
}

fn task() -> TaskTotals {
    TaskTotals {
        task: "evaluation".into(),
        calls: 1,
        input_tokens: 1_000,
        output_tokens: 500,
        total_tokens: 1_500,
        cost_usd: Some(0.001),
        unpriced_calls: 0,
    }
}

fn call(outcome: &str, cost: Option<f64>) -> RecentCall {
    RecentCall {
        id: 1,
        provider: "deepseek".into(),
        model: "deepseek-flash".into(),
        task: "evaluation".into(),
        job_title: Some("Engineer".into()),
        company: Some("Acme".into()),
        input_tokens: Some(8_120),
        output_tokens: Some(940),
        total_tokens: Some(9_060),
        source: "reported".into(),
        cost_usd: cost,
        cost_complete: Some(true),
        outcome: outcome.into(),
        error_kind: None,
        duration_ms: 900,
        created_at: Utc.with_ymd_and_hms(2026, 11, 7, 14, 2, 0).unwrap(),
    }
}

fn view_model() -> UsageViewModel {
    UsageViewModel::from_rows(
        Range::Last30Days,
        vec![group(Some(0.0018), 0, 1, 1)],
        vec![task()],
        vec![call("ok", Some(0.0018))],
    )
}

#[test]
fn the_header_shows_the_totals_and_the_provenance() {
    let view_model = view_model();
    let summary = view_model.totals.summary();
    let provenance = view_model.totals.provenance();
    let mut harness = harness(view_model);
    harness.run_steps(2);

    harness.get_by_label(&summary);
    harness.get_by_label(&provenance);
    harness.get_by_label("By provider and model");
    harness.get_by_label("By task");
    harness.get_by_label("Recent calls");
}

#[test]
fn the_provider_model_grid_shows_its_row() {
    let mut harness = harness(view_model());
    harness.run_steps(2);

    // The ledger stores the provider id; the screen shows its label.
    harness.get_by_label("DeepSeek");
    harness.get_by_label("1,234");
    harness.get_by_label("567");
    harness.get_by_label("1,801");
}

#[test]
fn the_task_grid_names_the_task() {
    let mut harness = harness(view_model());
    harness.run_steps(2);

    harness.get_by_label("1,000");
    harness.get_by_label("1,500");
}

#[test]
fn the_recent_calls_list_names_the_job_and_the_outcome() {
    let mut harness = harness(view_model());
    harness.run_steps(2);

    harness.get_by_label("Engineer · Acme");
    harness.get_by_label("8,120 / 940");
    harness.get_by_label("2026-11-07 14:02");
    harness.get_by_label("ok");
}

#[test]
fn the_range_chips_emit_a_change() {
    let mut harness = harness(view_model());
    harness.run_steps(2);

    harness
        .get_by_role_and_label(Role::Button, "7 days")
        .click();
    harness.run_steps(2);

    assert_eq!(
        harness.state().actions.last(),
        Some(&UsageAction::SetRange(Range::Last7Days))
    );
}

#[test]
fn reload_emits_a_reload() {
    let mut harness = harness(view_model());
    harness.run_steps(2);

    harness
        .get_by_role_and_label(Role::Button, "Reload")
        .click();
    harness.run_steps(2);

    assert_eq!(harness.state().actions.last(), Some(&UsageAction::Reload));
}

#[test]
fn an_empty_ledger_shows_the_empty_state() {
    let mut harness = harness(UsageViewModel::from_rows(
        Range::Last30Days,
        Vec::new(),
        Vec::new(),
        Vec::new(),
    ));
    harness.run_steps(2);

    harness.get_by_label("No LLM calls recorded yet");
}

#[test]
fn an_unpriced_group_is_not_drawn_as_zero() {
    let mut harness = harness(UsageViewModel::from_rows(
        Range::Last30Days,
        vec![group(None, 1, 0, 1)],
        vec![task()],
        Vec::new(),
    ));
    harness.run_steps(2);

    assert!(
        harness.query_by_label("est. $0.00").is_none(),
        "an unknown cost must not become a zero"
    );
    // The cost cell is the only em dash without a recent-calls row.
    harness.get_by_label("—");
}

#[test]
fn a_partial_cost_is_a_lower_bound() {
    let mut harness = harness(UsageViewModel::from_rows(
        Range::Last30Days,
        vec![group(Some(0.5), 1, 1, 1)],
        Vec::new(),
        Vec::new(),
    ));
    harness.run_steps(2);

    harness.get_by_label("≥ $0.50");
}

#[test]
fn an_error_call_is_visible() {
    let mut harness = harness(UsageViewModel::from_rows(
        Range::Last30Days,
        vec![group(Some(0.0018), 0, 1, 0)],
        Vec::new(),
        vec![call("error", None)],
    ));
    harness.run_steps(2);

    harness.get_by_label("error");
}

#[test]
fn an_error_message_is_shown_with_a_retry() {
    let mut harness = harness_with(TestState {
        view_model: UsageViewModel::from_rows(
            Range::Last30Days,
            Vec::new(),
            Vec::new(),
            Vec::new(),
        ),
        state: UsageRenderState {
            loading: false,
            error: Some("The database is not open."),
        },
        actions: Vec::new(),
    });
    harness.run_steps(2);

    harness.get_by_label("The database is not open.");
    harness.get_by_role_and_label(Role::Button, "Retry").click();
    harness.run_steps(2);

    assert_eq!(harness.state().actions.last(), Some(&UsageAction::Reload));
}
