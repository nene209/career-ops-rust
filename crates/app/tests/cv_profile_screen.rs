//! CV / Profile screen tests.
//!
//! The screen is rendered with `egui_kittest` against hand-built view-models.
//! The database round trip lives in `app.rs`; these cover the fields, the save
//! gating, the entry forms, the delete confirmation, reordering, and that the
//! preview is the canonical renderer's output.

use career_ops_app::screens::cv_profile::{
    self, CvProfileAction, CvProfileRenderState, CvProfileView,
};
use career_ops_app::view_models::cv_profile::{CvProfileViewModel, MoveDirection};
use career_ops_core::model::{CvEntry, Profile};
use career_ops_core::repo::cv_entries::CvEntryKind;
use career_ops_core::repo::profile::ProfileDetails;
use chrono::Utc;
use egui::accesskit::Role;
use egui_kittest::kittest::{NodeT, Queryable};
use egui_kittest::Harness;

struct TestState {
    view_model: CvProfileViewModel,
    view: CvProfileView,
    actions: Vec<CvProfileAction>,
}

fn profile() -> Profile {
    Profile {
        id: 1,
        name: "Ada Lovelace".into(),
        email: "ada@example.com".into(),
        phone: Some("+44 20 0000 0000".into()),
        location: Some("London".into()),
        summary: Some("Mathematician.".into()),
        archetypes: None,
        preferences: None,
        created_at: Utc::now(),
        updated_at: Utc::now(),
    }
}

fn entry(id: i64, kind: CvEntryKind, title: &str, sort_order: i64) -> CvEntry {
    CvEntry {
        id,
        profile_id: 1,
        kind: kind.as_str().into(),
        title: title.into(),
        organization: None,
        start_date: None,
        end_date: None,
        description: None,
        metadata: None,
        sort_order,
    }
}

fn state() -> TestState {
    TestState {
        view_model: CvProfileViewModel::from_profile(profile(), Vec::new()),
        view: CvProfileView::default(),
        actions: Vec::new(),
    }
}

fn state_with(entries: Vec<CvEntry>) -> TestState {
    TestState {
        view_model: CvProfileViewModel::from_profile(profile(), entries),
        view: CvProfileView::default(),
        actions: Vec::new(),
    }
}

fn harness(state: TestState) -> Harness<'static, TestState> {
    Harness::builder()
        // Tall enough that the whole form stays on screen: the queries below
        // walk the accessibility tree, and a clipped scroll area would hide
        // the preview node.
        .with_size(egui::vec2(900.0, 2400.0))
        .build_ui_state(
            move |ui, state: &mut TestState| {
                if let Some(action) = cv_profile::show(
                    ui,
                    &mut state.view_model,
                    &mut state.view,
                    CvProfileRenderState::default(),
                ) {
                    state.actions.push(action);
                }
            },
            state,
        )
}

fn titles(view_model: &CvProfileViewModel) -> Vec<String> {
    view_model.groups()[0]
        .rows
        .iter()
        .map(|row| row.title.clone())
        .collect()
}

#[test]
fn profile_fields_are_queryable_by_their_labels() {
    let mut harness = harness(state());
    harness.run();

    for label in ["Name", "Email", "Phone", "Location", "Summary"] {
        let _ = harness.get_by_label(label);
    }

    assert!(
        harness.unnamed_widgets().is_empty(),
        "every text input must have an accessible name"
    );
}

#[test]
fn save_profile_is_disabled_until_the_form_changes() {
    let mut harness = harness(state());
    harness.run();

    let save = harness.get_by_role_and_label(Role::Button, "Save profile");
    assert!(
        save.accesskit_node().is_disabled(),
        "an unchanged form must not offer a save"
    );

    harness.state_mut().view_model.form.summary = "New summary.".into();
    harness.run();

    let save = harness.get_by_role_and_label(Role::Button, "Save profile");
    assert!(!save.accesskit_node().is_disabled());
    save.click();
    harness.run();

    let expected = ProfileDetails {
        name: "Ada Lovelace".into(),
        email: "ada@example.com".into(),
        phone: Some("+44 20 0000 0000".into()),
        location: Some("London".into()),
        summary: Some("New summary.".into()),
    };
    assert_eq!(
        harness.state().actions.last(),
        Some(&CvProfileAction::SaveProfile(expected))
    );
}

#[test]
fn the_entry_save_button_needs_a_title() {
    let mut harness = harness(state());
    harness.run();

    harness
        .get_by_role_and_label(Role::Button, "Add experience")
        .click();
    harness.run();

    let save = harness.get_by_role_and_label(Role::Button, "Save entry");
    assert!(
        save.accesskit_node().is_disabled(),
        "an entry without a title must not be saveable"
    );
    let _ = harness.get_by_label("A title is required.");

    harness
        .state_mut()
        .view
        .adding
        .as_mut()
        .expect("the add form is open")
        .title = "Engineer".into();
    harness.run();

    let save = harness.get_by_role_and_label(Role::Button, "Save entry");
    assert!(!save.accesskit_node().is_disabled());
    save.click();
    harness.run();

    match harness.state().actions.last() {
        Some(CvProfileAction::CreateEntry(draft)) => {
            assert_eq!(draft.title, "Engineer");
            assert_eq!(draft.kind, CvEntryKind::Experience);
        }
        other => panic!("expected a create action, got {other:?}"),
    }
}

