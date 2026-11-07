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

            // The caller's company name is a label (a portal name, or "Manual"
            // for a single fetched posting). The page's own organization is
            // more accurate when the caller had nothing to offer.
            let organization = item
                .get("hiringOrganization")
                .and_then(|org| org.get("name"))
                .and_then(|name| name.as_str())
                .map(str::trim)
                .filter(|name| !name.is_empty());
            let company =
                if company_name.trim().is_empty() || company_name.eq_ignore_ascii_case("manual") {
                    organization.unwrap_or(company_name).to_string()
                } else {
                    company_name.to_string()
                };

            jobs.push(NormalizedJob {
                external_id,
                title,
                company,
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

#[cfg(test)]
mod tests {
    use super::*;

    const NVIDIA_POSTING: &str = r#"<script type="application/ld+json">
        {
          "@type": "JobPosting",
          "title": "Engineer",
          "description": "<p>Build things.</p>",
          "hiringOrganization": { "name": "NVIDIA" }
        }
    </script>"#;

    #[test]
    fn a_manual_fetch_takes_the_company_from_the_page() {
        let url = Url::parse("https://jobs.example.com/1").expect("url");

        let jobs = extract(NVIDIA_POSTING, &url, "Manual").expect("jobs");

        assert_eq!(jobs[0].company, "NVIDIA");
    }

    #[test]
    fn a_portal_name_is_not_overridden_by_the_page() {
        let url = Url::parse("https://jobs.example.com/1").expect("url");

        let jobs = extract(NVIDIA_POSTING, &url, "NVIDIA Careers").expect("jobs");

        assert_eq!(jobs[0].company, "NVIDIA Careers");
    }
}
