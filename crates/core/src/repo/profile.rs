//! Profile — the candidate singleton row.
//!
//! `profile` has exactly one row (`id = 1`). `ensure_singleton` creates it on
//! demand, so the CV / Profile screen and Phase 3's `cv_summary` can assume it
//! exists without a first-launch special case.
//!
//! The summary renderer here is the **only** format for candidate context. The
//! UI preview and Phase 3's `{{cv_summary}}` both call it; a second formatter
//! would let the two drift.

use chrono::Utc;
use sqlx::SqlitePool;

use crate::error::{CoreError, Result};
use crate::model::{CvEntry, Profile};

use super::cv_entries::{self, CvEntryKind};

/// Editable profile fields.
///
/// Optional fields that are `None` or blank are stored as `NULL`; `name` and
/// `email` are stored trimmed (an empty email is allowed).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProfileDetails {
    pub name: String,
    pub email: String,
    pub phone: Option<String>,
    pub location: Option<String>,
    pub summary: Option<String>,
}

pub async fn ensure_singleton(pool: &SqlitePool) -> Result<Profile> {
    if let Some(p) = get(pool).await? {
        return Ok(p);
    }
    let now = Utc::now();
    sqlx::query(
        "INSERT INTO profile (id, name, email, created_at, updated_at)
         VALUES (1, '', '', ?, ?)",
    )
    .bind(now)
    .bind(now)
    .execute(pool)
    .await?;
    get(pool)
        .await?
        .ok_or(crate::error::CoreError::Internal("profile insert"))
}

pub async fn get(pool: &SqlitePool) -> Result<Option<Profile>> {
    let row = sqlx::query_as::<_, Profile>("SELECT * FROM profile WHERE id = 1")
        .fetch_optional(pool)
        .await?;
    Ok(row)
}

pub async fn update_identity(
    pool: &SqlitePool,
    name: &str,
    email: &str,
    location: Option<&str>,
) -> Result<()> {
    let now = Utc::now();
    sqlx::query(
        "UPDATE profile SET name = ?, email = ?, location = ?, updated_at = ? WHERE id = 1",
    )
    .bind(name)
    .bind(email)
    .bind(location)
    .bind(now)
    .execute(pool)
    .await?;
    Ok(())
}

/// Update the editable profile fields. Creates the singleton if it is absent.
pub async fn update_details(pool: &SqlitePool, details: &ProfileDetails) -> Result<Profile> {
    ensure_singleton(pool).await?;

    let now = Utc::now();
    let result = sqlx::query(
        "UPDATE profile
         SET name = ?, email = ?, phone = ?, location = ?, summary = ?, updated_at = ?
         WHERE id = 1",
    )
    .bind(details.name.trim())
    .bind(details.email.trim())
    .bind(trimmed_opt(details.phone.as_deref()))
    .bind(trimmed_opt(details.location.as_deref()))
    .bind(trimmed_opt(details.summary.as_deref()))
    .bind(now)
    .execute(pool)
    .await?;

    if result.rows_affected() == 0 {
        return Err(CoreError::Internal(
            "profile::update_details: row not found",
        ));
    }

    get(pool)
        .await?
        .ok_or(CoreError::Internal("profile::update_details"))
}

/// Load the singleton and its CV entries, then render the canonical summary.
pub async fn cv_summary(pool: &SqlitePool) -> Result<Option<String>> {
    let profile = ensure_singleton(pool).await?;
    let entries = cv_entries::list_for_profile(pool, profile.id).await?;
    Ok(render_cv_summary(&profile, &entries))
}

/// Render the canonical candidate summary used by the UI preview and Phase 3's
/// `{{cv_summary}}` variable.
///
/// Returns `None` when the profile has no name, no summary, and no titled CV
/// entries. Phase 3 turns that into an actionable "add your CV" error instead of
/// sending an empty candidate section to a model.
///
/// Format:
///
/// ```text
/// {name}
/// {location} | {email} | {phone}
///
/// Summary:
/// {summary}
///
/// Experience:
/// - {title} — {organization} ({start} - {end})
///   {description}
///
/// Education:
/// - ...
///
/// Skills:
/// - ...
/// ```
///
/// Empty fields and sections are omitted. Entries are grouped by kind in the
/// fixed order Experience, Education, Skills and ordered by `sort_order` then
/// `id` within each kind. Descriptions keep their internal line breaks; fields
/// are trimmed. No HTML or markup is produced.
pub fn render_cv_summary(profile: &Profile, entries: &[CvEntry]) -> Option<String> {
    let name = profile.name.trim();
    let summary = profile.summary.as_deref().map(str::trim).unwrap_or("");

    let contact = [
        profile.location.as_deref().map(str::trim),
        Some(profile.email.trim()),
        profile.phone.as_deref().map(str::trim),
    ]
    .into_iter()
    .flatten()
    .filter(|part| !part.is_empty())
    .collect::<Vec<_>>()
    .join(" | ");

    let mut blocks: Vec<String> = Vec::new();
    let mut has_entries = false;

    let mut header = String::new();
    if !name.is_empty() {
        header.push_str(name);
    }
    if !contact.is_empty() {
        if !header.is_empty() {
            header.push('\n');
        }
        header.push_str(&contact);
    }
    if !header.is_empty() {
        blocks.push(header);
    }

    if !summary.is_empty() {
        blocks.push(format!("Summary:\n{summary}"));
    }

    for (kind, heading) in [
        (CvEntryKind::Experience, "Experience:"),
        (CvEntryKind::Education, "Education:"),
        (CvEntryKind::Skill, "Skills:"),
    ] {
        let mut group: Vec<&CvEntry> = entries
            .iter()
            .filter(|entry| CvEntryKind::parse(&entry.kind) == Some(kind))
            .collect();
        group.sort_by_key(|entry| (entry.sort_order, entry.id));

        let lines: Vec<String> = group.iter().filter_map(|entry| entry_line(entry)).collect();
        if !lines.is_empty() {
            has_entries = true;
            blocks.push(format!("{heading}\n{}", lines.join("\n")));
        }
    }

    if name.is_empty() && summary.is_empty() && !has_entries {
        return None;
    }

    Some(blocks.join("\n\n"))
}

