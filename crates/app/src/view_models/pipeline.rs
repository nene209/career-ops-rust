//! Pipeline view-model.
//!
//! The app loads `job_descriptions::list_recent` and `applications::list`, joins
//! them by job id, and hands the result to this view model. Filtering and
//! labels are pure data shaping; the screen renders the result.

use std::collections::{HashMap, HashSet};

use career_ops_core::model::{ApplicationStatus, JobDescription};
use career_ops_core::text;
use chrono::Utc;

use super::portals::relative_time;

#[derive(Debug, Default)]
pub struct PipelineViewModel {
    pub rows: Vec<JobRow>,
    pub filter: JobFilter,
    /// Case-insensitive substring over title and company. Kept here rather
    /// than in the screen so a reload does not clear it.
    pub search: String,
}

#[derive(Debug, Clone)]
pub struct JobRow {
    pub id: i64,
    pub title: String,
    pub company: String,
    pub tier: TierLabel,
    pub fetched_at: String,
    pub status: Option<StatusLabel>,
    /// Included so the detail view can render the posting without a second
    /// database read.
    pub source_url: String,
    pub raw_text: String,
    /// The stored description with markup removed, for the detail view. Kept
    /// beside `raw_text` so the screen can toggle between them without
    /// re-parsing every frame.
    pub plain_text: String,
    /// The stored text was markup, so the raw view is worth offering.
    pub stored_is_markup: bool,
    /// The user has a status, evaluation, or cover letter for this job, so
    /// deleting it needs the typed confirmation.
    pub has_user_state: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TierLabel {
    Ats,
    Html,
    Tier2,
    Manual,
    Other,
}

impl TierLabel {
    pub fn from_fetch_tier(raw: &str) -> Self {
        match raw {
            "tier1a:ats" => Self::Ats,
            "tier1b:html" => Self::Html,
            "tier2:obscura" | "tier2:rust-headless-chrome" => Self::Tier2,
            "manual" => Self::Manual,
            _ if raw.starts_with("tier1a") => Self::Ats,
            _ if raw.starts_with("tier1b") => Self::Html,
            _ if raw.starts_with("tier2") => Self::Tier2,
            _ => Self::Other,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Ats => "ATS",
            Self::Html => "HTML",
            Self::Tier2 => "Tier 2",
            Self::Manual => "Manual",
            Self::Other => "Other",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusLabel {
    Discovered,
    Evaluating,
    Ready,
    Applied,
    Interviewing,
    Rejected,
    Offer,
    NotAMatch,
}

impl StatusLabel {
    pub fn from_application_status(status: ApplicationStatus) -> Self {
        match status {
            ApplicationStatus::Discovered => Self::Discovered,
            ApplicationStatus::Evaluating => Self::Evaluating,
            ApplicationStatus::Ready => Self::Ready,
            ApplicationStatus::Applied => Self::Applied,
            ApplicationStatus::Interviewing => Self::Interviewing,
            ApplicationStatus::Rejected => Self::Rejected,
            ApplicationStatus::Offer => Self::Offer,
            ApplicationStatus::NotAMatch => Self::NotAMatch,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Discovered => "Discovered",
            Self::Evaluating => "Evaluating",
            Self::Ready => "Ready",
            Self::Applied => "Applied",
            Self::Interviewing => "Interviewing",
            Self::Rejected => "Rejected",
            Self::Offer => "Offer",
            Self::NotAMatch => "Not a match",
        }
    }

    /// The status can still be moved to `not_a_match`.
    pub fn can_mark_not_a_match(self) -> bool {
        matches!(self, Self::Discovered | Self::Evaluating | Self::Ready)
    }

    /// Post-submission statuses, used by the `Applied` filter.
    pub fn is_applied(self) -> bool {
        matches!(self, Self::Applied | Self::Interviewing | Self::Offer)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum JobFilter {
    #[default]
    All,
    NotApplied,
    Applied,
    Rejected,
    NotAMatch,
}

impl JobFilter {
    pub fn all() -> &'static [JobFilter] {
        &[
            JobFilter::All,
            JobFilter::NotApplied,
            JobFilter::Applied,
            JobFilter::Rejected,
            JobFilter::NotAMatch,
        ]
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::NotApplied => "Not applied",
            Self::Applied => "Applied",
            Self::Rejected => "Rejected",
            Self::NotAMatch => "Not a match",
        }
    }

    pub fn matches(self, row: &JobRow) -> bool {
        match self {
            Self::All => true,
            Self::NotApplied => match row.status {
                None => true,
                Some(status) => {
                    !status.is_applied()
                        && status != StatusLabel::Rejected
                        && status != StatusLabel::NotAMatch
                }
            },
            Self::Applied => row.status.is_some_and(StatusLabel::is_applied),
            Self::Rejected => row.status == Some(StatusLabel::Rejected),
            Self::NotAMatch => row.status == Some(StatusLabel::NotAMatch),
        }
    }
}

impl PipelineViewModel {
    pub fn from_jobs(
        jobs: Vec<JobDescription>,
        statuses: &HashMap<i64, ApplicationStatus>,
        touched: &HashSet<i64>,
    ) -> Self {
        Self {
            rows: rows_from_jobs(jobs, statuses, touched),
            filter: JobFilter::All,
            search: String::new(),
        }
    }

    /// Replace the rows after a reload, keeping the user's filter and search.
    ///
    /// Both are view preferences, not data: refreshing must not reset them.
    pub fn replace_jobs(
        &mut self,
        jobs: Vec<JobDescription>,
        statuses: &HashMap<i64, ApplicationStatus>,
        touched: &HashSet<i64>,
    ) {
        self.rows = rows_from_jobs(jobs, statuses, touched);
    }

    /// Rows matching the current filter and search, in the original
    /// fetched-at order.
    pub fn apply_filter(&self) -> impl Iterator<Item = &JobRow> {
        let query = self.search.trim().to_lowercase();
        self.rows.iter().filter(move |row| {
            self.filter.matches(row) && (query.is_empty() || row.matches_search(&query))
        })
    }

    pub fn row(&self, id: i64) -> Option<&JobRow> {
        self.rows.iter().find(|row| row.id == id)
    }
}

impl JobRow {
    /// Case-insensitive match against an already-lowercased query.
    fn matches_search(&self, query: &str) -> bool {
        self.title.to_lowercase().contains(query) || self.company.to_lowercase().contains(query)
    }
}

fn rows_from_jobs(
    jobs: Vec<JobDescription>,
    statuses: &HashMap<i64, ApplicationStatus>,
    touched: &HashSet<i64>,
) -> Vec<JobRow> {
    jobs.into_iter()
        .map(|job| {
            let status = statuses
                .get(&job.id)
                .copied()
                .map(StatusLabel::from_application_status);
            let has_user_state = touched.contains(&job.id)
                || status.is_some_and(|status| status != StatusLabel::Discovered);
            let description = text::plain_text(&job.raw_text, usize::MAX);
            // Only markup deserves the raw toggle; a plain paste that merely
            // collapsed blank lines has nothing to reveal.
            let stored_is_markup =
                job.raw_text.contains('<') && description.text.trim() != job.raw_text.trim();

            JobRow {
                id: job.id,
                title: job.title,
                company: job.company,
                tier: TierLabel::from_fetch_tier(&job.fetch_tier),
                fetched_at: relative_time(job.fetched_at, Utc::now()),
                status,
                source_url: job.source_url,
                raw_text: job.raw_text,
                plain_text: description.text,
                stored_is_markup,
                has_user_state,
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;

    fn job(id: i64, title: &str, tier: &str) -> JobDescription {
        JobDescription {
            id,
            title: title.into(),
            company: "Acme".into(),
            source_url: format!("https://example.com/{id}"),
            raw_text: "body".into(),
            fetched_at: Utc::now(),
            fetch_tier: tier.into(),
            external_id: None,
            content_hash: None,
        }
    }

    fn row(id: i64, status: Option<StatusLabel>) -> JobRow {
        JobRow {
            id,
            title: format!("Job {id}"),
            company: "Acme".into(),
            tier: TierLabel::Ats,
            fetched_at: "just now".into(),
            status,
            source_url: format!("https://example.com/{id}"),
            raw_text: "body".into(),
            plain_text: "body".into(),
            stored_is_markup: false,
            has_user_state: status.is_some_and(|status| status != StatusLabel::Discovered),
        }
    }

    #[test]
    fn status_label_maps_every_application_status() {
        let cases = [
            (ApplicationStatus::Discovered, StatusLabel::Discovered),
            (ApplicationStatus::Evaluating, StatusLabel::Evaluating),
            (ApplicationStatus::Ready, StatusLabel::Ready),
            (ApplicationStatus::Applied, StatusLabel::Applied),
            (ApplicationStatus::Interviewing, StatusLabel::Interviewing),
            (ApplicationStatus::Rejected, StatusLabel::Rejected),
            (ApplicationStatus::Offer, StatusLabel::Offer),
            (ApplicationStatus::NotAMatch, StatusLabel::NotAMatch),
        ];

        for (status, expected) in cases {
            assert_eq!(StatusLabel::from_application_status(status), expected);
            assert!(!expected.label().is_empty());
        }
    }

    #[test]
    fn tier_label_maps_known_prefixes_and_unknown_values() {
        assert_eq!(TierLabel::from_fetch_tier("tier1a:ats"), TierLabel::Ats);
        assert_eq!(TierLabel::from_fetch_tier("tier1b:html"), TierLabel::Html);
        assert_eq!(
            TierLabel::from_fetch_tier("tier2:obscura"),
            TierLabel::Tier2
        );
        assert_eq!(
            TierLabel::from_fetch_tier("tier2:rust-headless-chrome"),
            TierLabel::Tier2
        );
        assert_eq!(TierLabel::from_fetch_tier("manual"), TierLabel::Manual);
        assert_eq!(TierLabel::from_fetch_tier("tier1a:future"), TierLabel::Ats);
        assert_eq!(TierLabel::from_fetch_tier("unknown"), TierLabel::Other);
    }

    #[test]
    fn from_jobs_maps_fields_and_statuses() {
        let jobs = vec![
            job(1, "Engineer", "tier1a:ats"),
            job(2, "Designer", "manual"),
        ];
        let statuses = HashMap::from([(1, ApplicationStatus::Ready)]);

        let view_model = PipelineViewModel::from_jobs(jobs, &statuses, &HashSet::new());

        assert_eq!(view_model.rows.len(), 2);
        assert_eq!(view_model.rows[0].title, "Engineer");
        assert_eq!(view_model.rows[0].company, "Acme");
        assert_eq!(view_model.rows[0].tier, TierLabel::Ats);
        assert_eq!(view_model.rows[0].status, Some(StatusLabel::Ready));
        assert!(!view_model.rows[0].fetched_at.is_empty());
        assert_eq!(view_model.rows[0].source_url, "https://example.com/1");
        assert_eq!(view_model.rows[1].tier, TierLabel::Manual);
        assert!(view_model.rows[1].status.is_none());
        assert_eq!(view_model.filter, JobFilter::All);
    }

    #[test]
    fn rows_carry_plain_text_and_a_markup_flag() {
        let mut html_job = job(1, "Engineer", "tier1a:ats");
        html_job.raw_text = "<p>Hello <b>world</b>.</p>".into();
        let mut plain_job = job(2, "Other", "tier1a:ats");
        plain_job.raw_text = "Just prose.".into();
        let mut spaced_job = job(3, "Third", "tier1a:ats");
        spaced_job.raw_text = "A\n\n\n\nB".into();

        let view_model = PipelineViewModel::from_jobs(
            vec![html_job, plain_job, spaced_job],
            &HashMap::new(),
            &HashSet::new(),
        );

        assert_eq!(view_model.rows[0].plain_text, "Hello world.");
        assert!(view_model.rows[0].stored_is_markup);
        assert_eq!(view_model.rows[1].plain_text, "Just prose.");
        assert!(!view_model.rows[1].stored_is_markup);
        assert_eq!(view_model.rows[2].plain_text, "A\n\nB");
        assert!(
            !view_model.rows[2].stored_is_markup,
            "collapsed blank lines are not markup, so there is no raw view"
        );
    }

    #[test]
    fn filters_partition_the_statuses() {
        let rows = vec![
            row(1, None),
            row(2, Some(StatusLabel::Discovered)),
            row(3, Some(StatusLabel::Evaluating)),
            row(4, Some(StatusLabel::Ready)),
            row(5, Some(StatusLabel::Applied)),
            row(6, Some(StatusLabel::Interviewing)),
            row(7, Some(StatusLabel::Offer)),
            row(8, Some(StatusLabel::Rejected)),
            row(9, Some(StatusLabel::NotAMatch)),
        ];

        let ids = |filter: JobFilter| {
            let view_model = PipelineViewModel {
                rows: rows.clone(),
                filter,
                search: String::new(),
            };
            view_model
                .apply_filter()
                .map(|row| row.id)
                .collect::<Vec<_>>()
        };

        assert_eq!(ids(JobFilter::All), vec![1, 2, 3, 4, 5, 6, 7, 8, 9]);
        assert_eq!(ids(JobFilter::NotApplied), vec![1, 2, 3, 4]);
        assert_eq!(ids(JobFilter::Applied), vec![5, 6, 7]);
        assert_eq!(ids(JobFilter::Rejected), vec![8]);
        assert_eq!(ids(JobFilter::NotAMatch), vec![9]);
    }

    #[test]
    fn not_a_match_is_not_part_of_not_applied() {
        assert!(!JobFilter::NotApplied.matches(&row(1, Some(StatusLabel::NotAMatch))));
        assert!(JobFilter::NotAMatch.matches(&row(1, Some(StatusLabel::NotAMatch))));
        assert!(!StatusLabel::NotAMatch.is_applied());
        assert!(StatusLabel::Discovered.can_mark_not_a_match());
        assert!(StatusLabel::Ready.can_mark_not_a_match());
        assert!(!StatusLabel::Applied.can_mark_not_a_match());
        assert!(!StatusLabel::NotAMatch.can_mark_not_a_match());
    }

    #[test]
    fn replace_jobs_keeps_the_current_filter() {
        let mut view_model = PipelineViewModel {
            rows: Vec::new(),
            filter: JobFilter::Applied,
            search: "engineer".into(),
        };

        view_model.replace_jobs(
            vec![job(9, "New", "tier1a:ats")],
            &HashMap::new(),
            &HashSet::new(),
        );

        assert_eq!(view_model.rows.len(), 1);
        assert_eq!(view_model.rows[0].title, "New");
        assert_eq!(view_model.filter, JobFilter::Applied);
        assert_eq!(
            view_model.search, "engineer",
            "a reload must not clear the search"
        );
    }

    #[test]
    fn search_matches_title_or_company_case_insensitively() {
        let mut engineer = row(1, None);
        engineer.title = "Senior Network Engineer".into();
        let mut designer = row(2, None);
        designer.company = "Engineering Co".into();
        let other = row(3, None);

        let view_model = PipelineViewModel {
            rows: vec![engineer, designer, other],
            filter: JobFilter::All,
            search: "ENGINEER".into(),
        };

        let ids: Vec<i64> = view_model.apply_filter().map(|row| row.id).collect();
        assert_eq!(ids, vec![1, 2], "title and company both match");
    }

    #[test]
    fn search_composes_with_the_status_filter() {
        let mut rejected = row(1, Some(StatusLabel::Rejected));
        rejected.title = "Engineer".into();
        let mut applied = row(2, Some(StatusLabel::Applied));
        applied.title = "Engineer".into();

        let view_model = PipelineViewModel {
            rows: vec![rejected, applied],
            filter: JobFilter::Applied,
            search: "engineer".into(),
        };

        let ids: Vec<i64> = view_model.apply_filter().map(|row| row.id).collect();
        assert_eq!(ids, vec![2]);
    }

    #[test]
    fn a_status_beyond_discovered_counts_as_user_state() {
        let jobs = vec![
            job(1, "Engineer", "tier1a:ats"),
            job(2, "Other", "tier1a:ats"),
        ];
        let statuses = HashMap::from([(1, ApplicationStatus::NotAMatch)]);
        let touched = HashSet::from([2]);

        let view_model = PipelineViewModel::from_jobs(jobs, &statuses, &touched);

        assert!(view_model.rows[0].has_user_state, "the status counts");
        assert!(
            view_model.rows[1].has_user_state,
            "a report or letter counts"
        );
    }

    #[test]
    fn row_finds_a_row_by_id() {
        let view_model = PipelineViewModel {
            rows: vec![row(1, None), row(2, None)],
            filter: JobFilter::All,
            search: String::new(),
        };

        assert_eq!(view_model.row(2).map(|row| row.id), Some(2));
        assert!(view_model.row(99).is_none());
    }

    #[test]
    fn tier_labels_are_unique_and_non_empty() {
        let labels: Vec<&str> = [
            TierLabel::Ats,
            TierLabel::Html,
            TierLabel::Tier2,
            TierLabel::Manual,
            TierLabel::Other,
        ]
        .iter()
        .map(|tier| tier.label())
        .collect();
        let unique: std::collections::HashSet<&str> = labels.iter().copied().collect();

        assert_eq!(labels.len(), unique.len(), "duplicate labels: {labels:?}");
        assert!(labels.iter().all(|label| !label.is_empty()));
    }

    #[test]
    fn filter_labels_are_unique() {
        let labels: Vec<&str> = JobFilter::all()
            .iter()
            .map(|filter| filter.label())
            .collect();
        let unique: std::collections::HashSet<&str> = labels.iter().copied().collect();
        assert_eq!(labels.len(), unique.len(), "duplicate labels: {labels:?}");
    }
}
