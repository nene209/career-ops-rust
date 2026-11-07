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
    /// Unix seconds. Read defensively: see `deserialize_optional_timestamp`.
    #[serde(default, deserialize_with = "deserialize_optional_timestamp")]
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
    data: RawListPage,
}

/// `data` with its postings left unparsed.
///
/// Positions are deserialized one by one in `parse_page` so a single malformed
/// posting costs one posting, not the page. That matters: NVIDIA sends
/// `postedTs: 0.0` on some postings, and a page-level parse failure used to stop
/// a whole walk at that offset forever.
#[derive(Debug, Default, Deserialize)]
struct RawListPage {
    #[serde(default)]
    count: Option<usize>,
    #[serde(default)]
    positions: Vec<serde_json::Value>,
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
    #[serde(default, deserialize_with = "deserialize_optional_id")]
    id: Option<String>,
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    public_url: Option<String>,
    #[serde(default)]
    locations: Vec<String>,
    #[serde(default)]
    job_description: Option<String>,
    #[serde(default)]
    department: Option<String>,
    #[serde(default)]
    efcustom_text_time_type: Vec<String>,
}

/// Accept the optional id as a number or a string, or as nothing at all.
fn deserialize_optional_id<'de, D>(deserializer: D) -> Result<Option<String>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    match value {
        None | Some(serde_json::Value::Null) => Ok(None),
        Some(serde_json::Value::Number(number)) => Ok(Some(number.to_string())),
        Some(serde_json::Value::String(text)) => Ok(Some(text)),
        Some(other) => Err(de::Error::custom(format!(
            "eightfold position id must be a number or a string, got {other}"
        ))),
    }
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

/// Accept `postedTs` as an integer, a float, or a numeric string.
///
/// Some NVIDIA postings carry `"postedTs": 0.0`, which a plain `Option<i64>`
/// rejects — and because positions were parsed as one list, that single value
/// failed the entire page and stalled every walk at that offset. A timestamp is
/// optional metadata: zero and anything unreadable mean "no date" (epoch is not
/// a posting date), never an error.
fn deserialize_optional_timestamp<'de, D>(deserializer: D) -> Result<Option<i64>, D::Error>
where
    D: Deserializer<'de>,
{
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    let seconds = match value {
        Some(serde_json::Value::Number(number)) => number
            .as_i64()
            .or_else(|| number.as_f64().map(|float| float.trunc() as i64)),
        Some(serde_json::Value::String(text)) => text.trim().parse::<i64>().ok(),
        _ => None,
    };
    Ok(seconds.filter(|seconds| *seconds > 0))
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

    let RawListPage { count, positions } = envelope.data;
    let total = positions.len();
    let mut skipped = 0usize;
    let mut parsed = Vec::with_capacity(total);
    for (index, value) in positions.into_iter().enumerate() {
        match serde_json::from_value::<Position>(value) {
            Ok(position) => parsed.push(position),
            Err(error) => {
                skipped += 1;
                tracing::warn!(
                    index,
                    positions = total,
                    %error,
                    "eightfold position skipped: it did not parse"
                )
            }
        }
    }
    if skipped > 0 {
        // The page-level count is what tells a payload bug from a transport
        // one: the response arrived, and this many postings did not survive it.
        tracing::warn!(
            positions = total,
            skipped,
            "eightfold page kept without the postings it could not parse"
        );
    }

    Ok(ListPage {
        count,
        positions: parsed,
    })
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
    detail_url_for(host, domain, position_id)
}