/// One CV entry as a bullet line plus an optional indented description.
///
/// An untitled entry is skipped: the editor requires a title, and an empty
/// bullet would carry no information for the model.
fn entry_line(entry: &CvEntry) -> Option<String> {
    let title = entry.title.trim();
    if title.is_empty() {
        return None;
    }

    let mut line = format!("- {title}");

    if let Some(organization) = trimmed_opt(entry.organization.as_deref()) {
        line.push_str(" — ");
        line.push_str(organization);
    }

    let dates = date_range(
        trimmed_opt(entry.start_date.as_deref()),
        trimmed_opt(entry.end_date.as_deref()),
    );
    if let Some(dates) = dates {
        line.push(' ');
        line.push_str(&dates);
    }

    if let Some(description) = trimmed_opt(entry.description.as_deref()) {
        for description_line in description.lines() {
            let description_line = description_line.trim_end();
            if description_line.is_empty() {
                line.push('\n');
            } else {
                line.push_str("\n  ");
                line.push_str(description_line);
            }
        }
    }

    Some(line)
}

fn date_range(start: Option<&str>, end: Option<&str>) -> Option<String> {
    match (start, end) {
        (Some(start), Some(end)) => Some(format!("({start} - {end})")),
        (Some(start), None) => Some(format!("({start})")),
        (None, Some(end)) => Some(format!("(until {end})")),
        (None, None) => None,
    }
}

