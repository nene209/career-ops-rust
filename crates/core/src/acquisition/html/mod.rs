// crates/core/src/acquisition/html/mod.rs

//! Static HTML extraction for career pages that are not on a recognized ATS.

use std::collections::HashSet;

use async_trait::async_trait;
use reqwest::Client;
use scraper::{Html, Selector};
use tracing::warn;
use url::Url;

use self::selectors::{PaginationConfig, SelectorConfig};
use super::ats::NormalizedJob;
use super::error::{AtsError, AtsResult};
use super::http;
use super::report::{PageReport, PaginationStop};

/// Fetch the configured pages of a URL and extract jobs from their static HTML.
///
/// The extraction chain per page is:
///
/// 1. Fetch via `http::get_with_retry`. Transport failures become
///    `AtsError::Transport`.
/// 2. Reject non-2xx status, classified via `AtsError::from_status`
///    so a 403 becomes `Blocked` and a 5xx becomes `Blocked`.
/// 3. Detect JS-required. A page with an SPA shell and no job-like
///    links becomes `AtsError::Unsupported("js_required")`, which the
///    router escalates.
/// 4. Try JSON-LD `JobPosting`, then CSS selectors from the portal
///    config.
///
/// Pagination follows `selector_config.pagination` when present. Page one is the
/// portal URL; later pages come from the page's next link or from a URL
/// template. A failure after page one is not fatal: the pages already collected
/// are returned, because a partial board is more useful than none.
///
/// The `scraper_config` argument is already-parsed. The router parses
/// `portals.scraper_config` once and passes the result through, so this
/// function does not see raw JSON.
pub async fn fetch_static_jobs(
    client: &Client,
    url: &Url,
    company_name: &str,
    scraper_config: Option<&SelectorConfig>,
    start_page: usize,
) -> AtsResult<PageReport> {
    let source = HttpPageSource { client };
    fetch_pages_from(&source, url, company_name, scraper_config, start_page).await
}

/// Fetch from page one. The tests use this shape; the router calls
/// `fetch_pages_from` with an explicit start.
#[cfg(test)]
async fn fetch_pages(
    source: &dyn PageSource,
    first_url: &Url,
    company_name: &str,
    scraper_config: Option<&SelectorConfig>,
) -> AtsResult<PageReport> {
    fetch_pages_from(source, first_url, company_name, scraper_config, 1).await
}

/// Fetches one page of HTML.
///
/// A seam, not an abstraction for its own sake: the pagination stop
/// conditions (a later page failing, a repeated page, the cap) are only
/// testable without a real server, and neither CI nor the development sandbox
/// is guaranteed a network.
#[async_trait]
trait PageSource: Send + Sync {
    async fn fetch_page(&self, url: &Url) -> AtsResult<String>;
}

/// The production `PageSource`: one GET, classified the way the router expects.
struct HttpPageSource<'a> {
    client: &'a Client,
}

#[async_trait]
impl PageSource for HttpPageSource<'_> {
    async fn fetch_page(&self, url: &Url) -> AtsResult<String> {
        let resp = http::get_with_retry(self.client, url.as_str())
            .await
            .map_err(|e| AtsError::Transport(e.to_string()))?;

        // Reject non-2xx before the JS-required check. A 500 with an SPA
        // shell is a server problem, not a JS-required page, and escalating
        // it to Tier 2 would just fail again.
        if !resp.status().is_success() {
            return Err(AtsError::from_status(resp.status()));
        }

        let body = resp
            .text()
            .await
            .map_err(|e| AtsError::Transport(e.to_string()))?;

        if is_js_required(&body) {
            return Err(AtsError::Unsupported("js_required"));
        }

        Ok(body)
    }
}

