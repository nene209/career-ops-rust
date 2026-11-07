//! CV / Profile view-model.
//!
//! The editor holds the profile fields as editable strings, the persisted row
//! they were loaded from (for the dirty comparison), and the CV entries. It
//! never formats the preview itself: `preview` calls
//! `repo::profile::render_cv_summary`, the same function Phase 3 uses, so the
//! screen and the prompt cannot drift.
//!
//! Grouping and reordering are pure functions over the row list, which is why
//! they live here and not in the screen.

use chrono::Utc;

use career_ops_core::model::{CvEntry, Profile};
use career_ops_core::repo::cv_entries::{CvEntryKind, NewCvEntry};
use career_ops_core::repo::profile::{self, ProfileDetails};

/// The kinds in display order. The summary renderer uses the same order, so a
/// reload never reshuffles a section.
pub const KIND_ORDER: [CvEntryKind; 3] = [
    CvEntryKind::Experience,
    CvEntryKind::Education,
    CvEntryKind::Skill,
];

/// The group heading for a kind.
pub fn kind_title(kind: CvEntryKind) -> &'static str {
    match kind {
        CvEntryKind::Experience => "Experience",
        CvEntryKind::Education => "Education",
        CvEntryKind::Skill => "Skills",
    }
}

/// The label of the per-group add button, e.g. "Add skill".
pub fn kind_add_label(kind: CvEntryKind) -> &'static str {
    match kind {
        CvEntryKind::Experience => "Add experience",
        CvEntryKind::Education => "Add education",
        CvEntryKind::Skill => "Add skill",
    }
}

/// The profile fields as the editor holds them.
///
/// Everything is a `String` because that is what `TextEdit` edits; empty means
/// "not set". `ProfileForm::apply_to` and `details` trim and map blanks to
/// `None`, matching what `profile::update_details` stores.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProfileForm {
    pub name: String,
    pub email: String,
    pub phone: String,
    pub location: String,
    pub summary: String,
}

impl ProfileForm {
    pub fn from_profile(profile: &Profile) -> Self {
        Self {
            name: profile.name.clone(),
            email: profile.email.clone(),
            phone: profile.phone.clone().unwrap_or_default(),
            location: profile.location.clone().unwrap_or_default(),
            summary: profile.summary.clone().unwrap_or_default(),
        }
    }

    /// Write the trimmed form values onto `profile`, leaving every column the
    /// editor does not own untouched. Used for the preview, not for saving.
    pub fn apply_to(&self, profile: &mut Profile) {
        profile.name = self.name.trim().to_string();
        profile.email = self.email.trim().to_string();
        profile.phone = optional(&self.phone);
        profile.location = optional(&self.location);
        profile.summary = optional(&self.summary);
    }

    pub fn details(&self) -> ProfileDetails {
        ProfileDetails {
            name: self.name.trim().to_string(),
            email: self.email.trim().to_string(),
            phone: optional(&self.phone),
            location: optional(&self.location),
            summary: optional(&self.summary),
        }
    }
}

/// One CV entry as the screen displays it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CvEntryRow {
    pub id: i64,
    pub kind: CvEntryKind,
    pub title: String,
    pub organization: Option<String>,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
    pub description: Option<String>,
    /// Carried through an edit so a value written by a future importer is not
    /// silently dropped.
    pub metadata: Option<String>,
    pub sort_order: i64,
}

impl CvEntryRow {
    /// `None` for a kind this build does not know. The editor is the only
    /// writer of `cv_entries`, so an unknown kind means the row came from
    /// somewhere else; showing it as an editable entry would rewrite its kind.
    pub fn from_entry(entry: &CvEntry) -> Option<Self> {
        Some(Self {
            id: entry.id,
            kind: CvEntryKind::parse(&entry.kind)?,
            title: entry.title.clone(),
            organization: entry.organization.clone(),
            start_date: entry.start_date.clone(),
            end_date: entry.end_date.clone(),
            description: entry.description.clone(),
            metadata: entry.metadata.clone(),
            sort_order: entry.sort_order,
        })
    }

