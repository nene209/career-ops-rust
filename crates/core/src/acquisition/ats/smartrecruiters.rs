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
//! ## Descriptions come from a second request
//!
//! The list item has no body. `GET .../postings/{id}` carries
//! `jobAd.sections` (company description, job description, qualifications,
//! additional information); `compose_description` concatenates the
//! non-empty ones in that order. The router drives this pass through the
//! same `detail.max_jobs` bound as every other portal, so the adapter
//! exposes *where* the detail lives (`detail_url`) and *how* to read it
//! (`detail_from_body`) rather than fetching by itself.
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
//! The parsing logic is split into pure functions so each piece can be
//! tested in isolation:
//!
//! - `parse_page` — one JSON page → `Page` struct
//! - `dedupe_by_ref` — group by `refNumber`, prefer the English copy
//! - `normalize` — one `Posting` → `NormalizedJob`
//! - `parse_detail` / `compose_description` — one detail response → text

use std::collections::HashMap;

use async_trait::async_trait;
use serde::Deserialize;
use url::Url;

use super::{parse_iso8601, AtsAdapter, JobDetail, NormalizedJob};
use crate::acquisition::error::{AtsError, AtsResult};
use crate::acquisition::html::detail;
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
    pub department: Option<Label>,
    pub type_of_employment: Option<Label>,
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

/// A `{id, label}` object. The `id` is a number on the detail endpoint and a
/// string on some list responses, so only the label is read.
#[derive(Debug, Deserialize)]
pub struct Label {
    pub label: Option<String>,
}

/// One posting-detail response.
///
/// Only the fields that change the stored row are modelled. `postingUrl` is
/// deliberately ignored: it appends a slugified title, so it changes when a
/// posting is retitled, while `{slug}/{id}` does not.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PostingDetail {
    pub department: Option<Label>,
    pub type_of_employment: Option<Label>,
    pub job_ad: Option<JobAd>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct JobAd {
    #[serde(default)]
    pub sections: Sections,
}

#[derive(Debug, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sections {
    pub company_description: Option<Section>,
    pub job_description: Option<Section>,
    pub qualifications: Option<Section>,
    pub additional_information: Option<Section>,
}

#[derive(Debug, Deserialize)]
pub struct Section {
    pub title: Option<String>,
    pub text: Option<String>,
}

/// Parse one page of SmartRecruiters JSON.
pub fn parse_page(body: &str) -> AtsResult<Page> {
    serde_json::from_str(body).map_err(|e| AtsError::Parse(format!("smartrecruiters json: {e}")))
}

/// Parse one posting-detail response.
pub fn parse_detail(body: &str) -> AtsResult<PostingDetail> {
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

/// Concatenate the non-empty description sections in reading order.
///
/// Each section is prefixed with its own title when the response supplies
/// one, separated by a blank line. Empty sections are skipped rather than
/// producing a dangling heading — Ubisoft's `additionalInformation` is
/// routinely empty. An all-empty response yields an empty string, which the
/// router treats as "nothing enriched".
pub fn compose_description(sections: &Sections) -> String {
    let ordered = [
        sections.company_description.as_ref(),
        sections.job_description.as_ref(),
        sections.qualifications.as_ref(),
        sections.additional_information.as_ref(),
    ];

    let mut parts: Vec<String> = Vec::new();
    for section in ordered.into_iter().flatten() {
        let text = section.text.as_deref().unwrap_or("").trim();
        if text.is_empty() {
            continue;
        }
        match section.title.as_deref().map(str::trim) {
            Some(title) if !title.is_empty() => parts.push(format!("{title}\n\n{text}")),
            _ => parts.push(text.to_string()),
        }
    }
    parts.join("\n\n")
}

/// The stable page for one posting.
///
/// Built from the API `id`, not the API's own `postingUrl`: the latter
/// appends a slugified title and differs per language, so it changes when a
/// posting is retitled and would defeat deduplication.
pub fn posting_url(slug: &str, id: &str) -> AtsResult<Url> {
    Url::parse(&format!("https://jobs.smartrecruiters.com/{slug}/{id}"))
        .map_err(|e| AtsError::Parse(format!("smartrecruiters url: {e}")))
}

/// Normalize one `Posting` into the common shape.
///
/// Location is joined as `"{city}, {country}"`, matching the other
/// adapters. `raw_text` is left empty: the list response omits descriptions
/// and the router enriches them through `detail_url`. Department and
/// employment type are available on the list item, so every row carries them
/// even when only the first `max_jobs` postings get a detail request.
pub fn normalize(p: Posting, company_name: &str, slug: &str) -> AtsResult<NormalizedJob> {
    let location = p.location.and_then(|l| match (l.city, l.country) {
        (Some(c), Some(k)) => Some(format!("{c}, {k}")),
        (Some(c), None) => Some(c),
        (None, Some(k)) => Some(k),
        _ => None,
    });

    Ok(NormalizedJob {
        // Prefer refNumber over id: it is the stable identifier across
        // language variants. The id is what the per-posting URL needs.
        external_id: p.ref_number.clone().unwrap_or_else(|| p.id.clone()),
        title: p.name,
        company: company_name.to_string(),
        location,
        department: p.department.and_then(|d| d.label),
        employment_type: p.type_of_employment.and_then(|t| t.label),
        workplace_type: None,
        source_url: posting_url(slug, &p.id)?,
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

    /// The detail endpoint for a posting. The API id is the last segment of
    /// the per-posting URL that `normalize` builds.
    fn detail_url(&self, slug: &str, job: &NormalizedJob) -> Option<Url> {
        let id = job.source_url.path_segments()?.next_back()?;
        if id.is_empty() {
            return None;
        }
        Url::parse(&format!(
            "https://api.smartrecruiters.com/v1/companies/{slug}/postings/{id}"
        ))
        .ok()
    }

    fn detail_from_body(&self, body: &str) -> Option<JobDetail> {
        let detail = parse_detail(body).ok()?;
        let sections = detail.job_ad.map(|ad| ad.sections).unwrap_or_default();

        Some(JobDetail {
            text: detail::truncate(compose_description(&sections)),
            department: detail.department.and_then(|d| d.label),
            employment_type: detail.type_of_employment.and_then(|t| t.label),
        })
    }
}
