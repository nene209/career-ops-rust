//! Eightfold site provider.
//!
//! Eightfold career sites are shells around a JSON API:
//!
//! - list: `GET https://{host}/api/pcsx/search?domain={domain}&start={offset}`
//! - detail: `GET https://{host}/api/pcsx/position_details?position_id={id}&domain={domain}&hl=en`
//!
//! This is an **engine**, not a site profile: every Eightfold employer uses the
//! same endpoints. `known_sites.rs` names the host and the domain; the code
//! below knows nothing about NVIDIA.
//!
//! ## Shape notes (probed 2026-11-03)
//!
//! The list returns ten positions per page and ignores `num`; the loop advances
//! by the number of positions the API actually returned. A position's `id` is a
//! JSON number on NVIDIA and may be a string elsewhere, so the deserialiser
//! accepts both. `positionUrl` is relative (`/careers/job/{id}`) and is resolved
//! against the portal host. The list carries no description; the detail's
//! `jobDescription` is HTML and is stored as-is, truncated like every other
//! provider.

use std::collections::HashSet;

use async_trait::async_trait;
use reqwest::Client;
use serde::de::{self, Deserializer};
use serde::Deserialize;
use url::Url;

use super::ats::{JobDetail, NormalizedJob};
use super::error::{AtsError, AtsResult};
use super::html::detail;
use super::http;
use super::report::{PageReport, PaginationStop};

/// One position from the list endpoint.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Position {
    #[serde(deserialize_with = "deserialize_id")]
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub locations: Vec<String>,
    #[serde(default)]
    pub standardized_locations: Vec<String>,
    #[serde(default)]
    pub department: Option<String>,
    #[serde(default)]
    pub position_url: Option<String>,
    #[serde(default)]
    pub posted_ts: Option<i64>,
}

/// `data` from the list endpoint.
#[derive(Debug, Default, Clone, Deserialize)]
pub struct ListPage {
    /// Total positions on the board, when the API reports it. `None` means the
    /// loop cannot stop on the count and relies on the page cap or an empty
    /// page.
    #[serde(default)]
    pub count: Option<usize>,
    #[serde(default)]
    pub positions: Vec<Position>,
}

#[derive(Debug, Deserialize)]
struct ListEnvelope {
    #[serde(default)]
    status: Option<u16>,
    data: ListPage,
}

#[derive(Debug, Deserialize)]
struct DetailEnvelope {
    #[serde(default)]
    status: Option<u16>,
    data: DetailPage,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct DetailPage {
    #[serde(default)]
    job_description: Option<String>,
    #[serde(default)]
    department: Option<String>,
    #[serde(default)]
    efcustom_text_time_type: Vec<String>,
}

/// Accept the id as a number (NVIDIA) or a string (other Eightfold sites).
fn deserialize_id<'de, D>(deserializer: D) -> Result<String, D::Error>
where
    D: Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    match value {
        serde_json::Value::Number(number) => Ok(number.to_string()),
        serde_json::Value::String(text) => Ok(text),
        other => Err(de::Error::custom(format!(
            "eightfold position id must be a number or a string, got {other}"
        ))),
    }
}

/// Parse one list response.
pub fn parse_page(body: &str) -> AtsResult<ListPage> {
    let envelope: ListEnvelope = serde_json::from_str(body)
        .map_err(|error| AtsError::Parse(format!("eightfold json: {error}")))?;

    if let Some(status) = envelope.status {
        if status != 200 {
            return Err(AtsError::Parse(format!(
                "eightfold returned status {status} in a success response"
            )));
        }
    }

    Ok(envelope.data)
}

/// Normalize one position into the common shape.
pub fn normalize(
    position: &Position,
    company_name: &str,
    host: &str,
    domain: &str,
) -> AtsResult<NormalizedJob> {
    let location = position
        .locations
        .iter()
        .find(|value| !value.trim().is_empty())
        .cloned()
        .or_else(|| {
            position
                .standardized_locations
                .iter()
                .find(|value| !value.trim().is_empty())
                .cloned()
        });

    Ok(NormalizedJob {
        external_id: format!("eightfold:{domain}:{}", position.id),
        title: position.name.clone(),
        company: company_name.to_string(),
        location,
        department: position.department.clone(),
        employment_type: None,
        workplace_type: None,
        source_url: position_url(host, position)?,
        raw_text: String::new(),
        posted_at: position
            .posted_ts
            .and_then(|timestamp| chrono::DateTime::from_timestamp(timestamp, 0)),
    })
}