    /// The persisted shape, for the preview renderer.
    pub fn to_entry(&self, profile_id: i64) -> CvEntry {
        CvEntry {
            id: self.id,
            profile_id,
            kind: self.kind.as_str().to_string(),
            title: self.title.clone(),
            organization: self.organization.clone(),
            start_date: self.start_date.clone(),
            end_date: self.end_date.clone(),
            description: self.description.clone(),
            metadata: self.metadata.clone(),
            sort_order: self.sort_order,
        }
    }

    pub fn to_draft(&self) -> CvEntryDraft {
        CvEntryDraft {
            kind: self.kind,
            title: self.title.clone(),
            organization: self.organization.clone().unwrap_or_default(),
            start_date: self.start_date.clone().unwrap_or_default(),
            end_date: self.end_date.clone().unwrap_or_default(),
            description: self.description.clone().unwrap_or_default(),
        }
    }

    /// The row's date range without the summary's parentheses: `2020 - Present`,
    /// `2021`, or `until 2022`.
    pub fn dates(&self) -> Option<String> {
        match (
            self.start_date.as_deref().map(str::trim),
            self.end_date.as_deref().map(str::trim),
        ) {
            (Some(start), Some(end)) => Some(format!("{start} - {end}")),
            (Some(start), None) => Some(start.to_string()),
            (None, Some(end)) => Some(format!("until {end}")),
            (None, None) => None,
        }
    }
}

/// The editable fields of one entry, for the add and edit forms.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CvEntryDraft {
    pub kind: CvEntryKind,
    pub title: String,
    pub organization: String,
    pub start_date: String,
    pub end_date: String,
    pub description: String,
}

impl CvEntryDraft {
    pub fn new(kind: CvEntryKind) -> Self {
        Self {
            kind,
            title: String::new(),
            organization: String::new(),
            start_date: String::new(),
            end_date: String::new(),
            description: String::new(),
        }
    }

    /// Only the title is required; dates stay free text by design.
    pub fn is_valid(&self) -> bool {
        !self.title.trim().is_empty()
    }

    /// Build the repository input. `metadata` is carried over from the row
    /// being edited; a new entry passes `None`.
    pub fn to_new_entry(&self, profile_id: i64, metadata: Option<&str>) -> NewCvEntry {
        NewCvEntry {
            profile_id,
            kind: self.kind,
            title: self.title.trim().to_string(),
            organization: optional(&self.organization),
            start_date: optional(&self.start_date),
            end_date: optional(&self.end_date),
            description: optional(&self.description),
            metadata: metadata.and_then(|value| serde_json::from_str(value).ok()),
            sort_order: None,
        }
    }
}

/// Which way a row moves within its group.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MoveDirection {
    Up,
    Down,
}

/// One kind's rows in display order.
#[derive(Debug)]
pub struct CvEntryGroup<'a> {
    pub kind: CvEntryKind,
    pub title: &'static str,
    pub rows: Vec<&'a CvEntryRow>,
}

#[derive(Debug)]
pub struct CvProfileViewModel {
    /// The last loaded or saved row. `form` may be newer.
    pub profile: Profile,
    pub form: ProfileForm,
    pub entries: Vec<CvEntryRow>,
}

impl Default for CvProfileViewModel {
    fn default() -> Self {
        Self {
            profile: empty_profile(),
            form: ProfileForm::default(),
            entries: Vec::new(),
        }
    }
}

impl CvProfileViewModel {
    pub fn from_profile(profile: Profile, entries: Vec<CvEntry>) -> Self {
        Self {
            form: ProfileForm::from_profile(&profile),
            profile,
            entries: rows_from_entries(&entries),
        }
    }

    /// Adopt the row a save returned. The form takes the stored (trimmed)
    /// values, which is what clears the dirty state.
    pub fn apply_saved_profile(&mut self, profile: Profile) {
        self.form = ProfileForm::from_profile(&profile);
        self.profile = profile;
    }

    pub fn replace_entries(&mut self, entries: Vec<CvEntry>) {
        self.entries = rows_from_entries(&entries);
    }

    /// The form differs from the row it was loaded from.
    pub fn is_dirty(&self) -> bool {
        self.form != ProfileForm::from_profile(&self.profile)
    }

