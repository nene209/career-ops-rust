//! The suggestion output contract, its validator, and URL derivation.
//!
//! The model answers with companies, engines, and board slugs. A URL is never
//! taken from the reply: [`SuggestEngine::url`] builds it from the engine and
//! the slug, so a hostile or confused reply cannot point the app at an
//! arbitrary host. The validator refuses an unknown engine (serde does that),
//! an unsafe slug, an empty company or rationale, a duplicate board, and a
//! batch over the ceiling.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::acquisition::known_sites::SiteEngine;

/// The most proposals one reply may carry. The prompt asks for 5 to 10; a
/// reply over the ceiling is refused rather than truncated, the same rule as
/// the triage batch.
pub const MAX_SUGGESTIONS: usize = 10;

/// The engines a proposal may name.
///
/// These are the boards whose URL is fully determined by a slug. A selector
/// site (Waymo) needs a hand-written config and is never suggested.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub enum SuggestEngine {
    #[serde(rename = "greenhouse")]
    Greenhouse,
    #[serde(rename = "lever")]
    Lever,
    #[serde(rename = "ashby")]
    Ashby,
    #[serde(rename = "smartrecruiters")]
    SmartRecruiters,
    #[serde(rename = "eightfold")]
    Eightfold,
}

impl SuggestEngine {
    pub const ALL: [SuggestEngine; 5] = [
        SuggestEngine::Greenhouse,
        SuggestEngine::Lever,
        SuggestEngine::Ashby,
        SuggestEngine::SmartRecruiters,
        SuggestEngine::Eightfold,
    ];

    /// The stored and JSON form. Part of the output contract.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Greenhouse => "greenhouse",
            Self::Lever => "lever",
            Self::Ashby => "ashby",
            Self::SmartRecruiters => "smartrecruiters",
            Self::Eightfold => "eightfold",
        }
    }

    /// The display name the review row shows.
    pub fn label(self) -> &'static str {
        match self {
            Self::Greenhouse => "Greenhouse",
            Self::Lever => "Lever",
            Self::Ashby => "Ashby",
            Self::SmartRecruiters => "SmartRecruiters",
            Self::Eightfold => "Eightfold",
        }
    }

    /// Map an embedded ATS adapter name, for the known-site block.
    pub fn from_ats(name: &str) -> Option<Self> {
        match name {
            "greenhouse" => Some(Self::Greenhouse),
            "lever" => Some(Self::Lever),
            "ashby" => Some(Self::Ashby),
            "smartrecruiters" => Some(Self::SmartRecruiters),
            _ => None,
        }
    }

    pub fn from_site_engine(engine: SiteEngine) -> Option<Self> {
        match engine {
            SiteEngine::Ats(name) => Self::from_ats(name),
            // Eightfold's URL is `jobs.{domain}/careers`; its engine label is
            // the same provider name.
            SiteEngine::Provider(crate::acquisition::config::Provider::Eightfold) => {
                Some(Self::Eightfold)
            }
            SiteEngine::Provider(_) => None,
        }
    }

    /// The board URL for `slug`, or `None` when the slug is not safe.
    ///
    /// The only way a reply reaches the network is through this function, so a
    /// slug containing a slash, a space, or a URL scheme is refused here rather
    /// than sanitized.
    pub fn url(self, slug: &str) -> Option<String> {
        if !is_safe_slug(slug) {
            return None;
        }

        Some(match self {
            Self::Greenhouse => format!("https://job-boards.greenhouse.io/{slug}"),
            Self::Lever => format!("https://jobs.lever.co/{slug}"),
            Self::Ashby => format!("https://jobs.ashbyhq.com/{slug}"),
            Self::SmartRecruiters => format!("https://jobs.smartrecruiters.com/{slug}"),
            Self::Eightfold => {
                // An Eightfold board is a company domain, not a short name.
                if !slug.contains('.') {
                    return None;
                }
                format!("https://jobs.{slug}/careers")
            }
        })
    }

    /// The config to store when the derived URL is not a known site.
    ///
    /// An ATS board is selected by its URL, so it needs no config. Eightfold is
    /// selected by `scraper_config`, so the domain has to travel with the row.
    pub fn fallback_config(self, slug: &str) -> Option<String> {
        match self {
            Self::Eightfold => Some(format!(r#"{{"provider":"eightfold","domain":"{slug}"}}"#)),
            _ => None,
        }
    }
}

/// One proposal as the model returned it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SuggestedPortal {
    pub company: String,
    pub engine: SuggestEngine,
    /// The board slug, not a URL and not a display name.
    pub slug: String,
    /// Omitted when the model does not know the count.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub open_roles: Option<u32>,
    /// One sentence, shown on the review row.
    pub rationale: String,
}

