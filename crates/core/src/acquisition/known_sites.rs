//! Embedded known-site registry.
//!
//! A **known site** is a profile, not an engine: a host, the engine that can
//! fetch it, the payload that engine needs, plus the date its live test last
//! passed. The engine is either a config-selected [`Provider`] (Waymo's
//! selectors, NVIDIA's Eightfold domain) or an ATS adapter selected by the board
//! URL (Figma on Greenhouse). Both shapes use one [`SiteEngine`], so onboarding a
//! site follows one checklist whether it reuses an engine or needs one.
//!
//! This is app content, not user data: it ships in the binary so a bad entry is
//! fixed by a release, not a migration. The portal form offers an entry; it
//! never forces it.

use url::Url;

use super::config::Provider;

/// The engine a known site uses.
///
/// `Provider` is selected by `scraper_config.provider` and is what an ATS entry
/// stores. An ATS adapter is selected by the board URL instead; keeping the two
/// in one enum lets the chooser and the attribution line use one label without
/// pretending an ATS board is a selector scrape.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SiteEngine {
    /// Selected by `scraper_config.provider`.
    Provider(Provider),
    /// Selected by the board URL; the string is `AtsAdapter::name()`.
    Ats(&'static str),
}

impl SiteEngine {
    /// The provider the form stores. An ATS entry stores the default engine
    /// with an empty config; the URL decides the adapter at fetch time.
    pub fn provider(self) -> Provider {
        match self {
            Self::Provider(provider) => provider,
            Self::Ats(_) => Provider::Selectors,
        }
    }

    /// The label the chooser and the attribution line show.
    pub fn label(self) -> &'static str {
        match self {
            Self::Provider(provider) => provider.label(),
            Self::Ats("greenhouse") => "Greenhouse",
            Self::Ats("lever") => "Lever",
            Self::Ats("ashby") => "Ashby",
            Self::Ats("smartrecruiters") => "SmartRecruiters",
            // A new adapter still renders correctly before this list grows.
            Self::Ats(other) => other,
        }
    }
}

/// One known careers site.
#[derive(Debug, Clone, Copy)]
pub struct KnownSite {
    pub host: &'static str,
    /// Optional path prefix when one host serves several boards.
    pub path_prefix: Option<&'static str>,
    pub name: &'static str,
    pub engine: SiteEngine,
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
            Some(date) => format!("{} ({}) - verified {date}", self.name, self.engine.label()),
            None => format!("{} ({}) - unverified", self.name, self.engine.label()),
        }
    }

    /// Case-insensitive substring over the name, host, and engine label.
    ///
    /// Notes are deliberately excluded: matching prose makes the results feel
    /// arbitrary. An empty or whitespace-only query matches everything.
    pub fn matches(&self, query: &str) -> bool {
        let query = query.trim();
        if query.is_empty() {
            return true;
        }
        let query = query.to_lowercase();

        self.name.to_lowercase().contains(&query)
            || self.host.to_lowercase().contains(&query)
            || self.engine.label().to_lowercase().contains(&query)
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
  "detail": { "max_jobs": 90 }
}"#;

pub const WAYMO: KnownSite = KnownSite {
    host: "careers.withwaymo.com",
    path_prefix: Some("/jobs/search"),
    name: "Waymo",
    engine: SiteEngine::Provider(Provider::Selectors),
    config: WAYMO_CONFIG,
    verified_at: Some("2026-11-03"),
    notes: "AWS WAF intermittently answers 202 with an empty body; the live test probes and skips.",
};

pub const NVIDIA: KnownSite = KnownSite {
    host: "jobs.nvidia.com",
    path_prefix: Some("/careers"),
    name: "NVIDIA",
    engine: SiteEngine::Provider(Provider::Eightfold),
    config: r#"{"provider":"eightfold","domain":"nvidia.com"}"#,
    verified_at: Some("2026-11-03"),
    notes: "Client-rendered Eightfold site; the list API reports ~2,750 positions.",
};

pub const FIGMA: KnownSite = KnownSite {
    host: "job-boards.greenhouse.io",
    path_prefix: Some("/figma"),
    name: "Figma",
    engine: SiteEngine::Ats("greenhouse"),
    config: r#"{"provider":"selectors"}"#,
    verified_at: Some("2026-11-05"),
    notes: "Greenhouse board; the list carries descriptions. The board page redirects to \
            figma.com/careers; the adapter fetches the Greenhouse API. Other Greenhouse boards \
            work through Other site.",
};

pub const SPOTIFY: KnownSite = KnownSite {
    host: "jobs.lever.co",
    path_prefix: Some("/spotify"),
    name: "Spotify",
    engine: SiteEngine::Ats("lever"),
    config: r#"{"provider":"selectors"}"#,
    verified_at: Some("2026-11-05"),
    notes:
        "Lever board; the list carries descriptions. Other Lever boards work through Other site.",
};

pub const LINEAR: KnownSite = KnownSite {
    host: "jobs.ashbyhq.com",
    path_prefix: Some("/linear"),
    name: "Linear",
    engine: SiteEngine::Ats("ashby"),
    config: r#"{"provider":"selectors"}"#,
    verified_at: Some("2026-11-05"),
    notes:
        "Ashby board; the list carries descriptions. Other Ashby boards work through Other site.",
};

pub const UBISOFT: KnownSite = KnownSite {
    host: "jobs.smartrecruiters.com",
    path_prefix: Some("/Ubisoft2"),
    name: "Ubisoft",
    engine: SiteEngine::Ats("smartrecruiters"),
    config: r#"{"provider":"selectors","detail":{"max_jobs":25}}"#,
    verified_at: Some("2026-11-05"),
    notes: "SmartRecruiters carries no descriptions in its list; the first 25 postings are \
            enriched, one request each.",
};

