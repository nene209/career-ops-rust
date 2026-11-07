//! Shared test support for the Career-Ops workspace.
//!
//! Provides a persistent in-memory keychain backend for integration tests
//! that need to simulate writing a credential in one "session" and reading
//! it back in another. The built-in `keyring::mock` backend is entry-scoped
//! and cannot support that flow.
//!
//! Also supports **failure injection**: `install_failure` makes the next
//! read of the keychain KEK return `Error::PlatformFailure`, which is the
//! same error a real platform keychain produces when it is unavailable or
//! locked. This lets tests exercise the "keychain broken, fall back to
//! passphrase" path that `test-no-secret-service` is meant to cover.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, Once};

use keyring::credential::{
    Credential, CredentialApi, CredentialBuilder, CredentialBuilderApi, CredentialPersistence,
};
use keyring::Error;

static TEST_KEYCHAIN: Mutex<Option<HashMap<(String, String), String>>> = Mutex::new(None);

/// When true, the next `get_secret` / `get_password` call returns
/// `Error::PlatformFailure`. Reset to false after one failure so tests
/// can verify recovery on a subsequent attempt.
static INJECT_FAILURE: AtomicBool = AtomicBool::new(false);

/// When true, every `get_secret` / `get_password` call returns
/// `Error::PlatformFailure` until cleared. Used for tests that want
/// the keychain to stay broken for the duration of a scenario.
static PERSISTENT_FAILURE: AtomicBool = AtomicBool::new(false);

fn keychain() -> std::sync::MutexGuard<'static, Option<HashMap<(String, String), String>>> {
    TEST_KEYCHAIN.lock().expect("test keychain mutex poisoned")
}

fn platform_failure() -> Error {
    // The concrete error type does not matter — only that it is a
    // PlatformFailure, which the keystore maps to KeychainUnavailable.
    Error::PlatformFailure(std::io::Error::other("injected test failure").into())
}

struct PersistentTestCredential {
    service: String,
    user: String,
}

impl CredentialApi for PersistentTestCredential {
    fn set_password(&self, password: &str) -> keyring::Result<()> {
        self.set_secret(password.as_bytes())
    }

    fn get_password(&self) -> keyring::Result<String> {
        let secret = self.get_secret()?;
        String::from_utf8(secret).map_err(|_| {
            Error::Invalid(
                "stored secret is not valid UTF-8".to_string(),
                "test keychain".to_string(),
            )
        })
    }

    fn set_secret(&self, password: &[u8]) -> keyring::Result<()> {
        if PERSISTENT_FAILURE.load(Ordering::SeqCst) {
            return Err(platform_failure());
        }
        let password = String::from_utf8(password.to_vec()).map_err(|_| {
            Error::Invalid(
                "password is not valid UTF-8".to_string(),
                "test keychain".to_string(),
            )
        })?;
        let mut guard = keychain();
        let map = guard.get_or_insert_with(HashMap::new);
        map.insert((self.service.clone(), self.user.clone()), password);
        Ok(())
    }

    fn get_secret(&self) -> keyring::Result<Vec<u8>> {
        if PERSISTENT_FAILURE.load(Ordering::SeqCst) {
            return Err(platform_failure());
        }
        if INJECT_FAILURE.swap(false, Ordering::SeqCst) {
            return Err(platform_failure());
        }
        let guard = keychain();
        guard
            .as_ref()
            .and_then(|m| m.get(&(self.service.clone(), self.user.clone())))
            .map(|s| s.as_bytes().to_vec())
            .ok_or(Error::NoEntry)
    }

    fn delete_credential(&self) -> keyring::Result<()> {
        let mut guard = keychain();
        if let Some(map) = guard.as_mut() {
            map.remove(&(self.service.clone(), self.user.clone()));
        }
        Ok(())
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

struct PersistentTestCredentialBuilder;

impl CredentialBuilderApi for PersistentTestCredentialBuilder {
    fn build(
        &self,
        _target: Option<&str>,
        service: &str,
        user: &str,
    ) -> keyring::Result<Box<Credential>> {
        Ok(Box::new(PersistentTestCredential {
            service: service.to_string(),
            user: user.to_string(),
        }))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn persistence(&self) -> CredentialPersistence {
        CredentialPersistence::UntilDelete
    }
}

/// Install the persistent test keychain backend as the process-global
/// credential builder. Must be called before any `keyring::Entry` is created.
pub fn install() {
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        let builder: Box<CredentialBuilder> = Box::new(PersistentTestCredentialBuilder);
        keyring::set_default_credential_builder(builder);
    });
}

/// Arm a single-shot failure on the next keychain read.
///
/// The next call to `get_secret` or `get_password` returns
/// `Error::PlatformFailure`. Subsequent calls behave normally. Use this
/// to test that the keystore handles a transient keychain failure
/// without losing the ability to retry.
pub fn install_failure() {
    install();
    INJECT_FAILURE.store(true, Ordering::SeqCst);
}

/// Arm persistent failure on all keychain reads.
///
/// Every `get_secret` or `get_password` call returns
/// `Error::PlatformFailure` until `clear_failure` is called. Use this to
/// test a scenario where the keychain is unavailable for the entire
/// duration of an operation.
pub fn install_persistent_failure() {
    install();
    PERSISTENT_FAILURE.store(true, Ordering::SeqCst);
}

/// Clear persistent failure. Single-shot failure is also cleared.
pub fn clear_failure() {
    INJECT_FAILURE.store(false, Ordering::SeqCst);
    PERSISTENT_FAILURE.store(false, Ordering::SeqCst);
}

/// Clear the in-memory keychain map and all failure flags.
///
/// Call this between tests to ensure one test's credentials and failure
/// state do not leak into another. Tests that run in parallel should
/// serialize on a mutex before calling this.
pub fn reset() {
    let mut guard = keychain();
    *guard = None;
    clear_failure();
}