    pub fn details(&self) -> ProfileDetails {
        self.form.details()
    }

    /// The canonical summary for the editor's current state.
    ///
    /// The fields come from the form, the entries from the last load; entry
    /// writes reload the list, so in practice both are current.
    pub fn preview(&self) -> Option<String> {
        let mut profile = self.profile.clone();
        self.form.apply_to(&mut profile);
        let entries: Vec<CvEntry> = self
            .entries
            .iter()
            .map(|row| row.to_entry(profile.id))
            .collect();
        profile::render_cv_summary(&profile, &entries)
    }

    /// Every kind in display order, each group sorted by `(sort_order, id)`.
    pub fn groups(&self) -> Vec<CvEntryGroup<'_>> {
        KIND_ORDER
            .iter()
            .map(|&kind| CvEntryGroup {
                kind,
                title: kind_title(kind),
                rows: self.rows_in_order(kind),
            })
            .collect()
    }

    pub fn entry(&self, id: i64) -> Option<&CvEntryRow> {
        self.entries.iter().find(|row| row.id == id)
    }

    /// Move a row one position within its kind.
    ///
    /// Returns the complete display order (Experience, then Education, then
    /// Skills) when the move happened, ready for `cv_entries::reorder`, and
    /// renumbers the in-memory `sort_order` to match so the next frame shows
    /// the new order without a reload. `None` when the row is already at the
    /// edge of its group; a row never moves into another kind.
    pub fn move_entry(&mut self, id: i64, direction: MoveDirection) -> Option<Vec<i64>> {
        let kind = self.entries.iter().find(|row| row.id == id)?.kind;

        let mut group: Vec<i64> = self.rows_in_order(kind).iter().map(|row| row.id).collect();
        let index = group.iter().position(|&row_id| row_id == id)?;
        let target = match direction {
            MoveDirection::Up if index > 0 => index - 1,
            MoveDirection::Down if index + 1 < group.len() => index + 1,
            _ => return None,
        };
        group.swap(index, target);

        let mut order: Vec<i64> = Vec::with_capacity(self.entries.len());
        for group_kind in KIND_ORDER {
            if group_kind == kind {
                order.extend(group.iter().copied());
            } else {
                order.extend(self.rows_in_order(group_kind).iter().map(|row| row.id));
            }
        }

        for (position, row_id) in order.iter().enumerate() {
            if let Some(row) = self.entries.iter_mut().find(|row| row.id == *row_id) {
                row.sort_order = position as i64;
            }
        }

        Some(order)
    }

    fn rows_in_order(&self, kind: CvEntryKind) -> Vec<&CvEntryRow> {
        let mut rows: Vec<&CvEntryRow> =
            self.entries.iter().filter(|row| row.kind == kind).collect();
        rows.sort_by_key(|row| (row.sort_order, row.id));
        rows
    }
}

fn rows_from_entries(entries: &[CvEntry]) -> Vec<CvEntryRow> {
    entries.iter().filter_map(CvEntryRow::from_entry).collect()
}

fn optional(value: &str) -> Option<String> {
    let trimmed = value.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_string())
}