/// The detail endpoint for a position id. Used when the id came from a URL
/// rather than from a list row.
pub fn detail_url_for(host: &str, domain: &str, position_id: &str) -> Option<Url> {
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

/// The numeric (or string) position id embedded in an Eightfold posting URL.
///
/// Registry URLs look like `/careers/job/{id}`; a slugged URL looks like
/// `/careers/job/{id}/some-title`. The segment after `job` wins, and an
/// all-digit trailing segment is the fallback.
pub fn position_id_from_url(url: &Url) -> Option<String> {
    let segments: Vec<&str> = url.path_segments()?.filter(|s| !s.is_empty()).collect();

    if let Some(window) = segments.windows(2).find(|pair| pair[0] == "job") {
        let candidate = window[1];
        if !candidate.is_empty() {
            return Some(candidate.to_string());
        }
    }

    segments
        .iter()
        .rev()
        .find(|segment| segment.chars().all(|c| c.is_ascii_digit()))
        .map(|segment| segment.to_string())
}

/// Build a complete posting from a detail response.
///
/// The list carries the same fields, but a single manual add does not have the
/// list row. The detail response is enough: id, name, public URL, locations,
/// department, employment type, and the description.
pub fn job_from_detail_body(
    body: &str,
    company: &str,
    domain: &str,
    fallback_url: &Url,
) -> Option<NormalizedJob> {
    let envelope: DetailEnvelope = serde_json::from_str(body).ok()?;
    if envelope.status.is_some_and(|status| status != 200) {
        return None;
    }

    let data = envelope.data;
    let id = data.id?;
    let title = data.name?.trim().to_string();
    if title.is_empty() {
        return None;
    }

    let source_url = data
        .public_url
        .as_deref()
        .and_then(|raw| Url::parse(raw.trim()).ok())
        .unwrap_or_else(|| fallback_url.clone());

    Some(NormalizedJob {
        external_id: format!("eightfold:{domain}:{id}"),
        title,
        company: company.to_string(),
        location: data
            .locations
            .into_iter()
            .find(|value| !value.trim().is_empty()),
        department: data.department,
        employment_type: data
            .efcustom_text_time_type
            .into_iter()
            .find(|value| !value.trim().is_empty()),
        workplace_type: None,
        source_url,
        raw_text: detail::truncate(data.job_description.unwrap_or_default()),
        posted_at: None,
    })
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

/// Fetch up to `max_pages` list pages from an Eightfold site, starting at
/// `start_page` (1-based).
///
/// The API ignores `num` and returns ten positions per page, so a page number
/// maps to an offset. If the vendor ever changes that, dedup keeps the result
/// correct and only the cursor can drift.
pub async fn fetch_jobs(
    client: &Client,
    host: &str,
    domain: &str,
    company_name: &str,
    start_page: usize,
    max_pages: usize,
) -> AtsResult<PageReport> {
    let source = HttpPageSource { client };
    fetch_jobs_with(&source, host, domain, company_name, start_page, max_pages).await
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

/// The API returns ten positions per page regardless of `num`.
const PAGE_SIZE: usize = 10;

async fn fetch_jobs_with(
    source: &dyn PageSource,
    host: &str,
    domain: &str,
    company_name: &str,
    start_page: usize,
    max_pages: usize,
) -> AtsResult<PageReport> {
    let max_pages = max_pages.max(1);
    let mut jobs: Vec<NormalizedJob> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut start = (start_page.max(1) - 1) * PAGE_SIZE;
    let mut pages_fetched = 0;

    for _page in 0..max_pages {
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
        // The same field names as the HTML loop, so one query finds both
        // engines. `offset` is the `start` parameter the API was given.
        tracing::info!(
            page = pages_fetched,
            offset = start,
            positions = page.positions.len(),
            added = jobs.len() - before,
            "page read"
        );

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

    fn fallback_url() -> Url {
        Url::parse("https://jobs.nvidia.com/careers/job/893394864453").expect("url")
    }

    #[test]
    fn a_detail_body_builds_a_complete_posting() {
        let job = job_from_detail_body(DETAIL_FIXTURE, "NVIDIA", "nvidia.com", &fallback_url())
            .expect("posting");

        assert_eq!(job.external_id, "eightfold:nvidia.com:893394864453");
        assert_eq!(job.title, "Senior Network Product Engineer");
        assert_eq!(job.company, "NVIDIA");
        assert_eq!(job.location.as_deref(), Some("Remote - Vietnam"));
        assert_eq!(
            job.department.as_deref(),
            Some("Engineer, Product Development")
        );
        assert_eq!(job.employment_type.as_deref(), Some("Full time"));
        assert!(job.raw_text.contains("network stack"));
        assert_eq!(
            job.source_url.as_str(),
            "https://jobs.nvidia.com/careers/job/893394864453"
        );
    }

    #[test]
    fn a_detail_body_without_a_name_is_not_a_posting() {
        let body = r#"{"status":200,"data":{"id":7,"jobDescription":"text"}}"#;

        assert!(job_from_detail_body(body, "NVIDIA", "nvidia.com", &fallback_url()).is_none());
    }

    #[test]
    fn a_detail_body_without_a_public_url_uses_the_pasted_url() {
        let body = r#"{"status":200,"data":{"id":7,"name":"Engineer","jobDescription":"text"}}"#;
        let pasted = Url::parse("https://jobs.nvidia.com/careers/job/7/engineer").expect("url");

        let job = job_from_detail_body(body, "NVIDIA", "nvidia.com", &pasted).expect("posting");

        assert_eq!(job.source_url, pasted);
    }

    #[test]
    fn position_ids_are_read_from_urls() {
        let cases = [
            ("https://jobs.nvidia.com/careers/job/42", Some("42")),
            (
                "https://jobs.nvidia.com/careers/job/42/senior-engineer",
                Some("42"),
            ),
            ("https://jobs.nvidia.com/careers/job/", None),
            ("https://jobs.nvidia.com/careers", None),
            ("https://jobs.nvidia.com/careers/42", Some("42")),
        ];

        for (raw, expected) in cases {
            let url = Url::parse(raw).expect("url");
            assert_eq!(position_id_from_url(&url).as_deref(), expected, "{raw}");
        }
    }

    #[test]
    fn a_detail_url_needs_a_position_id() {
        assert!(detail_url_for("jobs.nvidia.com", "nvidia.com", "").is_none());
        assert!(detail_url_for("jobs.nvidia.com", "nvidia.com", "42").is_some());
    }

    #[test]
    fn parse_page_reads_numbers_and_strings() {
        let page = parse_page(LIST_FIXTURE).expect("parse");

        assert_eq!(page.count, Some(2750));
        assert_eq!(page.positions.len(), 3);
        assert_eq!(page.positions[0].id, "893394864453");
        assert_eq!(page.positions[2].id, "893394864455");
    }

    /// NVIDIA sends `postedTs: 0.0` on some postings (page 30 of the board, for
    /// example). A plain `Option<i64>` rejects that and used to fail the whole
    /// page, which stalled every walk at that offset.
    #[test]
    fn a_float_timestamp_does_not_fail_the_page() {
        let body = r#"{"status":200,"data":{"count":2,"positions":[
            {"id":1,"name":"A","postedTs":0.0},
            {"id":2,"name":"B","postedTs":1793577600}
        ]}}"#;

        let page = parse_page(body).expect("parse");

        assert_eq!(page.positions.len(), 2);
        assert_eq!(
            page.positions[0].posted_ts, None,
            "zero means no date, not 1970"
        );
        assert_eq!(page.positions[1].posted_ts, Some(1793577600));
    }

    /// One malformed posting costs one posting, not the page.
    #[test]
    fn a_position_that_cannot_be_parsed_is_skipped() {
        let body = r#"{"status":200,"data":{"positions":[
            {"id":1,"name":"A"},
            {"id":2},
            "not an object"
        ]}}"#;

        let page = parse_page(body).expect("the page still parses");

        assert_eq!(
            page.positions.len(),
            1,
            "only the complete posting survives"
        );
        assert_eq!(page.positions[0].id, "1");
    }

    /// A skipped posting is logged with its index and the page's size: that is
    /// the pair that separates a payload bug from a transport one, and the pair
    /// that was missing when Phase 2.8's page 30 stalled.
    #[test]
    fn a_skipped_posting_names_its_index_and_the_page_size() {
        let handle = crate::logging::handle();
        let before = handle.latest_seq();
        let body = r#"{"status":200,"data":{"count":3,"positions":[
            {"id":1,"name":"A"},
            {"id":2,"name":"B"},
            "not an object"
        ]}}"#;

        let page = parse_page(body).expect("the page still parses");
        assert_eq!(page.positions.len(), 2);

        let lines = handle.snapshot();
        let skips: Vec<_> = lines
            .iter()
            .filter(|line| {
                line.seq > before && line.message.starts_with("eightfold position skipped")
            })
            .collect();
        assert_eq!(skips.len(), 1, "one warning per skipped posting");

        fn field<'a>(line: &'a crate::logging::LogLine, name: &str) -> Option<&'a str> {
            line.fields
                .iter()
                .find(|(key, _)| key == name)
                .map(|(_, value)| value.as_str())
        }
        assert_eq!(field(skips[0], "index"), Some("2"), "the array position");
        assert_eq!(field(skips[0], "positions"), Some("3"), "the page size");

        let summary = lines
            .iter()
            .find(|line| line.seq > before && line.message.starts_with("eightfold page kept"))
            .expect("the page says how much it lost");
        assert_eq!(field(summary, "positions"), Some("3"));
        assert_eq!(field(summary, "skipped"), Some("1"));
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

        let report = fetch_jobs_with(&source, "jobs.example.com", "example.com", "Acme", 1, 5)
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

        let report = fetch_jobs_with(&source, "jobs.example.com", "example.com", "Acme", 1, 2)
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

        let report = fetch_jobs_with(&source, "jobs.example.com", "example.com", "Acme", 1, 5)
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

        let report = fetch_jobs_with(&source, "jobs.example.com", "example.com", "Acme", 1, 5)
            .await
            .expect("partial results are not an error");

        assert_eq!(report.jobs.len(), 1);
        assert_eq!(report.pages_fetched, 1);
        assert_eq!(report.stop, PaginationStop::FetchFailed);
    }

    #[tokio::test]
    async fn a_failing_first_page_is_an_error() {
        let source = ScriptedPages::new(vec![Err(AtsError::Transport("down".into()))]);

        let error = fetch_jobs_with(&source, "jobs.example.com", "example.com", "Acme", 1, 5)
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

        let report = fetch_jobs_with(&source, "jobs.example.com", "example.com", "Acme", 1, 5)
            .await
            .expect("fetch");

        assert_eq!(report.jobs.len(), 1, "the duplicate is not stored twice");
        assert_eq!(report.pages_fetched, 2);
        assert_eq!(report.stop, PaginationStop::NoNewJobs);
    }
}