/// The detail endpoint for a position, derived from its stable external id.
pub fn detail_url(host: &str, domain: &str, job: &NormalizedJob) -> Option<Url> {
    let prefix = format!("eightfold:{domain}:");
    let position_id = job.external_id.strip_prefix(&prefix)?;
    if position_id.is_empty() {
        return None;
    }

    let mut url = Url::parse(&format!("https://{host}/api/pcsx/position_details")).ok()?;
    url.query_pairs_mut()
        .append_pair("position_id", position_id)
        .append_pair("domain", domain)
        .append_pair("hl", "en");
    Some(url)
}

/// Parse one detail response into the description and metadata.
pub fn detail_from_body(body: &str) -> Option<JobDetail> {
    let envelope: DetailEnvelope = match serde_json::from_str(body) {
        Ok(envelope) => envelope,
        Err(error) => {
            tracing::warn!(%error, "eightfold detail did not parse");
            return None;
        }
    };

    if envelope.status.is_some_and(|status| status != 200) {
        tracing::warn!(status = ?envelope.status, "eightfold detail returned a non-200 envelope");
        return None;
    }

    let text = detail::truncate(envelope.data.job_description.unwrap_or_default());
    let employment_type = envelope
        .data
        .efcustom_text_time_type
        .into_iter()
        .find(|value| !value.trim().is_empty());

    Some(JobDetail {
        text,
        department: envelope.data.department,
        employment_type,
    })
}

/// Fetch up to `max_pages` list pages from an Eightfold site.
pub async fn fetch_jobs(
    client: &Client,
    host: &str,
    domain: &str,
    company_name: &str,
    max_pages: usize,
) -> AtsResult<PageReport> {
    let source = HttpPageSource { client };
    fetch_jobs_with(&source, host, domain, company_name, max_pages).await
}

/// The paging loop's HTTP seam, so stop conditions are testable without a
/// network. Mirrors the selector path's `PageSource`.
#[async_trait]
trait PageSource: Send + Sync {
    async fn fetch_page(&self, url: &Url) -> AtsResult<ListPage>;
}

struct HttpPageSource<'a> {
    client: &'a Client,
}

#[async_trait]
impl PageSource for HttpPageSource<'_> {
    async fn fetch_page(&self, url: &Url) -> AtsResult<ListPage> {
        let response = http::get_with_retry(self.client, url.as_str())
            .await
            .map_err(|error| AtsError::Transport(error.to_string()))?;

        if !response.status().is_success() {
            return Err(AtsError::from_status(response.status()));
        }

        let body = response
            .text()
            .await
            .map_err(|error| AtsError::Transport(error.to_string()))?;
        parse_page(&body)
    }
}

async fn fetch_jobs_with(
    source: &dyn PageSource,
    host: &str,
    domain: &str,
    company_name: &str,
    max_pages: usize,
) -> AtsResult<PageReport> {
    let max_pages = max_pages.max(1);
    let mut jobs: Vec<NormalizedJob> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut start = 0usize;
    let mut pages_fetched = 0;

    for _page in 1..=max_pages {
        let url = search_url(host, domain, start)?;
        let page = match source.fetch_page(&url).await {
            Ok(page) => page,
            Err(error) if pages_fetched == 0 => return Err(error),
            Err(error) => {
                tracing::warn!(start, %error, "eightfold pagination stopped: page fetch failed");
                return Ok(PageReport {
                    jobs,
                    pages_fetched,
                    stop: PaginationStop::FetchFailed,
                });
            }
        };

        if page.positions.is_empty() {
            return Ok(PageReport {
                jobs,
                pages_fetched,
                stop: PaginationStop::EndOfList,
            });
        }

        let before = jobs.len();
        for position in &page.positions {
            let job = match normalize(position, company_name, host, domain) {
                Ok(job) => job,
                Err(error) if pages_fetched == 0 => return Err(error),
                Err(error) => {
                    tracing::warn!(position = %position.id, %error, "eightfold position did not normalize");
                    return Ok(PageReport {
                        jobs,
                        pages_fetched,
                        stop: PaginationStop::ExtractionFailed,
                    });
                }
            };
            if seen.insert(job.external_id.clone()) {
                jobs.push(job);
            }
        }
        pages_fetched += 1;

        if jobs.len() == before && pages_fetched > 1 {
            return Ok(PageReport {
                jobs,
                pages_fetched,
                stop: PaginationStop::NoNewJobs,
            });
        }

        start += page.positions.len();
        if page.count.is_some_and(|count| start >= count) {
            return Ok(PageReport {
                jobs,
                pages_fetched,
                stop: PaginationStop::EndOfList,
            });
        }
    }

    Ok(PageReport {
        jobs,
        pages_fetched,
        stop: PaginationStop::LimitReached,
    })
}

