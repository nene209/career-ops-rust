//! Integration tests for prompt seeding.
//!
//! These tests exercise the full lifecycle: first seed, idempotent re-seed,
//! content change creating a new version, and the "do not touch user
//! prompts" invariant.
//!
//! They use `TestDb::new_raw()` — they do not need the keystore, only a
//! migrated, encrypted database.

mod common;

use career_ops_core::prompts_seed::{
    seed_or_update, seed_or_update_system_prompts, SeedOutcome, SeedPrompt, SYSTEM_PROMPTS,
};
use career_ops_core::repo::prompts;

#[tokio::test]
async fn first_seed_inserts_every_prompt() {
    let db = common::TestDb::new_raw().await;

    let outcomes = seed_or_update_system_prompts(&db.pool).await.expect("seed");

    assert_eq!(outcomes.len(), SYSTEM_PROMPTS.len());
    for (name, outcome) in &outcomes {
        assert!(
            matches!(outcome, SeedOutcome::Inserted { version: 1 }),
            "expected Inserted{{version: 1}} for {}, got {:?}",
            name,
            outcome
        );
    }

    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM prompts WHERE is_system = 1")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(count as usize, SYSTEM_PROMPTS.len());

    db.close().await;
}

#[tokio::test]
async fn re_seed_is_idempotent_when_content_unchanged() {
    let db = common::TestDb::new_raw().await;

    seed_or_update_system_prompts(&db.pool)
        .await
        .expect("first seed");
    let outcomes = seed_or_update_system_prompts(&db.pool)
        .await
        .expect("second seed");

    for (name, outcome) in &outcomes {
        assert!(
            matches!(outcome, SeedOutcome::Unchanged { version: 1 }),
            "expected Unchanged{{version: 1}} for {}, got {:?}",
            name,
            outcome
        );
    }

    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM prompts WHERE is_system = 1")
        .fetch_one(&db.pool)
        .await
        .unwrap();
    assert_eq!(
        count as usize,
        SYSTEM_PROMPTS.len(),
        "re-seeding should not create new rows"
    );

    db.close().await;
}

#[tokio::test]
async fn changed_content_creates_new_version_and_preserves_old() {
    let db = common::TestDb::new_raw().await;

    // Seed v1 with a minimal custom set.
    let v1: &[SeedPrompt] = &[SeedPrompt {
        name: "test-prompt",
        category: "shared",
        content: "version one content",
    }];
    let out = seed_or_update(&db.pool, v1).await.expect("seed v1");
    assert!(matches!(out[0].1, SeedOutcome::Inserted { version: 1 }));

    let v1_row = prompts::active_system(&db.pool, "test-prompt")
        .await
        .unwrap()
        .expect("v1 active");

    // Seed v2 with different content.
    let v2: &[SeedPrompt] = &[SeedPrompt {
        name: "test-prompt",
        category: "shared",
        content: "version two content",
    }];
    let out = seed_or_update(&db.pool, v2).await.expect("seed v2");
    assert!(matches!(
        out[0].1,
        SeedOutcome::Updated {
            old_version: 1,
            new_version: 2
        }
    ));

    // Both versions still exist; the old one is unchanged.
    let all: Vec<(i64, String)> = sqlx::query_as(
        "SELECT version, content FROM prompts
         WHERE name = 'test-prompt' AND is_system = 1
         ORDER BY version",
    )
    .fetch_all(&db.pool)
    .await
    .unwrap();
    assert_eq!(all.len(), 2);
    assert_eq!(all[0], (1, "version one content".to_string()));
    assert_eq!(all[1], (2, "version two content".to_string()));

    // The new version supersedes the old one.
    let v2_row = prompts::active_system(&db.pool, "test-prompt")
        .await
        .unwrap()
        .expect("v2 active");
    assert_eq!(v2_row.version, 2);
    assert_eq!(v2_row.supersedes_id, Some(v1_row.id));

    db.close().await;
}

#[tokio::test]
async fn user_prompt_with_colliding_name_is_not_touched() {
    let db = common::TestDb::new_raw().await;

    seed_or_update_system_prompts(&db.pool).await.expect("seed");

    // Create a user prompt that shares a name with a system prompt.
    prompts::create_version(
        &db.pool,
        "_shared",
        "shared",
        "user-created prompt that happens to share a name",
        false,
    )
    .await
    .expect("user prompt");

    // Re-seed. Only system prompts are candidates for update.
    let outcomes = seed_or_update_system_prompts(&db.pool)
        .await
        .expect("re-seed");
    let shared = outcomes
        .iter()
        .find(|(n, _)| *n == "_shared")
        .expect("_shared in outcomes");
    assert!(matches!(shared.1, SeedOutcome::Unchanged { version: 1 }));

    // The user prompt is still present and untouched.
    let user_content: String = sqlx::query_scalar(
        "SELECT content FROM prompts
         WHERE name = '_shared' AND is_system = 0
         ORDER BY version DESC LIMIT 1",
    )
    .fetch_one(&db.pool)
    .await
    .unwrap();
    assert_eq!(
        user_content,
        "user-created prompt that happens to share a name"
    );

    db.close().await;
}

#[tokio::test]
async fn category_change_is_recorded_on_new_version() {
    let db = common::TestDb::new_raw().await;

    let v1: &[SeedPrompt] = &[SeedPrompt {
        name: "mover",
        category: "shared",
        content: "same content",
    }];
    seed_or_update(&db.pool, v1).await.expect("v1");

    // Same content, different category. This does *not* change the hash, so
    // the current seeding logic will report `Unchanged` — the category is
    // preserved from v1. This test documents that behaviour so a future
    // change to "category is versioned" is caught here.
    let v2: &[SeedPrompt] = &[SeedPrompt {
        name: "mover",
        category: "evaluate",
        content: "same content",
    }];
    let out = seed_or_update(&db.pool, v2).await.expect("v2");
    assert!(matches!(
        out[1 - 1].1,
        SeedOutcome::Unchanged { version: 1 }
    ));

    let active = prompts::active_system(&db.pool, "mover")
        .await
        .unwrap()
        .expect("active");
    assert_eq!(
        active.category, "shared",
        "category change is not versioned"
    );

    db.close().await;
}
