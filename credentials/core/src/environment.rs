use crate::{CredentialRef, Error, Result, Secret, SecretStore};
use std::collections::HashMap;

/// Explicit per-profile mapping; no implicit OPENAI_API_KEY probing or cross-owner
/// fallback. Variables are read only after this source is deliberately selected.
pub struct EnvironmentStore {
    names: HashMap<CredentialRef, String>,
}
impl EnvironmentStore {
    pub fn new(names: HashMap<CredentialRef, String>) -> Result<Self> {
        if names.values().any(|name| {
            name.is_empty()
                || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
                || name.as_bytes()[0].is_ascii_digit()
        }) {
            return Err(Error::InvalidIdentifier);
        }
        Ok(Self { names })
    }
}
impl SecretStore for EnvironmentStore {
    fn get(&self, reference: &CredentialRef) -> Result<Option<Secret>> {
        let Some(name) = self.names.get(reference) else {
            return Ok(None);
        };
        match std::env::var(name) {
            Ok(value) => Secret::new(value).map(Some),
            Err(std::env::VarError::NotPresent) => Ok(None),
            Err(std::env::VarError::NotUnicode(_)) => Err(Error::InvalidSecret),
        }
    }
    fn set(&self, _: &CredentialRef, _: &Secret) -> Result<()> {
        Err(Error::ReadOnly)
    }
    fn remove(&self, _: &CredentialRef) -> Result<bool> {
        Err(Error::ReadOnly)
    }
    fn is_read_only(&self) -> bool {
        true
    }
}
