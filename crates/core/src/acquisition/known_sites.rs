//! Embedded known-site registry.
//!
//! A **known site** is a profile, not an engine: a host, a provider engine, and
//! the payload that engine needs (a selector config or an Eightfold domain),
//! plus the date its live test last passed. Both sites here have the same shape,
//! so onboarding a site follows one checklist whether it reuses an engine
//! (Waymo, selectors) or needs one (NVIDIA, eightfold).
//!
//! This is app content, not user data: it ships in the binary so a bad entry is
//! fixed by a release, not a migration. The portal form offers an entry; it
//! never forces it.

use url::Url;

use super::config::Provider;

/// One known careers site.
#[derive(Debug, Clone, Copy)]
pub struct KnownSite {
    pub host: &'static str,
    /// Optional path prefix when one host serves several boards.
    pub path_prefix: Option<&'static str>,
    pub name: &'static str,
    pub provider: Provider,
    /// The full `scraper_config` offered to the user, `provider` included.
    pub config: &'static str,
    /// ISO date of the last passing live run; `None` until one passes.
    pub verified_at: Option<&'static str>,
    pub notes: &'static str,
}

impl KnownSite {
    /// The board URL to prefill: the host plus the path prefix, or its root.
    pub fn url(&self) -> String {
        format!("https://{}{}", self.host, self.path_prefix.unwrap_or("/"))
    }

    fn matches_path(&self, path: &str) -> bool {
        match self.path_prefix {
            None => true,
            Some(prefix) => path == prefix || path.starts_with(&format!("{prefix}/")),
        }
    }

    pub fn label(&self) -> String {
        match self.verified_at {
            Some(date) => format!(
                "{} ({}) - verified {date}",
                self.name,
                self.provider.label()
            ),
            None => format!("{} ({}) - unverified", self.name, self.provider.label()),
        }
    }
}

/// Phase 2.2's verified selector config for Waymo's search page.
pub const WAYMO_CONFIG: &str = r#"{
  "provider": "selectors",
  "list": "article.job-search-results-card-col",
  "fields": {
    "title": "h3.job-search-results-card-title a",
    "location": "li.job-component-location span",
    "department": "li.job-component-department span",
    "employment_type": "li.job-component-employment-type span",
    "description": "p.job-search-results-summary"
  },
  "pagination": {
    "next": "a[rel=next]",
    "url_template": "https://careers.withwaymo.com/jobs/search?page={page}",
    "max_pages": 3
  },
  "detail": { "max_jobs": 2 }
}"#;

pub const WAYMO: KnownSite = KnownSite {
    host: "careers.withwaymo.com",
    path_prefix: Some("/jobs/search"),
    name: "Waymo",
    provider: Provider::Selectors,
    config: WAYMO_CONFIG,
    verified_at: Some("2026-11-03"),
    notes: "AWS WAF intermittently answers 202 with an empty body; the live test probes and skips.",
};

pub const NVIDIA: KnownSite = KnownSite {
    host: "jobs.nvidia.com",
    path_prefix: Some("/careers"),
    name: "NVIDIA",
    provider: Provider::Eightfold,
    config: r#"{"provider":"eightfold","domain":"nvidia.com"}"#,
    verified_at: Some("2026-11-03"),
    notes: "Client-rendered Eightfold site; the list API reports ~2,750 positions.",
};

pub fn all() -> &'static [KnownSite] {
    &[WAYMO, NVIDIA]
}

/// The entry for a URL: host and optional path prefix must match. The longest
/// matching prefix wins, so a host can serve several boards.
pub fn lookup(url: &Url) -> Option<&'static KnownSite> {
    let host = url.host_str()?;
    all()
        .iter()
        .filter(|site| site.host.eq_ignore_ascii_case(host) && site.matches_path(url.path()))
        .max_by_key(|site| site.path_prefix.map_or(0, str::len))
}

#[cfg(test)]
mod tests {
    use chrono::NaiveDate;

