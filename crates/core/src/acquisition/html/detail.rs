//! Detail-page enrichment for selector-configured portals.
//!
//! A list page usually carries a teaser, not the posting. This module turns a
//! fetched detail document into the description text stored in
//! `job_descriptions.raw_text`.
//!
//! Two paths, in order:
//!
//! 1. **JSON-LD `JobPosting`.** Most ATS-hosted and many custom detail pages
//!    already emit it, so no selectors are needed. An exact title match is
//!    preferred; if none matches, the first posting with text is used, because
//!    a page with a single posting cannot be ambiguous and titles are often
//!    formatted slightly differently between list and detail.
//! 2. **A selector.** `detail.fields.description` for pages without JSON-LD.
//!
//! Extraction is best-effort by design: `None` means "keep the list-level
//! text", never "fail the fetch".

use scraper::{Html, Selector};
use serde::Deserialize;
use url::Url;

use super::jsonld;

/// Descriptions are truncated at this size. Postings with embedded base64
/// images exist; the cap keeps one row from dominating the database.
pub const MAX_DESCRIPTION_BYTES: usize = 200 * 1024;

/// Configuration for detail-page enrichment.
///
/// `fields` is optional: a site whose detail pages emit JSON-LD needs no
/// selectors at all, so `{"detail": {}}` is a valid config.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct DetailConfig {
    /// Selector fallback for pages without JSON-LD.
    #[serde(default)]
    pub fields: Option<DetailFields>,

    /// Postings to enrich per fetch. Defaults to 40; clamped to
    /// `1..=MAX_JOBS_CEILING`.
    #[serde(default)]
    pub max_jobs: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DetailFields {
    /// Selector whose text becomes `raw_text`.
    pub description: String,
}

impl DetailConfig {
    /// Enrichment cap when the config does not say.
    ///
    /// Forty requests at the default 400 ms + jitter pacing is roughly half a
    /// minute: long enough to be worth showing, short enough to stay a fetch.
    pub const DEFAULT_MAX_JOBS: usize = 40;

    /// Hard ceiling, regardless of configuration.
    pub const MAX_JOBS_CEILING: usize = 200;

    /// Configured cap, clamped to `1..=MAX_JOBS_CEILING`. Remove the `detail`
    /// block to disable enrichment entirely.
    pub fn max_jobs(&self) -> usize {
        self.max_jobs
            .unwrap_or(Self::DEFAULT_MAX_JOBS)
            .clamp(1, Self::MAX_JOBS_CEILING)
    }
}

/// Extract a description from a detail document.
///
/// JSON-LD wins when present; the selector is the fallback. The returned text
/// is truncated to `MAX_DESCRIPTION_BYTES` and trimmed.
pub fn extract_detail(
    html: &str,
    base_url: &Url,
    expected_title: &str,
    config: &DetailConfig,
) -> Option<String> {
    if let Some(text) = from_jsonld(html, base_url, expected_title) {
        return Some(truncate(text));
    }

    let description_selector = config.fields.as_ref()?.description.as_str();
    let selector = Selector::parse(description_selector).ok()?;
    let document = Html::parse_document(html);
    let text = document
        .select(&selector)
        .next()?
        .text()
        .collect::<String>();
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(truncate(trimmed.to_string()))
}

fn from_jsonld(html: &str, base_url: &Url, expected_title: &str) -> Option<String> {
    let jobs = jsonld::extract(html, base_url, "")?;
    let has_text = |job: &crate::acquisition::ats::NormalizedJob| !job.raw_text.trim().is_empty();

    let preferred = jobs
        .iter()
        .find(|job| has_text(job) && job.title.trim().eq_ignore_ascii_case(expected_title.trim()));
    let chosen = preferred.or_else(|| jobs.iter().find(|job| has_text(job)))?;
    Some(chosen.raw_text.trim().to_string())
}

