
// ----------------------------------------------------------------------
// Persistent test keychain backend.
//
// `keyring`'s built-in mock stores credentials inside the `Entry` object,
// so it cannot simulate the "write at first launch, read after restart"
// flow that `state_flow.rs` exercises. This backend stores entries in a
// process-global map keyed by (service, user), which persists for the
// lifetime of the test binary.
//
// It is installed by `setup_keyring_mock()` in place of the built-in mock.
// ----------------------------------------------------------------------

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
        let mut guard = keychain();
        let map = guard.get_or_insert_with(HashMap::new);
        map.insert((self.service.clone(), self.user.clone()), password.to_string());
        Ok(())
    }

    fn get_password(&self) -> keyring::Result<String> {
        let guard = keychain();
        guard
            .as_ref()
            .and_then(|m| m.get(&(self.service.clone(), self.user.clone())))
            .cloned()
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

fn persistent_test_builder() -> Box<CredentialBuilder> {
    Box::new(PersistentTestCredentialBuilder)
}