    use super::super::config::ScraperConfig;
    use super::*;

    fn url(raw: &str) -> Url {
        Url::parse(raw).expect("valid url")
    }

    #[test]
    fn every_entry_parses_and_matches_its_provider() {
        for site in all() {
            let parsed = ScraperConfig::parse(site.config)
                .unwrap_or_else(|error| panic!("{} config: {error}", site.name));

            assert_eq!(parsed.provider(), site.provider, "{}", site.name);
            match site.provider {
                Provider::Selectors => assert!(parsed.selectors().is_some(), "{}", site.name),
                Provider::Eightfold => assert!(parsed.eightfold().is_some(), "{}", site.name),
            }
        }
    }

    #[test]
    fn a_verified_date_is_an_iso_date() {
        // `None` is legitimate: WS4 leaves a skipped live run unverified, and
        // the label says so. A date that is present must be parseable.
        for site in all() {
            let Some(date) = site.verified_at else {
                continue;
            };
            NaiveDate::parse_from_str(date, "%Y-%m-%d")
                .unwrap_or_else(|error| panic!("{} has an invalid date: {error}", site.name));
        }
    }

    #[test]
    fn an_unverified_entry_says_so() {
        let site = KnownSite {
            verified_at: None,
            ..NVIDIA
        };

        assert_eq!(site.label(), "NVIDIA (Eightfold) - unverified");
    }

    #[test]
    fn an_entry_knows_its_board_url() {
        assert_eq!(WAYMO.url(), "https://careers.withwaymo.com/jobs/search");
        assert_eq!(NVIDIA.url(), "https://jobs.nvidia.com/careers");

        // A host that serves one board at its root.
        let root = KnownSite {
            host: "example.com",
            path_prefix: None,
            ..NVIDIA
        };
        assert_eq!(root.url(), "https://example.com/");
    }

    #[test]
    fn lookup_matches_host_and_prefix() {
        assert_eq!(
            lookup(&url("https://careers.withwaymo.com/jobs/search")).map(|site| site.name),
            Some("Waymo")
        );
        assert_eq!(
            lookup(&url("https://careers.withwaymo.com/jobs/search?page=2")).map(|site| site.name),
            Some("Waymo")
        );
        assert_eq!(
            lookup(&url("https://careers.withwaymo.com/jobs/search/")).map(|site| site.name),
            Some("Waymo")
        );
        assert!(lookup(&url("https://careers.withwaymo.com/other")).is_none());

        assert_eq!(
            lookup(&url("https://jobs.nvidia.com/careers")).map(|site| site.name),
            Some("NVIDIA")
        );
        assert_eq!(
            lookup(&url("https://jobs.nvidia.com/careers/job/1")).map(|site| site.name),
            Some("NVIDIA")
        );
        assert!(lookup(&url("https://jobs.nvidia.com/other")).is_none());
        assert!(lookup(&url("https://example.com/careers")).is_none());
    }

    #[test]
    fn lookup_is_case_insensitive_for_the_host() {
        // `Url` lowercases the host, so this documents the contract even though
        // the parser has already normalised it.
        assert_eq!(
            lookup(&url("https://JOBS.NVIDIA.COM/careers")).map(|site| site.name),
            Some("NVIDIA")
        );
    }

    #[test]
    fn waymo_is_a_site_profile_backed_by_selectors() {
        let site = lookup(&url("https://careers.withwaymo.com/jobs/search")).expect("waymo");

        assert_eq!(site.provider, Provider::Selectors);
        assert_eq!(site.name, "Waymo");
        assert!(site.config.contains("job-search-results-card-col"));
    }

    #[test]
    fn nvidia_is_a_site_profile_backed_by_eightfold() {
        let site = lookup(&url("https://jobs.nvidia.com/careers")).expect("nvidia");

        assert_eq!(site.provider, Provider::Eightfold);
        assert_eq!(site.label(), "NVIDIA (Eightfold) - verified 2026-11-03");
    }
}
