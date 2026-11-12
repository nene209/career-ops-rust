//! Prompt seeding, scoped versioning, resolution, and the compact prompts.
//!
//! The interesting half is the two version sequences: a user copy and a shipped
//! system prompt count independently, and neither resolution nor seeding may
//! confuse them. Phase 3.4 adds the current-version pointer and cleanup, which
//! the second half of this file covers.

mod common;

use career_ops_core::model::FetchTier;
use career_ops_core::prompts_seed::{self, SeedOutcome};
use career_ops_core::repo::applications;
use career_ops_core::repo::content_hash;
use career_ops_core::repo::cover_letters::{self, NewCoverLetter};
use career_ops_core::repo::evaluation_reports::{self, NewEvaluationReport};
use career_ops_core::repo::job_descriptions::{self as job_repo, NewJobDescription};
use career_ops_core::repo::prompts::{self, DeleteOutcome};
use sqlx::sqlite::{SqliteConnectOptions, SqliteJournalMode, SqlitePool, SqlitePoolOptions};
use sqlx::ConnectOptions;

async fn seed_all(pool: &SqlitePool) {
    prompts_seed::seed_or_update_system_prompts(pool)
        .await
        .expect("seed");
}

async fn system_version(pool: &SqlitePool, name: &str) -> i64 {
    prompts::newest_system(pool, name)
        .await
        .expect("newest_system")
        .unwrap_or_else(|| panic!("system prompt `{name}` is missing"))
        .version
}

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

#[tokio::test]
async fn seeding_creates_the_compact_prompts_and_is_idempotent() {
    let db = common::TestDb::new_raw().await;
    seed_all(&db.pool).await;

    let evaluate = prompts::newest_system(&db.pool, "evaluate")
        .await
        .expect("query")
        .expect("evaluate is seeded");
    assert!(evaluate.is_system);
    assert!(evaluate.is_current, "a seeded prompt is current");
    assert_eq!(evaluate.category, "evaluate");
    for variable in [
        "{{cv_summary}}",
        "{{job_description}}",
        "{{company_name}}",
        "{{job_title}}",
    ] {
        assert!(
            evaluate.content.contains(variable),
            "evaluate.md must use {variable}"
        );
    }
    assert!(evaluate.content.contains("\"match_score\""));

    let letter = prompts::newest_system(&db.pool, "cover_letter")
        .await
        .expect("query")
        .expect("cover_letter is seeded");
    assert!(letter.is_system);
    assert_eq!(letter.category, "cover_letter");
    assert!(letter.content.contains("{{cv_summary}}"));
    assert!(letter.content.contains("{{job_title}}"));
    assert!(letter.content.contains("{{tone}}"));

    let triage = prompts::newest_system(&db.pool, "triage")
        .await
        .expect("query")
        .expect("triage is seeded");
    assert!(triage.is_system);
    assert_eq!(triage.category, "triage");
    for variable in ["{{cv_summary}}", "{{preferences}}", "{{jobs}}"] {
        assert!(
            triage.content.contains(variable),
            "triage.md must use {variable}"
        );
    }
    assert!(triage.content.contains("\"decisions\""));
    assert!(triage.content.contains("worth_details"));

    // A second run must not create versions: the release content is identical.
    let outcomes = prompts_seed::seed_or_update_system_prompts(&db.pool)
        .await
        .expect("re-seed");
    for (name, outcome) in &outcomes {
        if *name == "evaluate" || *name == "cover_letter" || *name == "triage" {
            assert!(
                matches!(outcome, SeedOutcome::Unchanged { version: 1 }),
                "{name}: {outcome:?}"
            );
        }
    }

    // WS3 adds a unique index on (name, is_system, version). Prove that the
    // seeded set plus a user copy cannot violate it before the migration lands.
    prompts::create_version(&db.pool, "evaluate", "evaluate", "my copy", false)
        .await
        .expect("user copy");
    let duplicates: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM (
             SELECT name, is_system, version FROM prompts
             GROUP BY name, is_system, version
             HAVING count(*) > 1
         )",
    )
    .fetch_one(&db.pool)
    .await
    .expect("duplicate query");
    assert_eq!(duplicates, 0, "the future unique index is satisfiable");

    db.close().await;
}

