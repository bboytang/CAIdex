use crate::Error;
use caidex_credentials::{CredentialRef, SecretKind};
use caidex_provider_custom::CustomResponses;
use reqwest::Url;
use std::fmt;

/// Explicit executor-owned /v1 base, including local HTTP or a trusted HTTPS proxy.
pub struct OllamaConfig {
    base: Url,
    credential: Option<CredentialRef>,
    show: Option<CustomResponses>,
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
        Ok(Self {
            base,
            credential,
            show: None,
        })
    }
    /// Exact native endpoint on the same configured origin. Do not infer it
    /// by dropping a proxy's /v1 prefix, or send this profile's key elsewhere.
    pub fn with_show_endpoint(mut self, endpoint: &str) -> Result<Self, Error> {
        let show = CustomResponses::new(endpoint, self.credential.clone())?;
        let url = Url::parse(endpoint).map_err(|_| Error::InvalidEndpoint)?;
        if url.origin() != self.base.origin() {
            return Err(Error::InvalidEndpoint);
        }
        self.show = Some(show);
        Ok(self)
    }
    pub(crate) fn take_show_endpoint(&mut self) -> Option<CustomResponses> {
        self.show.take()
    }
    pub(crate) fn replay_scope(&self) -> serde_json::Value {
        // References contain owner/profile identifiers, never the secret.
        serde_json::json!({"base":self.base.as_str(),"credential":self.credential})
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
