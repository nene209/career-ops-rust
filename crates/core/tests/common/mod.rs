//! Shared test harness for `career-ops-core` integration tests.
//!
//! Provides a fresh, migrated, encrypted SQLite database in a tempdir.
//! Two constructors exist because different test suites need different
//! things:
//!
//!   - `TestDb::new_raw()` — generates a DEK directly, no keystore. Works
//!     without the `keychain` feature. Use for tests that need a database
//!     but not the full unlock flow (prompt seeding, repositories).
//!
//!   - `TestDb::new_via_keystore()` — runs through `Keystore::initialize`.
//!     Requires the `keychain` feature. Use for tests that need to verify
//!     the integration between keystore and DB (unlock flows, key rotation).
//!
//! # Cleanup
//!
//! Always call `db.close().await` at the end of a test. `SqlitePool` is
//! `Drop`, but `TempDir` cleanup can fail on Windows if the file is still
//! held; `close()` awaits `pool.close()` first, which releases the handle.

#![allow(dead_code)]

use std::path::Path;
use std::sync::Once;

use career_ops_core::db;
use rand::RngCore;
use sqlx::SqlitePool;
use tempfile::TempDir;
use zeroize::Zeroizing;

#[cfg(feature = "keychain")]
use career_ops_core::keystore::Keystore;

static KEYRING_INIT: Once = Once::new();

pub const TEST_PASSPHRASE: &str = "test-correct-horse-battery-staple";

/// Set up the in-process keychain mock. Idempotent: safe to call from every
/// test binary that uses the keystore.
pub fn setup_keyring_mock() {
    career_ops_test_support::install();
}

pub struct TestDb {
    pub pool: SqlitePool,
    pub dir: TempDir,
    pub dek: Zeroizing<[u8; 32]>,
}

impl TestDb {
    /// Fresh encrypted DB with a random DEK. No keystore, no keychain.
    pub async fn new_raw() -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let mut dek = Zeroizing::new([0u8; 32]);
        rand::thread_rng().fill_bytes(&mut *dek);

        let pool = db::create_and_migrate(dir.path(), &dek)
            .await
            .expect("db::create_and_migrate");

        Self { pool, dir, dek }
    }

    /// Fresh encrypted DB created through the full keystore flow.
    /// Requires the `keychain` feature.
    #[cfg(feature = "keychain")]
    pub async fn new_via_keystore() -> Self {
        setup_keyring_mock();
        let dir = tempfile::tempdir().expect("tempdir");

        let (_ks, dek) =
            Keystore::initialize(dir.path(), TEST_PASSPHRASE).expect("Keystore::initialize");

        let pool = db::create_and_migrate(dir.path(), &dek)
            .await
            .expect("db::create_and_migrate");

        Self { pool, dir, dek }
    }

    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    pub async fn close(self) {
        self.pool.close().await;
        drop(self.dir);
    }

    /// Reopen the encrypted database at `self.path()` with `self.dek`.
    /// Returns a fresh pool. The original pool stays open; callers should
    /// either close it first or accept two connections to the same file
    /// (SQLite permits this — WAL mode is set in `db::create_and_migrate`).
    pub async fn reopen(&self) -> Result<SqlitePool, career_ops_core::error::CoreError> {
        career_ops_core::db::open(self.path(), &self.dek).await
    }
}

// ----------------------------------------------------------------------
// Mock FetchSource for router and service-layer tests.
// ----------------------------------------------------------------------

use std::collections::VecDeque;
use std::sync::Mutex;

use async_trait::async_trait;
use career_ops_core::acquisition::ats::{AtsAdapter, NormalizedJob};
use career_ops_core::acquisition::error::AtsError;
use career_ops_core::acquisition::html::selectors::SelectorConfig;
use career_ops_core::acquisition::router::FetchSource;
use url::Url;

/// A `FetchSource` that returns scripted results and records calls.
///
/// Results are popped from a queue in call order. When the queue is
/// empty, `fetch_*` returns `Ok(vec![])`. This supports both:
///
/// - Single-call tests: `MockFetchSource::with_ats(Ok(vec![job]))`
/// - Batch tests: `MockFetchSource::sequence_ats([Ok(vec![j1]), Err(...), Ok(vec![j2])])`
pub struct MockFetchSource {
    ats_calls: Mutex<Vec<(String, String)>>,
    html_calls: Mutex<Vec<String>>,
    ats_queue: Mutex<VecDeque<Result<Vec<NormalizedJob>, AtsError>>>,
    html_queue: Mutex<VecDeque<Result<Vec<NormalizedJob>, AtsError>>>,
}

impl Default for MockFetchSource {
    fn default() -> Self {
        Self {
            ats_calls: Mutex::new(Vec::new()),
            html_calls: Mutex::new(Vec::new()),
            ats_queue: Mutex::new(VecDeque::new()),
            html_queue: Mutex::new(VecDeque::new()),
        }
    }
}

impl MockFetchSource {
    pub fn with_ats(result: Result<Vec<NormalizedJob>, AtsError>) -> Self {
        let mock = Self::default();
        mock.ats_queue.lock().unwrap().push_back(result);
        mock
    }

    pub fn with_html(result: Result<Vec<NormalizedJob>, AtsError>) -> Self {
        let mock = Self::default();
        mock.html_queue.lock().unwrap().push_back(result);
        mock
    }

    pub fn sequence_ats(
        results: impl IntoIterator<Item = Result<Vec<NormalizedJob>, AtsError>>,
    ) -> Self {
        let mock = Self::default();
        mock.ats_queue.lock().unwrap().extend(results);
        mock
    }

    pub fn sequence_html(
        results: impl IntoIterator<Item = Result<Vec<NormalizedJob>, AtsError>>,
    ) -> Self {
        let mock = Self::default();
        mock.html_queue.lock().unwrap().extend(results);
        mock
    }

    pub fn ats_calls(&self) -> Vec<(String, String)> {
        self.ats_calls.lock().unwrap().clone()
    }

    pub fn html_calls(&self) -> Vec<String> {
        self.html_calls.lock().unwrap().clone()
    }
}

#[async_trait]
impl FetchSource for MockFetchSource {
    async fn fetch_ats(
        &self,
        adapter: &dyn AtsAdapter,
        slug: &str,
        _company_name: &str,
    ) -> Result<Vec<NormalizedJob>, AtsError> {
        self.ats_calls
            .lock()
            .unwrap()
            .push((adapter.name().to_string(), slug.to_string()));
        self.ats_queue
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| Ok(vec![]))
    }

    async fn fetch_html(
        &self,
        url: &Url,
        _company_name: &str,
        _config: Option<&SelectorConfig>,
    ) -> Result<Vec<NormalizedJob>, AtsError> {
        self.html_calls.lock().unwrap().push(url.to_string());
        self.html_queue
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(|| Ok(vec![]))
    }
}

/// A `NormalizedJob` for tests. Deterministic — the same title produces
/// the same job, so tests can use it to assert on stored values.
pub fn sample_job(title: &str) -> NormalizedJob {
    NormalizedJob {
        external_id: format!("id-{title}"),
        title: title.into(),
        company: "Acme".into(),
        location: Some("Remote".into()),
        department: Some("Engineering".into()),
        employment_type: None,
        workplace_type: None,
        source_url: Url::parse(&format!("https://boards.greenhouse.io/acme/jobs/{title}")).unwrap(),
        raw_text: "test".into(),
        posted_at: None,
    }
}
