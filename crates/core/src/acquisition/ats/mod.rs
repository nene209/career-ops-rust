// crates/core/src/acquisition/ats/mod.rs

use async_trait::async_trait;
use url::Url;

use super::error::AtsResult;

#[derive(Debug, Clone)]
pub struct NormalizedJob {
    pub external_id: String,
    pub title: String,
    pub company: String,
    pub location: Option<String>,
    pub department: Option<String>,
    pub employment_type: Option<String>,
    pub workplace_type: Option<String>,
    pub source_url: Url,
    pub raw_text: String,
    pub posted_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[async_trait]
pub trait AtsAdapter: Send + Sync {
    /// Human-readable name for fetch_tier recording: "greenhouse", etc.
    fn name(&self) -> &'static str;

    /// Given a careers-page URL, return the board slug if this adapter
    /// recognises the host.
    fn detect(&self, url: &Url) -> Option<String>;

    /// Fetch all jobs for a board slug.
    async fn fetch_jobs(
        &self,
        client: &reqwest::Client,
        slug: &str,
        company_name: &str,
    ) -> AtsResult<Vec<NormalizedJob>>;
}

/// The registry of known adapters. Order matters: the first match wins.
pub fn adapters() -> Vec<Box<dyn AtsAdapter>> {
    vec![
        Box::new(greenhouse::GreenhouseAdapter),
        Box::new(lever::LeverAdapter),
        Box::new(ashby::AshbyAdapter),
        Box::new(smartrecruiters::SmartRecruitersAdapter),
    ]
}

pub(crate) fn parse_iso8601(s: &str) -> Option<chrono::DateTime<chrono::Utc>> {
    // Full RFC3339 timestamp (with time).
    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(s) {
        return Some(dt.with_timezone(&chrono::Utc));
    }
    // Date-only (some ATSs return "2026-10-01" rather than a full timestamp).
    if let Ok(d) = chrono::NaiveDate::parse_from_str(s, "%Y-%m-%d") {
        return Some(d.and_hms_opt(0, 0, 0)?.and_utc());
    }
    None
}

pub mod ashby;
pub mod greenhouse;
pub mod lever;
pub mod smartrecruiters;