#[test]
fn a_saved_entry_shows_under_its_group() {
    let mut harness = harness(state());
    harness.run();

    harness
        .get_by_role_and_label(Role::Button, "Add education")
        .click();
    harness.run();
    harness
        .state_mut()
        .view
        .adding
        .as_mut()
        .expect("the add form is open")
        .title = "BSc Mathematics".into();
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Save entry")
        .click();
    harness.run();

    // Stand in for the app: the write succeeded and the list was reloaded.
    let saved = CvEntry {
        organization: Some("University of London".into()),
        start_date: Some("2015".into()),
        end_date: Some("2019".into()),
        ..entry(1, CvEntryKind::Education, "BSc Mathematics", 0)
    };
    harness.state_mut().view_model.replace_entries(vec![saved]);
    harness.state_mut().view.adding = None;
    harness.run();

    let _ = harness.get_by_label("BSc Mathematics");
    let _ = harness.get_by_label("University of London");
    let _ = harness.get_by_label("2015 - 2019");
}

#[test]
fn delete_asks_for_confirmation_and_cancel_keeps_the_row() {
    let mut harness = harness(state_with(vec![entry(
        1,
        CvEntryKind::Experience,
        "Engineer",
        0,
    )]));
    harness.run();

    harness
        .get_by_role_and_label(Role::Button, "Delete")
        .click();
    harness.run();

    let _ = harness.get_by_label("Delete this entry?");
    harness
        .get_by_role_and_label(Role::Button, "Cancel")
        .click();
    harness.run();

    assert!(harness.state().view.deleting.is_none());
    assert!(
        harness.state().actions.is_empty(),
        "cancelling must not emit anything"
    );
    let _ = harness.get_by_role_and_label(Role::Button, "Edit");
}

#[test]
fn confirming_the_delete_emits_the_action() {
    let mut harness = harness(state_with(vec![entry(
        1,
        CvEntryKind::Experience,
        "Engineer",
        0,
    )]));
    harness.run();

    harness
        .get_by_role_and_label(Role::Button, "Delete")
        .click();
    harness.run();
    harness
        .get_by_role_and_label(Role::Button, "Confirm delete")
        .click();
    harness.run();

    assert_eq!(
        harness.state().actions.last(),
        Some(&CvProfileAction::DeleteEntry(1))
    );
}

#[test]
fn move_buttons_are_gated_at_the_edges_of_a_group() {
    let mut harness = harness(state_with(vec![
        entry(1, CvEntryKind::Experience, "First", 0),
        entry(2, CvEntryKind::Experience, "Second", 1),
    ]));
    harness.run();

    let ups: Vec<bool> = harness
        .get_all_by_role_and_label(Role::Button, "Move up")
        .map(|node| node.accesskit_node().is_disabled())
        .collect();
    assert_eq!(ups, vec![true, false], "only the first row can move up");

    let downs: Vec<bool> = harness
        .get_all_by_role_and_label(Role::Button, "Move down")
        .map(|node| node.accesskit_node().is_disabled())
        .collect();
    assert_eq!(downs, vec![false, true], "only the last row can move down");
}

#[test]
fn moving_a_row_down_emits_the_direction_and_reorders_the_list() {
    let mut harness = harness(state_with(vec![
        entry(1, CvEntryKind::Experience, "First", 0),
        entry(2, CvEntryKind::Experience, "Second", 1),
    ]));
    harness.run();

    harness
        .get_all_by_role_and_label(Role::Button, "Move down")
        .next()
        .expect("the first row has a move-down button")
        .click();
    harness.run();

    let action = harness.state().actions.last().cloned().expect("an action");
    assert_eq!(
        action,
        CvProfileAction::MoveEntry {
            id: 1,
            direction: MoveDirection::Down,
        }
    );

    // Stand in for the app, which applies the move before the next frame.
    if let CvProfileAction::MoveEntry { id, direction } = action {
        harness.state_mut().view_model.move_entry(id, direction);
    }
    harness.run();

    assert_eq!(titles(&harness.state().view_model), vec!["Second", "First"]);
    let second_top = harness.get_by_label("Second").rect().top();
    let first_top = harness.get_by_label("First").rect().top();
    assert!(
        second_top < first_top,
        "the reordered row must render above the other one"
    );
}

#[test]
fn the_preview_is_the_renderer_output() {
    let mut state = state_with(vec![entry(1, CvEntryKind::Skill, "Rust", 0)]);
    state.view_model.form.summary = "Unsaved summary.".into();
    let expected = state.view_model.preview().expect("preview");

    let mut harness = harness(state);
    harness.run();

    // The preview is one label whose text is exactly `render_cv_summary`'s
    // output, which is what Phase 3 will send.
    let _ = harness.get_by_label(expected.as_str());
}

#[test]
fn an_empty_editor_explains_the_preview_instead_of_showing_one() {
    let mut harness = harness(TestState {
        view_model: CvProfileViewModel::default(),
        view: CvProfileView::default(),
        actions: Vec::new(),
    });
    harness.run();

    let _ = harness
        .get_by_label("Add a name, a summary, or a CV entry and the summary will appear here.");
}
