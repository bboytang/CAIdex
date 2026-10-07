use crate::{Error, Result};
use caidex_credentials::CredentialRef;
use caidex_model_core::ResponsesDialect;
use reqwest::Url;
use std::{fmt, net::IpAddr};

/// Exact Responses endpoint, selected by executor configuration, never by a
/// model request. Plain HTTP is allowed only for literal loopback addresses.
pub struct CustomResponses {
    pub(crate) endpoint: Url,
    pub(crate) credential: Option<CredentialRef>,
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
        })
    }
}
impl fmt::Debug for CustomResponses {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("CustomResponses([ENDPOINT OMITTED])")
    }
}

pub struct ModelRoute {
    pub(crate) model: String,
    pub(crate) upstream_model: String,
    pub(crate) dialects: Vec<ResponsesDialect>,
    pub(crate) adapter: CustomResponses,
}
impl ModelRoute {
    pub fn new(
        model: String,
        upstream_model: String,
        dialects: Vec<ResponsesDialect>,
        adapter: CustomResponses,
    ) -> Result<Self> {
        if model.trim().is_empty() || upstream_model.trim().is_empty() || dialects.is_empty() {
            return Err(Error::InvalidRoute);
        }
        Ok(Self {
            model,
            upstream_model,
            dialects,
            adapter,
        })
    }
}
impl fmt::Debug for ModelRoute {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ModelRoute")
            .field("dialects", &self.dialects)
            .finish_non_exhaustive()
    }
}
