use std::sync::Arc;

use derive_more::AsRef;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use strum::{EnumIter, IntoEnumIterator};

use super::secret::Secret;
use crate::{
    config::balance::BalanceConfig, endpoints::ApiEndpoint,
    error::provider::ProviderError,
};

#[derive(
    Debug,
    Clone,
    Default,
    Copy,
    Deserialize,
    Eq,
    Hash,
    PartialEq,
    Serialize,
    EnumIter,
)]
#[serde(rename_all = "kebab-case")]
pub enum ModelProvider {
    #[default]
    #[serde(rename = "openai")]
    OpenAI,
    Anthropic,
    Amazon,
    Deepseek,
    Google,
}

impl std::fmt::Display for ModelProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ModelProvider::OpenAI => write!(f, "openai"),
            ModelProvider::Anthropic => write!(f, "anthropic"),
            ModelProvider::Amazon => write!(f, "amazon"),
            ModelProvider::Deepseek => write!(f, "deepseek"),
            ModelProvider::Google => write!(f, "google")
        }
    }
}

#[derive(
    Debug,
    Clone,
    Default,
    Copy,
    Deserialize,
    Eq,
    Hash,
    PartialEq,
    Serialize,
    EnumIter,
)]
#[serde(rename_all = "kebab-case")]
pub enum InferenceProvider {
    #[default]
    #[serde(rename = "openai")]
    OpenAI,
    Anthropic,
    Bedrock,
    VertexAi,
    Google,
    // Ollama
}

impl InferenceProvider {
    #[must_use]
    pub fn endpoints(&self) -> Vec<ApiEndpoint> {
        match self {
            InferenceProvider::OpenAI => {
                crate::endpoints::openai::OpenAI::iter()
                    .map(ApiEndpoint::OpenAI)
                    .collect()
            }
            InferenceProvider::Anthropic => {
                crate::endpoints::anthropic::Anthropic::iter()
                    .map(ApiEndpoint::Anthropic)
                    .collect()
            }
            InferenceProvider::Google => {
                crate::endpoints::google::Google::iter()
                    .map(ApiEndpoint::Google)
                    .collect()
            },
            InferenceProvider::Bedrock | InferenceProvider::VertexAi => vec![],
        }
    }
}

impl std::fmt::Display for InferenceProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InferenceProvider::OpenAI => write!(f, "openai"),
            InferenceProvider::Anthropic => write!(f, "anthropic"),
            InferenceProvider::Bedrock => write!(f, "bedrock"),
            InferenceProvider::Google => write!(f, "google"),
            InferenceProvider::VertexAi => write!(f, "vertexai"),
        }
    }
}

#[derive(Debug, Clone, AsRef)]
pub struct ProviderKeys(Arc<IndexMap<InferenceProvider, Secret<String>>>);

impl ProviderKeys {
    #[must_use]
    pub fn new(keys: IndexMap<InferenceProvider, Secret<String>>) -> Self {
        Self(Arc::new(keys))
    }

    pub fn from_env(
        balance_config: &BalanceConfig,
    ) -> Result<Self, ProviderError> {
        tracing::debug!("Discovering provider keys");
        let mut keys = IndexMap::new();
        let providers = balance_config.providers();

        for provider in providers {
            let provider_str = provider.to_string().to_uppercase();
            let env_var = format!("{provider_str}_API_KEY");
            if let Ok(key) = std::env::var(&env_var) {
                tracing::trace!(
                    provider = %provider,
                    "Got provider key"
                );
                keys.insert(provider, Secret(key));
            } else {
                return Err(ProviderError::ApiKeyNotFound(provider));
            }
        }

        Ok(Self(Arc::new(keys)))
    }
}
