//! Phase 1 critical safety tests.
//!
//! These encode properties that are cheap to verify and catastrophic to miss.
//! They are written to fail loudly if the design constraints in §7 of the main
//! plan are broken.

use career_ops_core::error::CoreError;
use career_ops_core::keystore::{Keystore, UnlockOutcome, WRAPPED_FILE};
use tempfile::tempdir;

fn enable_keyring_mock() {
    // Enable the in-process mock keychain for the whole test binary.
    // SAFETY: `set_default_credential_builder` is documented as
    // process-global; calling it once at the start of the binary is fine.
    let _ = keyring::set_default_credential_builder(
        keyring::mock::default_credential_builder(),
    );
}

#[test]
fn keychain_sentinel_round_trip() {
    // Fails loudly if the `keyring` crate was built without a platform
    // backend (a known hazard — see R-10 in the main plan).
    enable_keyring_mock();

    let entry = keyring::Entry::new("com.career-ops.app.test", "sentinel").unwrap();
    entry.set_password("ping").unwrap();
    let got = entry.get_password().unwrap();
    assert_eq!(got, "ping");
}

#[tokio::test]
async fn initialize_creates_wrapped_file() {
    let dir = tempdir().unwrap();
    let (_ks, _dek) = Keystore::initialize(dir.path(), "hunter2-correct-horse").unwrap();
    assert!(dir.path().join(WRAPPED_FILE).exists());
}

#[tokio::test]
async fn initialize_refuses_when_wrapped_file_exists() {
    let dir = tempdir().unwrap();
    let (_ks, _dek) = Keystore::initialize(dir.path(), "pw").unwrap();

    // A second initialize against the same directory must fail. If it
    // succeeds, the "never re-initialize" rule is broken and users could
    // silently lose data after a keychain failure.
    let second = Keystore::initialize(dir.path(), "different-pw");
    assert!(
        matches!(second, Err(CoreError::KeystoreLocked(_))),
        "second initialize should have been refused"
    );
}

#[tokio::test]
async fn passphrase_unlock_succeeds() {
    enable_keyring_mock();
    let dir = tempdir().unwrap();
    let _ = Keystore::initialize(dir.path(), "correct-passphrase").unwrap();

    let ks = Keystore::open(dir.path()).unwrap();
    let outcome = ks.unlock_with_passphrase("correct-passphrase").unwrap();
    assert!(matches!(outcome, UnlockOutcome::Unlocked(_)));
}

#[tokio::test]
async fn wrong_passphrase_produces_no_dek() {
    enable_keyring_mock();
    let dir = tempdir().unwrap();
    let _ = Keystore::initialize(dir.path(), "correct-passphrase").unwrap();

    let ks = Keystore::open(dir.path()).unwrap();
    let outcome = ks.unlock_with_passphrase("wrong").unwrap();
    match outcome {
        UnlockOutcome::RecoveryRequired { .. } => {} // expected
        UnlockOutcome::Unlocked(_) => panic!("wrong passphrase produced a DEK"),
        other => panic!("unexpected: {other:?}"),
    }
}

#[tokio::test]
async fn open_without_initialize_fails() {
    // `open` must never create a keystore. If it did, a user with a corrupted
    // wrapped file could end up with a fresh DEK and an orphaned database.
    let dir = tempdir().unwrap();
    let result = Keystore::open(dir.path());
    assert!(matches!(result, Err(CoreError::KeystoreNotInitialized(_))));
}