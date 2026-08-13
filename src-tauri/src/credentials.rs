use std::collections::HashMap;
use std::sync::Mutex;

#[derive(Debug, Default, Clone)]
pub struct CredentialStore {
    service: String,
    secrets: HashMap<String, String>,
}

impl CredentialStore {
    pub fn load(service: impl Into<String>) -> Self {
        let service = service.into();
        let secrets = crate::translators::engine_ids()
            .map(|provider| {
                (
                    provider.to_owned(),
                    read_secret(&service, &account(provider)),
                )
            })
            .collect();
        Self { service, secrets }
    }

    pub fn set(&mut self, provider: &str, secret: &str) -> Result<(), String> {
        if !crate::translators::is_known_engine(provider) {
            return Err(format!("Unsupported credential provider: {provider}"));
        }
        write_secret(&self.service, &account(provider), secret)?;
        self.secrets.insert(provider.into(), secret.into());
        Ok(())
    }

    pub fn get(&self, provider: &str) -> &str {
        self.secrets.get(provider).map_or("", String::as_str)
    }

    pub fn has(&self, provider: &str) -> bool {
        !self.get(provider).trim().is_empty()
    }

    /// Which translators are usable right now. Serializes as `{"qwen": true, …}`, so
    /// surfaces read it by provider id without a field per provider.
    pub fn status(&self) -> HashMap<String, bool> {
        crate::translators::engine_ids()
            .map(|provider| (provider.to_owned(), self.has(provider)))
            .collect()
    }
}

#[cfg(test)]
impl CredentialStore {
    /// Seeds a secret without touching the OS keychain.
    pub fn set_for_test(&mut self, provider: &str, secret: &str) {
        self.secrets.insert(provider.into(), secret.into());
    }
}

/// Derived rather than listed, so a new provider needs no entry here.
fn account(provider: &str) -> String {
    format!("{provider}-api-key")
}

fn entry(service: &str, account: &str) -> Result<keyring::Entry, String> {
    keyring::Entry::new(service, account).map_err(error_text)
}

fn read_secret(service: &str, account: &str) -> String {
    entry(service, account)
        .and_then(|entry| entry.get_password().map_err(error_text))
        .unwrap_or_default()
}

fn write_secret(service: &str, account: &str, secret: &str) -> Result<(), String> {
    let entry = entry(service, account)?;
    if secret.is_empty() {
        match entry.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(error) => Err(error_text(error)),
        }
    } else {
        entry.set_password(secret).map_err(error_text)
    }
}

fn error_text(error: impl std::fmt::Display) -> String {
    format!("Credential store error: {error}")
}

pub struct CredentialState(pub Mutex<CredentialStore>);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_unknown_provider_before_touching_the_keychain() {
        let mut store = CredentialStore::default();
        assert!(store.set("gemini", "secret").is_err());
        assert!(store.secrets.is_empty());
    }

    /// The account name is what the keychain already holds; changing it would orphan
    /// every key users have saved.
    #[test]
    fn account_names_stay_compatible_with_stored_keys() {
        assert_eq!(account("qwen"), "qwen-api-key");
        assert_eq!(account("openai"), "openai-api-key");
    }

    #[test]
    fn an_unset_provider_reads_as_missing() {
        let mut store = CredentialStore::default();
        assert!(!store.has("qwen"));
        store.secrets.insert("qwen".into(), "  ".into());
        assert!(!store.has("qwen"));
        store.secrets.insert("qwen".into(), "sk-test".into());
        assert!(store.has("qwen"));
    }
}
