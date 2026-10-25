//! Pipeline view-model.
//!
//! The app loads `job_descriptions::list_recent` and `applications::list`, joins
//! them by job id, and hands the result to this view model. Filtering and
//! labels are pure data shaping; the screen renders the result.

use std::collections::HashMap;

use career_ops_core::model::{ApplicationStatus, JobDescription};
use chrono::Utc;

use super::portals::relative_time;

#[derive(Debug, Default)]
pub struct PipelineViewModel {
    pub rows: Vec<JobRow>,
    pub filter: JobFilter,
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
        }
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
}

impl JobFilter {
    pub fn all() -> &'static [JobFilter] {
        &[
            JobFilter::All,
            JobFilter::NotApplied,
            JobFilter::Applied,
            JobFilter::Rejected,
        ]
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::NotApplied => "Not applied",
            Self::Applied => "Applied",
            Self::Rejected => "Rejected",
        }
    }

    pub fn matches(self, row: &JobRow) -> bool {
        match self {
            Self::All => true,
            Self::NotApplied => match row.status {
                None => true,
                Some(status) => !status.is_applied() && status != StatusLabel::Rejected,
            },
            Self::Applied => row.status.is_some_and(StatusLabel::is_applied),
            Self::Rejected => row.status == Some(StatusLabel::Rejected),
        }
    }
}

impl PipelineViewModel {
    pub fn from_jobs(
        jobs: Vec<JobDescription>,
        statuses: &HashMap<i64, ApplicationStatus>,
    ) -> Self {
        Self {
            rows: rows_from_jobs(jobs, statuses),
            filter: JobFilter::All,
        }
    }

    /// Replace the rows after a reload, keeping the user's filter.
    ///
    /// The filter is a view preference, not data: refreshing must not reset it.
    pub fn replace_jobs(
        &mut self,
        jobs: Vec<JobDescription>,
        statuses: &HashMap<i64, ApplicationStatus>,
    ) {
        self.rows = rows_from_jobs(jobs, statuses);
    }

    /// Rows matching the current filter, in the original fetched-at order.
    pub fn apply_filter(&self) -> impl Iterator<Item = &JobRow> {
        self.rows.iter().filter(move |row| self.filter.matches(row))
    }

    pub fn row(&self, id: i64) -> Option<&JobRow> {
        self.rows.iter().find(|row| row.id == id)
    }
}

fn rows_from_jobs(
    jobs: Vec<JobDescription>,
    statuses: &HashMap<i64, ApplicationStatus>,
) -> Vec<JobRow> {
    jobs.into_iter()
        .map(|job| JobRow {
            id: job.id,
            title: job.title,
            company: job.company,
            tier: TierLabel::from_fetch_tier(&job.fetch_tier),
            fetched_at: relative_time(job.fetched_at, Utc::now()),
            status: statuses
                .get(&job.id)
                .copied()
                .map(StatusLabel::from_application_status),
            source_url: job.source_url,
            raw_text: job.raw_text,
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

        let view_model = PipelineViewModel::from_jobs(jobs, &statuses);

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
        ];

        let ids = |filter: JobFilter| {
            let view_model = PipelineViewModel {
                rows: rows.clone(),
                filter,
            };
            view_model
                .apply_filter()
                .map(|row| row.id)
                .collect::<Vec<_>>()
        };

        assert_eq!(ids(JobFilter::All), vec![1, 2, 3, 4, 5, 6, 7, 8]);
        assert_eq!(ids(JobFilter::NotApplied), vec![1, 2, 3, 4]);
        assert_eq!(ids(JobFilter::Applied), vec![5, 6, 7]);
        assert_eq!(ids(JobFilter::Rejected), vec![8]);
    }

    #[test]
    fn replace_jobs_keeps_the_current_filter() {
        let mut view_model = PipelineViewModel {
            rows: Vec::new(),
            filter: JobFilter::Applied,
        };

        view_model.replace_jobs(vec![job(9, "New", "tier1a:ats")], &HashMap::new());

        assert_eq!(view_model.rows.len(), 1);
        assert_eq!(view_model.rows[0].title, "New");
        assert_eq!(view_model.filter, JobFilter::Applied);
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
