//! Triage: the structured output contract and its parser.
//!
//! The model returns one JSON object with one decision per posting. Parsing it
//! is schema validation plus the batch rules: the ids must be exactly the ids
//! sent, each once, and each decision must carry a reason. A reply that breaks
//! any of those is refused before anything is written to the queue.

use std::collections::HashSet;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::model::QueueState;

/// What the user should do with a posting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TriageChoice {
    /// Fetch the full description before deciding; the brief is not enough.
    WorthDetails,
    /// The title alone is a strong match; go straight to evaluation.
    Evaluate,
    /// Clearly outside the stated preferences or the candidate's work.
    Skip,
    /// The brief is too thin to judge; default toward seeing it.
    Unsure,
}

impl TriageChoice {
    pub const ALL: [TriageChoice; 4] = [
        TriageChoice::WorthDetails,
        TriageChoice::Evaluate,
        TriageChoice::Skip,
        TriageChoice::Unsure,
    ];

    /// The stored and JSON form. Part of the output contract.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::WorthDetails => "worth_details",
            Self::Evaluate => "evaluate",
            Self::Skip => "skip",
            Self::Unsure => "unsure",
        }
    }

    pub fn parse(raw: &str) -> Option<Self> {
        match raw.trim() {
            "worth_details" => Some(Self::WorthDetails),
            "evaluate" => Some(Self::Evaluate),
            "skip" => Some(Self::Skip),
            "unsure" => Some(Self::Unsure),
            _ => None,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::WorthDetails => "Worth details",
            Self::Evaluate => "Evaluate",
            Self::Skip => "Skip",
            Self::Unsure => "Unsure",
        }
    }

    /// What the decision means for the queue.
    ///
    /// `Unsure` deliberately lands on `evaluate`: a thin brief is not a reason
    /// to hide a posting, so the default is to see it.
    pub fn queue_state(self) -> QueueState {
        match self {
            Self::WorthDetails => QueueState::DetailsNeeded,
            Self::Evaluate | Self::Unsure => QueueState::Evaluate,
            Self::Skip => QueueState::Dismissed,
        }
    }
}

/// One posting's decision.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TriageDecision {
    /// The `job_descriptions.id` the decision is for.
    pub job_id: i64,
    pub decision: TriageChoice,
    /// One sentence the review UI shows. A decision without one is a black box.
    pub reason: String,
}

/// The model's answer, as the prompt's output contract describes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct TriageOutput {
    pub decisions: Vec<TriageDecision>,
}

/// Parse and check a reply against the batch that was sent.
///
/// JSON mode sometimes wraps the object in a fence or leads with a sentence,
/// so the object is located by its braces. Everything between them still has
/// to be one complete, schema-conforming object.
pub fn parse(text: &str, expected_ids: &[i64]) -> Result<TriageOutput, String> {
    let json = extract_json_object(text)
        .ok_or_else(|| "the reply contained no JSON object".to_string())?;
    let output: TriageOutput =
        serde_json::from_str(json).map_err(|error| format!("invalid JSON: {error}"))?;
    validate(&output, expected_ids)?;
    Ok(output)
}

/// The batch rules: exact ids, no duplicates, every reason present.
fn validate(output: &TriageOutput, expected_ids: &[i64]) -> Result<(), String> {
    if output.decisions.is_empty() {
        return Err("the reply contained no decisions".into());
    }

    let expected: HashSet<i64> = expected_ids.iter().copied().collect();
    let mut seen = HashSet::new();

    for decision in &output.decisions {
        if !expected.contains(&decision.job_id) {
            return Err(format!(
                "the reply decided job {}, which was not in the batch",
                decision.job_id
            ));
        }
        if !seen.insert(decision.job_id) {
            return Err(format!(
                "the reply decided job {} more than once",
                decision.job_id
            ));
        }
        if decision.reason.trim().is_empty() {
            return Err(format!(
                "the decision for job {} has no reason",
                decision.job_id
            ));
        }
    }

    if let Some(missing) = expected_ids.iter().find(|id| !seen.contains(id)) {
        return Err(format!("the reply has no decision for job {missing}"));
    }

    Ok(())
}