/// Clamp to `MAX_DESCRIPTION_BYTES`, cutting on a character boundary.
pub(crate) fn truncate(text: String) -> String {
    if text.len() <= MAX_DESCRIPTION_BYTES {
        return text;
    }

    let mut end = MAX_DESCRIPTION_BYTES;
    while !text.is_char_boundary(end) {
        end -= 1;
    }

    tracing::warn!(
        bytes = text.len(),
        kept = end,
        "job description truncated at the configured limit"
    );
    text[..end].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn url() -> Url {
        Url::parse("https://example.com/careers/1").expect("url")
    }

    fn jsonld_page(title: &str, description: &str) -> String {
        format!(
            r#"<html><head><script type="application/ld+json">
            {{"@context":"https://schema.org/","@type":"JobPosting","title":"{title}","description":"{description}"}}
            </script></head><body></body></html>"#
        )
    }

    fn selector_config() -> DetailConfig {
        serde_json::from_value(serde_json::json!({
            "fields": { "description": ".job-description" }
        }))
        .expect("valid config")
    }

    #[test]
    fn jsonld_description_is_extracted_without_selectors() {
        let config: DetailConfig = serde_json::from_value(serde_json::json!({})).expect("config");
        let html = jsonld_page("Engineer", "<p>Build things.</p>");

        let text = extract_detail(&html, &url(), "Engineer", &config).expect("description");

        assert_eq!(text, "<p>Build things.</p>");
    }

    #[test]
    fn jsonld_prefers_the_matching_title() {
        let config: DetailConfig = serde_json::from_value(serde_json::json!({})).expect("config");
        let html = r#"
        <html><body>
        <script type="application/ld+json">
        [
          {"@type":"JobPosting","title":"Designer","description":"Design things."},
          {"@type":"JobPosting","title":"Engineer","description":"Build things."}
        ]
        </script>
        </body></html>
        "#;

        let text = extract_detail(html, &url(), "engineer", &config).expect("description");

        assert_eq!(text, "Build things.");
    }

    #[test]
    fn jsonld_falls_back_to_the_only_posting() {
        let config: DetailConfig = serde_json::from_value(serde_json::json!({})).expect("config");
        let html = jsonld_page("Engineer (Remote)", "Build things.");

        let text = extract_detail(&html, &url(), "Engineer", &config).expect("description");

        assert_eq!(text, "Build things.");
    }

    #[test]
    fn the_selector_is_used_when_there_is_no_jsonld() {
        let html = r#"
        <html><body><div class="header">Acme</div>
        <div class="job-description"> Full description. </div></body></html>
        "#;

        let text =
            extract_detail(html, &url(), "Engineer", &selector_config()).expect("description");

        assert_eq!(text, "Full description.");
    }

    #[test]
    fn jsonld_wins_over_the_selector() {
        let html = r#"
        <html><head><script type="application/ld+json">
        {"@type":"JobPosting","title":"Engineer","description":"From JSON-LD."}
        </script></head>
        <body><div class="job-description">From the selector.</div></body></html>
        "#;

        let text =
            extract_detail(html, &url(), "Engineer", &selector_config()).expect("description");

        assert_eq!(text, "From JSON-LD.");
    }

    #[test]
    fn a_missing_selector_match_returns_none() {
        let html = "<html><body><p>Nothing useful.</p></body></html>";

        assert!(extract_detail(html, &url(), "Engineer", &selector_config()).is_none());
    }

    #[test]
    fn an_empty_description_returns_none() {
        let html = jsonld_page("Engineer", "");

        let config: DetailConfig = serde_json::from_value(serde_json::json!({})).expect("config");
        assert!(extract_detail(&html, &url(), "Engineer", &config).is_none());
    }

    #[test]
    fn an_oversize_description_is_truncated_on_a_character_boundary() {
        let huge = "é".repeat(MAX_DESCRIPTION_BYTES);
        let html = jsonld_page("Engineer", &huge);

        let config: DetailConfig = serde_json::from_value(serde_json::json!({})).expect("config");
        let text = extract_detail(&html, &url(), "Engineer", &config).expect("description");

        assert!(text.len() <= MAX_DESCRIPTION_BYTES);
        assert!(text.is_char_boundary(text.len()));
    }

    #[test]
    fn max_jobs_defaults_and_is_clamped() {
        let default: DetailConfig = serde_json::from_value(serde_json::json!({})).expect("config");
        assert_eq!(default.max_jobs(), DetailConfig::DEFAULT_MAX_JOBS);

        let huge: DetailConfig =
            serde_json::from_value(serde_json::json!({"max_jobs": 5000})).expect("config");
        assert_eq!(huge.max_jobs(), DetailConfig::MAX_JOBS_CEILING);

        let zero: DetailConfig =
            serde_json::from_value(serde_json::json!({"max_jobs": 0})).expect("config");
        assert_eq!(zero.max_jobs(), 1, "a config can never mean zero jobs");
    }
}
