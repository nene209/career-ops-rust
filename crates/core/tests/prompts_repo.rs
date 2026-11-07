//! Prompt seeding, scoped versioning, resolution, and the compact prompts.
//!
//! The interesting half is the two version sequences: a user copy and a shipped
//! system prompt count independently, and neither resolution nor seeding may
//! confuse them.

mod common;

use career_ops_core::prompts_seed::{self, SeedOutcome};
use career_ops_core::repo::content_hash;
use career_ops_core::repo::prompts;

async fn seed_all(pool: &sqlx::SqlitePool) {
    prompts_seed::seed_or_update_system_prompts(pool)
        .await
        .expect("seed");
}

async fn system_version(pool: &sqlx::SqlitePool, name: &str) -> i64 {
    prompts::active_system(pool, name)
        .await
        .expect("active_system")
        .unwrap_or_else(|| panic!("system prompt `{name}` is missing"))
        .version
}

#[tokio::test]
async fn seeding_creates_the_compact_prompts_and_is_idempotent() {
    let db = common::TestDb::new_raw().await;
    seed_all(&db.pool).await;

    let evaluate = prompts::active_system(&db.pool, "evaluate")
        .await
        .expect("query")
        .expect("evaluate is seeded");
    assert!(evaluate.is_system);
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

    let letter = prompts::active_system(&db.pool, "cover_letter")
        .await
        .expect("query")
        .expect("cover_letter is seeded");
    assert!(letter.is_system);
    assert_eq!(letter.category, "cover_letter");
    assert!(letter.content.contains("{{cv_summary}}"));
    assert!(letter.content.contains("{{job_title}}"));
    assert!(letter.content.contains("{{tone}}"));

    // A second run must not create versions: the release content is identical.
    let outcomes = prompts_seed::seed_or_update_system_prompts(&db.pool)
        .await
        .expect("re-seed");
    for (name, outcome) in &outcomes {
        if *name == "evaluate" || *name == "cover_letter" {
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

    // The user edits their copy: it is version 1 of the user sequence, not 2.
    let user_v1 = prompts::create_version(&db.pool, "evaluate", "evaluate", "my copy", false)
        .await
        .expect("user v1");
    assert_eq!(user_v1.version, 1);
    assert_eq!(user_v1.supersedes_id, None);

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

    let evaluate = prompts::active_system(&db.pool, "evaluate")
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

    let evaluate = prompts::active_system(&db.pool, "evaluate")
        .await
        .expect("query")
        .expect("seeded");
    let letter = prompts::active_system(&db.pool, "cover_letter")
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