fn extract_json_object(text: &str) -> Option<&str> {
    let start = text.find('{')?;
    let end = text.rfind('}')?;
    (end > start).then(|| &text[start..=end])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `(choice, job_id, reason)` per decision.
    fn body(decisions: &[(&str, i64, &str)]) -> String {
        let entries: Vec<String> = decisions
            .iter()
            .map(|(choice, id, reason)| {
                format!(r#"{{"job_id": {id}, "decision": "{choice}", "reason": "{reason}"}}"#)
            })
            .collect();
        format!(r#"{{"decisions": [{}]}}"#, entries.join(", "))
    }

    #[test]
    fn a_plain_object_with_a_decision_per_job_parses() {
        let reply = body(&[
            ("evaluate", 1, "Rust and compilers match the summary."),
            ("skip", 2, "Marketing, not engineering."),
        ]);
        let output = parse(&reply, &[1, 2]).expect("parse");

        assert_eq!(output.decisions.len(), 2);
        assert_eq!(output.decisions[0].decision, TriageChoice::Evaluate);
        assert_eq!(output.decisions[1].decision, TriageChoice::Skip);
    }

    #[test]
    fn fences_and_leading_prose_are_tolerated() {
        let reply = format!(
            "Here is the batch:\n```json\n{}\n```",
            body(&[("evaluate", 1, "Fits.")])
        );
        assert!(parse(&reply, &[1]).is_ok());
    }

    #[test]
    fn every_choice_value_parses() {
        for choice in TriageChoice::ALL {
            assert_eq!(TriageChoice::parse(choice.as_str()), Some(choice));
            assert!(!choice.label().is_empty());

            let reply = body(&[(choice.as_str(), 1, "reason")]);
            let output = parse(&reply, &[1]).expect("parse");
            assert_eq!(output.decisions[0].decision, choice);
        }
    }

    #[test]
    fn each_choice_maps_to_its_queue_state() {
        assert_eq!(
            TriageChoice::WorthDetails.queue_state(),
            QueueState::DetailsNeeded
        );
        assert_eq!(TriageChoice::Evaluate.queue_state(), QueueState::Evaluate);
        assert_eq!(
            TriageChoice::Unsure.queue_state(),
            QueueState::Evaluate,
            "unsure defaults toward seeing the posting"
        );
        assert_eq!(TriageChoice::Skip.queue_state(), QueueState::Dismissed);
    }

    #[test]
    fn an_unknown_choice_is_rejected() {
        let reply = body(&[("definitely", 1, "reason")]);
        let error = parse(&reply, &[1]).expect_err("unknown choice");
        assert!(error.contains("invalid JSON"), "{error}");
    }

    #[test]
    fn a_missing_reason_field_is_rejected() {
        let reply = r#"{"decisions": [{"job_id": 1, "decision": "evaluate"}]}"#;
        let error = parse(reply, &[1]).expect_err("missing reason");
        assert!(error.contains("invalid JSON"), "{error}");
    }

    #[test]
    fn an_empty_reason_is_rejected() {
        let reply = body(&[("evaluate", 1, "   ")]);
        let error = parse(&reply, &[1]).expect_err("empty reason");
        assert!(error.contains("no reason"), "{error}");
    }

    #[test]
    fn a_missing_decision_is_rejected() {
        let reply = body(&[("evaluate", 1, "Fits.")]);
        let error = parse(&reply, &[1, 2]).expect_err("missing job 2");
        assert!(error.contains("no decision for job 2"), "{error}");
    }

    #[test]
    fn a_duplicate_decision_is_rejected() {
        let reply = body(&[("evaluate", 1, "Fits."), ("skip", 1, "Does not fit.")]);
        let error = parse(&reply, &[1]).expect_err("duplicate");
        assert!(error.contains("more than once"), "{error}");
    }

    #[test]
    fn a_decision_for_an_unknown_job_is_rejected() {
        let reply = body(&[("evaluate", 9, "Fits.")]);
        let error = parse(&reply, &[1]).expect_err("unknown job");
        assert!(error.contains("not in the batch"), "{error}");
    }

    #[test]
    fn an_empty_batch_reply_is_rejected() {
        let error = parse(r#"{"decisions": []}"#, &[1]).expect_err("empty");
        assert!(error.contains("no decisions"), "{error}");
    }

    #[test]
    fn no_object_is_rejected() {
        let error = parse("I cannot triage these postings.", &[1]).expect_err("no object");
        assert!(error.contains("no JSON object"), "{error}");
    }

    #[test]
    fn the_output_schema_names_the_decision_fields() {
        let schema = schemars::schema_for!(TriageOutput);
        let value = serde_json::to_value(&schema).expect("serialize the schema");

        assert!(
            value["properties"]["decisions"].is_object(),
            "the contract must name `decisions`"
        );

        let definitions = value
            .get("$defs")
            .or_else(|| value.get("definitions"))
            .expect("the schema defines its item type");
        let decision = &definitions["TriageDecision"];
        let mut properties: Vec<&str> = decision["properties"]
            .as_object()
            .expect("TriageDecision has properties")
            .keys()
            .map(String::as_str)
            .collect();
        properties.sort_unstable();
        assert_eq!(properties, vec!["decision", "job_id", "reason"]);
    }
}