fn trimmed_opt(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

#[cfg(test)]
mod tests {
    use chrono::Utc;

    use super::*;

    fn profile() -> Profile {
        Profile {
            id: 1,
            name: "Ada Lovelace".into(),
            email: "ada@example.com".into(),
            phone: Some("+44 20 0000 0000".into()),
            location: Some("London".into()),
            summary: Some("Mathematician and first programmer.".into()),
            archetypes: None,
            preferences: None,
            created_at: Utc::now(),
            updated_at: Utc::now(),
        }
    }

    /// Test builder for a CV entry. Only the fields a test cares about are set,
    /// which also keeps the call sites under clippy's argument limit.
    struct EntrySpec(CvEntry);

    fn entry(id: i64, kind: CvEntryKind, title: &str) -> EntrySpec {
        EntrySpec(CvEntry {
            id,
            profile_id: 1,
            kind: kind.as_str().into(),
            title: title.into(),
            organization: None,
            start_date: None,
            end_date: None,
            description: None,
            metadata: None,
            sort_order: 0,
        })
    }

    impl EntrySpec {
        fn org(mut self, organization: &str) -> Self {
            self.0.organization = Some(organization.into());
            self
        }

        fn dates(mut self, start: Option<&str>, end: Option<&str>) -> Self {
            self.0.start_date = start.map(Into::into);
            self.0.end_date = end.map(Into::into);
            self
        }

        fn description(mut self, description: &str) -> Self {
            self.0.description = Some(description.into());
            self
        }

        fn order(mut self, sort_order: i64) -> Self {
            self.0.sort_order = sort_order;
            self
        }

        fn build(self) -> CvEntry {
            self.0
        }
    }

    #[test]
    fn renders_the_documented_format() {
        let entries = vec![
            entry(2, CvEntryKind::Experience, "Software Engineer")
                .org("Analytical Engines")
                .dates(Some("2020"), Some("Present"))
                .description("Built the first program.\nWrote the notes.")
                .order(1)
                .build(),
            entry(1, CvEntryKind::Education, "BSc Mathematics")
                .org("University of London")
                .dates(Some("2015"), Some("2019"))
                .build(),
            entry(3, CvEntryKind::Skill, "Rust").build(),
        ];

        let summary = render_cv_summary(&profile(), &entries).expect("summary");

        let expected = [
            "Ada Lovelace",
            "London | ada@example.com | +44 20 0000 0000",
            "",
            "Summary:",
            "Mathematician and first programmer.",
            "",
            "Experience:",
            "- Software Engineer — Analytical Engines (2020 - Present)",
            "  Built the first program.",
            "  Wrote the notes.",
            "",
            "Education:",
            "- BSc Mathematics — University of London (2015 - 2019)",
            "",
            "Skills:",
            "- Rust",
        ]
        .join("\n");

        assert_eq!(summary, expected);
    }

    #[test]
    fn an_empty_profile_has_no_summary() {
        let empty = Profile {
            name: String::new(),
            email: String::new(),
            phone: None,
            location: None,
            summary: None,
            ..profile()
        };

        assert_eq!(render_cv_summary(&empty, &[]), None);
    }

    #[test]
    fn contact_details_alone_are_not_candidate_data() {
        let contact_only = Profile {
            name: String::new(),
            summary: None,
            ..profile()
        };

        assert_eq!(render_cv_summary(&contact_only, &[]), None);
    }

    #[test]
    fn empty_sections_and_fields_are_omitted() {
        let sparse = Profile {
            phone: None,
            location: None,
            summary: None,
            ..profile()
        };
        let entries = vec![entry(1, CvEntryKind::Experience, "Engineer")
            .dates(Some("2022"), None)
            .build()];

        let summary = render_cv_summary(&sparse, &entries).expect("summary");

        assert_eq!(
            summary,
            "Ada Lovelace\nada@example.com\n\nExperience:\n- Engineer (2022)"
        );
    }

    #[test]
    fn sections_follow_the_fixed_kind_order() {
        let entries = vec![
            entry(1, CvEntryKind::Skill, "Rust").build(),
            entry(2, CvEntryKind::Experience, "Engineer")
                .order(9)
                .build(),
            entry(3, CvEntryKind::Education, "BSc").order(5).build(),
        ];
        let sparse = Profile {
            summary: None,
            ..profile()
        };

        let summary = render_cv_summary(&sparse, &entries).expect("summary");
        let order: Vec<&str> = summary.lines().filter(|line| line.ends_with(':')).collect();

        assert_eq!(order, vec!["Experience:", "Education:", "Skills:"]);
    }

    #[test]
    fn entries_sort_by_sort_order_then_id_within_a_kind() {
        let entries = vec![
            entry(3, CvEntryKind::Experience, "Third").order(2).build(),
            entry(1, CvEntryKind::Experience, "First").order(0).build(),
            entry(2, CvEntryKind::Experience, "Second").order(1).build(),
        ];
        let sparse = Profile {
            summary: None,
            ..profile()
        };

        let summary = render_cv_summary(&sparse, &entries).expect("summary");
        let bullets: Vec<&str> = summary
            .lines()
            .filter(|line| line.starts_with("- "))
            .collect();

        assert_eq!(bullets, vec!["- First", "- Second", "- Third"]);
    }

    #[test]
    fn partial_dates_are_explicit() {
        let entries = vec![
            entry(1, CvEntryKind::Experience, "Start only")
                .dates(Some("2021"), None)
                .build(),
            entry(2, CvEntryKind::Experience, "End only")
                .dates(None, Some("2022"))
                .order(1)
                .build(),
        ];
        let sparse = Profile {
            summary: None,
            ..profile()
        };

        let summary = render_cv_summary(&sparse, &entries).expect("summary");

        assert!(summary.contains("- Start only (2021)"), "{summary}");
        assert!(summary.contains("- End only (until 2022)"), "{summary}");
    }

    #[test]
    fn whitespace_is_trimmed_and_blank_lines_in_descriptions_survive() {
        let entries = vec![entry(1, CvEntryKind::Experience, "  Engineer  ")
            .org("  Acme  ")
            .dates(Some(" 2020 "), Some(" 2021 "))
            .description("  First paragraph.\n\nSecond paragraph.  ")
            .build()];
        let sparse = Profile {
            summary: None,
            ..profile()
        };

        let summary = render_cv_summary(&sparse, &entries).expect("summary");

        assert!(
            summary.contains(
                "- Engineer — Acme (2020 - 2021)\n  First paragraph.\n\n  Second paragraph."
            ),
            "{summary}"
        );
    }

    #[test]
    fn an_untitled_entry_is_skipped() {
        let entries = vec![entry(1, CvEntryKind::Experience, "   ")
            .org("Acme")
            .description("orphan")
            .build()];
        let sparse = Profile {
            summary: None,
            name: "Ada".into(),
            ..profile()
        };

        let summary = render_cv_summary(&sparse, &entries).expect("summary");
        assert!(!summary.contains("orphan"), "{summary}");
        assert!(!summary.contains("Experience:"), "{summary}");
    }

    #[test]
    fn rendering_is_deterministic() {
        let entries = vec![entry(1, CvEntryKind::Skill, "Rust").build()];
        let first = render_cv_summary(&profile(), &entries);
        let second = render_cv_summary(&profile(), &entries);
        assert_eq!(first, second);
    }
}