/// Fetch pages 1..=limit, accumulate their jobs, and stop on the first sign
/// that continuing is pointless.
///
/// Errors on page one are returned: the router needs them to decide whether to
/// escalate to Tier 2. Errors after page one stop the loop and keep what was
/// collected.
async fn fetch_pages_from(
    source: &dyn PageSource,
    first_url: &Url,
    company_name: &str,
    scraper_config: Option<&SelectorConfig>,
    start_page: usize,
) -> AtsResult<PageReport> {
    let pagination = scraper_config.and_then(|config| config.pagination.as_ref());
    let max_pages = pagination.map_or(1, PaginationConfig::page_limit);
    let start_page = start_page.max(1);

    // Page one is the portal URL itself. Starting later needs a template:
    // a chain of next links cannot be jumped into.
    let first_page_url = if start_page == 1 {
        first_url.clone()
    } else {
        let template = pagination
            .and_then(|pagination| pagination.url_template.as_deref())
            .ok_or(AtsError::Unsupported(
                "a start page requires a url_template",
            ))?;
        let raw = template.replace("{page}", &start_page.to_string());
        Url::parse(&raw)
            .or_else(|_| first_url.join(&raw))
            .map_err(|error| AtsError::Parse(format!("start page url: {error}")))?
    };

    let mut jobs: Vec<NormalizedJob> = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    let mut page_url = first_page_url;
    let mut pages_fetched = 0;

    for offset in 0..max_pages {
        let page = start_page + offset;
        let body = match source.fetch_page(&page_url).await {
            Ok(body) => body,
            Err(error) if offset == 0 => return Err(error),
            Err(error) => {
                warn!(page, %error, "pagination stopped: page fetch failed");
                return Ok(PageReport {
                    jobs,
                    pages_fetched,
                    stop: PaginationStop::FetchFailed,
                });
            }
        };

        let page_jobs = match extract_from_html(&body, &page_url, company_name, scraper_config) {
            Ok(jobs) => jobs,
            Err(error) if offset == 0 && start_page == 1 => return Err(error),
            Err(_) if offset == 0 => {
                // The first page of a resumed run produced nothing: the board
                // has fewer pages than the cursor said. That is an ending, not
                // a failure, so the cursor is cleared instead of retried.
                return Ok(PageReport {
                    jobs,
                    pages_fetched,
                    stop: PaginationStop::EndOfList,
                });
            }
            Err(error) => {
                warn!(page, %error, "pagination stopped: page extraction failed");
                return Ok(PageReport {
                    jobs,
                    pages_fetched,
                    stop: PaginationStop::ExtractionFailed,
                });
            }
        };

        pages_fetched += 1;
        let added = push_unique(&mut jobs, &mut seen, page_jobs);
        if added == 0 && offset > 0 {
            // The site ignored the page parameter and served the same page,
            // or the next link points back at content we already have.
            warn!(page, "pagination stopped: the page produced no new jobs");
            return Ok(PageReport {
                jobs,
                pages_fetched,
                stop: PaginationStop::NoNewJobs,
            });
        }

        let Some(pagination) = pagination else {
            return Ok(PageReport {
                jobs,
                pages_fetched,
                stop: PaginationStop::EndOfList,
            });
        };
        let Some(next) = next_page_url(&page_url, &body, pagination, page + 1) else {
            return Ok(PageReport {
                jobs,
                pages_fetched,
                stop: PaginationStop::EndOfList,
            });
        };
        if next == page_url {
            warn!(page, "pagination stopped: the next page URL did not change");
            return Ok(PageReport {
                jobs,
                pages_fetched,
                stop: PaginationStop::RepeatedUrl,
            });
        }
        page_url = next;
    }

    Ok(PageReport {
        jobs,
        pages_fetched,
        stop: PaginationStop::LimitReached,
    })
}

/// Append jobs whose URL has not been seen yet; returns how many were added.
fn push_unique(
    jobs: &mut Vec<NormalizedJob>,
    seen: &mut HashSet<String>,
    page_jobs: Vec<NormalizedJob>,
) -> usize {
    let mut added = 0;
    for job in page_jobs {
        if seen.insert(job.source_url.to_string()) {
            jobs.push(job);
            added += 1;
        }
    }
    added
}

