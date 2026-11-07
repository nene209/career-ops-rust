//! Logs screen and drawer tests.
//!
//! The screen is rendered against hand-built lines, so every assertion is about
//! the view: which lines a chip admits, what pause freezes, and which actions
//! leave the screen. The app-level tests cover navigation and the snapshot.

use career_ops_app::screens::logs::{self, LogsAction, LogsRenderState};
use career_ops_app::view_models::logs::{LevelFilter, LogsView};
use career_ops_core::logging::LogLine;
use chrono::Local;
use egui::accesskit::Role;
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;
use tracing::Level;

const POLICY: &str = "info (debug for career-ops targets)";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mount {
    Screen,
    Drawer,
}

struct TestState {
    lines: Vec<LogLine>,
    view: LogsView,
    actions: Vec<LogsAction>,
    path: Option<&'static str>,
    latest_seq: u64,
    dropped: u64,
    confirming_delete: bool,
    message: Option<&'static str>,
    mount: Mount,
}

impl TestState {
    fn render(&self) -> LogsRenderState<'static> {
        LogsRenderState {
            path: self.path,
            policy: POLICY,
            latest_seq: self.latest_seq,
            dropped: self.dropped,
            confirming_delete: self.confirming_delete,
            message: self.message,
        }
    }
}

fn line(seq: u64, level: Level, message: &str, fields: &[(&str, &str)]) -> LogLine {
    LogLine {
        seq,
        timestamp: Local::now(),
        level,
        target: "career_ops_core::acquisition::router",
        message: message.into(),
        fields: fields
            .iter()
            .map(|(key, value)| (key.to_string(), value.to_string()))
            .collect(),
    }
}

fn harness(lines: Vec<LogLine>, view: LogsView, mount: Mount) -> Harness<'static, TestState> {
    let state = TestState {
        latest_seq: lines.iter().map(|line| line.seq).max().unwrap_or(0),
        lines,
        view,
        actions: Vec::new(),
        path: None,
        dropped: 0,
        confirming_delete: false,
        message: None,
        mount,
    };

    Harness::new_ui_state(
        move |ui, state: &mut TestState| {
            // The screen takes the lines by slice while mutating the view, which
            // a test state cannot hand out at the same time.
            let lines = state.lines.clone();
            let render = state.render();
            let action = match state.mount {
                Mount::Screen => logs::show(ui, &lines, &mut state.view, render),
                Mount::Drawer => logs::drawer(ui, &lines, &mut state.view, render),
            };
            if let Some(action) = action {
                state.actions.push(action);
            }
        },
        state,
    )
}

#[test]
fn lines_render_with_their_fields() {
    let info = line(
        1,
        Level::INFO,
        "page read",
        &[("page", "30"), ("jobs", "10")],
    );
    let mut harness = harness(vec![info.clone()], LogsView::default(), Mount::Screen);
    harness.run();

    // The compact form is the row: timestamp, level, target, message, fields.
    harness.get_by_label(&info.compact());
    harness.get_by_label(&format!(
        "Capturing at {POLICY} · this session is in memory only"
    ));
}

#[test]
fn a_level_chip_narrows_the_list() {
    let info = line(1, Level::INFO, "info-marker", &[]);
    let warn = line(2, Level::WARN, "warn-marker", &[]);
    let mut harness = harness(
        vec![info.clone(), warn.clone()],
        LogsView::default(),
        Mount::Screen,
    );

    harness.run();
    harness.get_by_label(&info.compact());

    harness.get_by_label("Warn").click();
    harness.run();

    assert_eq!(harness.state().view.level, LevelFilter::Warn);
    assert!(
        harness.query_by_label(&info.compact()).is_none(),
        "a filter that hides info must hide the info line"
    );
    harness.get_by_label(&warn.compact());

    harness.get_by_label("All").click();
    harness.run();
    harness.get_by_label(&info.compact());
}

#[test]
fn the_search_narrows_the_list() {
    let page = line(1, Level::INFO, "page read", &[("page", "30")]);
    let other = line(2, Level::INFO, "fetch finished", &[]);
    let mut harness = harness(
        vec![page.clone(), other.clone()],
        LogsView::default(),
        Mount::Screen,
    );

    harness.run();
    let field = harness.get_by_label("Filter");
    field.focus();
    field.type_text("page");
    harness.run();

    harness.get_by_label(&page.compact());
    assert!(
        harness.query_by_label(&other.compact()).is_none(),
        "a query that does not match a line hides it"
    );
}

