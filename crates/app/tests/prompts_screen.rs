//! Prompt Manager screen tests.
//!
//! The screen is rendered with `egui_kittest` against hand-built view-models.
//! Database reads and writes are app-level tests; these cover what the screen
//! shows and which actions it emits.

use std::collections::HashMap;

use career_ops_app::screens::prompts::{self, PromptsAction, PromptsRenderState};
use career_ops_app::view_models::prompts::{
    PromptDetail, PromptSelection, PromptsView, PromptsViewModel,
};
use career_ops_core::model::Prompt;
use career_ops_core::repo::prompts::{PromptThread, PromptUsageEntry, PromptUsageKind};
use egui::accesskit::Role;
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;

struct TestState {
    view_model: PromptsViewModel,
    detail: Option<PromptDetail>,
    view: PromptsView,
    actions: Vec<PromptsAction>,
}

fn harness(state: TestState) -> Harness<'static, TestState> {
    Harness::builder()
        // Tall enough that the whole two-pane layout stays on screen; a clipped
        // scroll area would hide nodes the queries walk to.
        .with_size(egui::vec2(1200.0, 1800.0))
        .build_ui_state(
            move |ui, state: &mut TestState| {
                if let Some(action) = prompts::show(
                    ui,
                    &state.view_model,
                    state.detail.as_ref(),
                    &mut state.view,
                    PromptsRenderState::default(),
                ) {
                    state.actions.push(action);
                }
            },
            state,
        )
}

fn selected(name: &str, is_system: bool) -> PromptsView {
    PromptsView {
        selected: Some(PromptSelection::new(name, is_system)),
        ..PromptsView::default()
    }
}

#[test]
fn a_system_prompt_is_read_only_and_offers_a_copy() {
    let view_model = PromptsViewModel::from_threads(vec![thread("evaluate", true, 1)]);
    let detail = detail("evaluate", true, &["rubric text"], HashMap::new());

    let mut harness = harness(TestState {
        view_model,
        detail: Some(detail),
        view: selected("evaluate", true),
        actions: Vec::new(),
    });
    harness.run();

    let _ = harness.get_by_label("System · read-only");
    let _duplicate = harness.get_by_role_and_label(Role::Button, "Duplicate as my copy");

    assert!(
        harness.query_by_label("Save new version").is_none(),
        "a system prompt must not be saveable"
    );
    assert!(
        harness.query_by_label("Prompt content").is_none(),
        "a system prompt has no editor"
    );
    for label in ["Make current", "Delete…", "Retire…", "Restore"] {
        assert!(
            harness
                .query_by_role_and_label(Role::Button, label)
                .is_none(),
            "a system prompt must not offer `{label}`"
        );
    }

    harness
        .get_by_role_and_label(Role::Button, "Duplicate as my copy")
        .click();
    harness.run();
    assert_eq!(
        harness.state().actions.last(),
        Some(&PromptsAction::DuplicateAsUser)
    );
}

#[test]
fn a_user_copy_offers_the_editor_and_saves_a_new_version() {
    let view_model = PromptsViewModel::from_threads(vec![
        thread("evaluate", true, 1),
        thread("evaluate", false, 2),
    ]);
    let detail = detail(
        "evaluate",
        false,
        &["newer text", "older text"],
        HashMap::new(),
    );

    let mut view = selected("evaluate", false);
    view.editor = "edited text".into();

    let mut harness = harness(TestState {
        view_model,
        detail: Some(detail),
        view,
        actions: Vec::new(),
    });
    harness.run();

    let _ = harness.get_by_label("Prompt content");
    let save = harness.get_by_role_and_label(Role::Button, "Save new version");
    assert!(!save.accesskit_node().is_disabled());
    save.click();
    harness.run();

    assert_eq!(
        harness.state().actions.last(),
        Some(&PromptsAction::SaveDraft("edited text".into()))
    );
}

#[test]
fn version_history_lists_hashes_and_emits_a_selection() {
    let view_model = PromptsViewModel::from_threads(vec![thread("evaluate", false, 2)]);
    let detail = detail(
        "evaluate",
        false,
        &["newer text", "older text"],
        HashMap::new(),
    );
    let older_id = detail.versions[1].id;

    let mut harness = harness(TestState {
        view_model,
        detail: Some(detail),
        view: selected("evaluate", false),
        actions: Vec::new(),
    });
    harness.run();

    let _ = harness.get_by_label("v2");
    let _ = harness.get_by_label("v1");
    // The row carries the content hash prefix and the timestamp.
    let _ = harness.get_by_label("hash-new… · 2026-01-02 03:04 · not used yet");

    harness.get_by_label("v1").click();
    harness.run();
    assert_eq!(
        harness.state().actions.last(),
        Some(&PromptsAction::ShowVersion(Some(older_id)))
    );
}

