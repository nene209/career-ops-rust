//! List-fetch reporting shared by the acquisition engines.
//!
//! Every provider returns more than a list of jobs: it also knows how many pages
//! produced them and why it stopped. The router carries that into
//! `FetchOutcome`, so a partial fetch cannot present itself as complete.

use super::ats::NormalizedJob;

/// Why a list loop stopped.
///
/// The first two are the expected endings. Everything else means a page the
/// config asked for was not read, which the caller should surface: a partial
/// list that reports itself as complete is worse than a classified failure.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaginationStop {
    /// The source offered no further page, or there was nothing left to page.
    EndOfList,
    /// The configured `max_pages` limit was reached.
    LimitReached,
    /// A later page could not be fetched.
    FetchFailed,
    /// A later page fetched but could not be parsed into jobs.
    ExtractionFailed,
    /// A later page produced no jobs the earlier pages had not already given.
    NoNewJobs,
    /// The next-page address was identical to the current one.
    RepeatedUrl,
}

impl PaginationStop {
    /// A stable lowercase label for logs and summaries.
    ///
    /// Stable on purpose: the log is grepped, and a `Debug` repr would change
    /// with a variant name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::EndOfList => "end-of-list",
            Self::LimitReached => "limit-reached",
            Self::FetchFailed => "fetch-failed",
            Self::ExtractionFailed => "extraction-failed",
            Self::NoNewJobs => "no-new-jobs",
            Self::RepeatedUrl => "repeated-url",
        }
    }
}

/// One list fetch: the jobs, how many pages produced them, and why the loop
/// stopped.
#[derive(Debug, Clone)]
pub struct PageReport {
    pub jobs: Vec<NormalizedJob>,
    /// Pages that produced jobs. A page that failed before extraction is not
    /// counted, so `pages_fetched + 1` is the page that was not read.
    pub pages_fetched: usize,
    pub stop: PaginationStop,
}

impl PageReport {
    /// A single page with no pagination.
    pub fn single(jobs: Vec<NormalizedJob>) -> Self {
        Self {
            jobs,
            pages_fetched: 1,
            stop: PaginationStop::EndOfList,
        }
    }
}
