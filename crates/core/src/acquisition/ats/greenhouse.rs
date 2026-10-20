//! Greenhouse ATS adapter.
//!
//! Endpoint: `https://boards-api.greenhouse.io/v1/boards/{slug}/jobs?content=true`
//!
//! Greenhouse is the simplest of the four adapters: one request, no
//! pagination. Two things matter:
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
use crate::error::{CoreError, Result};

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
///
/// Pure function: takes the raw JSON text and returns parsed jobs.
/// Kept separate from `fetch_jobs` so the parsing logic — where all the
/// format-specific quirks live — can be tested without HTTP.
pub fn parse(body: &str, company_name: &str) -> Result<Vec<NormalizedJob>> {
    let response: Response = serde_json::from_str(body)
        .map_err(|_| CoreError::Internal("greenhouse json decode failed"))?;

    response
        .jobs
        .into_iter()
        .map(|j| -> Result<NormalizedJob> {
            // Unescape the double-encoded content. `content` is Some when
            // `?content=true` was in the request; we always request it, so
            // this is Some in practice.
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
                    .map_err(|_| CoreError::Internal("greenhouse bad url"))?,
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
    ) -> Result<Vec<NormalizedJob>> {
        let url =
            format!("https://boards-api.greenhouse.io/v1/boards/{slug}/jobs?content=true");
        let resp = client
            .get(&url)
            .send()
            .await
            .map_err(|_| CoreError::Internal("greenhouse request failed"))?
            .error_for_status()
            .map_err(|_| CoreError::Internal("greenhouse non-2xx"))?;
        let body = resp
            .text()
            .await
            .map_err(|_| CoreError::Internal("greenhouse body read"))?;
        parse(&body, company_name)
    }
}