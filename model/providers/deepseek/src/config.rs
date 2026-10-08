use crate::Error;
use caidex_credentials::{CredentialRef, SecretKind};
use caidex_provider_custom::CustomResponses;
use reqwest::Url;
use std::fmt;

/// Executor-owned endpoint and credential; model input never selects either.
pub struct DeepSeekConfig {
    base: Url,
    credential: CredentialRef,
}
impl DeepSeekConfig {
    pub fn new(credential: CredentialRef) -> Result<Self, Error> {
        if credential.provider.as_str() != "deepseek"
            || !matches!(
                credential.kind,
                SecretKind::ApiKey | SecretKind::AccessToken
            )
        {
            return Err(Error::InvalidRoute);
        }
        Ok(Self {
            base: Url::parse("https://api.deepseek.com/").expect("fixed URL"),
            credential,
        })
    }
    /// Explicit trusted proxy or loopback fixture, with the shared TLS policy.
    pub fn with_base_url(mut self, base: &str) -> Result<Self, Error> {
        CustomResponses::new(base, None)?;
        let mut base = Url::parse(base).map_err(|_| Error::InvalidEndpoint)?;
        if !base.path().ends_with('/') {
            base.set_path(&format!("{}/", base.path()));
        }
        self.base = base;
        self.endpoint("models")?;
        self.endpoint("responses")?;
        Ok(self)
    }
    pub(crate) fn endpoint(&self, path: &str) -> Result<CustomResponses, Error> {
        let url = self.base.join(path).map_err(|_| Error::InvalidEndpoint)?;
        CustomResponses::new(url.as_str(), Some(self.credential.clone()))
    }
    pub(crate) fn replay_scope(&self) -> serde_json::Value {
        serde_json::json!({"base":self.base.as_str(),"credential":self.credential})
    }
}
impl fmt::Debug for DeepSeekConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("DeepSeekConfig([PROFILE OMITTED])")
    }
}
