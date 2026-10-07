use crate::Error;
use caidex_credentials::{CredentialRef, SecretKind};
use caidex_provider_custom::CustomResponses;
use reqwest::Url;
use std::fmt;

/// Fixed executor profile, not model-provided URLs, organization or project.
pub struct OpenAiConfig {
    base_url: Url,
    credential: CredentialRef,
    organization: Option<String>,
    project: Option<String>,
}
impl OpenAiConfig {
    pub fn new(credential: CredentialRef) -> Result<Self, Error> {
        if credential.provider.as_str() != "openai"
            || !matches!(
                credential.kind,
                SecretKind::ApiKey | SecretKind::AccessToken
            )
        {
            return Err(Error::InvalidRoute);
        }
        Ok(Self {
            base_url: Url::parse("https://api.openai.com/v1/").expect("fixed URL"),
            credential,
            organization: None,
            project: None,
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
    pub fn with_scope(
        mut self,
        organization: Option<&str>,
        project: Option<&str>,
    ) -> Result<Self, Error> {
        CustomResponses::new(self.base_url.as_str(), None)?
            .with_openai_scope(organization, project)?;
        self.organization = organization.map(str::to_owned);
        self.project = project.map(str::to_owned);
        Ok(self)
    }
    pub(crate) fn endpoint(&self, path: &str) -> Result<CustomResponses, Error> {
        let url = self
            .base_url
            .join(path)
            .map_err(|_| Error::InvalidEndpoint)?;
        CustomResponses::new(url.as_str(), Some(self.credential.clone()))?
            .with_openai_scope(self.organization.as_deref(), self.project.as_deref())
    }
}
impl fmt::Debug for OpenAiConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("OpenAiConfig([PROFILE OMITTED])")
    }
}
