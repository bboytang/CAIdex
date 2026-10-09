use crate::Error;
use caidex_credentials::{CredentialRef, SecretKind};
use caidex_provider_custom::CustomResponses;
use reqwest::Url;
use std::fmt;

/// Explicit executor-selected regional/workspace origin and matching API key.
/// No default region, billing plan, environment key or cross-origin fallback.
pub struct QwenConfig {
    base: Url,
    credential: CredentialRef,
}
impl QwenConfig {
    pub fn new(base: &str, credential: CredentialRef) -> Result<Self, Error> {
        if credential.provider.as_str() != "qwen" || credential.kind != SecretKind::ApiKey {
            return Err(Error::InvalidRoute);
        }
        CustomResponses::new(base, None)?;
        let mut base = Url::parse(base).map_err(|_| Error::InvalidEndpoint)?;
        if !base.path().ends_with('/') {
            base.set_path(&format!("{}/", base.path()));
        }
        let config = Self { base, credential };
        config.endpoint("api/v1/models")?;
        config.endpoint("compatible-mode/v1/responses")?;
        Ok(config)
    }
    pub(crate) fn endpoint(&self, path: &str) -> Result<CustomResponses, Error> {
        let endpoint = self.base.join(path).map_err(|_| Error::InvalidEndpoint)?;
        CustomResponses::new(endpoint.as_str(), Some(self.credential.clone()))
    }
    pub(crate) fn replay_scope(&self) -> serde_json::Value {
        serde_json::json!({"base":self.base.as_str(),"credential":self.credential})
    }
}
impl fmt::Debug for QwenConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("QwenConfig([PROFILE OMITTED])")
    }
}
