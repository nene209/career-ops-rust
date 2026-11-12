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

// ----------------------------------------------------------------------
// Fixtures
// ----------------------------------------------------------------------

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
    PromptDetail::from_versions(name, "evaluate", is_system, prompts_for(contents), &usage)
}

fn detail_category(
    name: &str,
    category: &str,
    is_system: bool,
    contents: &[&str],
    usage: HashMap<i64, Vec<PromptUsageEntry>>,
) -> PromptDetail {
    PromptDetail::from_versions(name, category, is_system, prompts_for(contents), &usage)
}

fn prompts_for(contents: &[&str]) -> Vec<Prompt> {
    contents
        .iter()
        .enumerate()
        .map(|(index, content)| Prompt {
            id: (index + 1) as i64,
            name: "evaluate".into(),
            category: "evaluate".into(),
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
