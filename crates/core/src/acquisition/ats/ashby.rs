// crates/core/src/acquisition/ats/ashby.rs

//! Ashby ATS adapter.
//!
//! Endpoint: `https://api.ashbyhq.com/posting-api/job-board/{slug}?includeCompensation=true`
//!
//! One trap: **`isRemote` is `true` for Hybrid roles**. Trusting it would
//! mis-classify hybrid postings as fully remote. The correct field is
//! `workplaceType` (`"Remote"` / `"Hybrid"` / `"OnSite"`).

use async_trait::async_trait;
use serde::Deserialize;
use url::Url;

use super::{parse_iso8601, AtsAdapter, NormalizedJob};
use crate::acquisition::error::{AtsError, AtsResult};
use crate::acquisition::http;

pub struct AshbyAdapter;

#[derive(Debug, Deserialize)]
struct Response {
    jobs: Vec<Job>,
}

#[derive(Debug, Deserialize)]
struct Job {
    title: String,
    location: Option<String>,
    department: Option<String>,
    team: Option<String>,
    /// Present in the JSON but deliberately not read: `isRemote` is true
    /// for Hybrid roles, so `workplace_type` is the reliable field.
    #[serde(rename = "isRemote")]
    _is_remote: Option<bool>,
    #[serde(rename = "workplaceType")]
    workplace_type: Option<String>,
    #[serde(rename = "descriptionPlain")]
    description_plain: Option<String>,
    #[serde(rename = "publishedAt")]
    published_at: Option<String>,
    #[serde(rename = "employmentType")]
    employment_type: Option<String>,
    #[serde(rename = "jobUrl")]
    job_url: String,
}

/// Parse an Ashby response body into normalized jobs.
///
/// Ashby list responses do not carry a stable job ID field, so the
/// external ID is derived from the URL's final path segment.
pub fn parse(body: &str, company_name: &str) -> AtsResult<Vec<NormalizedJob>> {
    let response: Response =
        serde_json::from_str(body).map_err(|e| AtsError::Parse(format!("ashby json: {e}")))?;

    response
        .jobs
        .into_iter()
        .map(|j| -> Result<NormalizedJob, AtsError> {
            let external_id = j.job_url.rsplit('/').next().unwrap_or(&j.title).to_string();

            Ok(NormalizedJob {
                external_id,
                title: j.title,
                company: company_name.to_string(),
                location: j.location,
                // `department` is sometimes null and `team` carries the
                // value; prefer department, fall back to team.
                department: j.department.or(j.team),
                employment_type: j.employment_type,
                // Trust workplaceType over isRemote.
                workplace_type: j.workplace_type,
                source_url: Url::parse(&j.job_url)
                    .map_err(|e| AtsError::Parse(format!("ashby url: {e}")))?,
                raw_text: j.description_plain.unwrap_or_default(),
                posted_at: j.published_at.as_deref().and_then(parse_iso8601),
            })
        })
        .collect()
}

#[async_trait]
impl AtsAdapter for AshbyAdapter {
    fn name(&self) -> &'static str {
        "ashby"
    }

    fn detect(&self, url: &Url) -> Option<String> {
        if url.host_str()? != "jobs.ashbyhq.com" {
            return None;
        }
        url.path_segments()?.next().map(str::to_string)
    }

    async fn fetch_jobs(
        &self,
        client: &reqwest::Client,
        slug: &str,
        company_name: &str,
    ) -> AtsResult<Vec<NormalizedJob>> {
        let url = format!(
            "https://api.ashbyhq.com/posting-api/job-board/{slug}?includeCompensation=true"
        );
        let resp = http::get_with_retry(client, &url)
            .await
            .map_err(|e| AtsError::Transport(e.to_string()))?;

        if !resp.status().is_success() {
            return Err(AtsError::from_status(resp.status()));
        }

        let body = resp
            .text()
            .await
            .map_err(|e| AtsError::Transport(e.to_string()))?;
        parse(&body, company_name)
    }
}