/// Resolve the next page URL from the page's next link, or from the template.
///
/// The link wins when present because it is the site's own paging decision;
/// the template is the fallback for sites that only render numbered links.
fn next_page_url(
    page_url: &Url,
    body: &str,
    pagination: &PaginationConfig,
    next_page: usize,
) -> Option<Url> {
    if let Some(selector) = pagination.next.as_deref() {
        if let Ok(selector) = Selector::parse(selector) {
            let document = Html::parse_document(body);
            if let Some(href) = document
                .select(&selector)
                .next()
                .and_then(|element| element.value().attr("href"))
            {
                if let Ok(next) = page_url.join(href) {
                    return Some(next);
                }
            }
        }
    }

    let template = pagination.url_template.as_deref()?;
    let raw = template.replace("{page}", &next_page.to_string());
    Url::parse(&raw).or_else(|_| page_url.join(&raw)).ok()
}

/// Run the extraction chain on an HTML string.
///
/// This is the same logic the static-fetch path uses, minus the HTTP and
/// JS-required check. It exists as a separate public function so Tier 2
/// (which has already rendered the page and therefore cannot be
/// JS-required) can reuse the extraction without a fetch wrapper.
pub fn extract_from_html(
    html: &str,
    url: &Url,
    company_name: &str,
    scraper_config: Option<&SelectorConfig>,
) -> AtsResult<Vec<NormalizedJob>> {
    if let Some(jobs) = jsonld::extract(html, url, company_name) {
        if !jobs.is_empty() {
            return Ok(jobs);
        }
    }

    if let Some(config) = scraper_config {
        if let Some(jobs) = selectors::extract(html, url, company_name, config) {
            if !jobs.is_empty() {
                return Ok(jobs);
            }
        }
    }

    Err(AtsError::Parse(
        "no jobs found in HTML (neither JSON-LD nor selectors matched)".into(),
    ))
}

/// Heuristic: a page that returns a shell with a known SPA root and no
/// job-like links is likely to need JavaScript. This is deliberately
/// conservative — a false positive escalates to Tier 2, which works; a
/// false negative returns an empty board, which looks like a bug.
pub fn is_js_required(html: &str) -> bool {
    let document = Html::parse_document(html);

    // If JSON-LD JobPosting is present, the page is not JS-required —
    // the data is already in the initial HTML.
    if document
        .select(&Selector::parse("script[type='application/ld+json']").unwrap())
        .next()
        .is_some()
    {
        return false;
    }

    // Common SPA roots.
    let spa_roots = ["#root", "#app", "#__next", "[data-reactroot]"];
    for sel in spa_roots {
        if let Ok(s) = Selector::parse(sel) {
            if document.select(&s).next().is_some() {
                // An SPA root alone is not proof; require that there are
                // also no job-like links in the static HTML.
                let job_links =
                    Selector::parse("a[href*='job'], a[href*='career'], a[href*='position']")
                        .unwrap();
                if document.select(&job_links).next().is_none() {
                    return true;
                }
            }
        }
    }
    false
}

