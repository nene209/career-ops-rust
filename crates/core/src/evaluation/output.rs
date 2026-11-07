//! Evaluation: the structured output contract and its parser.
//!
//! The model returns one JSON object. Parsing it *is* schema validation: the
//! enum, the required fields, and the score range are checked here, before
//! anything is written. A malformed answer is never stored as a report.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// What the evaluation recommends, derived from the global 1–5 score.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Recommendation {
    StrongMatch,
    Match,
    WeakMatch,
    NoMatch,
}

impl Recommendation {
    /// The stored and JSON form. Part of the output contract.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::StrongMatch => "strong_match",
            Self::Match => "match",
            Self::WeakMatch => "weak_match",
            Self::NoMatch => "no_match",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim() {
            "strong_match" => Some(Self::StrongMatch),
            "match" => Some(Self::Match),
            "weak_match" => Some(Self::WeakMatch),
            "no_match" => Some(Self::NoMatch),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::StrongMatch => "Strong match",
            Self::Match => "Match",
            Self::WeakMatch => "Weak match",
            Self::NoMatch => "No match",
        }
    }

    /// The band a `0.0..=1.0` score falls in.
    ///
    /// The prompt derives both from the same 1–5 bands, so they agree; this is
    /// for code that only has the score (a badge, a filter) and for tests that
    /// pin the mapping at the boundaries.
    pub fn from_score(score: f32) -> Self {
        if score >= 0.9 {
            Self::StrongMatch
        } else if score >= 0.8 {
            Self::Match
        } else if score >= 0.7 {
            Self::WeakMatch
        } else {
            Self::NoMatch
        }
    }
}

/// The model's answer, as the prompt's output contract describes it.
///
/// `match_score` is the global 1–5 score divided by five: 4.5 → 0.9.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct EvaluationOutput {
    pub match_score: f32,
    pub recommendation: Recommendation,
    pub strengths: Vec<String>,
    pub gaps: Vec<String>,
    pub rationale: String,
    pub full_report_markdown: String,
}

/// Parse and validate a reply.
///
/// JSON mode sometimes wraps the object in a fence or leads with a sentence,
/// so the object is located by its braces. That is not "parsing free prose":
/// everything between them still has to be one complete, schema-conforming
/// object, or this returns the reason it is not.
pub fn parse(text: &str) -> Result<EvaluationOutput, String> {
    let json = extract_json_object(text)
        .ok_or_else(|| "the reply contained no JSON object".to_string())?;
    let output: EvaluationOutput =
        serde_json::from_str(json).map_err(|error| format!("invalid JSON: {error}"))?;

    if !output.match_score.is_finite() || !(0.0..=1.0).contains(&output.match_score) {
        return Err(format!(
            "match_score {} is outside 0.0..=1.0",
            output.match_score
        ));
    }
    if output.full_report_markdown.trim().is_empty() {
        return Err("full_report_markdown is empty".into());
    }
    if output.strengths.len() > 8 || output.gaps.len() > 8 {
        return Err("more than 8 strengths or gaps; the contract asks for 3 to 5".into());
    }

    Ok(output)
}

fn extract_json_object(text: &str) -> Option<&str> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    (end > start).then(|| &text[start..=end])
}

#[cfg(test)]
mod tests {
    use super::*;

    fn body(score: f32, recommendation: &str) -> String {
        format!(
            r###"{{
                "match_score": {score},
                "recommendation": "{recommendation}",
                "strengths": ["a", "b", "c"],
                "gaps": ["x"],
                "rationale": "because",
                "full_report_markdown": "## Summary\nok"
            }}"###
        )
    }

    #[test]
    fn a_plain_object_parses() {
        let output = parse(&body(0.9, "strong_match")).expect("parse");
        assert_eq!(output.recommendation, Recommendation::StrongMatch);
        assert_eq!(output.strengths.len(), 3);
    }

    #[test]
    fn fences_and_leading_prose_are_tolerated() {
        let wrapped = format!("Here is the report:\n```json\n{}\n```", body(0.82, "match"));
        let output = parse(&wrapped).expect("parse");
        assert_eq!(output.recommendation, Recommendation::Match);
    }

    #[test]
    fn an_out_of_range_score_is_rejected() {
        let error = parse(&body(1.4, "strong_match")).expect_err("out of range");
        assert!(error.contains("outside 0.0..=1.0"), "{error}");

        let error = parse(&body(-0.1, "no_match")).expect_err("out of range");
        assert!(error.contains("outside"), "{error}");
    }

    #[test]
    fn an_unknown_recommendation_is_rejected() {
        let error = parse(&body(0.9, "definitely")).expect_err("bad enum");
        assert!(error.contains("invalid JSON"), "{error}");
    }

    #[test]
    fn a_missing_field_is_rejected() {
        let error = parse(r#"{"match_score": 0.9, "recommendation": "match"}"#)
            .expect_err("missing fields");
        assert!(error.contains("invalid JSON"), "{error}");
    }

    #[test]
    fn no_object_is_rejected() {
        let error = parse("I cannot evaluate this posting.").expect_err("no object");
        assert!(error.contains("no JSON object"), "{error}");
    }

    #[test]
    fn an_empty_report_is_rejected() {
        let text = r#"{
            "match_score": 0.5,
            "recommendation": "weak_match",
            "strengths": [],
            "gaps": [],
            "rationale": "x",
            "full_report_markdown": "   "
        }"#;
        let error = parse(text).expect_err("empty report");
        assert!(error.contains("full_report_markdown"), "{error}");
    }

    #[test]
    fn recommendations_round_trip_and_map_from_scores() {
        for recommendation in [
            Recommendation::StrongMatch,
            Recommendation::Match,
            Recommendation::WeakMatch,
            Recommendation::NoMatch,
        ] {
            assert_eq!(
                Recommendation::parse(recommendation.as_str()),
                Some(recommendation)
            );
            assert!(!recommendation.label().is_empty());
        }

        assert_eq!(Recommendation::from_score(0.9), Recommendation::StrongMatch);
        assert_eq!(Recommendation::from_score(0.89), Recommendation::Match);
        assert_eq!(Recommendation::from_score(0.8), Recommendation::Match);
        assert_eq!(Recommendation::from_score(0.79), Recommendation::WeakMatch);
        assert_eq!(Recommendation::from_score(0.7), Recommendation::WeakMatch);
        assert_eq!(Recommendation::from_score(0.69), Recommendation::NoMatch);
        assert_eq!(Recommendation::from_score(0.0), Recommendation::NoMatch);
    }
}
