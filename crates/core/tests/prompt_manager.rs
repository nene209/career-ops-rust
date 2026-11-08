//! Prompt Manager read paths (Phase 3, WS5).
//!
//! The list groups a user copy and the system prompt into separate threads, and
//! the usage query answers "which reports recorded this version" from the
//! content hash. Both are offline, against a migrated database.

mod common;

use career_ops_core::model::FetchTier;
use career_ops_core::prompts_seed::seed_or_update_system_prompts;
use career_ops_core::repo::applications;
use career_ops_core::repo::cover_letters::{self, NewCoverLetter};
use career_ops_core::repo::evaluation_reports::{self, NewEvaluationReport};
use career_ops_core::repo::job_descriptions::{self as job_repo, NewJobDescription};
use career_ops_core::repo::prompts::{self, PromptUsageKind};

#[tokio::test]
async fn threads_group_owners_and_count_versions() {
    let db = common::TestDb::new_raw().await;
    seed_or_update_system_prompts(&db.pool).await.expect("seed");

    let threads = prompts::list_threads(&db.pool).await.expect("threads");
    let evaluate = threads
        .iter()
        .find(|thread| thread.name == "evaluate")
        .expect("evaluate is seeded");
    assert!(evaluate.is_system);
    assert_eq!(evaluate.latest_version, 1);
    assert_eq!(evaluate.version_count, 1);

    // A user copy starts its own sequence.
    prompts::create_version(&db.pool, "evaluate", "evaluate", "user v1", false)
        .await
        .expect("user v1");
    prompts::create_version(&db.pool, "evaluate", "evaluate", "user v2", false)
        .await
        .expect("user v2");

    let threads = prompts::list_threads(&db.pool).await.expect("threads");
    let system = threads
        .iter()
        .find(|thread| thread.name == "evaluate" && thread.is_system)
        .expect("system thread");
    let user = threads
        .iter()
        .find(|thread| thread.name == "evaluate" && !thread.is_system)
        .expect("user thread");

    assert_eq!(system.latest_version, 1);
    assert_eq!(system.version_count, 1, "the user's writes do not touch it");
    assert_eq!(user.latest_version, 2);
    assert_eq!(user.version_count, 2);

    // System before user, so the two owners read together.
    let system_index = threads.iter().position(|t| std::ptr::eq(t, system));
    let user_index = threads.iter().position(|t| std::ptr::eq(t, user));
    assert!(system_index < user_index);

    db.close().await;
}

#[tokio::test]
async fn usage_lists_the_reports_and_letters_that_recorded_the_hash() {
    let db = common::TestDb::new_raw().await;
    seed_or_update_system_prompts(&db.pool).await.expect("seed");
    let prompt = prompts::active_for_use(&db.pool, "evaluate")
        .await
        .expect("resolve")
        .expect("evaluate");

    let job = store_job(&db).await;
    let application = applications::ensure(&db.pool, job.id)
        .await
        .expect("ensure");

    let mut tx = db.pool.begin().await.expect("begin");
    let report_id = evaluation_reports::insert_tx(
        &mut tx,
        NewEvaluationReport {
            application_id: application.id,
            content: "report",
            model_used: "deepseek-chat",
            prompt_version: Some(prompt.version),
            prompt_hash: Some(prompt.content_hash.as_str()),
            match_score: Some(0.8),
            recommendation: Some("match"),
            input_chars: None,
            source_chars: None,
            truncated: false,
        },
    )
    .await
    .expect("insert report");
    tx.commit().await.expect("commit");

    let letter_id = cover_letters::insert(
        &db.pool,
        NewCoverLetter {
            application_id: application.id,
            content: "letter",
            model_used: "deepseek-chat",
            prompt_version: Some(prompt.version),
            prompt_hash: Some(prompt.content_hash.as_str()),
            input_chars: None,
        },
    )
    .await
    .expect("insert letter");

    let usage = prompts::usage(&db.pool, &prompt.content_hash)
        .await
        .expect("usage");
    assert_eq!(usage.len(), 2, "both artifacts record the same hash");

    let report = usage
        .iter()
        .find(|entry| entry.kind == PromptUsageKind::Evaluation)
        .expect("report usage");
    assert_eq!(report.id, report_id);
    assert_eq!(report.job_title.as_deref(), Some("Engineer"));
    assert_eq!(report.company.as_deref(), Some("Acme"));
    assert_eq!(report.model_used, "deepseek-chat");

    let letter = usage
        .iter()
        .find(|entry| entry.kind == PromptUsageKind::CoverLetter)
        .expect("letter usage");
    assert_eq!(letter.id, letter_id);

    // A version that was never sent has no usage.
    assert!(prompts::usage(&db.pool, "not-a-real-hash")
        .await
        .expect("usage")
        .is_empty());

    db.close().await;
}

#[tokio::test]
async fn a_thread_that_only_exists_as_a_user_copy_still_lists() {
    let db = common::TestDb::new_raw().await;
    seed_or_update_system_prompts(&db.pool).await.expect("seed");

    // A user prompt with no system counterpart (a future UI may allow this).
    prompts::create_version(&db.pool, "custom", "evaluate", "{{cv_summary}}", false)
        .await
        .expect("create");

    let threads = prompts::list_threads(&db.pool).await.expect("threads");
    let custom = threads
        .iter()
        .find(|thread| thread.name == "custom")
        .expect("custom thread");
    assert!(!custom.is_system);
    assert_eq!(custom.version_count, 1);

    // Its own version is the one in use; no system fallback is involved.
    let active = prompts::active_for_use(&db.pool, "custom")
        .await
        .expect("resolve")
        .expect("found");
    assert!(!active.is_system);

    db.close().await;
}

// ----------------------------------------------------------------------
// Helpers
// ----------------------------------------------------------------------

async fn store_job(db: &common::TestDb) -> career_ops_core::model::JobDescription {
    job_repo::upsert(
        &db.pool,
        NewJobDescription {
            title: "Engineer".into(),
            company: "Acme".into(),
            source_url: "https://example.com/jobs/1".into(),
            raw_text: "body".into(),
            fetch_tier: FetchTier::Tier1bHtml,
            external_id: None,
        },
    )
    .await
    .expect("store job")
}