pub fn all() -> &'static [KnownSite] {
    &[WAYMO, NVIDIA, FIGMA, SPOTIFY, LINEAR, UBISOFT]
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

    use super::super::ats;
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

            assert_eq!(parsed.provider(), site.engine.provider(), "{}", site.name);
            match site.engine.provider() {
                Provider::Selectors => assert!(parsed.selectors().is_some(), "{}", site.name),
                Provider::Eightfold => assert!(parsed.eightfold().is_some(), "{}", site.name),
            }
        }
    }

    #[test]
    fn every_ats_entry_is_detected_by_the_adapter_it_names() {
        let adapters = ats::adapters();

        for site in all() {
            let SiteEngine::Ats(expected) = site.engine else {
                continue;
            };
            let board = url(&site.url());
            let detected: Vec<(&str, String)> = adapters
                .iter()
                .filter_map(|adapter| adapter.detect(&board).map(|slug| (adapter.name(), slug)))
                .collect();

            assert_eq!(
                detected.len(),
                1,
                "{} must be detected by exactly one adapter, got {detected:?}",
                site.name
            );
            assert_eq!(detected[0].0, expected, "{}", site.name);
            assert!(!detected[0].1.is_empty(), "{}", site.name);
        }
    }

    #[test]
    fn an_ats_entry_carries_no_selector_list() {
        for site in all() {
            if !matches!(site.engine, SiteEngine::Ats(_)) {
                continue;
            }
            let ScraperConfig::Selectors(config) = ScraperConfig::parse(site.config)
                .unwrap_or_else(|error| panic!("{} config: {error}", site.name))
            else {
                panic!("{} must store a selectors config", site.name);
            };

            assert!(
                config.list.is_none(),
                "{} must not scrape its board page as HTML",
                site.name
            );
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
        assert_eq!(FIGMA.url(), "https://job-boards.greenhouse.io/figma");
        assert_eq!(LINEAR.url(), "https://jobs.ashbyhq.com/linear");

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
    fn lookup_matches_each_ats_board_and_not_its_neighbours() {
        for (raw, name) in [
            ("https://job-boards.greenhouse.io/figma", "Figma"),
            ("https://job-boards.greenhouse.io/figma/jobs/123", "Figma"),
            ("https://jobs.lever.co/spotify", "Spotify"),
            ("https://jobs.lever.co/spotify/abc", "Spotify"),
            ("https://jobs.ashbyhq.com/linear", "Linear"),
            ("https://jobs.smartrecruiters.com/Ubisoft2", "Ubisoft"),
        ] {
            assert_eq!(lookup(&url(raw)).map(|site| site.name), Some(name), "{raw}");
        }

        // Another board on a known host is not the known board.
        assert!(lookup(&url("https://job-boards.greenhouse.io/other")).is_none());
        assert!(lookup(&url("https://jobs.lever.co/other")).is_none());
        assert!(lookup(&url("https://jobs.smartrecruiters.com/Other1")).is_none());
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
    fn site_engine_labels_are_display_names() {
        assert_eq!(
            SiteEngine::Provider(Provider::Selectors).label(),
            "Selectors"
        );
        assert_eq!(
            SiteEngine::Provider(Provider::Eightfold).label(),
            "Eightfold"
        );
        assert_eq!(SiteEngine::Ats("greenhouse").label(), "Greenhouse");
        assert_eq!(
            SiteEngine::Ats("smartrecruiters").label(),
            "SmartRecruiters"
        );
        assert_eq!(SiteEngine::Ats("future-ats").label(), "future-ats");
    }

    #[test]
    fn an_ats_entry_labels_its_adapter() {
        let figma = lookup(&url("https://job-boards.greenhouse.io/figma")).expect("figma");

        assert!(
            figma.label().starts_with("Figma (Greenhouse) - "),
            "{}",
            figma.label()
        );
        assert_eq!(figma.engine.provider(), Provider::Selectors);
    }

    #[test]
    fn matches_filters_by_name_host_and_engine_only() {
        let figma = lookup(&url("https://job-boards.greenhouse.io/figma")).expect("figma");

        for query in ["figma", "FIGMA", "greenhouse", "job-boards", "  figma  "] {
            assert!(figma.matches(query), "{query:?} should match Figma");
        }
        for query in ["lever", "nvidia", "spotify"] {
            assert!(!figma.matches(query), "{query:?} must not match Figma");
        }

        // An empty query is the unfiltered list.
        assert!(figma.matches(""));
        assert!(figma.matches("   "));

        // Notes are not searched: this word only appears in Figma's notes.
        assert!(!figma.matches("figma.com"));
    }

    #[test]
    fn waymo_is_a_site_profile_backed_by_selectors() {
        let site = lookup(&url("https://careers.withwaymo.com/jobs/search")).expect("waymo");

        assert_eq!(site.engine, SiteEngine::Provider(Provider::Selectors));
        assert_eq!(site.name, "Waymo");
        assert!(site.config.contains("job-search-results-card-col"));
    }

    #[test]
    fn nvidia_is_a_site_profile_backed_by_eightfold() {
        let site = lookup(&url("https://jobs.nvidia.com/careers")).expect("nvidia");

        assert_eq!(site.engine, SiteEngine::Provider(Provider::Eightfold));
        assert_eq!(site.label(), "NVIDIA (Eightfold) - verified 2026-11-03");
    }
}
