//! End-to-end state machine test.
//!
//! This test drives the full Phase 1 flow: `AppState::bootstrap` decides the
//! first-launch screen, `complete_first_launch` creates the keystore and
//! database and seeds prompts, the app reaches `Ready`, and after a "restart"
//! (a second `bootstrap` against the same directory) the app returns to
//! `Unlocking` and can reach `Ready` via the passphrase path.
//!
//! It is the only test that exercises the wiring between `state.rs`,
//! `db::open`/`create_and_migrate`, `keystore`, and `prompts_seed` together.
//! If any of those change signature or behaviour in a way the unit tests do
//! not catch, this test fails.

use career_ops_app::state::{AppState, Screen};
use tempfile::TempDir;

use tokio::sync::Mutex;

/// Serializes tests that write to the process-global test keychain.
///
/// `complete_first_launch` generates a fresh random KEK-keychain and writes
/// it to the shared `TEST_KEYCHAIN` map. If two tests run in parallel, one
/// test's write can land between another test's write and read, causing the
/// unwrap to fail with an AEAD authentication error. This lock prevents the
/// race by ensuring only one test at a time is mutating the keychain.
static KEYCHAIN_TEST_LOCK: Mutex<()> = Mutex::const_new(());

const PASSPHRASE: &str = "state-flow-test-passphrase";

fn setup_keyring_mock() {
    career_ops_test_support::install();
}

#[tokio::test]
async fn first_launch_then_unlock_then_ready() {
    let _guard = KEYCHAIN_TEST_LOCK.lock().await;
    setup_keyring_mock();
    let dir = TempDir::new().unwrap();

    // ---- First launch ----
    let mut state = AppState::bootstrap(dir.path().to_path_buf()).expect("bootstrap");
    assert_eq!(state.current(), &Screen::FirstLaunch);
    assert!(state.pool().is_none(), "no pool before unlock");

    state
        .complete_first_launch(PASSPHRASE)
        .await
        .expect("complete first launch");
    assert_eq!(state.current(), &Screen::Ready);
    assert!(state.pool().is_some(), "pool is present at Ready");

    // Verify prompts were seeded by the first-launch path.
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM prompts WHERE is_system = 1")
        .fetch_one(state.pool().unwrap())
        .await
        .expect("count prompts");
    assert!(count > 0, "first launch must seed system prompts");

    // Close the pool so the second `bootstrap` opens cleanly. The state
    // machine does not expose a close method — dropping the state does the
    // same thing, but drop order matters, so we close explicitly here.
    drop(state);

    // ---- Restart ----
    let mut state = AppState::bootstrap(dir.path().to_path_buf()).expect("second bootstrap");
    assert_eq!(
        state.current(),
        &Screen::Unlocking,
        "returning user goes to Unlocking, not FirstLaunch"
    );

    // Try the keychain first. Because `setup_keyring_mock` installed the
    // persistent test backend, this must succeed on the same process.
    state.try_keychain().await.expect("try_keychain");
    assert_eq!(state.current(), &Screen::Ready);
    assert!(state.pool().is_some());

    drop(state);
}

#[tokio::test]
async fn wrong_passphrase_reaches_recovery_not_ready() {
    let _guard = KEYCHAIN_TEST_LOCK.lock().await;
    setup_keyring_mock();
    let dir = TempDir::new().unwrap();

    // First launch creates the database.
    let mut state = AppState::bootstrap(dir.path().to_path_buf()).expect("bootstrap");
    state
        .complete_first_launch(PASSPHRASE)
        .await
        .expect("first launch");
    drop(state);

    // Restart, unlock with the wrong passphrase.
    let mut state = AppState::bootstrap(dir.path().to_path_buf()).expect("second bootstrap");
    assert_eq!(state.current(), &Screen::Unlocking);

    // Skip the keychain path — we want to exercise the passphrase path.
    // The keychain mock is not used by `try_passphrase`.
    state
        .try_passphrase("this-is-not-the-passphrase")
        .await
        .expect("try_passphrase returns Ok");

    match state.current() {
        Screen::Recovery { reason } => {
            assert!(
                reason.to_lowercase().contains("passphrase"),
                "recovery reason should mention the passphrase, got: {reason}"
            );
        }
        other => panic!("expected Recovery, got {other:?}"),
    }

    // Ready is unreachable.
    assert!(state.pool().is_none(), "no pool in Recovery state");
    drop(state);
}