#[tokio::test]
async fn a_system_update_does_not_collide_with_a_user_version() {
    let db = common::TestDb::new_raw().await;
    let system_v1 = prompts::insert_system_prompt(
        &db.pool,
        "evaluate",
        "evaluate",
        "system one",
        &content_hash("system one"),
        1,
    )
    .await
    .expect("system v1");
    assert!(system_v1.is_current);

    // The user edits their copy: it is version 1 of the user sequence, not 2.
    let user_v1 = prompts::create_version(&db.pool, "evaluate", "evaluate", "my copy", false)
        .await
        .expect("user v1");
    assert_eq!(user_v1.version, 1);
    assert_eq!(user_v1.supersedes_id, None);
    assert!(user_v1.is_current);

    // The next release seeds system version 2. It must not be stopped by the
    // user's row, and it must not supersede it.
    let system_v2 = prompts::insert_system_prompt(
        &db.pool,
        "evaluate",
        "evaluate",
        "system two",
        &content_hash("system two"),
        2,
    )
    .await
    .expect("system v2");
    assert_eq!(system_v2.version, 2);
    assert_eq!(
        system_v2.supersedes_id,
        Some(system_v1.id),
        "the chain stays inside the system sequence"
    );
    assert_eq!(system_version(&db.pool, "evaluate").await, 2);

    // The user's copy still resolves, because resolution prefers it.
    let resolved = prompts::active_for_use(&db.pool, "evaluate")
        .await
        .expect("resolve")
        .expect("a prompt");
    assert_eq!(resolved.id, user_v1.id);

    // Each thread has exactly one current row.
    let current: Vec<(i64, bool)> = sqlx::query_as(
        "SELECT id, is_system FROM prompts WHERE name = 'evaluate' AND is_current = 1 ORDER BY id",
    )
    .fetch_all(&db.pool)
    .await
    .expect("current rows");
    assert_eq!(current, vec![(user_v1.id, false), (system_v2.id, true)]);

    // Two rows with the same name and version, distinguished by their owner:
    // this is what the future unique index on (name, is_system, version) allows
    // and what scoping the version by owner produces.
    let duplicates: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM (
             SELECT name, is_system, version FROM prompts
             GROUP BY name, is_system, version
             HAVING count(*) > 1
         )",
    )
    .fetch_one(&db.pool)
    .await
    .expect("duplicate query");
    assert_eq!(duplicates, 0);

    db.close().await;
}

#[tokio::test]
async fn user_versions_chain_and_system_versions_stay_independent() {
    let db = common::TestDb::new_raw().await;
    seed_all(&db.pool).await;
    assert_eq!(system_version(&db.pool, "evaluate").await, 1);

    let user_v1 = prompts::create_version(&db.pool, "evaluate", "evaluate", "first", false)
        .await
        .expect("user v1");
    let user_v2 = prompts::create_version(&db.pool, "evaluate", "evaluate", "second", false)
        .await
        .expect("user v2");

    assert_eq!(user_v2.version, 2);
    assert_eq!(user_v2.supersedes_id, Some(user_v1.id));
    assert_eq!(
        system_version(&db.pool, "evaluate").await,
        1,
        "the user sequence does not advance the system prompt"
    );

    let system = prompts::list_versions(&db.pool, "evaluate", true)
        .await
        .expect("list");
    assert_eq!(system.len(), 1);
    let user = prompts::list_versions(&db.pool, "evaluate", false)
        .await
        .expect("list");
    let versions: Vec<i64> = user.iter().map(|prompt| prompt.version).collect();
    assert_eq!(versions, vec![2, 1], "newest first");

    db.close().await;
}

#[tokio::test]
async fn resolution_falls_back_to_the_system_prompt() {
    let db = common::TestDb::new_raw().await;
    seed_all(&db.pool).await;

    let system = prompts::active_for_use(&db.pool, "cover_letter")
        .await
        .expect("resolve")
        .expect("system fallback");
    assert!(system.is_system);

    prompts::create_version(&db.pool, "cover_letter", "cover_letter", "mine", false)
        .await
        .expect("user copy");
    let user = prompts::active_for_use(&db.pool, "cover_letter")
        .await
        .expect("resolve")
        .expect("user copy");
    assert!(!user.is_system);
    assert_eq!(user.content, "mine");

    assert!(prompts::active_for_use(&db.pool, "does-not-exist")
        .await
        .expect("resolve")
        .is_none());

    db.close().await;
}