#[test]
fn usage_lists_the_reports_that_used_the_version() {
    let view_model = PromptsViewModel::from_threads(vec![thread("evaluate", false, 1)]);
    let mut usage = HashMap::new();
    usage.insert(
        1,
        vec![PromptUsageEntry {
            kind: PromptUsageKind::Evaluation,
            id: 7,
            generated_at: fixed_time(),
            model_used: "deepseek-flash".into(),
            job_title: Some("Engineer".into()),
            company: Some("Acme".into()),
        }],
    );
    let detail = detail("evaluate", false, &["text"], usage);

    let mut harness = harness(TestState {
        view_model,
        detail: Some(detail),
        view: selected("evaluate", false),
        actions: Vec::new(),
    });
    harness.run();

    let _ = harness
        .get_by_label("Evaluation #7 · Engineer at Acme · 2026-01-02 03:04 · deepseek-flash");
}

#[test]
fn a_reference_category_explains_that_placeholders_are_free_form() {
    let view_model =
        PromptsViewModel::from_threads(vec![thread_with_category("pdf", "render", true, 1)]);
    let detail = detail_category("pdf", "render", true, &["{{PHOTO}}"], HashMap::new());

    let mut harness = harness(TestState {
        view_model,
        detail: Some(detail),
        view: selected("pdf", true),
        actions: Vec::new(),
    });
    harness.run();

    let _ = harness.get_by_label(
        "Reference prompt: the app does not render this category, so placeholder text is \
         left as written.",
    );
}

#[test]
fn an_activation_emits_no_write_and_keeps_both_versions() {
    let view_model = PromptsViewModel::from_threads(vec![thread("evaluate", false, 2)]);
    let detail = detail(
        "evaluate",
        false,
        &["newer text", "older text"],
        HashMap::new(),
    );
    let older_id = detail.versions[1].id;
    let mut view = selected("evaluate", false);
    view.selected_version = Some(older_id);

    let mut harness = harness(TestState {
        view_model,
        detail: Some(detail),
        view,
        actions: Vec::new(),
    });
    harness.run();

    let _ = harness.get_by_label("Viewing v1");
    assert!(harness.query_by_label("v2").is_some());
    assert!(harness.query_by_label("v1").is_some());
    harness
        .get_by_role_and_label(Role::Button, "Make current")
        .click();
    harness.run();

    assert_eq!(
        harness.state().actions.last(),
        Some(&PromptsAction::SetCurrentVersion(older_id))
    );
    assert!(
        !harness
            .state()
            .actions
            .iter()
            .any(|action| matches!(action, PromptsAction::SaveDraft(_))),
        "activation must not ask for a new version"
    );
}

#[test]
fn the_current_marker_follows_the_pointer() {
    let view_model = PromptsViewModel::from_threads(vec![thread("evaluate", false, 2)]);
    let mut detail = detail(
        "evaluate",
        false,
        &["newer text", "older text"],
        HashMap::new(),
    );
    let older_id = detail.versions[1].id;
    make_current(&mut detail, older_id);

    let mut harness = harness(TestState {
        view_model,
        detail: Some(detail),
        view: selected("evaluate", false),
        actions: Vec::new(),
    });
    harness.run();

    let _ = harness.get_by_label("Current version (v1)");
    assert_eq!(harness.get_all_by_label("current").count(), 1);
}

#[test]
fn the_delete_dialog_names_the_promoted_version() {
    let view_model = PromptsViewModel::from_threads(vec![thread("evaluate", false, 2)]);
    let detail = detail(
        "evaluate",
        false,
        &["newer text", "older text"],
        HashMap::new(),
    );

    let mut harness = harness(TestState {
        view_model,
        detail: Some(detail),
        view: selected("evaluate", false),
        actions: Vec::new(),
    });
    harness.run();

    harness
        .get_by_role_and_label(Role::Button, "Delete…")
        .click();
    harness.run();

    let _ = harness.get_by_label("v1 becomes current.");
    let _ = harness.get_by_role_and_label(Role::Button, "Delete permanently");
    harness
        .get_by_role_and_label(Role::Button, "Cancel")
        .click();
    harness.run();
    assert!(harness.query_by_label("v1 becomes current.").is_none());
}

#[test]
fn the_retire_dialog_names_the_usage_count() {
    let view_model = PromptsViewModel::from_threads(vec![thread("evaluate", false, 1)]);
    let mut usage = HashMap::new();
    usage.insert(
        1,
        vec![PromptUsageEntry {
            kind: PromptUsageKind::Triage,
            id: 9,
            generated_at: fixed_time(),
            model_used: "deepseek-flash".into(),
            job_title: Some("Engineer".into()),
            company: Some("Acme".into()),
        }],
    );
    let detail = detail("evaluate", false, &["only text"], usage);

    let mut harness = harness(TestState {
        view_model,
        detail: Some(detail),
        view: selected("evaluate", false),
        actions: Vec::new(),
    });
    harness.run();

    harness
        .get_by_role_and_label(Role::Button, "Retire…")
        .click();
    harness.run();

    let _ = harness.get_by_label(
        "v1 has been used 1 time. Deleting it would break the link from those runs to this text.",
    );
    harness
        .get_by_role_and_label(Role::Button, "Retire")
        .click();
    harness.run();
    assert_eq!(
        harness.state().actions.last(),
        Some(&PromptsAction::RemoveVersion {
            version_id: 1,
            retire: true,
        })
    );
}

