//! CSS-selector fallback for portals that are not on a recognised ATS
//! and do not expose JSON-LD `JobPosting` schema.
//!
//! The configuration is stored as JSON in `portals.scraper_config` and
//! deserialised into `SelectorConfig`. Every field except `list` and
//! `title` is optional; a portal with a minimal config (list + title
//! selector) will work.
//!
//! ## URL resolution
//!
//! The job URL is resolved in this order:
//!
//! 1. If `fields.url` is set, the first element matching that selector
//!    must carry the `url_attribute` (default `href`). Explicit config
//!    wins.
//! 2. Otherwise, if the element matching `fields.title` carries the
//!    attribute, that is used. This handles `<a class="title"
//!    href="/jobs/1">Title</a>`.
//! 3. Otherwise, a descendant `a[href]` of the title element is used.
//!    This handles the most common pattern, `<h2 class="title"><a
//!    href="/jobs/1">Title</a></h2>`.
//!
//! The extracted value is resolved against `source_url`, so relative
//! paths work as expected.

use serde::Deserialize;
use scraper::{ElementRef, Html, Selector};
use url::Url;

use super::super::ats::NormalizedJob;

#[derive(Debug, Clone, Deserialize)]
pub struct SelectorConfig {
    /// Selector for each job card in the list. One `NormalizedJob` is
    /// produced per matched element.
    pub list: String,

    /// Per-card field selectors.
    pub fields: FieldSelectors,

    /// Attribute name that holds the job URL. Defaults to `"href"`.
    #[serde(default = "default_url_attribute")]
    pub url_attribute: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct FieldSelectors {
    /// Title text. Required.
    pub title: String,

    /// Selector for the element whose `url_attribute` gives the URL.
    /// If omitted, the URL is taken from the title element or a
    /// descendant anchor of it.
    #[serde(default)]
    pub url: Option<String>,

    #[serde(default)]
    pub location: Option<String>,

    #[serde(default)]
    pub department: Option<String>,

    #[serde(default)]
    pub employment_type: Option<String>,

    /// Optional description snippet. If absent, `raw_text` is empty and
    /// the router is expected to fetch the detail page later.
    #[serde(default)]
    pub description: Option<String>,
}

fn default_url_attribute() -> String {
    "href".to_string()
}

/// Extract jobs from a static HTML page using the supplied selectors.
///
/// Returns `None` if the page yields no jobs — that is a routing signal,
/// not an error. A parse failure (invalid CSS selector) also returns
/// `None`; the caller cannot distinguish an empty board from a bad
/// config, and both are handled the same way (escalate to Tier 2).
pub fn extract(
    html: &str,
    source_url: &Url,
    company_name: &str,
    config: &SelectorConfig,
) -> Option<Vec<NormalizedJob>> {
    let document = Html::parse_document(html);
    let list_sel = Selector::parse(&config.list).ok()?;

    let mut jobs = Vec::new();
    for card in document.select(&list_sel) {
        let Some(title) = select_text(&card, &config.fields.title) else {
            continue;
        };
        if title.is_empty() {
            continue;
        }

        let Some(href) = select_job_url(
            &card,
            &config.fields.title,
            config.fields.url.as_deref(),
            &config.url_attribute,
        ) else {
            continue;
        };
        let Ok(job_url) = source_url.join(&href) else {
            continue;
        };

        // Without a stable external ID in the page, use the resolved URL.
        // URLs are stable per posting, so this is the best available
        // identifier.
        let external_id = format!("tier1b:{}", job_url.as_str());

        jobs.push(NormalizedJob {
            external_id,
            title,
            company: company_name.to_string(),
            location: config
                .fields
                .location
                .as_deref()
                .and_then(|s| select_text(&card, s)),
            department: config
                .fields
                .department
                .as_deref()
                .and_then(|s| select_text(&card, s)),
            employment_type: config
                .fields
                .employment_type
                .as_deref()
                .and_then(|s| select_text(&card, s)),
            workplace_type: None,
            source_url: job_url,
            raw_text: config
                .fields
                .description
                .as_deref()
                .and_then(|s| select_text(&card, s))
                .unwrap_or_default(),
            posted_at: None,
        });
    }

    if jobs.is_empty() {
        None
    } else {
        Some(jobs)
    }
}

/// Resolve the job URL for one card. See the module docstring for the
/// resolution order.
fn select_job_url(
    card: &ElementRef<'_>,
    title_selector: &str,
    explicit_url_selector: Option<&str>,
    attr: &str,
) -> Option<String> {
    // Explicit config wins: the first element matching the selector must
    // carry the attribute. No fallback — if the user configured a URL
    // selector, respect it.
    if let Some(sel) = explicit_url_selector {
        return select_attr(card, sel, attr);
    }

    // No explicit URL selector. Try the title element itself first...
    if let Some(v) = select_attr(card, title_selector, attr) {
        return Some(v);
    }

    // ...then fall back to a descendant `a[href]` of the title element.
    // Real career pages almost always wrap the title text in an anchor.
    let title_sel = Selector::parse(title_selector).ok()?;
    let title_el = card.select(&title_sel).next()?;
    let a_sel = Selector::parse("a[href]").ok()?;
    title_el
        .select(&a_sel)
        .next()?
        .value()
        .attr(attr)
        .map(str::to_string)
}

fn select_text(el: &ElementRef<'_>, selector: &str) -> Option<String> {
    let sel = Selector::parse(selector).ok()?;
    let text = el.select(&sel).next()?.text().collect::<String>();
    let trimmed = text.trim().to_string();
    if trimmed.is_empty() {
        None
    } else {
        Some(trimmed)
    }
}

fn select_attr(el: &ElementRef<'_>, selector: &str, attr: &str) -> Option<String> {
    let sel = Selector::parse(selector).ok()?;
    el.select(&sel).next()?.value().attr(attr).map(str::to_string)
}