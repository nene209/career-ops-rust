//! Phase 3 WS7: the curated fixtures, replayed from cassettes.
//!
//! These tests are offline and deterministic. They prove the whole pipeline
//! against three known postings — prompt assembly, validation, storage,
//! status, and the queue — and they keep the output contract from drifting.
//! They do **not** prove the model's judgment: the shipped cassettes are marked
//! `synthetic` until re-recorded, and the live band check is
//! `crates/core/tests/llm_eval_live.rs` (`#[ignore]`).

mod common;

use career_ops_core::cover_letter::{run_cover_letter, CoverLetterRequest};
use career_ops_core::evaluation::{
    parse, run_evaluation, EvaluationOutput, EvaluationRequest, Recommendation,
};
use career_ops_core::model::{ApplicationStatus, QueueState};
use career_ops_core::prompt_render::{UNTRUSTED_BEGIN, UNTRUSTED_END};
use career_ops_core::prompts_seed::seed_or_update_system_prompts;
use career_ops_core::repo::{
    applications, cover_letters, evaluation_reports, job_descriptions as job_repo, prompts,
};

use common::phase3::{self, FIXTURES};

#[tokio::test]
async fn each_fixture_scores_in_its_expected_band() {
    for fixture in FIXTURES {
        let db = common::TestDb::new_raw().await;
        phase3::seed(&db).await;
        let job = phase3::store_job(&db, fixture).await;
        let backend = phase3::CassetteBackend::new(fixture.name);

        let result = run_evaluation(
            &db.pool,
            &EvaluationRequest::new(job.id),
            &backend,
            None,
            None,
        )
        .await
        .unwrap_or_else(|error| panic!("{}: {error}", fixture.name));

        assert!(
            (fixture.band.0..=fixture.band.1).contains(&result.match_score),
            "{} scored {:.2}, expected {:.2}..={:.2}",
            fixture.name,
            result.match_score,
            fixture.band.0,
            fixture.band.1
        );

        let expected = Recommendation::parse(
            backend.cassette().reply["recommendation"]
                .as_str()
                .expect("the cassette has a recommendation"),
        )
        .expect("a valid recommendation");
        assert_eq!(
            result.recommendation, expected,
            "{}: the enum and the score disagree",
            fixture.name
        );

        // The prompt carried the right posting, fenced as data, with no markup.
        let prompt = &backend.prompts()[0];
        assert!(
            prompt.contains(fixture.distinctive),
            "{}: the posting did not reach the prompt",
            fixture.name
        );
        assert!(prompt.contains(UNTRUSTED_BEGIN));
        assert!(prompt.contains(UNTRUSTED_END));
        assert!(!prompt.contains("<p>"), "markup reached the model");
        assert!(result.input_chars > 0);
        assert!(!result.truncated, "the fixtures are small enough to fit");

        // Storage and the lifecycle follow the report.
        let stored = evaluation_reports::get(&db.pool, result.report_id)
            .await
            .expect("get")
            .expect("report");
        assert_eq!(stored.recommendation.as_deref(), Some(expected.as_str()));

        let application = applications::get(&db.pool, result.application_id)
            .await
            .expect("get")
            .expect("row");
        if fixture.expect_not_a_match {
            assert_eq!(application.status, ApplicationStatus::NotAMatch.as_str());
        } else {
            assert_eq!(application.status, ApplicationStatus::Ready.as_str());
        }

        let stored_job = job_repo::get(&db.pool, job.id)
            .await
            .expect("get")
            .expect("row");
        assert_eq!(stored_job.queue_state, QueueState::Evaluated.as_str());

        db.close().await;
    }
}