pub mod detail;
pub mod jsonld;
pub mod selectors;

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;
    use std::sync::Mutex;

    use super::selectors::{PaginationConfig, SelectorConfig};
    use super::*;

    /// A `PageSource` that returns scripted pages in call order and records the
    /// URLs it was asked for.
    struct ScriptedPages {
        pages: Mutex<VecDeque<AtsResult<String>>>,
        calls: Mutex<Vec<String>>,
    }

    impl ScriptedPages {
        fn new(pages: Vec<AtsResult<String>>) -> Self {
            Self {
                pages: Mutex::new(pages.into()),
                calls: Mutex::new(Vec::new()),
            }
        }

        fn calls(&self) -> Vec<String> {
            self.calls.lock().unwrap().clone()
        }
    }

    #[async_trait::async_trait]
    impl PageSource for ScriptedPages {
        async fn fetch_page(&self, url: &Url) -> AtsResult<String> {
            self.calls.lock().unwrap().push(url.to_string());
            self.pages
                .lock()
                .unwrap()
                .pop_front()
                .unwrap_or_else(|| Err(AtsError::Transport("no scripted page left".into())))
        }
    }

    fn url(raw: &str) -> Url {
        Url::parse(raw).expect("valid url")
    }

    /// One card per `(title, href)`, plus an optional next link.
    fn page(jobs: &[(&str, &str)], next: Option<&str>) -> String {
        let mut html = String::from("<html><body>");
        for (title, href) in jobs {
            html.push_str(&format!(
                r#"<article class="job-card"><h2 class="title"><a href="{href}">{title}</a></h2></article>"#
            ));
        }
        if let Some(next) = next {
            html.push_str(&format!(r#"<a rel="next" href="{next}">Next</a>"#));
        }
        html.push_str("</body></html>");
        html
    }

    fn config(pagination: Option<serde_json::Value>) -> SelectorConfig {
        let mut value = serde_json::json!({
            "list": "article.job-card",
            "fields": { "title": "h2.title" }
        });
        if let Some(pagination) = pagination {
            value["pagination"] = pagination;
        }
        serde_json::from_value(value).expect("valid config")
    }

    fn next_selector(max_pages: Option<usize>) -> serde_json::Value {
        let mut value = serde_json::json!({ "next": "a[rel='next']" });
        if let Some(max_pages) = max_pages {
            value["max_pages"] = serde_json::json!(max_pages);
        }
        value
    }

    #[tokio::test]
    async fn no_pagination_config_fetches_one_page() {
        let source =
            ScriptedPages::new(vec![Ok(page(&[("A", "/jobs/a")], Some("/careers?page=2")))]);
        let config = config(None);

        let report = fetch_pages(
            &source,
            &url("https://example.com/careers"),
            "Acme",
            Some(&config),
        )
        .await
        .expect("fetch");
        let jobs = report.jobs;

        assert_eq!(jobs.len(), 1);
        assert_eq!(source.calls(), vec!["https://example.com/careers"]);
        assert_eq!(report.pages_fetched, 1);
        assert_eq!(report.stop, PaginationStop::EndOfList);
    }

    #[tokio::test]
    async fn a_pagination_block_with_no_mechanism_fetches_one_page() {
        let source =
            ScriptedPages::new(vec![Ok(page(&[("A", "/jobs/a")], Some("/careers?page=2")))]);
        let config = config(Some(serde_json::json!({})));

        let report = fetch_pages(
            &source,
            &url("https://example.com/careers"),
            "Acme",
            Some(&config),
        )
        .await
        .expect("fetch");
        let jobs = report.jobs;

        assert_eq!(jobs.len(), 1);
        assert_eq!(source.calls().len(), 1);
    }

    #[tokio::test]
    async fn next_links_are_followed_and_resolved_against_the_page() {
        let source = ScriptedPages::new(vec![
            Ok(page(&[("A", "/jobs/a")], Some("?page=2"))),
            Ok(page(
                &[("B", "/jobs/b")],
                Some("https://example.com/careers?page=3"),
            )),
            Ok(page(&[("C", "/jobs/c")], None)),
        ]);
        let config = config(Some(next_selector(None)));

        let report = fetch_pages(
            &source,
            &url("https://example.com/careers"),
            "Acme",
            Some(&config),
        )
        .await
        .expect("fetch");
        let jobs = report.jobs;

        assert_eq!(jobs.len(), 3);
        assert_eq!(
            source.calls(),
            vec![
                "https://example.com/careers",
                "https://example.com/careers?page=2",
                "https://example.com/careers?page=3",
            ],
            "relative and absolute next links both resolve"
        );
        assert_eq!(report.pages_fetched, 3);
        assert_eq!(report.stop, PaginationStop::EndOfList);
    }

    #[tokio::test]
    async fn a_url_template_builds_pages_from_two() {
        let source = ScriptedPages::new(vec![
            Ok(page(&[("A", "/jobs/a")], None)),
            Ok(page(&[("B", "/jobs/b")], None)),
            Ok(page(&[("C", "/jobs/c")], None)),
        ]);
        let config = config(Some(serde_json::json!({
            "url_template": "https://example.com/careers?page={page}",
            "max_pages": 3,
        })));

        let report = fetch_pages(
            &source,
            &url("https://example.com/careers"),
            "Acme",
            Some(&config),
        )
        .await
        .expect("fetch");
        let jobs = report.jobs;

        assert_eq!(jobs.len(), 3);
        assert_eq!(
            source.calls(),
            vec![
                "https://example.com/careers",
                "https://example.com/careers?page=2",
                "https://example.com/careers?page=3",
            ]
        );
    }

    #[tokio::test]
    async fn max_pages_is_honoured() {
        let source = ScriptedPages::new(vec![
            Ok(page(&[("A", "/jobs/a")], Some("/careers?page=2"))),
            Ok(page(&[("B", "/jobs/b")], Some("/careers?page=3"))),
            Ok(page(&[("C", "/jobs/c")], Some("/careers?page=4"))),
        ]);
        let config = config(Some(next_selector(Some(2))));

        let report = fetch_pages(
            &source,
            &url("https://example.com/careers"),
            "Acme",
            Some(&config),
        )
        .await
        .expect("fetch");
        let jobs = report.jobs;

        assert_eq!(jobs.len(), 2);
        assert_eq!(source.calls().len(), 2);
        assert_eq!(report.pages_fetched, 2);
        assert_eq!(report.stop, PaginationStop::LimitReached);
    }

    #[tokio::test]
    async fn the_default_page_limit_is_five() {
        let pages: Vec<AtsResult<String>> = (1..=6)
            .map(|n| {
                let title = format!("Job {n}");
                let href = format!("/jobs/{n}");
                let next = format!("/careers?page={}", n + 1);
                Ok(page(
                    &[(title.as_str(), href.as_str())],
                    Some(next.as_str()),
                ))
            })
            .collect();
        let source = ScriptedPages::new(pages);
        let config = config(Some(next_selector(None)));

        let report = fetch_pages(
            &source,
            &url("https://example.com/careers"),
            "Acme",
            Some(&config),
        )
        .await
        .expect("fetch");
        let jobs = report.jobs;

        assert_eq!(source.calls().len(), 5, "default limit is five pages");
        assert_eq!(jobs.len(), 5);
    }

    #[test]
    fn the_page_limit_is_clamped() {
        let unlimited = config(Some(next_selector(Some(500))));
        let limit = unlimited
            .pagination
            .as_ref()
            .expect("pagination")
            .page_limit();
        assert_eq!(limit, PaginationConfig::MAX_PAGES_CEILING);

        let zero = config(Some(next_selector(Some(0))));
        let limit = zero.pagination.as_ref().expect("pagination").page_limit();
        assert_eq!(limit, 1, "a config can never mean zero pages");
    }

    #[tokio::test]
    async fn a_repeated_page_stops_the_loop() {
        let first = page(&[("A", "/jobs/a")], Some("/careers?page=2"));
        let source = ScriptedPages::new(vec![Ok(first.clone()), Ok(first)]);
        let config = config(Some(next_selector(Some(5))));

        let report = fetch_pages(
            &source,
            &url("https://example.com/careers"),
            "Acme",
            Some(&config),
        )
        .await
        .expect("fetch");
        let jobs = report.jobs;

        assert_eq!(jobs.len(), 1, "a repeated page must not duplicate jobs");
        assert_eq!(
            source.calls().len(),
            2,
            "the loop stops after the repeated page"
        );
        assert_eq!(report.pages_fetched, 2);
        assert_eq!(report.stop, PaginationStop::NoNewJobs);
    }

    #[tokio::test]
    async fn duplicate_jobs_are_kept_once_while_new_ones_continue_the_loop() {
        let source = ScriptedPages::new(vec![
            Ok(page(&[("A", "/jobs/a")], Some("/careers?page=2"))),
            Ok(page(
                &[("A", "/jobs/a"), ("B", "/jobs/b")],
                Some("/careers?page=3"),
            )),
            Ok(page(&[("B", "/jobs/b")], None)),
        ]);
        let config = config(Some(next_selector(Some(5))));

        let report = fetch_pages(
            &source,
            &url("https://example.com/careers"),
            "Acme",
            Some(&config),
        )
        .await
        .expect("fetch");
        let jobs = report.jobs;

        let titles: Vec<&str> = jobs.iter().map(|job| job.title.as_str()).collect();
        assert_eq!(titles, vec!["A", "B"]);
    }

    #[tokio::test]
    async fn a_failing_later_page_returns_the_earlier_jobs() {
        let source = ScriptedPages::new(vec![
            Ok(page(&[("A", "/jobs/a")], Some("/careers?page=2"))),
            Err(AtsError::Timeout("slow".into())),
            Ok(page(&[("C", "/jobs/c")], None)),
        ]);
        let config = config(Some(next_selector(Some(5))));

        let report = fetch_pages(
            &source,
            &url("https://example.com/careers"),
            "Acme",
            Some(&config),
        )
        .await
        .expect("partial results are not an error");
        let jobs = report.jobs;

        assert_eq!(jobs.len(), 1);
        assert_eq!(source.calls().len(), 2, "the loop stops at the failure");
        assert_eq!(report.pages_fetched, 1);
        assert_eq!(report.stop, PaginationStop::FetchFailed);
    }

    #[tokio::test]
    async fn a_failing_first_page_is_returned_to_the_caller() {
        let source = ScriptedPages::new(vec![Err(AtsError::Transport("down".into()))]);
        let config = config(Some(next_selector(Some(5))));

        let error = fetch_pages(
            &source,
            &url("https://example.com/careers"),
            "Acme",
            Some(&config),
        )
        .await
        .expect_err("the router needs the first-page error");

        assert!(matches!(error, AtsError::Transport(_)));
    }

    #[tokio::test]
    async fn resuming_past_the_end_is_an_ending_not_a_failure() {
        let source = ScriptedPages::new(vec![Ok(
            "<html><body><p>No jobs here.</p></body></html>".into()
        )]);
        let config = config(Some(serde_json::json!({
            "url_template": "https://example.com/careers?page={page}",
            "max_pages": 5,
        })));

        let report = fetch_pages_from(
            &source,
            &url("https://example.com/careers"),
            "Acme",
            Some(&config),
            4,
        )
        .await
        .expect("a resumed empty page is not an error");

        assert_eq!(report.pages_fetched, 0);
        assert_eq!(report.stop, PaginationStop::EndOfList);
        assert_eq!(
            source.calls(),
            vec!["https://example.com/careers?page=4"],
            "the resumed page is the one requested"
        );
    }

    #[tokio::test]
    async fn a_later_page_with_no_jobs_stops_the_loop() {
        let source = ScriptedPages::new(vec![
            Ok(page(&[("A", "/jobs/a")], Some("/careers?page=2"))),
            Ok("<html><body><p>No jobs here.</p></body></html>".into()),
        ]);
        let config = config(Some(next_selector(Some(5))));

        let report = fetch_pages(
            &source,
            &url("https://example.com/careers"),
            "Acme",
            Some(&config),
        )
        .await
        .expect("the first page is still good");
        let jobs = report.jobs;

        assert_eq!(jobs.len(), 1);
        assert_eq!(report.pages_fetched, 1);
        assert_eq!(report.stop, PaginationStop::ExtractionFailed);
    }

    #[tokio::test]
    async fn a_template_without_a_placeholder_stops_instead_of_looping() {
        let source = ScriptedPages::new(vec![
            Ok(page(&[("A", "/jobs/a")], None)),
            Ok(page(&[("A", "/jobs/a")], None)),
        ]);
        let config = config(Some(serde_json::json!({
            "url_template": "https://example.com/careers",
            "max_pages": 5,
        })));

        let report = fetch_pages(
            &source,
            &url("https://example.com/careers"),
            "Acme",
            Some(&config),
        )
        .await
        .expect("fetch");
        let jobs = report.jobs;

        assert_eq!(jobs.len(), 1);
        assert_eq!(
            source.calls().len(),
            1,
            "an unchanged next URL must stop before the redundant fetch"
        );
        assert_eq!(report.stop, PaginationStop::RepeatedUrl);
    }
}
