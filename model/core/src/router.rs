use crate::{
    CanonicalRequest, CapabilitySupport, CredentialRequirement, Error, EvidenceSource,
    ModelCapabilities, ModelMetadata, ModelProvider, ModelRegistry, ProviderError, ProviderFuture,
    ProviderResponse, ProviderResult, RequestContext, Result, StreamingResponse,
};
use std::{collections::BTreeMap, sync::Arc};

/// Explicit public model routes over existing adapters. No default provider,
/// retries, credential access or cross-provider history conversion is added.
pub struct ModelRouter {
    registry: ModelRegistry,
    routes: BTreeMap<String, Arc<dyn ModelProvider>>,
}

impl ModelRouter {
    pub fn new(routes: Vec<(String, Arc<dyn ModelProvider>)>) -> Result<Self> {
        let mut models = Vec::with_capacity(routes.len());
        let mut providers = BTreeMap::new();
        for (id, provider) in routes {
            let model = provider.metadata(&id).map_err(|_| Error::InvalidMetadata)?;
            if model.id != id || providers.insert(id, provider).is_some() {
                return Err(Error::InvalidMetadata);
            }
            models.push(model);
        }
        Ok(Self {
            registry: ModelRegistry::new(models)?,
            routes: providers,
        })
    }

    fn provider(&self, model: &str) -> ProviderResult<&Arc<dyn ModelProvider>> {
        self.routes
            .get(model)
            .ok_or_else(|| ProviderError::new(404, "unknown_model"))
    }

    fn request_provider(
        &self,
        request: &CanonicalRequest,
        streaming: bool,
    ) -> ProviderResult<&Arc<dyn ModelProvider>> {
        let metadata = self.metadata(request.model())?;
        if !metadata.dialects.contains(&request.dialect()) {
            return Err(ProviderError::new(400, "unsupported_dialect"));
        }
        if streaming && metadata.capabilities.streaming == CapabilitySupport::Unsupported {
            return Err(ProviderError::new(400, "unsupported_streaming"));
        }
        self.provider(request.model())
    }
}

impl ModelProvider for ModelRouter {
    fn list_models(&self) -> ProviderFuture<'_, Vec<ModelMetadata>> {
        Box::pin(async move {
            let mut visited = Vec::new();
            let mut models = BTreeMap::new();
            for provider in self.routes.values() {
                if visited.iter().any(|seen| Arc::ptr_eq(seen, provider)) {
                    continue;
                }
                visited.push(provider.clone());
                for model in provider.list_models().await? {
                    let Some(owner) = self.routes.get(&model.id) else {
                        continue;
                    };
                    if !Arc::ptr_eq(owner, provider) {
                        continue;
                    }
                    let registered = self.metadata(&model.id)?;
                    let mut binding = model.clone();
                    if binding.source == EvidenceSource::ProviderCatalog {
                        binding.source = registered.source;
                    }
                    if model.validate().is_err()
                        || binding != registered
                        || models.insert(model.id.clone(), model).is_some()
                    {
                        return Err(ProviderError::new(502, "provider_invalid_metadata"));
                    }
                }
            }
            Ok(models.into_values().collect())
        })
    }

    fn create_response(
        &self,
        request: CanonicalRequest,
        context: RequestContext,
    ) -> ProviderFuture<'_, ProviderResponse> {
        Box::pin(async move {
            self.request_provider(&request, false)?
                .create_response(request, context)
                .await
        })
    }

    fn stream_response(
        &self,
        request: CanonicalRequest,
        context: RequestContext,
    ) -> ProviderFuture<'_, StreamingResponse> {
        Box::pin(async move {
            self.request_provider(&request, true)?
                .stream_response(request, context)
                .await
        })
    }

    fn capabilities(&self, model: &str) -> ProviderResult<ModelCapabilities> {
        Ok(self.metadata(model)?.capabilities)
    }

    fn metadata(&self, model: &str) -> ProviderResult<ModelMetadata> {
        let metadata = self.provider(model)?.metadata(model)?;
        if self.registry.get(model) != Some(&metadata) {
            return Err(ProviderError::new(502, "provider_invalid_metadata"));
        }
        Ok(metadata)
    }

    fn credential_requirements(&self, model: &str) -> ProviderResult<CredentialRequirement> {
        self.metadata(model)?;
        self.provider(model)?.credential_requirements(model)
    }
}