fn empty_profile() -> Profile {
    let now = Utc::now();
    Profile {
        id: 1,
        name: String::new(),
        email: String::new(),
        phone: None,
        location: None,
        summary: None,
        archetypes: None,
        preferences: None,
        created_at: now,
        updated_at: now,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use career_ops_core::model::CvEntry;

    fn profile() -> Profile {
        Profile {
            id: 1,
            name: "Ada Lovelace".into(),
            email: "ada@example.com".into(),
            phone: Some("+44 20 0000 0000".into()),
            location: Some("London".into()),
            summary: Some("Mathematician.".into()),
            ..empty_profile()
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

    #[test]
    fn the_form_round_trips_through_a_profile() {
        let form = ProfileForm::from_profile(&profile());
        assert_eq!(form.name, "Ada Lovelace");
        assert_eq!(form.phone, "+44 20 0000 0000");

        let mut target = empty_profile();
        form.apply_to(&mut target);
        assert_eq!(target.name, "Ada Lovelace");
        assert_eq!(target.phone.as_deref(), Some("+44 20 0000 0000"));
        assert_eq!(target.email, "ada@example.com");
    }

    #[test]
    fn blank_fields_become_none_and_are_trimmed() {
        let form = ProfileForm {
            name: "  Ada  ".into(),
            email: "  ada@example.com ".into(),
            phone: "   ".into(),
            location: String::new(),
            summary: "  A summary.  ".into(),
        };

        let details = form.details();
        assert_eq!(details.name, "Ada");
        assert_eq!(details.email, "ada@example.com");
        assert_eq!(details.phone, None);
        assert_eq!(details.location, None);
        assert_eq!(details.summary.as_deref(), Some("A summary."));
    }

    #[test]
    fn dirty_tracks_the_saved_row() {
        let mut view_model = CvProfileViewModel::from_profile(profile(), Vec::new());
        assert!(!view_model.is_dirty());

        view_model.form.name.push('!');
        assert!(view_model.is_dirty());

        let mut saved = profile();
        saved.name.push('!');
        view_model.apply_saved_profile(saved);
        assert!(!view_model.is_dirty());
    }

    #[test]
    fn a_whitespace_only_edit_is_dirty_until_it_is_saved() {
        let mut view_model = CvProfileViewModel::from_profile(profile(), Vec::new());
        view_model.form.phone = "   ".into();
        assert!(view_model.is_dirty());

        let mut saved = profile();
        saved.phone = None;
        view_model.apply_saved_profile(saved);
        assert!(!view_model.is_dirty());
        assert_eq!(view_model.form.phone, "");
    }

    #[test]
    fn the_preview_matches_the_renderer() {
        let entries = vec![
            entry(1, CvEntryKind::Experience, "Engineer", 0),
            entry(2, CvEntryKind::Skill, "Rust", 1),
        ];
        let view_model = CvProfileViewModel::from_profile(profile(), entries.clone());

        let mut expected_profile = profile();
        view_model.form.apply_to(&mut expected_profile);
        let expected = profile::render_cv_summary(&expected_profile, &entries);

        assert_eq!(view_model.preview(), expected);
        assert!(view_model
            .preview()
            .is_some_and(|text| text.contains("- Engineer")));
    }

    #[test]
    fn the_preview_shows_unsaved_edits() {
        let mut view_model = CvProfileViewModel::from_profile(profile(), Vec::new());
        view_model.form.summary = "Fresh summary.".into();
        view_model.form.name = "Grace Hopper".into();

        let preview = view_model.preview().expect("preview");
        assert!(preview.starts_with("Grace Hopper"), "{preview}");
        assert!(preview.contains("Summary:\nFresh summary."), "{preview}");
    }

    #[test]
    fn an_empty_editor_has_no_preview() {
        let view_model = CvProfileViewModel::default();
        assert_eq!(view_model.preview(), None);
    }

    #[test]
    fn groups_follow_the_fixed_kind_order() {
        let entries = vec![
            entry(1, CvEntryKind::Skill, "Rust", 0),
            entry(2, CvEntryKind::Education, "BSc", 1),
            entry(3, CvEntryKind::Experience, "Engineer", 2),
        ];
        let view_model = CvProfileViewModel::from_profile(profile(), entries);

        let titles: Vec<&str> = view_model
            .groups()
            .iter()
            .map(|group| group.title)
            .collect();
        assert_eq!(titles, vec!["Experience", "Education", "Skills"]);
    }

    #[test]
    fn groups_sort_by_sort_order_then_id() {
        let entries = vec![
            entry(3, CvEntryKind::Experience, "Third", 2),
            entry(1, CvEntryKind::Experience, "First", 0),
            entry(2, CvEntryKind::Experience, "Second", 0),
        ];
        let view_model = CvProfileViewModel::from_profile(profile(), entries);

        let groups = view_model.groups();
        let titles: Vec<&str> = groups[0]
            .rows
            .iter()
            .map(|row| row.title.as_str())
            .collect();
        assert_eq!(titles, vec!["First", "Second", "Third"]);
    }

    #[test]
    fn moving_down_swaps_within_the_group_and_returns_the_full_order() {
        let entries = vec![
            entry(1, CvEntryKind::Experience, "First", 0),
            entry(2, CvEntryKind::Experience, "Second", 1),
            entry(3, CvEntryKind::Skill, "Rust", 2),
        ];
        let mut view_model = CvProfileViewModel::from_profile(profile(), entries);

        let order = view_model
            .move_entry(1, MoveDirection::Down)
            .expect("the first row can move down");

        assert_eq!(order, vec![2, 1, 3]);
        let groups = view_model.groups();
        let titles: Vec<&str> = groups[0]
            .rows
            .iter()
            .map(|row| row.title.as_str())
            .collect();
        assert_eq!(titles, vec!["Second", "First"]);
        assert_eq!(groups[2].rows[0].id, 3, "the other kind keeps its place");
    }

    #[test]
    fn moving_up_from_the_top_of_a_group_does_nothing() {
        let entries = vec![
            entry(1, CvEntryKind::Experience, "First", 0),
            entry(2, CvEntryKind::Education, "BSc", 1),
        ];
        let mut view_model = CvProfileViewModel::from_profile(profile(), entries);

        assert_eq!(view_model.move_entry(1, MoveDirection::Up), None);
        assert_eq!(
            view_model.move_entry(2, MoveDirection::Down),
            None,
            "the last education row cannot move into skills"
        );
    }

    #[test]
    fn a_move_renumbers_the_whole_display_order() {
        let entries = vec![
            entry(1, CvEntryKind::Experience, "Old job", 0),
            entry(2, CvEntryKind::Experience, "New job", 1),
            entry(3, CvEntryKind::Education, "BSc", 2),
            entry(4, CvEntryKind::Skill, "Rust", 3),
        ];
        let mut view_model = CvProfileViewModel::from_profile(profile(), entries);

        view_model
            .move_entry(2, MoveDirection::Up)
            .expect("move the second row up");

        let mut orders: Vec<(i64, i64)> = view_model
            .entries
            .iter()
            .map(|row| (row.id, row.sort_order))
            .collect();
        orders.sort_by_key(|(id, _)| *id);
        assert_eq!(orders, vec![(1, 1), (2, 0), (3, 2), (4, 3)]);
    }

    #[test]
    fn an_unknown_kind_is_not_shown() {
        let mut stranger = entry(1, CvEntryKind::Skill, "Rust", 0);
        stranger.kind = "volunteering".into();

        let view_model = CvProfileViewModel::from_profile(profile(), vec![stranger]);
        assert!(view_model.entries.is_empty());
    }

    #[test]
    fn row_dates_are_compact() {
        let mut row =
            CvEntryRow::from_entry(&entry(1, CvEntryKind::Experience, "E", 0)).expect("known kind");

        assert_eq!(row.dates(), None);
        row.start_date = Some("2020".into());
        assert_eq!(row.dates().as_deref(), Some("2020"));
        row.start_date = None;
        row.end_date = Some("2022".into());
        assert_eq!(row.dates().as_deref(), Some("until 2022"));
        row.start_date = Some("2020".into());
        assert_eq!(row.dates().as_deref(), Some("2020 - 2022"));
    }

    #[test]
    fn a_draft_needs_a_title_and_trims_the_rest() {
        let mut draft = CvEntryDraft::new(CvEntryKind::Education);
        assert!(!draft.is_valid());

        draft.title = "BSc Mathematics".into();
        draft.organization = "  University of London ".into();
        assert!(draft.is_valid());

        let entry = draft.to_new_entry(1, None);
        assert_eq!(entry.title, "BSc Mathematics");
        assert_eq!(entry.organization.as_deref(), Some("University of London"));
        assert_eq!(entry.start_date, None);
        assert_eq!(entry.kind, CvEntryKind::Education);
    }

    #[test]
    fn an_edit_preserves_metadata() {
        let mut row =
            CvEntryRow::from_entry(&entry(1, CvEntryKind::Skill, "Rust", 0)).expect("known kind");
        row.metadata = Some(r#"{"level":"expert"}"#.to_string());

        let draft = row.to_draft();
        let input = draft.to_new_entry(1, row.metadata.as_deref());

        assert_eq!(
            input.metadata,
            Some(serde_json::json!({ "level": "expert" }))
        );
    }
}
