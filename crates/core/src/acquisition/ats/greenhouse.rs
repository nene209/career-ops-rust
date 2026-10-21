// crates/core/src/acquisition/ats/greenhouse.rs

//! Greenhouse ATS adapter.
//!
//! Endpoint: `https://boards-api.greenhouse.io/v1/boards/{slug}/jobs?content=true`
//!
//! Two things matter:
//!
//! 1. `?content=true` returns descriptions in the same call. Without it,
//!    each job would require a separate request.
//! 2. The content field is **double-encoded HTML** — HTML entities wrap
//!    HTML tags (`&lt;p&gt;` for `<p>`). It must be unescaped before
//!    storage, or downstream code sees entity soup instead of markup.

use async_trait::async_trait;
use serde::Deserialize;
use url::Url;

use super::{parse_iso8601, AtsAdapter, NormalizedJob};
use crate::acquisition::error::{AtsError, AtsResult};
use crate::acquisition::http;

pub struct GreenhouseAdapter;

#[derive(Debug, Deserialize)]
struct Response {
    jobs: Vec<Job>,
}

#[derive(Debug, Deserialize)]
struct Job {
    id: i64,
    title: String,
    updated_at: String,
    absolute_url: String,
    location: Option<Location>,
    content: Option<String>,
    departments: Option<Vec<Department>>,
}

#[derive(Debug, Deserialize)]
struct Location {
    name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Department {
    name: String,
}

/// Parse a Greenhouse response body into normalized jobs.
pub fn parse(body: &str, company_name: &str) -> AtsResult<Vec<NormalizedJob>> {
    let response: Response =
        serde_json::from_str(body).map_err(|e| AtsError::Parse(format!("greenhouse json: {e}")))?;

    response
        .jobs
        .into_iter()
        .map(|j| -> Result<NormalizedJob, AtsError> {
            // Unescape the double-encoded content.
            let raw_text = j
                .content
                .map(|c| html_escape::decode_html_entities(&c).into_owned())
                .unwrap_or_default();

            Ok(NormalizedJob {
                external_id: j.id.to_string(),
                title: j.title,
                company: company_name.to_string(),
                location: j.location.and_then(|l| l.name),
                department: j
                    .departments
                    .and_then(|d| d.into_iter().next())
                    .map(|d| d.name),
                employment_type: None,
                workplace_type: None,
                source_url: Url::parse(&j.absolute_url)
                    .map_err(|e| AtsError::Parse(format!("greenhouse url: {e}")))?,
                raw_text,
                posted_at: parse_iso8601(&j.updated_at),
            })
        })
        .collect()
}

#[async_trait]
impl AtsAdapter for GreenhouseAdapter {
    fn name(&self) -> &'static str {
        "greenhouse"
    }

    fn detect(&self, url: &Url) -> Option<String> {
        if url.host_str()? != "boards.greenhouse.io" {
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
        let url = format!("https://boards-api.greenhouse.io/v1/boards/{slug}/jobs?content=true");
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
