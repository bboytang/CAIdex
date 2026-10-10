use crate::Error;
use caidex_credentials::{CredentialRef, SecretKind};
use caidex_provider_custom::CustomResponses;
use reqwest::Url;
use std::fmt;

/// Fixed executor endpoint and API-key reference, never model-provided URLs.
pub struct OpenRouterConfig {
    base_url: Url,
    credential: CredentialRef,
}
impl OpenRouterConfig {
    pub fn new(credential: CredentialRef) -> Result<Self, Error> {
        if credential.provider.as_str() != "openrouter" || credential.kind != SecretKind::ApiKey {
            return Err(Error::InvalidRoute);
        }
        Ok(Self {
            base_url: Url::parse("https://openrouter.ai/api/v1/").expect("fixed URL"),
            credential,
        })
    }
    /// Explicit proxy/regional base or loopback fixture. TLS policy stays on.
    pub fn with_base_url(mut self, base_url: &str) -> Result<Self, Error> {
        // Reuse the exact endpoint safety policy before constructing paths.
        CustomResponses::new(base_url, None)?;
        let mut base = Url::parse(base_url).map_err(|_| Error::InvalidEndpoint)?;
        if !base.path().ends_with('/') {
            base.set_path(&format!("{}/", base.path()));
        }
        self.base_url = base;
        self.endpoint("models")?;
        self.endpoint("responses")?;
        Ok(self)
    }
    pub(crate) fn replay_scope(&self) -> serde_json::Value {
        serde_json::json!({"base":self.base_url.as_str(),"credential":self.credential})
    }
    pub(crate) fn endpoint(&self, path: &str) -> Result<CustomResponses, Error> {
        let url = self
            .base_url
            .join(path)
            .map_err(|_| Error::InvalidEndpoint)?;
        CustomResponses::new(url.as_str(), Some(self.credential.clone()))
    }
}
impl fmt::Debug for OpenRouterConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("OpenRouterConfig([PROFILE OMITTED])")
    }
}