/// The model's answer, as the prompt's output contract describes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct SuggestionsOutput {
    pub suggestions: Vec<SuggestedPortal>,
}

/// Parse and check a reply.
///
/// JSON mode sometimes wraps the object in a fence or leads with a sentence,
/// so the object is located by its braces. Everything between them still has
/// to be one complete, schema-conforming object.
pub fn parse(text: &str) -> Result<SuggestionsOutput, String> {
    let json = extract_json_object(text)
        .ok_or_else(|| "the reply contained no JSON object".to_string())?;
    let output: SuggestionsOutput =
        serde_json::from_str(json).map_err(|error| format!("invalid JSON: {error}"))?;
    validate(&output)?;
    Ok(output)
}

fn validate(output: &SuggestionsOutput) -> Result<(), String> {
    if output.suggestions.is_empty() {
        return Err("the reply contained no suggestions".into());
    }
    if output.suggestions.len() > MAX_SUGGESTIONS {
        return Err(format!(
            "the reply has {} suggestions; the limit is {MAX_SUGGESTIONS}",
            output.suggestions.len()
        ));
    }

    let mut seen: Vec<(SuggestEngine, String)> = Vec::new();

    for suggestion in &output.suggestions {
        let company = suggestion.company.trim();
        if company.is_empty() {
            return Err("a suggestion has no company".into());
        }
        if suggestion.rationale.trim().is_empty() {
            return Err(format!("the suggestion for {company} has no rationale"));
        }
        if suggestion.engine.url(&suggestion.slug).is_none() {
            return Err(format!(
                "the suggestion for {company} has an unusable {} slug `{}`",
                suggestion.engine.as_str(),
                suggestion.slug
            ));
        }

        let key = (suggestion.engine, suggestion.slug.clone());
        if seen.contains(&key) {
            return Err(format!(
                "the reply proposes {} `{}` more than once",
                suggestion.engine.as_str(),
                suggestion.slug
            ));
        }
        seen.push(key);
    }

    Ok(())
}

/// One path segment: letters, digits, dots, dashes, underscores.
///
/// Dots are allowed because an Eightfold slug is a domain. `..` is refused so a
/// slug cannot climb out of the path it is placed in.
fn is_safe_slug(slug: &str) -> bool {
    !slug.is_empty()
        && slug.chars().count() <= 63
        && !slug.contains("..")
        && slug
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_'))
}