#[tokio::test]
async fn categories_and_hash_lookups() {
    let db = common::TestDb::new_raw().await;
    seed_all(&db.pool).await;

    let categories = prompts::list_categories(&db.pool)
        .await
        .expect("categories");
    assert!(categories.contains(&"evaluate".to_string()));
    assert!(categories.contains(&"cover_letter".to_string()));
    assert!(categories.contains(&"shared".to_string()));
    let mut sorted = categories.clone();
    sorted.sort();
    assert_eq!(categories, sorted, "categories are ordered");
    let mut deduped = categories.clone();
    deduped.dedup();
    assert_eq!(categories.len(), deduped.len(), "categories are distinct");

    let evaluate = prompts::newest_system(&db.pool, "evaluate")
        .await
        .expect("query")
        .expect("seeded");
    let found = prompts::get_by_hash(&db.pool, &evaluate.content_hash)
        .await
        .expect("by hash")
        .expect("the hash resolves");
    assert_eq!(found.id, evaluate.id);

    assert!(prompts::get_by_hash(&db.pool, "not-a-hash")
        .await
        .expect("by hash")
        .is_none());

    db.close().await;
}

#[tokio::test]
async fn the_compact_prompts_are_single_shot_and_bounded() {
    let db = common::TestDb::new_raw().await;
    seed_all(&db.pool).await;

    let evaluate = prompts::newest_system(&db.pool, "evaluate")
        .await
        .expect("query")
        .expect("seeded");
    let letter = prompts::newest_system(&db.pool, "cover_letter")
        .await
        .expect("query")
        .expect("seeded");

    // Roughly 2k and 1.2k tokens. The ceiling is a correctness constraint: the
    // originals are 93 KB and 16 KB and assume an agent with file access.
    assert!(
        evaluate.content.chars().count() <= 8_000,
        "evaluate.md is {} chars",
        evaluate.content.chars().count()
    );
    assert!(
        letter.content.chars().count() <= 4_800,
        "cover_letter.md is {} chars",
        letter.content.chars().count()
    );

    for prompt in [&evaluate, &letter] {
        let text = &prompt.content;
        for forbidden in [
            "WebSearch",
            "Read `cv.md`",
            "Playwright",
            "node ",
            "reports/",
        ] {
            assert!(
                !text.contains(forbidden),
                "{} still contains `{forbidden}`",
                prompt.name
            );
        }
        assert!(
            text.contains("untrusted"),
            "{} names the trust boundary",
            prompt.name
        );
    }

    assert!(evaluate.content.contains("{{company_name}}"));
    assert!(evaluate.content.contains("{{job_title}}"));
    assert!(letter.content.contains("{{company_name}}"));

    db.close().await;
}

// ----------------------------------------------------------------------
// Current-version pointer
// ----------------------------------------------------------------------

#[tokio::test]
async fn reverting_activates_an_existing_version_without_adding_one() {
    let db = common::TestDb::new_raw().await;
    seed_all(&db.pool).await;
    let v1 = prompts::create_version(&db.pool, "evaluate", "evaluate", "first", false)
        .await
        .expect("v1");
    let v2 = prompts::create_version(&db.pool, "evaluate", "evaluate", "second", false)
        .await
        .expect("v2");
    assert!(v2.is_current);

    let activated = prompts::set_current(&db.pool, v1.id)
        .await
        .expect("activate v1");
    assert!(activated.is_current);
    assert_eq!(activated.content, "first");

    let resolved = prompts::active_for_use(&db.pool, "evaluate")
        .await
        .expect("resolve")
        .expect("found");
    assert_eq!(resolved.id, v1.id);
    assert_eq!(resolved.version, 1);
    assert_eq!(resolved.content, "first");

    let versions = prompts::list_versions(&db.pool, "evaluate", false)
        .await
        .expect("versions");
    assert_eq!(versions.len(), 2, "activation does not add a version");
    assert_eq!(
        versions.iter().filter(|version| version.is_current).count(),
        1,
        "the pointer is unique"
    );
    assert_eq!(
        versions
            .iter()
            .find(|version| version.is_current)
            .map(|version| version.id),
        Some(v1.id)
    );

    db.close().await;
}