fn search_url(host: &str, domain: &str, start: usize) -> AtsResult<Url> {
    let mut url = Url::parse(&format!("https://{host}/api/pcsx/search"))
        .map_err(|error| AtsError::Parse(format!("eightfold url: {error}")))?;
    url.query_pairs_mut()
        .append_pair("domain", domain)
        .append_pair("start", &start.to_string());
    Ok(url)
}

fn position_url(host: &str, position: &Position) -> AtsResult<Url> {
    let raw = position.position_url.as_deref().ok_or_else(|| {
        AtsError::Parse(format!(
            "eightfold position {} has no positionUrl",
            position.id
        ))
    })?;

    if let Ok(url) = Url::parse(raw) {
        return Ok(url);
    }

    Url::parse(&format!("https://{host}"))
        .and_then(|base| base.join(raw))
        .map_err(|error| AtsError::Parse(format!("eightfold position url: {error}")))
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::Mutex;

    use super::*;

    /// Real-shaped list response with three positions. NVIDIA sends numeric ids
    /// and relative position URLs.
    const LIST_FIXTURE: &str = r#"{
      "status": 200,
      "data": {
        "count": 2750,
        "positions": [
          {
            "id": 893394864453,
            "name": "Senior Network Product Engineer",
            "locations": ["Remote - Vietnam"],
            "standardizedLocations": ["VN"],
            "department": "Engineer, Product Development",
            "postedTs": 1793664000,
            "positionUrl": "/careers/job/893394864453"
          },
          {
            "id": 893394864454,
            "name": "GPU Architect",
            "locations": ["Santa Clara, CA"],
            "standardizedLocations": ["US"],
            "department": "Hardware Engineering",
            "postedTs": 1793577600,
            "positionUrl": "/careers/job/893394864454"
          },
          {
            "id": "893394864455",
            "name": "Compiler Engineer",
            "locations": [],
            "standardizedLocations": ["DE"],
            "department": "Software Engineering",
            "postedTs": 1793491200,
            "positionUrl": "https://jobs.nvidia.com/careers/job/893394864455"
          }
        ]
      }
    }"#;

    const DETAIL_FIXTURE: &str = r#"{
      "status": 200,
      "data": {
        "id": 893394864453,
        "name": "Senior Network Product Engineer",
        "publicUrl": "https://jobs.nvidia.com/careers/job/893394864453",
        "department": "Engineer, Product Development",
        "locations": ["Remote - Vietnam"],
        "efcustomTextTimeType": ["Full time"],
        "jobDescription": "<p>Build the network stack.</p>"
      }
    }"#;

    struct ScriptedPages {
        pages: Mutex<VecDeque<AtsResult<ListPage>>>,
        calls: Mutex<Vec<String>>,
    }

    impl ScriptedPages {
        fn new(pages: Vec<AtsResult<ListPage>>) -> Self {
            Self {
                pages: Mutex::new(pages.into()),
                calls: Mutex::new(Vec::new()),
            }
        }

        fn calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl PageSource for ScriptedPages {
        async fn fetch_page(&self, url: &Url) -> AtsResult<ListPage> {
            self.calls.lock().unwrap().push(url.to_string());
            self.pages
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Err(AtsError::Transport("no scripted page left".into())))
        }
    }

    fn position(id: &str, name: &str) -> Position {
        Position {
            id: id.into(),
            name: name.into(),
            locations: vec!["Remote".into()],
            standardized_locations: Vec::new(),
            department: Some("Engineering".into()),
            position_url: Some(format!("/careers/job/{id}")),
            posted_ts: None,
        }
    }

    fn page(count: usize, positions: Vec<Position>) -> ListPage {
        ListPage {
            count: Some(count),
            positions,
        }
    }

    #[test]
    fn parse_page_reads_numbers_and_strings() {
        let page = parse_page(LIST_FIXTURE).expect("parse");

        assert_eq!(page.count, Some(2750));
        assert_eq!(page.positions.len(), 3);
        assert_eq!(page.positions[0].id, "893394864453");
        assert_eq!(page.positions[2].id, "893394864455");
    }

    #[test]
    fn parse_page_rejects_a_non_200_envelope() {
        let error =
            parse_page(r#"{"status": 404, "data": {"positions": []}}"#).expect_err("non-200");
        assert!(matches!(error, AtsError::Parse(_)));
    }

    #[test]
    fn normalize_maps_the_list_fields() {
        let page = parse_page(LIST_FIXTURE).expect("parse");

        let first = normalize(
            &page.positions[0],
            "NVIDIA",
            "jobs.nvidia.com",
            "nvidia.com",
        )
        .expect("normalize");
        assert_eq!(first.external_id, "eightfold:nvidia.com:893394864453");
        assert_eq!(first.title, "Senior Network Product Engineer");
        assert_eq!(first.company, "NVIDIA");
        assert_eq!(first.location.as_deref(), Some("Remote - Vietnam"));
        assert_eq!(
            first.department.as_deref(),
            Some("Engineer, Product Development")
        );
        assert_eq!(
            first.source_url.as_str(),
            "https://jobs.nvidia.com/careers/job/893394864453"
        );
        assert!(first.posted_at.is_some());
        assert!(first.raw_text.is_empty(), "the list has no description");

        let absolute = normalize(
            &page.positions[2],
            "NVIDIA",
            "jobs.nvidia.com",
            "nvidia.com",
        )
        .expect("normalize");
        assert_eq!(
            absolute.source_url.as_str(),
            "https://jobs.nvidia.com/careers/job/893394864455",
            "absolute position URLs are kept"
        );
        assert_eq!(
            absolute.location.as_deref(),
            Some("DE"),
            "an empty locations list falls back to the standardized location"
        );
    }

    #[test]
    fn an_id_is_the_stable_key_across_url_changes() {
        let mut first = position("42", "Engineer");
        let mut renamed = first.clone();
        renamed.position_url = Some("/careers/job/42-renamed".into());

        let a = normalize(&first, "NVIDIA", "jobs.nvidia.com", "nvidia.com").expect("normalize");
        let b = normalize(&renamed, "NVIDIA", "jobs.nvidia.com", "nvidia.com").expect("normalize");

        assert_eq!(a.external_id, b.external_id);

        first.position_url = None;
        assert!(normalize(&first, "NVIDIA", "jobs.nvidia.com", "nvidia.com").is_err());
    }

    #[test]
    fn detail_url_carries_the_position_and_domain() {
        let page = parse_page(LIST_FIXTURE).expect("parse");
        let job = normalize(
            &page.positions[0],
            "NVIDIA",
            "jobs.nvidia.com",
            "nvidia.com",
        )
        .expect("normalize");

        let url = detail_url("jobs.nvidia.com", "nvidia.com", &job).expect("detail url");

        assert_eq!(url.host_str(), Some("jobs.nvidia.com"));
        assert_eq!(url.path(), "/api/pcsx/position_details");
        let query: Vec<(String, String)> = url.query_pairs().into_owned().collect();
        assert!(query.contains(&("position_id".into(), "893394864453".into())));
        assert!(query.contains(&("domain".into(), "nvidia.com".into())));
    }

    #[test]
    fn detail_from_body_yields_text_and_employment_type() {
        let detail = detail_from_body(DETAIL_FIXTURE).expect("detail");

        assert_eq!(detail.text, "<p>Build the network stack.</p>");
        assert_eq!(
            detail.department.as_deref(),
            Some("Engineer, Product Development")
        );
        assert_eq!(detail.employment_type.as_deref(), Some("Full time"));
    }

    #[test]
    fn detail_from_body_tolerates_missing_sections() {
        let detail = detail_from_body(r#"{"status":200,"data":{"id":1}}"#).expect("detail");

        assert!(detail.text.is_empty());
        assert!(detail.department.is_none());
        assert!(detail.employment_type.is_none());
    }

    #[test]
    fn detail_from_body_rejects_broken_json() {
        assert!(detail_from_body("not json").is_none());
    }

    #[tokio::test]
    async fn pages_until_the_count_is_reached() {
        let source = ScriptedPages::new(vec![
            Ok(page(2, vec![position("1", "A")])),
            Ok(page(2, vec![position("2", "B")])),
        ]);

        let report = fetch_jobs_with(&source, "jobs.example.com", "example.com", "Acme", 5)
            .await
            .expect("fetch");

        assert_eq!(report.jobs.len(), 2);
        assert_eq!(report.pages_fetched, 2);
        assert_eq!(report.stop, PaginationStop::EndOfList);
        assert_eq!(
            source.calls(),
            vec![
                "https://jobs.example.com/api/pcsx/search?domain=example.com&start=0",
                "https://jobs.example.com/api/pcsx/search?domain=example.com&start=1",
            ]
        );
    }

    #[tokio::test]
    async fn the_page_cap_is_honoured() {
        let source = ScriptedPages::new(vec![
            Ok(page(30, vec![position("1", "A")])),
            Ok(page(30, vec![position("2", "B")])),
            Ok(page(30, vec![position("3", "C")])),
        ]);

        let report = fetch_jobs_with(&source, "jobs.example.com", "example.com", "Acme", 2)
            .await
            .expect("fetch");

        assert_eq!(report.jobs.len(), 2);
        assert_eq!(report.pages_fetched, 2);
        assert_eq!(report.stop, PaginationStop::LimitReached);
        assert_eq!(source.calls().len(), 2);
    }

    #[tokio::test]
    async fn an_empty_page_ends_the_list() {
        let source = ScriptedPages::new(vec![
            Ok(page(30, vec![position("1", "A")])),
            Ok(page(30, Vec::new())),
        ]);

        let report = fetch_jobs_with(&source, "jobs.example.com", "example.com", "Acme", 5)
            .await
            .expect("fetch");

        assert_eq!(report.jobs.len(), 1);
        assert_eq!(report.pages_fetched, 1);
        assert_eq!(report.stop, PaginationStop::EndOfList);
    }

    #[tokio::test]
    async fn a_failing_later_page_keeps_the_earlier_jobs() {
        let source = ScriptedPages::new(vec![
            Ok(page(30, vec![position("1", "A")])),
            Err(AtsError::Timeout("slow".into())),
        ]);

        let report = fetch_jobs_with(&source, "jobs.example.com", "example.com", "Acme", 5)
            .await
            .expect("partial results are not an error");

        assert_eq!(report.jobs.len(), 1);
        assert_eq!(report.pages_fetched, 1);
        assert_eq!(report.stop, PaginationStop::FetchFailed);
    }

    #[tokio::test]
    async fn a_failing_first_page_is_an_error() {
        let source = ScriptedPages::new(vec![Err(AtsError::Transport("down".into()))]);

        let error = fetch_jobs_with(&source, "jobs.example.com", "example.com", "Acme", 5)
            .await
            .expect_err("the router needs the first-page error");

        assert!(matches!(error, AtsError::Transport(_)));
    }

    #[tokio::test]
    async fn a_page_with_no_new_positions_stops() {
        let source = ScriptedPages::new(vec![
            Ok(page(30, vec![position("1", "A")])),
            Ok(page(30, vec![position("1", "A")])),
        ]);

        let report = fetch_jobs_with(&source, "jobs.example.com", "example.com", "Acme", 5)
            .await
            .expect("fetch");

        assert_eq!(report.jobs.len(), 1, "the duplicate is not stored twice");
        assert_eq!(report.pages_fetched, 2);
        assert_eq!(report.stop, PaginationStop::NoNewJobs);
    }
}
