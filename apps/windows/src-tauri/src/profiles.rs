use crate::{Result, provider_error};
use caidex_credentials::{Broker, CredentialRef, Id, SecretKind, SecretStore};
use caidex_model_core::{
    ModelMetadata, ModelProvider, ModelRouter, RequestContext, ResponsesDialect,
};
use caidex_provider_custom::{
    ConfiguredModel, CustomResponses, CustomResponsesProvider, Limits, parse_model_catalog,
};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderKind {
    Openai,
    Anthropic,
    Google,
    Deepseek,
    Qwen,
    Openrouter,
    Ollama,
    CustomResponses,
    CustomChatCompletions,
}
impl ProviderKind {
    fn id(self) -> &'static str {
        match self {
            Self::Openai => "openai",
            Self::Anthropic => "anthropic",
            Self::Google => "google",
            Self::Deepseek => "deepseek",
            Self::Qwen => "qwen",
            Self::Openrouter => "openrouter",
            Self::Ollama => "ollama",
            Self::CustomResponses => "custom-responses",
            Self::CustomChatCompletions => "custom-chat-completions",
        }
    }
}
#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Profile {
    pub id: Uuid,
    pub name: String,
    pub kind: ProviderKind,
    pub endpoint: String,
    #[serde(default)]
    pub model: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub effort: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tier: Option<String>,
}
impl Profile {
    pub fn validate(&self) -> Result<()> {
        if self.name.trim().is_empty()
            || self.name.len() > 128
            || self.model.len() > 256
            || self.endpoint.len() > 2048
            || self.model.chars().any(char::is_control)
            || self.name.chars().any(char::is_control)
        {
            return Err("invalid_configuration");
        }
        if self.effort.as_deref().is_some_and(|v| {
            !matches!(
                v,
                "none" | "minimal" | "low" | "medium" | "high" | "xhigh" | "max"
            )
        }) || self
            .tier
            .as_deref()
            .is_some_and(|v| !matches!(v, "auto" | "default" | "priority" | "flex"))
        {
            return Err("unsupported_controls");
        }
        CustomResponses::new(&self.endpoint, None).map_err(|_| "invalid_endpoint")?;
        Ok(())
    }
    pub fn reference(&self) -> CredentialRef {
        CredentialRef {
            owner: caidex_chat::Namespace::guest().credential_owner(),
            provider: Id::new(self.kind.id()).unwrap(),
            profile: Id::new(self.id.to_string()).unwrap(),
            kind: SecretKind::ApiKey,
        }
    }
}
pub fn model<S: SecretStore + 'static>(
    profile: &Profile,
    broker: Arc<Broker<S>>,
) -> Result<Arc<dyn ModelProvider>> {
    profile.validate()?;
    if profile.model.trim().is_empty() {
        return Err("model_unavailable");
    }
    let id = profile.id.to_string();
    let metadata = ModelMetadata::configured(
        id.clone(),
        profile.model.clone(),
        vec![ResponsesDialect::Classic],
    );
    let key = profile.reference();
    let limits = Limits::default();
    let endpoint = &profile.endpoint;
    macro_rules! config {
        ($value:expr) => {
            $value.map_err(|_| "invalid_configuration")?
        };
    }
    let provider: Arc<dyn ModelProvider> = match profile.kind {
        ProviderKind::CustomResponses => Arc::new(config!(CustomResponsesProvider::new(
            vec![config!(ConfiguredModel::new(
                id.clone(),
                profile.model.clone(),
                vec![ResponsesDialect::Classic],
                config!(CustomResponses::new(endpoint, Some(key)))
            ))],
            broker,
            limits
        ))),
        ProviderKind::CustomChatCompletions => Arc::new(config!(
            caidex_provider_chat_completions::ChatCompletionsProvider::new(
                config!(
                    caidex_provider_chat_completions::ChatCompletionsConfig::new(
                        endpoint,
                        Some(key)
                    )
                ),
                vec![metadata],
                broker,
                limits
            )
        )),
        ProviderKind::Openai => Arc::new(config!(caidex_provider_openai::OpenAiProvider::new(
            config!(
                config!(caidex_provider_openai::OpenAiConfig::new(key)).with_base_url(endpoint)
            ),
            vec![metadata],
            broker,
            limits
        ))),
        ProviderKind::Deepseek => {
            let mut provider = config!(caidex_provider_deepseek::DeepSeekProvider::new(
                config!(
                    config!(caidex_provider_deepseek::DeepSeekConfig::new(key))
                        .with_base_url(endpoint)
                ),
                vec![metadata],
                broker,
                limits
            ))
            .with_native_tools();
            for effort in ["none", "low", "high", "max"] {
                provider = provider
                    .with_reasoning_effort_mapping(effort.into(), effort.into())
                    .map_err(provider_error)?;
            }
            Arc::new(provider)
        }
        ProviderKind::Qwen => Arc::new(config!(caidex_provider_qwen::QwenProvider::new(
            config!(caidex_provider_qwen::QwenConfig::new(endpoint, key)),
            vec![metadata],
            broker,
            limits
        ))),
        ProviderKind::Openrouter => Arc::new(config!(
            caidex_provider_openrouter::OpenRouterProvider::new(
                config!(
                    config!(caidex_provider_openrouter::OpenRouterConfig::new(key))
                        .with_base_url(endpoint)
                ),
                vec![metadata],
                broker,
                limits
            )
        )),
        ProviderKind::Ollama => Arc::new(config!(caidex_provider_ollama::OllamaProvider::new(
            config!(caidex_provider_ollama::OllamaConfig::new(
                endpoint,
                if broker
                    .status(&key)
                    .map_err(|_| "credential_unavailable")?
                    .configured
                {
                    Some(key)
                } else {
                    None
                }
            )),
            vec![metadata],
            broker,
            limits
        ))),
        ProviderKind::Anthropic => {
            let client = config!(caidex_provider_anthropic::AnthropicClient::new(
                config!(
                    config!(caidex_provider_anthropic::AnthropicConfig::new(key))
                        .with_base_url(endpoint)
                ),
                broker,
                limits
            ));
            Arc::new(config!(caidex_provider_anthropic::AnthropicProvider::new(
                client,
                vec![caidex_provider_anthropic::AnthropicModel::new(
                    metadata, 4096, 1
                )],
                64
            )))
        }
        ProviderKind::Google => {
            let client = config!(caidex_provider_google::GeminiClient::new(
                config!(
                    config!(caidex_provider_google::GeminiConfig::new(key)).with_base_url(endpoint)
                ),
                broker,
                limits
            ));
            Arc::new(config!(caidex_provider_google::GeminiProvider::new(
                client,
                vec![caidex_provider_google::GeminiModel::new(metadata, 4096, 1)],
                64
            )))
        }
    };
    Ok(Arc::new(config!(ModelRouter::new(vec![(id, provider)]))))
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogModel {
    pub id: String,
    pub name: String,
    pub efforts: Vec<String>,
    pub tiers: Vec<String>,
}
impl CatalogModel {
    pub fn validate(&self) -> Result<()> {
        if self.id.trim().is_empty()
            || self.id.len() > 256
            || self.name.trim().is_empty()
            || self.name.len() > 512
            || self.id.chars().any(char::is_control)
            || self.name.chars().any(char::is_control)
            || self.efforts.len() > 7
            || self.tiers.len() > 4
            || self.efforts.iter().any(|v| {
                !matches!(
                    v.as_str(),
                    "none" | "minimal" | "low" | "medium" | "high" | "xhigh" | "max"
                )
            })
            || self
                .tiers
                .iter()
                .any(|v| !matches!(v.as_str(), "auto" | "default" | "priority" | "flex"))
        {
            return Err("invalid_catalog");
        }
        Ok(())
    }
}
pub fn choice(kind: ProviderKind, id: &str, wire: &serde_json::Value) -> Result<CatalogModel> {
    let declared: Vec<String> = wire["reasoning_efforts"]
        .as_array()
        .map(|v| {
            v.iter()
                .filter_map(|v| {
                    v.as_str()
                        .filter(|v| {
                            matches!(
                                *v,
                                "none" | "minimal" | "low" | "medium" | "high" | "xhigh" | "max"
                            )
                        })
                        .map(String::from)
                })
                .collect()
        })
        .unwrap_or_default();
    // These are explicit protocol/model declarations, not a guess from model-name prefixes.
    let efforts = match kind {
        ProviderKind::Deepseek => vec!["none", "low", "high", "max"]
            .into_iter()
            .map(String::from)
            .collect(),
        ProviderKind::Openai => {
            if !declared.is_empty() {
                declared
            } else {
                match id {
                    "gpt-5" | "gpt-5-mini" | "gpt-5-nano" => {
                        vec!["minimal", "low", "medium", "high"]
                    }
                    "gpt-5.4" => vec!["none", "low", "medium", "high", "xhigh"],
                    "gpt-6.1-sol" | "gpt-6-astra" => vec!["low", "medium", "high", "xhigh", "max"],
                    _ => vec![],
                }
                .into_iter()
                .map(String::from)
                .collect()
            }
        }
        _ => vec![],
    };
    let tiers = if kind == ProviderKind::Openai
        && matches!(id, "gpt-5.4" | "gpt-6.1-sol" | "gpt-6-astra")
    {
        vec!["default".into(), "priority".into()]
    } else {
        vec![]
    };
    let name = wire["display_name"]
        .as_str()
        .or(wire["displayName"].as_str())
        .or(wire["name"].as_str())
        .unwrap_or(id);
    let model = CatalogModel {
        id: id.into(),
        name: name.into(),
        efforts,
        tiers,
    };
    model.validate()?;
    Ok(model)
}

pub async fn discover<S: SecretStore + 'static>(
    profile: &Profile,
    broker: Arc<Broker<S>>,
) -> Result<Vec<CatalogModel>> {
    profile.validate()?;
    let metadata = ModelMetadata::configured(
        profile.id.to_string(),
        "models/catalog".into(),
        vec![ResponsesDialect::Classic],
    );
    let key = profile.reference();
    let limits = Limits::default();
    let context = RequestContext {
        deadline: Some(std::time::Instant::now() + std::time::Duration::from_secs(30)),
        ..Default::default()
    };
    let endpoint = &profile.endpoint;
    let mut models = Vec::new();
    macro_rules! config {
        ($value:expr) => {
            $value.map_err(|_| "invalid_configuration")?
        };
    }
    macro_rules! collect {
        ($provider:expr) => {
            for native in $provider
                .discover_models(context)
                .await
                .map_err(provider_error)?
            {
                models.push(choice(profile.kind, native.id(), native.wire())?);
            }
        };
    }
    match profile.kind {
        ProviderKind::Openai => collect!(config!(caidex_provider_openai::OpenAiProvider::new(
            config!(
                config!(caidex_provider_openai::OpenAiConfig::new(key)).with_base_url(endpoint)
            ),
            vec![metadata],
            broker,
            limits
        ))),
        ProviderKind::Deepseek => {
            collect!(config!(caidex_provider_deepseek::DeepSeekProvider::new(
                config!(
                    config!(caidex_provider_deepseek::DeepSeekConfig::new(key))
                        .with_base_url(endpoint)
                ),
                vec![metadata],
                broker,
                limits
            )))
        }
        ProviderKind::Qwen => collect!(config!(caidex_provider_qwen::QwenProvider::new(
            config!(caidex_provider_qwen::QwenConfig::new(endpoint, key)),
            vec![metadata],
            broker,
            limits
        ))),
        ProviderKind::Openrouter => collect!(config!(
            caidex_provider_openrouter::OpenRouterProvider::new(
                config!(
                    config!(caidex_provider_openrouter::OpenRouterConfig::new(key))
                        .with_base_url(endpoint)
                ),
                vec![metadata],
                broker,
                limits
            )
        )),
        ProviderKind::Ollama => {
            let key = broker
                .status(&key)
                .map_err(|_| "credential_unavailable")?
                .configured
                .then_some(key);
            collect!(config!(caidex_provider_ollama::OllamaProvider::new(
                config!(caidex_provider_ollama::OllamaConfig::new(endpoint, key)),
                vec![metadata],
                broker,
                limits
            )));
        }
        ProviderKind::Anthropic => {
            let client = config!(caidex_provider_anthropic::AnthropicClient::new(
                config!(
                    config!(caidex_provider_anthropic::AnthropicConfig::new(key))
                        .with_base_url(endpoint)
                ),
                broker,
                limits
            ));
            for native in client
                .discover_models(4096, context)
                .await
                .map_err(provider_error)?
            {
                models.push(choice(profile.kind, native.id(), native.wire())?);
            }
        }
        ProviderKind::Google => {
            let client = config!(caidex_provider_google::GeminiClient::new(
                config!(
                    config!(caidex_provider_google::GeminiConfig::new(key)).with_base_url(endpoint)
                ),
                broker,
                limits
            ));
            for native in client
                .discover_models(4096, context)
                .await
                .map_err(provider_error)?
            {
                models.push(choice(profile.kind, native.name(), native.wire())?);
            }
        }
        ProviderKind::CustomResponses | ProviderKind::CustomChatCompletions => {
            // Preserve a proxy's path prefix; change only the exact configured operation suffix.
            let base = endpoint.trim_end_matches('/');
            let base = base
                .strip_suffix("/responses")
                .or_else(|| base.strip_suffix("/chat/completions"))
                .unwrap_or(base);
            let models_endpoint = config!(CustomResponses::new(
                &format!("{base}/models"),
                Some(key.clone())
            ));
            let transport = config!(CustomResponsesProvider::new(
                vec![config!(ConfiguredModel::new(
                    profile.id.to_string(),
                    "catalog".into(),
                    vec![ResponsesDialect::Classic],
                    config!(CustomResponses::new(endpoint, Some(key)))
                ))],
                broker,
                limits
            ));
            let wire = transport
                .get_json(&models_endpoint, context)
                .await
                .map_err(provider_error)?;
            for native in parse_model_catalog(wire).map_err(provider_error)? {
                models.push(choice(profile.kind, native.id(), native.wire())?);
            }
        }
    }
    if models.is_empty() {
        return Err("empty_catalog");
    }
    if models.len() > 4096 {
        return Err("catalog_limit");
    }
    models.sort_by(|a, b| a.id.cmp(&b.id));
    if models.windows(2).any(|v| v[0].id == v[1].id) {
        return Err("invalid_catalog");
    }
    Ok(models)
}
