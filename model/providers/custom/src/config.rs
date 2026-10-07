use crate::{Error, Result};
use caidex_credentials::CredentialRef;
use caidex_model_core::{ModelMetadata, ResponsesDialect};
use reqwest::{Url, header::HeaderValue};
use std::{fmt, net::IpAddr};

/// Exact Responses endpoint, selected by executor configuration, never by a
/// model request. Plain HTTP is allowed only for literal loopback addresses.
pub struct CustomResponses {
    pub(crate) endpoint: Url,
    pub(crate) credential: Option<CredentialRef>,
    pub(crate) scope: Vec<(&'static str, HeaderValue)>,
}
impl CustomResponses {
    pub fn new(endpoint: &str, credential: Option<CredentialRef>) -> Result<Self> {
        let endpoint = Url::parse(endpoint).map_err(|_| Error::InvalidEndpoint)?;
        let loopback = endpoint
            .host_str()
            .and_then(|host| host.trim_matches(['[', ']']).parse::<IpAddr>().ok())
            .is_some_and(|ip| ip.is_loopback());
        if endpoint.host_str().is_none()
            || !(endpoint.scheme() == "https" || endpoint.scheme() == "http" && loopback)
            || !endpoint.username().is_empty()
            || endpoint.password().is_some()
            || endpoint.query().is_some()
            || endpoint.fragment().is_some()
        {
            return Err(Error::InvalidEndpoint);
        }
        Ok(Self {
            endpoint,
            credential,
            scope: Vec::new(),
        })
    }
    /// Executor-owned OpenAI routing scope, never caller-supplied HTTP headers.
    pub fn with_openai_scope(
        mut self,
        organization: Option<&str>,
        project: Option<&str>,
    ) -> Result<Self> {
        self.scope.clear();
        for (name, value) in [
            ("openai-organization", organization),
            ("openai-project", project),
        ] {
            if let Some(value) = value {
                if value.is_empty()
                    || value.len() > 1024
                    || !value.bytes().all(|byte| (33..=126).contains(&byte))
                {
                    return Err(Error::InvalidScope);
                }
                let mut value = HeaderValue::from_str(value).map_err(|_| Error::InvalidScope)?;
                value.set_sensitive(true);
                self.scope.push((name, value));
            }
        }
        Ok(self)
    }
}
impl fmt::Debug for CustomResponses {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("CustomResponses([ENDPOINT OMITTED])")
    }
}

pub struct ConfiguredModel {
    pub(crate) metadata: ModelMetadata,
    pub(crate) adapter: CustomResponses,
}
impl ConfiguredModel {
    pub fn new(
        model: String,
        upstream_model: String,
        dialects: Vec<ResponsesDialect>,
        adapter: CustomResponses,
    ) -> Result<Self> {
        let metadata = ModelMetadata::configured(model, upstream_model, dialects);
        metadata.validate().map_err(|_| Error::InvalidRoute)?;
        Ok(Self { metadata, adapter })
    }
    pub fn with_metadata(mut self, metadata: ModelMetadata) -> Result<Self> {
        metadata.validate().map_err(|_| Error::InvalidRoute)?;
        if metadata.id != self.metadata.id
            || metadata.native_model != self.metadata.native_model
            || metadata.dialects != self.metadata.dialects
        {
            return Err(Error::InvalidRoute);
        }
        self.metadata = metadata;
        Ok(self)
    }
    pub fn metadata(&self) -> &ModelMetadata {
        &self.metadata
    }
}
impl fmt::Debug for ConfiguredModel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ConfiguredModel")
            .field("metadata", &self.metadata)
            .finish_non_exhaustive()
    }
}
