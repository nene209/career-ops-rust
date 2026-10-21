// crates/core/src/acquisition/ats/lever.rs

//! Lever ATS adapter.
//!
//! Endpoint: `https://api.lever.co/v0/postings/{slug}?mode=json`
//!
//! Two traps:
//!
//! 1. **Lever's JSON is camelCase**, not snake_case. The struct needs
//!    `#[serde(rename_all = "camelCase")]` or every field lookup fails.
//! 2. **`createdAt` is epoch milliseconds**, not ISO-8601. A naive parse
//!    as seconds produces a date in 1970.
//!
//! EU-hosted boards 404 on the US host and must be retried against
//! `api.eu.lever.co`. That fallback lives in `fetch_jobs`, not `parse`.

use async_trait::async_trait;
use serde::Deserialize;
use url::Url;

use super::{AtsAdapter, NormalizedJob};
use crate::acquisition::error::{AtsError, AtsResult};
use crate::acquisition::http;

pub struct LeverAdapter;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Posting {
    id: String,
    text: String,
    hosted_url: String,
    created_at: i64,
    description_plain: Option<String>,
    categories: Option<Categories>,
    workplace_type: Option<String>,
}

#[derive(Debug, Deserialize)]
struct Categories {
    location: Option<String>,
    department: Option<String>,
    commitment: Option<String>,
}

/// Parse a Lever response body into normalized jobs.
///
/// Lever returns a bare JSON array, not a wrapper object.
pub fn parse(body: &str, company_name: &str) -> AtsResult<Vec<NormalizedJob>> {
    let postings: Vec<Posting> =
        serde_json::from_str(body).map_err(|e| AtsError::Parse(format!("lever json: {e}")))?;

    postings
        .into_iter()
        .map(|p| -> Result<NormalizedJob, AtsError> {
            Ok(NormalizedJob {
                external_id: p.id,
                title: p.text,
                company: company_name.to_string(),
                location: p.categories.as_ref().and_then(|c| c.location.clone()),
                department: p.categories.as_ref().and_then(|c| c.department.clone()),
                employment_type: p.categories.as_ref().and_then(|c| c.commitment.clone()),
                workplace_type: p.workplace_type,
                source_url: Url::parse(&p.hosted_url)
                    .map_err(|e| AtsError::Parse(format!("lever url: {e}")))?,
                raw_text: p.description_plain.unwrap_or_default(),
                // epoch milliseconds, not seconds.
                posted_at: chrono::DateTime::from_timestamp_millis(p.created_at),
            })
        })
        .collect()
}

#[async_trait]
impl AtsAdapter for LeverAdapter {
    fn name(&self) -> &'static str {
        "lever"
    }

    fn detect(&self, url: &Url) -> Option<String> {
        if url.host_str()? != "jobs.lever.co" {
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
        let us = format!("https://api.lever.co/v0/postings/{slug}?mode=json");
        let mut resp = http::get_with_retry(client, &us)
            .await
            .map_err(|e| AtsError::Transport(e.to_string()))?;

        // EU-hosted boards 404 on the US host. Retry once against EU.
        if resp.status() == reqwest::StatusCode::NOT_FOUND {
            let eu = format!("https://api.eu.lever.co/v0/postings/{slug}?mode=json");
            resp = http::get_with_retry(client, &eu)
                .await
                .map_err(|e| AtsError::Transport(e.to_string()))?;
        }

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