#[tokio::test]
async fn creating_a_version_makes_it_current() {
    let db = common::TestDb::new_raw().await;
    seed_all(&db.pool).await;

    let v1 = prompts::create_version(&db.pool, "evaluate", "evaluate", "first", false)
        .await
        .expect("v1");
    let v2 = prompts::create_version(&db.pool, "evaluate", "evaluate", "second", false)
        .await
        .expect("v2");

    assert_eq!(v2.version, 2);
    assert!(v2.is_current);
    assert!(
        !prompts::list_versions(&db.pool, "evaluate", false)
            .await
            .expect("versions")
            .iter()
            .find(|version| version.id == v1.id)
            .expect("v1")
            .is_current
    );

    let current = prompts::current(&db.pool, "evaluate", false)
        .await
        .expect("query")
        .expect("a current row");
    assert_eq!(current.id, v2.id);

    db.close().await;
}

#[tokio::test]
async fn deleting_a_middle_version_splices_the_chain() {
    let db = common::TestDb::new_raw().await;
    seed_all(&db.pool).await;
    let v1 = prompts::create_version(&db.pool, "evaluate", "evaluate", "first", false)
        .await
        .expect("v1");
    let v2 = prompts::create_version(&db.pool, "evaluate", "evaluate", "second", false)
        .await
        .expect("v2");
    let v3 = prompts::create_version(&db.pool, "evaluate", "evaluate", "third", false)
        .await
        .expect("v3");
    assert_eq!(v3.supersedes_id, Some(v2.id));

    let outcome = prompts::delete_version(&db.pool, v2.id)
        .await
        .expect("delete v2");
    assert_eq!(outcome, DeleteOutcome::Deleted { promoted: None });

    let versions = prompts::list_versions(&db.pool, "evaluate", false)
        .await
        .expect("versions");
    assert!(versions.iter().all(|version| version.id != v2.id));
    let v3_after = versions
        .iter()
        .find(|version| version.id == v3.id)
        .expect("v3 survives");
    assert_eq!(
        v3_after.supersedes_id,
        Some(v1.id),
        "the child adopts the deleted row's parent"
    );
    assert_eq!(
        prompts::current(&db.pool, "evaluate", false)
            .await
            .expect("query")
            .expect("current")
            .id,
        v3.id,
        "deleting a non-current version leaves the pointer alone"
    );

    db.close().await;
}

#[tokio::test]
async fn deleting_the_current_version_promotes_the_newest_remaining() {
    let db = common::TestDb::new_raw().await;
    seed_all(&db.pool).await;
    let v1 = prompts::create_version(&db.pool, "evaluate", "evaluate", "first", false)
        .await
        .expect("v1");
    let v2 = prompts::create_version(&db.pool, "evaluate", "evaluate", "second", false)
        .await
        .expect("v2");

    let outcome = prompts::delete_version(&db.pool, v2.id)
        .await
        .expect("delete v2");
    assert_eq!(
        outcome,
        DeleteOutcome::Deleted {
            promoted: Some(v1.id)
        }
    );

    let current = prompts::current(&db.pool, "evaluate", false)
        .await
        .expect("query")
        .expect("a promoted current row");
    assert_eq!(current.id, v1.id);
    assert!(current.is_current);

    db.close().await;
}

#[tokio::test]
async fn deleting_the_current_version_when_only_retired_remain_is_refused() {
    let db = common::TestDb::new_raw().await;
    seed_all(&db.pool).await;
    let v1 = prompts::create_version(&db.pool, "evaluate", "evaluate", "first", false)
        .await
        .expect("v1");
    let v2 = prompts::create_version(&db.pool, "evaluate", "evaluate", "second", false)
        .await
        .expect("v2");
    prompts::retire_version(&db.pool, v1.id)
        .await
        .expect("retire the non-current version");

    let error = prompts::delete_version(&db.pool, v2.id)
        .await
        .expect_err("deleting the last active version would orphan the thread");
    assert!(
        error.to_string().contains("Restore another version"),
        "{error}"
    );
    assert_eq!(
        prompts::current(&db.pool, "evaluate", false)
            .await
            .expect("query")
            .expect("still current")
            .id,
        v2.id
    );

    db.close().await;
}

