// crates/core/src/acquisition/ats/smartrecruiters.rs

//! SmartRecruiters ATS adapter.
//!
//! Endpoint: `https://api.smartrecruiters.com/v1/companies/{slug}/postings?limit=N&offset=M`
//!
//! SmartRecruiters is the only adapter that paginates, and the only one
//! whose list response omits descriptions. It also **publishes the same
//! posting once per language**, all sharing one `refNumber`. Without
//! dedup, a 210-posting board published in two languages lands as 420
//! rows, all duplicating the same job.
//!
//! ## Empty boards vs. wrong slugs
//!
//! An unrecognised company slug returns **200 with an empty `content`
//! array**, not a 404. The adapter cannot distinguish "this company has
//! no open postings" from "this company slug does not exist" from the
//! response alone. The Portals UI health drill-down should present
//! "0 jobs — check the slug" rather than "fetch succeeded" when a
//! SmartRecruiters board returns empty.
//!
//! The parsing logic is split into three pure functions so each piece
//! can be tested in isolation:
//!
//! - `parse_page` — one JSON page → `Page` struct
//! - `dedupe_by_ref` — group by `refNumber`, prefer the English copy
//! - `normalize` — one `Posting` → `NormalizedJob`

use std::collections::HashMap;

use async_trait::async_trait;
use serde::Deserialize;
use url::Url;

use super::{parse_iso8601, AtsAdapter, NormalizedJob};
use crate::acquisition::error::{AtsError, AtsResult};
use crate::acquisition::http;

pub struct SmartRecruitersAdapter;

const PAGE_SIZE: usize = 100;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Page {
    pub offset: usize,
    pub limit: usize,
    pub total_found: usize,
    pub content: Vec<Posting>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Posting {
    pub id: String,
    pub name: String,
    pub ref_number: Option<String>,
    pub language: Option<Language>,
    pub location: Option<Location>,
    pub released_date: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct Language {
    pub code: String,
}

#[derive(Debug, Deserialize)]
pub struct Location {
    pub city: Option<String>,
    pub country: Option<String>,
}

/// Parse one page of SmartRecruiters JSON.
pub fn parse_page(body: &str) -> AtsResult<Page> {
    serde_json::from_str(body).map_err(|e| AtsError::Parse(format!("smartrecruiters json: {e}")))
}

/// Group postings by `refNumber` and prefer the English copy.
///
/// Postings without a `refNumber` are keyed by their own `id`, so they
/// do not collide. The result is sorted by `id` for deterministic
/// ordering in tests and logs.
pub fn dedupe_by_ref(postings: Vec<Posting>) -> Vec<Posting> {
    let mut by_ref: HashMap<String, Posting> = HashMap::new();

    for p in postings {
        let key = p.ref_number.clone().unwrap_or_else(|| p.id.clone());
        let is_english = p
            .language
            .as_ref()
            .map(|l| l.code.to_lowercase().starts_with("en"))
            .unwrap_or(false);

        match by_ref.get(&key) {
            None => {
                by_ref.insert(key, p);
            }
            Some(existing) => {
                let existing_english = existing
                    .language
                    .as_ref()
                    .map(|l| l.code.to_lowercase().starts_with("en"))
                    .unwrap_or(false);
                if is_english && !existing_english {
                    by_ref.insert(key, p);
                }
            }
        }
    }

    let mut result: Vec<Posting> = by_ref.into_values().collect();
    result.sort_by(|a, b| a.id.cmp(&b.id));
    result
}

/// Normalize one `Posting` into the common shape.
///
/// Location is joined as `"{city}, {country}"`, matching the other
/// adapters. `raw_text` is left empty: SmartRecruiters list responses
/// omit descriptions, and fetching them would require one request per
/// job. The router decides whether to fetch descriptions lazily.
pub fn normalize(p: Posting, company_name: &str, slug: &str) -> AtsResult<NormalizedJob> {
    let location = p.location.and_then(|l| match (l.city, l.country) {
        (Some(c), Some(k)) => Some(format!("{c}, {k}")),
        (Some(c), None) => Some(c),
        (None, Some(k)) => Some(k),
        _ => None,
    });

    Ok(NormalizedJob {
        // Prefer refNumber over id: it is the stable identifier across
        // language variants.
        external_id: p.ref_number.unwrap_or(p.id),
        title: p.name,
        company: company_name.to_string(),
        location,
        department: None,
        employment_type: None,
        workplace_type: None,
        source_url: Url::parse(&format!("https://jobs.smartrecruiters.com/{slug}"))
            .map_err(|e| AtsError::Parse(format!("smartrecruiters url: {e}")))?,
        raw_text: String::new(),
        posted_at: p.released_date.as_deref().and_then(parse_iso8601),
    })
}

#[async_trait]
impl AtsAdapter for SmartRecruitersAdapter {
    fn name(&self) -> &'static str {
        "smartrecruiters"
    }

    fn detect(&self, url: &Url) -> Option<String> {
        let host = url.host_str()?;
        if host != "careers.smartrecruiters.com" && host != "jobs.smartrecruiters.com" {
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
        let mut all: Vec<Posting> = Vec::new();
        let mut offset = 0usize;
        let mut total = usize::MAX;

        while offset < total {
            let url = format!(
                "https://api.smartrecruiters.com/v1/companies/{slug}/postings?limit={PAGE_SIZE}&offset={offset}"
            );
            let resp = http::get_with_retry(client, &url)
                .await
                .map_err(|e| AtsError::Transport(e.to_string()))?;

            if !resp.status().is_success() {
                return Err(AtsError::from_status(resp.status()));
            }

            let page: Page = resp
                .json()
                .await
                .map_err(|e| AtsError::Parse(format!("smartrecruiters json: {e}")))?;

            total = page.total_found;

            // Guard against infinite loop if the API returns empty
            // content but a nonzero totalFound.
            if page.content.is_empty() {
                break;
            }
            offset += page.content.len();
            all.extend(page.content);
        }

        let deduped = dedupe_by_ref(all);
        deduped
            .into_iter()
            .map(|p| normalize(p, company_name, slug))
            .collect()
    }
}
