//! Deduplication identity.
//!
//! A posting is identified by its canonical URL plus the provider's id, so a
//! second extraction of the same posting updates one row instead of adding a
//! duplicate. These tests pin the rules that Phase 2.5 introduced after a
//! manual run produced two NVIDIA rows for one job.

mod common;

use career_ops_core::model::FetchTier;
use career_ops_core::repo::job_descriptions::{self, NewJobDescription};
use common::TestDb;

fn job(url: &str, external_id: Option<&str>, text: &str) -> NewJobDescription {
    NewJobDescription {
        title: "Engineer".into(),
        company: "Acme".into(),
        source_url: url.into(),
        raw_text: text.into(),
        fetch_tier: FetchTier::Tier1bHtml,
        external_id: external_id.map(str::to_string),
    }
}

async fn count(pool: &sqlx::SqlitePool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM job_descriptions")
        .fetch_one(pool)
        .await
        .expect("count")
}

#[tokio::test]
async fn a_tracking_parameter_is_not_a_second_posting() {
    let db = TestDb::new_raw().await;

    let first = job_descriptions::upsert(&db.pool, job("https://example.com/jobs/1", None, "body"))
        .await
        .expect("first");
    let second = job_descriptions::upsert(
        &db.pool,
        job(
            "https://example.com/jobs/1/?utm_source=newsletter",
            None,
            "body",
        ),
    )
    .await
    .expect("second");

    assert_eq!(first.id, second.id);
    assert_eq!(count(&db.pool).await, 1);
    db.close().await;
}

#[tokio::test]
async fn a_provider_id_matches_across_a_renamed_slug() {
    let db = TestDb::new_raw().await;
    let external = "eightfold:nvidia.com:42";

    let first = job_descriptions::upsert(
        &db.pool,
        job(
            "https://jobs.nvidia.com/careers/job/42",
            Some(external),
            "body",
        ),
    )
    .await
    .expect("first");
    let renamed = job_descriptions::upsert(
        &db.pool,
        job(
            "https://jobs.nvidia.com/careers/job/42-senior-engineer",
            Some(external),
            "body",
        ),
    )
    .await
    .expect("renamed");

    assert_eq!(first.id, renamed.id);
    assert_eq!(count(&db.pool).await, 1);
    db.close().await;
}

/// The reported bug: the portal row and the manual row carried different
/// external ids for the same URL. The title/company rule must still merge them.
#[tokio::test]
async fn a_second_extraction_of_the_same_posting_updates_it() {
    let db = TestDb::new_raw().await;

    let first = job_descriptions::upsert(
        &db.pool,
        job(
            "https://jobs.nvidia.com/careers/job/42",
            Some("jsonld:one"),
            "",
        ),
    )
    .await
    .expect("first");
    let second = job_descriptions::upsert(
        &db.pool,
        job(
            "https://jobs.nvidia.com/careers/job/42/?utm_source=x",
            Some("jsonld:two"),
            "the full description",
        ),
    )
    .await
    .expect("second");

    assert_eq!(first.id, second.id);
    assert_eq!(second.raw_text, "the full description");
    assert_eq!(count(&db.pool).await, 1);
    db.close().await;
}

#[tokio::test]
async fn an_empty_refetch_keeps_the_stored_description() {
    let db = TestDb::new_raw().await;
    let url = "https://example.com/jobs/1";

    job_descriptions::upsert(&db.pool, job(url, None, "the full description"))
        .await
        .expect("first");
    let updated = job_descriptions::upsert(&db.pool, job(url, None, "   "))
        .await
        .expect("empty refetch");

    assert_eq!(updated.raw_text, "the full description");
    db.close().await;
}

#[tokio::test]
async fn a_truncated_teaser_never_replaces_a_description() {
    let db = TestDb::new_raw().await;
    let url = "https://example.com/jobs/1";

    job_descriptions::upsert(&db.pool, job(url, None, "A long description of the role."))
        .await
        .expect("first");
    let updated = job_descriptions::upsert(&db.pool, job(url, None, "A long description"))
        .await
        .expect("teaser");

    assert_eq!(
        updated.raw_text, "A long description of the role.",
        "a strict prefix is the site truncating its own teaser"
    );
    db.close().await;
}

#[tokio::test]
async fn a_richer_description_replaces_a_teaser() {
    let db = TestDb::new_raw().await;
    let url = "https://example.com/jobs/1";

    job_descriptions::upsert(&db.pool, job(url, None, "Short teaser."))
        .await
        .expect("first");
    let updated = job_descriptions::upsert(
        &db.pool,
        job(url, None, "Short teaser. And the rest of the posting."),
    )
    .await
    .expect("richer");

    assert_eq!(
        updated.raw_text,
        "Short teaser. And the rest of the posting."
    );
    db.close().await;
}

/// `db::open` fills `canonical_url` for rows written before migration 0003, so
/// an old row and a new fetch of the same posting still meet.
#[tokio::test]
async fn reopening_the_database_backfills_canonical_urls() {
    let db = TestDb::new_raw().await;

    sqlx::query(
        "INSERT INTO job_descriptions
         (title, company, source_url, raw_text, fetched_at, fetch_tier, external_id, content_hash)
         VALUES ('Engineer', 'Acme', 'https://Example.com/jobs/1/?utm_source=x', 'body', ?,
                 'tier1b:html', NULL, NULL)",
    )
    .bind(chrono::Utc::now())
    .execute(&db.pool)
    .await
    .expect("legacy row");

    let reopened = db.reopen().await.expect("reopen");

    let canonical: Option<String> =
        sqlx::query_scalar("SELECT canonical_url FROM job_descriptions")
            .fetch_one(&reopened)
            .await
            .expect("read backfill");
    assert_eq!(canonical.as_deref(), Some("https://example.com/jobs/1"));

    reopened.close().await;
    db.close().await;
}