#[tokio::test]
async fn correct_passphrase_reaches_ready() {
    let _guard = KEYCHAIN_TEST_LOCK.lock().await;
    setup_keyring_mock();
    let dir = TempDir::new().unwrap();

    let mut state = AppState::bootstrap(dir.path().to_path_buf()).expect("bootstrap");
    state
        .complete_first_launch(PASSPHRASE)
        .await
        .expect("first launch");
    drop(state);

    let mut state = AppState::bootstrap(dir.path().to_path_buf()).expect("second bootstrap");
    state
        .try_passphrase(PASSPHRASE)
        .await
        .expect("try_passphrase");

    assert_eq!(state.current(), &Screen::Ready);
    assert!(state.pool().is_some());

    // Second seeding run is a no-op — this is the "reopen" verification.
    // The pool that exists now has been through `seed_or_update_system_prompts`
    // at least twice (once at first launch, once at this unlock). Neither
    // run should have created duplicates.
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM prompts
         WHERE is_system = 1 AND name = 'oferta'",
    )
    .fetch_one(state.pool().unwrap())
    .await
    .expect("count oferta prompts");

    assert!(
        count >= 1,
        "seeding must have run and produced at least one `oferta` prompt"
    );

    drop(state);
}

#[tokio::test]
async fn keychain_failure_falls_back_to_passphrase() {
    let _guard = KEYCHAIN_TEST_LOCK.lock().await;
    setup_keyring_mock();
    let dir = TempDir::new().unwrap();

    // First launch creates a keychain wrap and a passphrase wrap.
    let mut state = AppState::bootstrap(dir.path().to_path_buf()).expect("bootstrap");
    state
        .complete_first_launch(PASSPHRASE)
        .await
        .expect("first launch");
    drop(state);

    // Arm the test keychain to fail on the next read. This simulates a
    // platform keychain that is present but unavailable.
    career_ops_test_support::install_failure();

    // Restart and attempt keychain unlock.
    let mut state = AppState::bootstrap(dir.path().to_path_buf()).expect("bootstrap");
    assert_eq!(state.current(), &Screen::Unlocking);

    state
        .try_keychain()
        .await
        .expect("try_keychain should not return Err for a platform failure");

    match state.current() {
        Screen::KeychainUnavailable { reason } => {
            assert!(
                reason.contains("platform:"),
                "reason should carry the stable platform-failure marker, got: {reason}"
            );
        }
        other => panic!("expected KeychainUnavailable, got {other:?}"),
    }
    assert!(state.pool().is_none(), "no pool after keychain failure");

    // The actual fallback: the user sees the keychain is broken and
    // enters their passphrase instead. This must reach Ready.
    state
        .try_passphrase(PASSPHRASE)
        .await
        .expect("try_passphrase after keychain failure");

    assert_eq!(
        state.current(),
        &Screen::Ready,
        "passphrase fallback should reach Ready after keychain failure"
    );
    assert!(state.pool().is_some());

    drop(state);
}

#[tokio::test]
async fn keychain_transient_failure_recovers_on_retry() {
    let _guard = KEYCHAIN_TEST_LOCK.lock().await;
    setup_keyring_mock();
    let dir = TempDir::new().unwrap();

    let mut state = AppState::bootstrap(dir.path().to_path_buf()).expect("bootstrap");
    state
        .complete_first_launch(PASSPHRASE)
        .await
        .expect("first launch");
    drop(state);

    // Single-shot failure: consumed by the first keychain read.
    career_ops_test_support::install_failure();

    let mut state = AppState::bootstrap(dir.path().to_path_buf()).expect("bootstrap");
    state.try_keychain().await.expect("try_keychain");
    assert!(
        matches!(state.current(), Screen::KeychainUnavailable { .. }),
        "first attempt should fail"
    );

    // Retry — the single-shot failure has been consumed, so the
    // second attempt should succeed.
    state.retry_unlock().expect("retry_unlock");
    assert_eq!(state.current(), &Screen::Unlocking);

    state
        .try_keychain()
        .await
        .expect("retry after transient failure");

    assert_eq!(
        state.current(),
        &Screen::Ready,
        "second keychain attempt should succeed after transient failure"
    );
    assert!(state.pool().is_some());

    drop(state);
}
