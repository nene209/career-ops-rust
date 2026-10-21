// crates/core/src/acquisition/html/jsonld.rs

use scraper::{Html, Selector};
use url::Url;

use super::super::ats::NormalizedJob;

pub fn extract(html: &str, source_url: &Url, company_name: &str) -> Option<Vec<NormalizedJob>> {
    let document = Html::parse_document(html);
    let sel = Selector::parse("script[type='application/ld+json']").ok()?;

    let mut jobs = Vec::new();
    for el in document.select(&sel) {
        let text = el.text().collect::<String>();
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };

        // JSON-LD can be a single object or an array.
        let items: Vec<&serde_json::Value> = match &value {
            serde_json::Value::Array(a) => a.iter().collect(),
            v => vec![v],
        };

        for item in items {
            if item.get("@type").and_then(|t| t.as_str()) != Some("JobPosting") {
                continue;
            }
            let title = item
                .get("title")
                .and_then(|t| t.as_str())
                .unwrap_or("")
                .to_string();
            if title.is_empty() {
                continue;
            }

            let description = item
                .get("description")
                .and_then(|d| d.as_str())
                .unwrap_or("")
                .to_string();

            let external_id = item
                .get("identifier")
                .and_then(|id| id.get("value").or(Some(id)))
                .and_then(|v| v.as_str())
                .map(str::to_string)
                .unwrap_or_else(|| format!("{source_url}#{title}"));

            jobs.push(NormalizedJob {
                external_id,
                title,
                company: company_name.to_string(),
                location: item
                    .get("jobLocation")
                    .and_then(|l| l.get("address"))
                    .and_then(|a| a.get("addressLocality"))
                    .and_then(|l| l.as_str())
                    .map(str::to_string),
                department: None,
                employment_type: item
                    .get("employmentType")
                    .and_then(|e| e.as_str())
                    .map(str::to_string),
                workplace_type: None,
                source_url: source_url.clone(),
                raw_text: description,
                posted_at: item
                    .get("datePosted")
                    .and_then(|d| d.as_str())
                    .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
                    .map(|d| d.with_timezone(&chrono::Utc)),
            });
        }
    }

    if jobs.is_empty() {
        None
    } else {
        Some(jobs)
    }
}