#[tokio::test]
async fn deleting_the_only_user_version_falls_back_to_system() {
    let db = common::TestDb::new_raw().await;
    seed_all(&db.pool).await;
    let user = prompts::create_version(&db.pool, "evaluate", "evaluate", "mine", false)
        .await
        .expect("user copy");

    let outcome = prompts::delete_version(&db.pool, user.id)
        .await
        .expect("delete");
    assert_eq!(outcome, DeleteOutcome::Deleted { promoted: None });

    let resolved = prompts::active_for_use(&db.pool, "evaluate")
        .await
        .expect("resolve")
        .expect("system fallback");
    assert!(resolved.is_system);

    let threads = prompts::list_threads(&db.pool).await.expect("threads");
    assert!(
        !threads
            .iter()
            .any(|thread| thread.name == "evaluate" && !thread.is_system),
        "the empty user thread disappears"
    );

    db.close().await;
}

#[tokio::test]
async fn a_referenced_version_is_refused_and_survives() {
    let db = common::TestDb::new_raw().await;
    seed_all(&db.pool).await;
    let user = prompts::create_version(&db.pool, "evaluate", "evaluate", "mine", false)
        .await
        .expect("user copy");
    let job = store_job(&db).await;

    sqlx::query(
        "UPDATE job_descriptions SET triage_prompt_hash = ?, triaged_at = ?, triage_model = ? WHERE id = ?",
    )
    .bind(&user.content_hash)
    .bind(chrono::Utc::now())
    .bind("deepseek-flash")
    .bind(job.id)
    .execute(&db.pool)
    .await
    .expect("record triage");

    let outcome = prompts::delete_version(&db.pool, user.id)
        .await
        .expect("delete returns an outcome");
    match outcome {
        DeleteOutcome::InUse(refs) => {
            assert_eq!(refs.triage, 1);
            assert_eq!(refs.total(), 1);
        }
        other => panic!("expected a refusal, got {other:?}"),
    }

    assert!(
        prompts::get_by_hash(&db.pool, &user.content_hash)
            .await
            .expect("by hash")
            .is_some(),
        "a referenced version cannot disappear"
    );

    db.close().await;
}