#[test]
fn pause_freezes_the_view_and_counts_new_lines() {
    let first = line(1, Level::INFO, "first-marker", &[]);
    let second = line(2, Level::INFO, "second-marker", &[]);
    let mut harness = harness(vec![first], LogsView::default(), Mount::Screen);

    harness.run();
    harness.get_by_role_and_label(Role::Button, "Pause").click();
    harness.run();
    assert!(harness.state().view.is_paused());

    harness.state_mut().lines.push(second.clone());
    harness.state_mut().latest_seq = 2;
    harness.run();

    harness.get_by_label("1 new line");
    assert!(
        harness.query_by_label(&second.compact()).is_none(),
        "a paused view must not move"
    );

    harness
        .get_by_role_and_label(Role::Button, "Resume")
        .click();
    harness.run();
    assert!(!harness.state().view.is_paused());
    harness.get_by_label(&second.compact());
}

#[test]
fn the_dropped_marker_reports_what_the_buffer_lost() {
    let mut harness = harness(
        vec![line(1, Level::INFO, "kept", &[])],
        LogsView::default(),
        Mount::Screen,
    );
    harness.state_mut().dropped = 3;
    harness.run();

    harness.get_by_label("3 earlier line(s) dropped from the in-memory buffer");
}

#[test]
fn the_copy_button_is_offered_for_the_visible_lines() {
    let mut harness = harness(
        vec![line(1, Level::INFO, "copy-marker", &[])],
        LogsView::default(),
        Mount::Screen,
    );
    harness.run();

    harness.get_by_role_and_label(Role::Button, "Copy visible");
}

#[test]
fn open_folder_needs_a_file_and_delete_asks_first() {
    let mut harness = harness(
        vec![line(1, Level::INFO, "marker", &[])],
        LogsView::default(),
        Mount::Screen,
    );
    harness.run();

    let open = harness.get_by_role_and_label(Role::Button, "Open folder");
    assert!(
        open.accesskit_node().is_disabled(),
        "a buffer-only session has no folder to open"
    );

    harness
        .get_by_role_and_label(Role::Button, "Delete logs")
        .click();
    harness.run();

    assert_eq!(
        harness.state().actions,
        vec![LogsAction::RequestDelete],
        "delete must ask before it deletes"
    );

    // The confirmation is a second step, with its own buttons.
    harness.state_mut().confirming_delete = true;
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Delete permanently")
        .click();
    harness.run();
    assert!(harness.state().actions.contains(&LogsAction::ConfirmDelete));

    harness
        .get_by_role_and_label(Role::Button, "Cancel")
        .click();
    harness.run();
    assert!(harness.state().actions.contains(&LogsAction::CancelDelete));
}

#[test]
fn a_file_session_offers_the_folder() {
    let mut harness = harness(
        vec![line(1, Level::INFO, "marker", &[])],
        LogsView::default(),
        Mount::Screen,
    );
    harness.state_mut().path = Some("/tmp/career-ops/logs/career-ops.log");
    harness.run();

    let open = harness.get_by_role_and_label(Role::Button, "Open folder");
    assert!(!open.accesskit_node().is_disabled());
    open.click();
    harness.run();

    assert_eq!(harness.state().actions, vec![LogsAction::OpenFolder]);
    harness.get_by_label(&format!(
        "Capturing at {POLICY} · /tmp/career-ops/logs/career-ops.log"
    ));
}

#[test]
fn the_drawer_opens_the_screen_and_closes() {
    let marker = line(1, Level::INFO, "drawer-marker", &[]);
    let mut harness = harness(vec![marker.clone()], LogsView::default(), Mount::Drawer);
    harness.run();

    // The drawer renders the same rows as the screen.
    harness.get_by_label(&marker.compact());
    harness
        .get_by_role_and_label(Role::Button, "Open screen")
        .click();
    harness.run();
    assert_eq!(harness.state().actions, vec![LogsAction::OpenScreen]);

    harness.get_by_role_and_label(Role::Button, "Close").click();
    harness.run();
    assert!(harness.state().actions.contains(&LogsAction::CloseDrawer));
}
