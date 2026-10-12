use crate::Result;
use caidex_credentials::{Broker, CredentialRef, Id, SecretKind, SecretStore};
use caidex_model_core::{ModelMetadata, ModelProvider, ModelRouter, ResponsesDialect};
use caidex_provider_custom::{ConfiguredModel, CustomResponses, CustomResponsesProvider, Limits};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
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
    pub model: String,
}
impl Profile {
    pub fn validate(&self) -> Result<()> {
        if self.name.trim().is_empty()
            || self.name.len() > 128
            || self.model.trim().is_empty()
            || self.model.len() > 256
            || self.endpoint.len() > 2048
            || self.model.chars().any(char::is_control)
            || self.name.chars().any(char::is_control)
        {
            return Err("invalid_configuration");
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
            Arc::new(config!(caidex_provider_deepseek::DeepSeekProvider::new(
                config!(
                    config!(caidex_provider_deepseek::DeepSeekConfig::new(key))
                        .with_base_url(endpoint)
                ),
                vec![metadata],
                broker,
                limits
            )))
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
                    metadata, 4096, 0
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
                vec![caidex_provider_google::GeminiModel::new(metadata, 4096, 0)],
                64
            )))
        }
    };
    Ok(Arc::new(config!(ModelRouter::new(vec![(id, provider)]))))
}