#[test]
fn show_retired_reveals_a_retired_version_and_restore_emits() {
    let view_model = PromptsViewModel::from_threads(vec![thread("evaluate", false, 2)]);
    let mut detail = detail(
        "evaluate",
        false,
        &["newer text", "older text"],
        HashMap::new(),
    );
    let older_id = detail.versions[1].id;
    detail.versions[1].retired_at = Some("2026-01-02 03:04".into());

    let mut harness = harness(TestState {
        view_model,
        detail: Some(detail),
        view: selected("evaluate", false),
        actions: Vec::new(),
    });
    harness.run();

    assert!(
        harness.query_by_label("v1 (retired)").is_none(),
        "retired rows are hidden by default"
    );
    harness.get_by_label("Show retired (1)").click();
    harness.run();

    let _ = harness.get_by_label("v1 (retired)");
    harness.get_by_label("v1 (retired)").click();
    harness.run();
    assert_eq!(
        harness.state().actions.last(),
        Some(&PromptsAction::ShowVersion(Some(older_id)))
    );

    // The app applies the emitted selection; mirror that here so the header
    // and its Restore action render.
    harness.state_mut().view.selected_version = Some(older_id);
    harness.run();

    let _ = harness.get_by_label("Retired v1 (2026-01-02 03:04)");
    harness
        .get_by_role_and_label(Role::Button, "Restore")
        .click();
    harness.run();
    assert_eq!(
        harness.state().actions.last(),
        Some(&PromptsAction::RestoreVersion(older_id))
    );
}

#[test]
fn a_historical_view_returns_to_the_current_version() {
    let view_model = PromptsViewModel::from_threads(vec![thread("evaluate", false, 2)]);
    let detail = detail(
        "evaluate",
        false,
        &["newer text", "older text"],
        HashMap::new(),
    );
    let older_id = detail.versions[1].id;
    let mut view = selected("evaluate", false);
    view.selected_version = Some(older_id);

    let mut harness = harness(TestState {
        view_model,
        detail: Some(detail),
        view,
        actions: Vec::new(),
    });
    harness.run();

    let _ = harness.get_by_label("Viewing v1");
    harness
        .get_by_role_and_label(Role::Button, "Back to current")
        .click();
    harness.run();
    assert_eq!(
        harness.state().actions.last(),
        Some(&PromptsAction::ShowVersion(None))
    );
}

// ----------------------------------------------------------------------
// Fixtures
// ----------------------------------------------------------------------

fn make_current(detail: &mut PromptDetail, version_id: i64) {
    for version in &mut detail.versions {
        version.is_current = version.id == version_id;
    }
    detail.current_id = Some(version_id);
}

fn fixed_time() -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::parse_from_rfc3339("2026-01-02T03:04:00Z")
        .expect("timestamp")
        .with_timezone(&chrono::Utc)
}

fn thread(name: &str, is_system: bool, version: i64) -> PromptThread {
    PromptThread {
        name: name.into(),
        category: "evaluate".into(),
        is_system,
        current_id: version,
        current_version: version,
        version_count: version,
        updated_at: fixed_time(),
    }
}

fn thread_with_category(name: &str, category: &str, is_system: bool, version: i64) -> PromptThread {
    PromptThread {
        name: name.into(),
        category: category.into(),
        is_system,
        current_id: version,
        current_version: version,
        version_count: version,
        updated_at: fixed_time(),
    }
}

fn detail(
    name: &str,
    is_system: bool,
    contents: &[&str],
    usage: HashMap<i64, Vec<PromptUsageEntry>>,
) -> PromptDetail {
    PromptDetail::from_versions(name, is_system, prompts_for("evaluate", contents), &usage)
}

fn detail_category(
    name: &str,
    category: &str,
    is_system: bool,
    contents: &[&str],
    usage: HashMap<i64, Vec<PromptUsageEntry>>,
) -> PromptDetail {
    PromptDetail::from_versions(name, is_system, prompts_for(category, contents), &usage)
}

fn prompts_for(category: &str, contents: &[&str]) -> Vec<Prompt> {
    contents
        .iter()
        .enumerate()
        .map(|(index, content)| Prompt {
            id: (index + 1) as i64,
            name: "evaluate".into(),
            category: category.into(),
            content: (*content).into(),
            is_system: false,
            version: (contents.len() - index) as i64,
            // The fixture's newest version is `hash-new`, the one after it
            // `hash-2`, so the list reads predictably.
            content_hash: if index == 0 {
                "hash-new0123456789".into()
            } else {
                format!("hash-{}0123456789", index + 1)
            },
            supersedes_id: None,
            is_current: index == 0,
            retired_at: None,
            created_at: fixed_time(),
            updated_at: fixed_time(),
        })
        .collect()
}