#[tokio::test]
async fn references_count_reports_letters_and_triage() {
    let db = common::TestDb::new_raw().await;
    seed_all(&db.pool).await;
    let prompt = prompts::active_for_use(&db.pool, "evaluate")
        .await
        .expect("resolve")
        .expect("evaluate");
    let job = store_job(&db).await;
    let application = applications::ensure(&db.pool, job.id)
        .await
        .expect("ensure");

    let mut tx = db.pool.begin().await.expect("begin");
    evaluation_reports::insert_tx(
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

    cover_letters::insert(
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

    sqlx::query(
        "UPDATE job_descriptions SET triage_prompt_hash = ?, triaged_at = ?, triage_model = ? WHERE id = ?",
    )
    .bind(&prompt.content_hash)
    .bind(chrono::Utc::now())
    .bind("deepseek-flash")
    .bind(job.id)
    .execute(&db.pool)
    .await
    .expect("record triage");

    let refs = prompts::references(&db.pool, &prompt.content_hash)
        .await
        .expect("references");
    assert_eq!(refs.reports, 1);
    assert_eq!(refs.letters, 1);
    assert_eq!(refs.triage, 1);
    assert_eq!(refs.total(), 3);

    let usage = prompts::usage(&db.pool, &prompt.content_hash)
        .await
        .expect("usage");
    assert_eq!(usage.len(), 3);
    assert!(
        usage
            .iter()
            .any(|entry| entry.kind == prompts::PromptUsageKind::Triage),
        "triage is a usage entry, not only a delete guard"
    );

    db.close().await;
}

#[tokio::test]
async fn retiring_a_version_hides_it_but_the_hash_still_resolves() {
    let db = common::TestDb::new_raw().await;
    seed_all(&db.pool).await;
    let v1 = prompts::create_version(&db.pool, "evaluate", "evaluate", "first", false)
        .await
        .expect("v1");
    let v2 = prompts::create_version(&db.pool, "evaluate", "evaluate", "second", false)
        .await
        .expect("v2");

    prompts::retire_version(&db.pool, v1.id)
        .await
        .expect("retire v1");

    let versions = prompts::list_versions(&db.pool, "evaluate", false)
        .await
        .expect("versions");
    assert_eq!(versions.len(), 2, "retiring keeps the row");
    assert!(versions
        .iter()
        .find(|version| version.id == v1.id)
        .expect("v1")
        .retired_at
        .is_some());
    assert_eq!(
        prompts::current(&db.pool, "evaluate", false)
            .await
            .expect("query")
            .expect("current")
            .id,
        v2.id
    );
    assert_eq!(
        prompts::get_by_hash(&db.pool, &v1.content_hash)
            .await
            .expect("by hash")
            .expect("a retired row still resolves")
            .id,
        v1.id
    );

    prompts::restore_version(&db.pool, v1.id)
        .await
        .expect("restore");
    let restored = prompts::list_versions(&db.pool, "evaluate", false)
        .await
        .expect("versions")
        .into_iter()
        .find(|version| version.id == v1.id)
        .expect("v1");
    assert!(restored.retired_at.is_none());

    db.close().await;
}

#[tokio::test]
async fn the_last_active_version_cannot_be_retired() {
    let db = common::TestDb::new_raw().await;
    seed_all(&db.pool).await;
    let only = prompts::create_version(&db.pool, "evaluate", "evaluate", "mine", false)
        .await
        .expect("user copy");

    let error = prompts::retire_version(&db.pool, only.id)
        .await
        .expect_err("the only active version cannot be retired");
    assert!(error.to_string().contains("last active"), "{error}");

    assert_eq!(
        prompts::current(&db.pool, "evaluate", false)
            .await
            .expect("query")
            .expect("still current")
            .id,
        only.id
    );

    db.close().await;
}

#[tokio::test]
async fn system_rows_are_read_only() {
    let db = common::TestDb::new_raw().await;
    seed_all(&db.pool).await;
    let system = prompts::newest_system(&db.pool, "evaluate")
        .await
        .expect("query")
        .expect("seeded");

    assert!(prompts::delete_version(&db.pool, system.id).await.is_err());
    assert!(prompts::retire_version(&db.pool, system.id).await.is_err());
    assert!(prompts::restore_version(&db.pool, system.id).await.is_err());
    assert!(prompts::set_current(&db.pool, system.id).await.is_err());

    db.close().await;
}

#[tokio::test]
async fn set_current_refuses_a_retired_version() {
    let db = common::TestDb::new_raw().await;
    seed_all(&db.pool).await;
    let v1 = prompts::create_version(&db.pool, "evaluate", "evaluate", "first", false)
        .await
        .expect("v1");
    prompts::create_version(&db.pool, "evaluate", "evaluate", "second", false)
        .await
        .expect("v2");
    prompts::retire_version(&db.pool, v1.id)
        .await
        .expect("retire v1");

    let error = prompts::set_current(&db.pool, v1.id)
        .await
        .expect_err("a retired version must be restored first");
    assert!(error.to_string().contains("Restore"), "{error}");

    db.close().await;
}

#[tokio::test]
async fn a_sibling_with_the_same_hash_keeps_a_referenced_row_deletable() {
    let db = common::TestDb::new_raw().await;
    seed_all(&db.pool).await;
    let v1 = prompts::create_version(&db.pool, "evaluate", "evaluate", "same text", false)
        .await
        .expect("v1");
    let v2 = prompts::create_version(&db.pool, "evaluate", "evaluate", "same text", false)
        .await
        .expect("v2");
    assert_eq!(v1.content_hash, v2.content_hash);
    let job = store_job(&db).await;
    sqlx::query(
        "UPDATE job_descriptions SET triage_prompt_hash = ?, triaged_at = ?, triage_model = ? WHERE id = ?",
    )
    .bind(&v1.content_hash)
    .bind(chrono::Utc::now())
    .bind("deepseek-flash")
    .bind(job.id)
    .execute(&db.pool)
    .await
    .expect("record triage");

    let outcome = prompts::delete_version(&db.pool, v1.id)
        .await
        .expect("delete v1");
    assert!(
        matches!(outcome, DeleteOutcome::Deleted { .. }),
        "the sibling still resolves the hash"
    );
    assert_eq!(
        prompts::get_by_hash(&db.pool, &v1.content_hash)
            .await
            .expect("by hash")
            .expect("the sibling resolves")
            .id,
        v2.id
    );

    db.close().await;
}

// ----------------------------------------------------------------------
// Migration 0010
// ----------------------------------------------------------------------

const MIGRATIONS_BEFORE_0010: [&str; 9] = [
    include_str!("../migrations/0001_initial.sql"),
    include_str!("../migrations/0002_portal_fetch_status.sql"),
    include_str!("../migrations/0003_job_identity.sql"),
    include_str!("../migrations/0004_portal_cursor.sql"),
    include_str!("../migrations/0005_job_queue.sql"),
    include_str!("../migrations/0006_evaluation_provenance.sql"),
    include_str!("../migrations/0007_llm_usage.sql"),
    include_str!("../migrations/0008_triage_provenance.sql"),
    include_str!("../migrations/0009_job_fetched_index.sql"),
];

const MIGRATION_0010: &str = include_str!("../migrations/0010_prompt_current_version.sql");

async fn pool_at_0009() -> (tempfile::TempDir, SqlitePool) {
    let dir = tempfile::tempdir().expect("tempdir");
    let options = SqliteConnectOptions::new()
        .filename(dir.path().join("prompts-migration-test.db"))
        .create_if_missing(true)
        .journal_mode(SqliteJournalMode::Wal)
        .pragma("key", format!("\"x'{}'\"", "cd".repeat(32)))
        .pragma("foreign_keys", "ON")
        .disable_statement_logging();

    let pool = SqlitePoolOptions::new()
        .max_connections(1)
        .connect_with(options)
        .await
        .expect("open");

    for sql in MIGRATIONS_BEFORE_0010 {
        sqlx::raw_sql(sql).execute(&pool).await.expect("migrate");
    }

    (dir, pool)
}

#[tokio::test]
async fn migration_0010_marks_one_current_per_thread_and_preserves_rows() {
    let (_dir, pool) = pool_at_0009().await;

    for (id, is_system, version) in [(1_i64, 1_i64, 1_i64), (2, 1, 2), (3, 0, 1)] {
        sqlx::query(
            "INSERT INTO prompts
             (id, name, category, content, is_system, version, content_hash,
              supersedes_id, created_at, updated_at)
             VALUES (?, 'evaluate', 'evaluate', ?, ?, ?, ?, NULL,
                     '2026-01-01T00:00:00Z', '2026-01-01T00:00:00Z')",
        )
        .bind(id)
        .bind(format!("content {id}"))
        .bind(is_system)
        .bind(version)
        .bind(format!("hash {id}"))
        .execute(&pool)
        .await
        .expect("insert prompt");
    }

    sqlx::raw_sql(MIGRATION_0010)
        .execute(&pool)
        .await
        .expect("apply 0010");

    let current: Vec<(i64,)> =
        sqlx::query_as("SELECT id FROM prompts WHERE is_current = 1 ORDER BY id")
            .fetch_all(&pool)
            .await
            .expect("current rows");
    assert_eq!(
        current,
        vec![(2,), (3,)],
        "each thread's newest version is current"
    );

    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM prompts")
        .fetch_one(&pool)
        .await
        .expect("count");
    assert_eq!(total, 3, "0010 must not remove rows");

    let index: Option<(String,)> = sqlx::query_as(
        "SELECT name FROM sqlite_master
         WHERE type = 'index' AND name = 'ux_prompts_current_per_thread'",
    )
    .fetch_optional(&pool)
    .await
    .expect("read the schema");
    assert!(index.is_some(), "0010 must create the pointer index");

    // The partial unique index refuses a second current row in one thread.
    let duplicate = sqlx::query("UPDATE prompts SET is_current = 1 WHERE id = 1")
        .execute(&pool)
        .await;
    assert!(
        duplicate.is_err(),
        "a thread cannot have two current versions"
    );

    pool.close().await;
}
