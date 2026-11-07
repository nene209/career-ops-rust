//! Posting identity.
//!
//! Deduplication is only as good as the URL it compares. Providers emit the
//! same posting with different query strings, a trailing slash, or a renamed
//! slug, so a URL is canonicalized before it becomes an identity. The raw URL
//! is still stored for the user; this form exists only to compare.

use url::{form_urlencoded, Url};

/// Query parameters that describe a campaign, not a posting.
const TRACKING_PARAMS: &[&str] = &[
    "fbclid",
    "gclid",
    "mc_cid",
    "mc_eid",
    "ref",
    "referrer",
    "source",
    "utm_campaign",
    "utm_content",
    "utm_medium",
    "utm_source",
    "utm_term",
];

/// A stable identity for a posting URL.
///
/// - host is lowercased (done by `Url` itself) and the default port is dropped
/// - the fragment is discarded
/// - one trailing slash is dropped, so `/jobs/1/` and `/jobs/1` agree
/// - tracking parameters are removed and the rest are sorted, so `?b=2&a=1`
///   and `?a=1&b=2` agree
/// - everything else is preserved: paths are case-sensitive, and a query that
///   identifies a job (for example `?jobId=7`) survives.
pub fn canonical_url(url: &Url) -> String {
    let host = url.host_str().unwrap_or_default();
    let port = url
        .port()
        .map(|port| format!(":{port}"))
        .unwrap_or_default();
    let path = url.path().trim_end_matches('/');

    let mut canonical = format!("{}://{host}{port}{path}", url.scheme());

    if let Some(query) = canonical_query(url) {
        canonical.push('?');
        canonical.push_str(&query);
    }

    canonical
}

fn canonical_query(url: &Url) -> Option<String> {
    let mut pairs: Vec<(String, String)> = url
        .query_pairs()
        .filter(|(key, _)| !is_tracking(key))
        .map(|(key, value)| (key.into_owned(), value.into_owned()))
        .collect();

    if pairs.is_empty() {
        return None;
    }

    pairs.sort();
    let mut serializer = form_urlencoded::Serializer::new(String::new());
    serializer.extend_pairs(
        pairs
            .iter()
            .map(|(key, value)| (key.as_str(), value.as_str())),
    );
    Some(serializer.finish())
}

fn is_tracking(key: &str) -> bool {
    let key = key.to_ascii_lowercase();
    key.starts_with("utm_") || TRACKING_PARAMS.contains(&key.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn canonical(raw: &str) -> String {
        canonical_url(&Url::parse(raw).expect("url"))
    }

    #[test]
    fn the_host_is_lowercased_and_the_fragment_is_dropped() {
        assert_eq!(
            canonical("https://Jobs.NVIDIA.com/careers/job/1#apply"),
            "https://jobs.nvidia.com/careers/job/1"
        );
    }

    #[test]
    fn a_trailing_slash_is_not_significant() {
        assert_eq!(
            canonical("https://example.com/jobs/1/"),
            "https://example.com/jobs/1"
        );
        assert_eq!(canonical("https://example.com/"), "https://example.com");
        assert_eq!(canonical("https://example.com"), "https://example.com");
    }

    #[test]
    fn tracking_parameters_are_dropped_and_the_rest_are_sorted() {
        assert_eq!(
            canonical("https://example.com/jobs?utm_source=x&b=2&a=1&gclid=abc"),
            "https://example.com/jobs?a=1&b=2"
        );
        assert_eq!(
            canonical("https://example.com/jobs?utm_source=x"),
            "https://example.com/jobs"
        );
    }

    #[test]
    fn a_query_that_identifies_the_posting_survives() {
        assert_eq!(
            canonical("https://example.com/job?jobId=7"),
            "https://example.com/job?jobId=7"
        );
    }

    #[test]
    fn default_ports_are_dropped_and_others_are_kept() {
        assert_eq!(
            canonical("https://example.com:443/jobs"),
            "https://example.com/jobs"
        );
        assert_eq!(
            canonical("https://example.com:8443/jobs"),
            "https://example.com:8443/jobs"
        );
    }

    #[test]
    fn paths_are_case_sensitive() {
        assert_ne!(
            canonical("https://example.com/Jobs/1"),
            canonical("https://example.com/jobs/1")
        );
    }
}