fn extract_json_object(text: &str) -> Option<&str> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    (end > start).then(|| &text[start..=end])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reply(entries: &[(&str, &str, &str, &str)]) -> String {
        let rows: Vec<String> = entries
            .iter()
            .map(|(company, engine, slug, rationale)| {
                format!(
                    r#"{{"company":"{company}","engine":"{engine}","slug":"{slug}","rationale":"{rationale}"}}"#
                )
            })
            .collect();
        format!(r#"{{"suggestions":[{}]}}"#, rows.join(", "))
    }

    #[test]
    fn a_plain_object_parses_and_derives_known_urls() {
        let output = parse(&reply(&[
            ("Figma", "greenhouse", "figma", "Design tooling matches."),
            ("Spotify", "lever", "spotify", "Audio platform work."),
        ]))
        .expect("parse");

        assert_eq!(output.suggestions.len(), 2);
        assert_eq!(
            output.suggestions[0].engine.url("figma").as_deref(),
            Some("https://job-boards.greenhouse.io/figma")
        );
        assert_eq!(
            output.suggestions[1].engine.url("spotify").as_deref(),
            Some("https://jobs.lever.co/spotify")
        );
    }

    #[test]
    fn every_engine_derives_its_own_host() {
        assert_eq!(
            SuggestEngine::Ashby.url("linear").as_deref(),
            Some("https://jobs.ashbyhq.com/linear")
        );
        assert_eq!(
            SuggestEngine::SmartRecruiters.url("Ubisoft2").as_deref(),
            Some("https://jobs.smartrecruiters.com/Ubisoft2")
        );
        assert_eq!(
            SuggestEngine::Eightfold.url("nvidia.com").as_deref(),
            Some("https://jobs.nvidia.com/careers")
        );
        assert!(
            SuggestEngine::Eightfold.url("nvidia").is_none(),
            "an Eightfold slug must be a domain"
        );
    }

    #[test]
    fn a_slug_cannot_carry_a_path_or_a_scheme() {
        for slug in [
            "acme/jobs",
            "../acme",
            "acme?x=1",
            "acme#frag",
            "https://evil.example/acme",
            "acme jobs",
            "",
            "acme..jobs",
        ] {
            assert!(
                SuggestEngine::Greenhouse.url(slug).is_none(),
                "{slug:?} must be refused"
            );
        }
    }

    #[test]
    fn a_fenced_reply_with_leading_prose_is_tolerated() {
        let text = format!(
            "Here is the list:\n```json\n{}\n```",
            reply(&[("Figma", "greenhouse", "figma", "Fits.")])
        );
        assert!(parse(&text).is_ok());
    }

    #[test]
    fn an_empty_list_is_rejected() {
        let error = parse(r#"{"suggestions":[]}"#).expect_err("empty");
        assert!(error.contains("no suggestions"), "{error}");
    }

    #[test]
    fn more_than_the_ceiling_is_rejected() {
        let rows: Vec<String> = (0..=MAX_SUGGESTIONS)
            .map(|index| {
                format!(
                    r#"{{"company":"Acme {index}","engine":"greenhouse","slug":"acme{index}","rationale":"Fits."}}"#
                )
            })
            .collect();
        let error =
            parse(&format!(r#"{{"suggestions":[{}]}}"#, rows.join(", "))).expect_err("too many");
        assert!(error.contains("limit is"), "{error}");
    }

    #[test]
    fn a_duplicate_board_is_rejected() {
        let error = parse(&reply(&[
            ("Figma", "greenhouse", "figma", "Fits."),
            ("Figma again", "greenhouse", "figma", "Also fits."),
        ]))
        .expect_err("duplicate");
        assert!(error.contains("more than once"), "{error}");
    }

    #[test]
    fn a_missing_rationale_is_rejected() {
        let error = parse(r#"{"suggestions":[{"company":"Acme","engine":"lever","slug":"acme","rationale":"  "}]}"#)
            .expect_err("empty reason");
        assert!(error.contains("no rationale"), "{error}");
    }

    #[test]
    fn an_unknown_engine_is_rejected() {
        let error = parse(&reply(&[("Acme", "workday", "acme", "Fits.")])).expect_err("unknown");
        assert!(error.contains("invalid JSON"), "{error}");
    }

    #[test]
    fn the_output_schema_names_the_suggestion_fields() {
        let schema = schemars::schema_for!(SuggestionsOutput);
        let value = serde_json::to_value(&schema).expect("serialize the schema");

        assert!(value["properties"]["suggestions"].is_object());

        let definitions = value
            .get("$defs")
            .or_else(|| value.get("definitions"))
            .expect("the schema defines its item type");
        let suggestion = &definitions["SuggestedPortal"];
        let mut properties: Vec<&str> = suggestion["properties"]
            .as_object()
            .expect("SuggestedPortal has properties")
            .keys()
            .map(String::as_str)
            .collect();
        properties.sort_unstable();
        assert_eq!(
            properties,
            vec!["company", "engine", "open_roles", "rationale", "slug"]
        );
    }

    #[test]
    fn the_engine_names_match_the_prompt_contract() {
        for engine in SuggestEngine::ALL {
            // The prompt spells these exactly; a mismatch would make the model
            // return a reply the parser refuses.
            assert!(!engine.as_str().is_empty());
            assert!(!engine.label().is_empty());
            let json = serde_json::to_string(&engine).expect("serialize");
            assert_eq!(json, format!("\"{}\"", engine.as_str()));
        }
    }
}