#[tokio::test]
async fn the_cover_letter_cassette_stays_within_the_prompt_rules() {
    let db = common::TestDb::new_raw().await;
    phase3::seed(&db).await;
    let job = phase3::store_job(&db, phase3::Fixture::by_name("strong")).await;

    let backend = phase3::LetterCassetteBackend::new("cover_letter_strong");
    let result = run_cover_letter(&db.pool, &CoverLetterRequest::new(job.id), &backend)
        .await
        .expect("write the letter");

    // The prompt banned these outright; a letter that contains one is a prompt
    // failure even when the model otherwise wrote well.
    let letter = result.content.to_lowercase();
    for banned in [
        "i am writing to",
        "i am excited",
        "i am pleased to",
        "perfect fit",
        "passionate",
        "team player",
        "hit the ground running",
        "proven track record",
        "unique opportunity",
        "move the needle",
        "data-driven",
        "actionable insights",
        "holistic",
        "championed",
        "orchestrated",
        "stakeholder alignment",
        "strong track record",
    ] {
        assert!(
            !letter.contains(banned),
            "the letter uses a banned phrase: {banned}"
        );
    }
    assert!(!result.content.contains('—'), "the prompt bans em dashes");

    // It is grounded in both sides: the posting's stack and the CV's evidence.
    assert!(result.content.contains("SQLite"));
    assert!(result.content.contains("egui"));

    let stored = cover_letters::get(&db.pool, result.cover_letter_id)
        .await
        .expect("get")
        .expect("letter");
    assert_eq!(stored.content, result.content);
    assert_eq!(stored.model_used, "deepseek-flash");
    assert_eq!(stored.prompt_version, Some(1));

    // The letter saw the fenced posting, not markup.
    let prompt = &backend.prompts()[0];
    assert!(prompt.contains(UNTRUSTED_BEGIN));
    assert!(!prompt.contains("<p>"));

    db.close().await;
}

#[test]
fn the_evaluation_output_schema_is_stable() {
    let schema = schemars::schema_for!(EvaluationOutput);
    let value = serde_json::to_value(&schema).expect("serialize the schema");

    let mut properties: Vec<&str> = value["properties"]
        .as_object()
        .expect("the schema has properties")
        .keys()
        .map(String::as_str)
        .collect();
    properties.sort_unstable();
    assert_eq!(
        properties,
        vec![
            "full_report_markdown",
            "gaps",
            "match_score",
            "rationale",
            "recommendation",
            "strengths",
        ],
        "the output contract changed; update the prompts and the parser together"
    );

    let required: Vec<&str> = value["required"]
        .as_array()
        .expect("every field is required")
        .iter()
        .map(|name| name.as_str().expect("a string"))
        .collect();
    for property in &properties {
        assert!(required.contains(property), "{property} is not required");
    }

    // Every stored enum value parses, and nothing else does.
    for recommendation in ["strong_match", "match", "weak_match", "no_match"] {
        let reply = serde_json::json!({
            "match_score": 0.8,
            "recommendation": recommendation,
            "strengths": ["a"],
            "gaps": ["b"],
            "rationale": "r",
            "full_report_markdown": "x",
        });
        assert!(
            parse(&reply.to_string()).is_ok(),
            "{recommendation} must be accepted"
        );
    }
    let reply = serde_json::json!({
        "match_score": 0.8,
        "recommendation": "definitely",
        "strengths": ["a"],
        "gaps": ["b"],
        "rationale": "r",
        "full_report_markdown": "x",
    });
    assert!(parse(&reply.to_string()).is_err());
}

#[tokio::test]
async fn the_seeded_prompt_names_every_output_field() {
    let db = common::TestDb::new_raw().await;
    seed_or_update_system_prompts(&db.pool).await.expect("seed");
    let prompt = prompts::active_for_use(&db.pool, "evaluate")
        .await
        .expect("resolve")
        .expect("evaluate");

    for field in [
        "match_score",
        "recommendation",
        "strengths",
        "gaps",
        "rationale",
        "full_report_markdown",
    ] {
        assert!(
            prompt.content.contains(field),
            "the schema and the prompt must agree on `{field}`"
        );
    }

    db.close().await;
}

// ----------------------------------------------------------------------
// Helpers
// ----------------------------------------------------------------------
