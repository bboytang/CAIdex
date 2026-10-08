use crate::Error;
use caidex_credentials::{CredentialRef, SecretKind};
use caidex_provider_custom::CustomResponses;
use reqwest::Url;
use std::fmt;

/// Explicit executor-owned /v1 base, including local HTTP or a trusted HTTPS proxy.
pub struct OllamaConfig {
    base: Url,
    credential: Option<CredentialRef>,
}
impl OllamaConfig {
    pub fn new(base: &str, credential: Option<CredentialRef>) -> Result<Self, Error> {
        CustomResponses::new(base, None)?;
        if credential.as_ref().is_some_and(|reference| {
            reference.provider.as_str() != "ollama"
                || !matches!(reference.kind, SecretKind::ApiKey | SecretKind::AccessToken)
        }) {
            return Err(Error::InvalidRoute);
        }
        let mut base = Url::parse(base).map_err(|_| Error::InvalidEndpoint)?;
        if !base.path().ends_with('/') {
            base.set_path(&format!("{}/", base.path()));
        }
        Ok(Self { base, credential })
    }
    pub(crate) fn endpoint(&self, path: &str) -> Result<CustomResponses, Error> {
        let endpoint = self.base.join(path).map_err(|_| Error::InvalidEndpoint)?;
        CustomResponses::new(endpoint.as_str(), self.credential.clone())
    }
}
impl fmt::Debug for OllamaConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("OllamaConfig([PROFILE OMITTED])")
    }
}
