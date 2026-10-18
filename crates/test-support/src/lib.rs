//! Shared test support for the Career-Ops workspace.
//!
//! Provides a persistent in-memory keychain backend for integration tests
//! that need to simulate writing a credential in one "session" and reading
//! it back in another. The built-in `keyring::mock` backend is entry-scoped
//! and cannot support that flow [citation:1].

use std::collections::HashMap;
use std::sync::Mutex;

use keyring::credential::{
    Credential, CredentialApi, CredentialBuilder, CredentialBuilderApi,
    CredentialPersistence,
};
use keyring::Error;

static TEST_KEYCHAIN: Mutex<Option<HashMap<(String, String), String>>> = Mutex::new(None);

fn keychain() -> std::sync::MutexGuard<'static, Option<HashMap<(String, String), String>>> {
    TEST_KEYCHAIN.lock().expect("test keychain mutex poisoned")
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
///
/// Idempotent: uses a `Once` internally so concurrent callers are safe.
pub fn install() {
    use std::sync::Once;
    static INIT: Once = Once::new();
    INIT.call_once(|| {
        let builder: Box<CredentialBuilder> = Box::new(PersistentTestCredentialBuilder);
        keyring::set_default_credential_builder(builder);
    });
}