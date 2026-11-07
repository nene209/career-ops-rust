//! API keys, in a dedicated OS-keychain namespace.
//!
//! `core::keystore` owns the database's envelope key and deliberately does not
//! grow a general secret API; these functions are that API, next to it,
//! sharing its service name so the OS groups the entries together. Keys never
//! reach the database, the log, or an error message.
//!
//! Without the `keychain` feature every function reports
//! [`CoreError::KeychainUnavailable`], matching the keystore: a build that
//! cannot store a key must say so rather than appear to succeed.

use super::config::LlmProvider;

#[cfg(not(feature = "keychain"))]
use crate::error::Result;
#[cfg(not(feature = "keychain"))]
use zeroize::Zeroizing;

/// The keychain service name, shared with the keystore's KEK entry.
pub const SERVICE: &str = "com.career-ops.app";

/// One account per provider, versioned like the KEK's (`-v1`) so a future
/// change of shape can migrate rather than overwrite.
pub fn account(provider: LlmProvider) -> String {
    format!("llm-api-key-{}-v1", provider.as_str())
}

#[cfg(feature = "keychain")]
mod inner {
    use keyring::Entry;
    use zeroize::Zeroizing;

    use super::super::config::LlmProvider;
    use super::{account, SERVICE};
    use crate::error::{CoreError, Result};

    pub fn get_api_key(provider: LlmProvider) -> Result<Option<Zeroizing<String>>> {
        let entry = Entry::new(SERVICE, &account(provider)).map_err(map_keyring_error)?;
        match entry.get_password() {
            Ok(secret) => Ok(Some(Zeroizing::new(secret))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(error) => Err(map_keyring_error(error)),
        }
    }

    pub fn set_api_key(provider: LlmProvider, key: &str) -> Result<()> {
        let key = key.trim();
        if key.is_empty() {
            return Err(CoreError::Message("the API key is empty".into()));
        }
        let entry = Entry::new(SERVICE, &account(provider)).map_err(map_keyring_error)?;
        entry.set_password(key).map_err(map_keyring_error)
    }

    /// Deleting a key that is not there is not an error: the caller asked for
    /// it to be gone.
    pub fn delete_api_key(provider: LlmProvider) -> Result<()> {
        let entry = Entry::new(SERVICE, &account(provider)).map_err(map_keyring_error)?;
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(map_keyring_error(error)),
        }
    }

    pub fn has_api_key(provider: LlmProvider) -> Result<bool> {
        Ok(get_api_key(provider)?.is_some())
    }

    fn map_keyring_error(error: keyring::Error) -> CoreError {
        match error {
            keyring::Error::PlatformFailure(inner) => {
                CoreError::KeychainUnavailable(format!("platform: {inner}"))
            }
            other => CoreError::KeychainUnavailable(other.to_string()),
        }
    }
}

#[cfg(feature = "keychain")]
pub use inner::{delete_api_key, get_api_key, has_api_key, set_api_key};

#[cfg(not(feature = "keychain"))]
pub fn get_api_key(_provider: LlmProvider) -> Result<Option<Zeroizing<String>>> {
    Err(unsupported())
}

#[cfg(not(feature = "keychain"))]
pub fn set_api_key(_provider: LlmProvider, _key: &str) -> Result<()> {
    Err(unsupported())
}

#[cfg(not(feature = "keychain"))]
pub fn delete_api_key(_provider: LlmProvider) -> Result<()> {
    Err(unsupported())
}

#[cfg(not(feature = "keychain"))]
pub fn has_api_key(provider: LlmProvider) -> Result<bool> {
    get_api_key(provider).map(|key| key.is_some())
}

#[cfg(not(feature = "keychain"))]
fn unsupported() -> crate::error::CoreError {
    crate::error::CoreError::KeychainUnavailable("built without `keychain` feature".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accounts_are_stable_and_distinct() {
        assert_eq!(account(LlmProvider::DeepSeek), "llm-api-key-deepseek-v1");
        assert_eq!(account(LlmProvider::OpenAI), "llm-api-key-openai-v1");

        let mut seen = std::collections::HashSet::new();
        for provider in LlmProvider::ALL {
            assert!(seen.insert(account(provider)), "{provider:?}");
        }
    }
}
