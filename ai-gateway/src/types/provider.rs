use std::{str::FromStr, sync::Arc};

use compact_str::CompactString;
use rustc_hash::FxHashMap as HashMap;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use strum::{EnumIter, IntoEnumIterator};

use super::secret::Secret;
use crate::{
    config::{providers::ProvidersConfig, router::RouterConfig},
    endpoints::ApiEndpoint,
    error::provider::ProviderError,
};

#[derive(
    Debug,
    Clone,
    Default,
    Copy,
    Eq,
    Hash,
    PartialEq,
    EnumIter,
    strum::Display,
    strum::EnumString,
)]
#[strum(serialize_all = "kebab-case")]
pub enum ModelProvider {
    #[default]
    OpenAI,
    Anthropic,
    Amazon,
    Deepseek,
    Google,
}

impl<'de> Deserialize<'de> for ModelProvider {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        ModelProvider::from_str(&s).map_err(serde::de::Error::custom)
    }
}

impl Serialize for ModelProvider {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

#[derive(
    Debug,
    Clone,
    Default,
    Eq,
    Hash,
    PartialEq,
    EnumIter,
    serde::Serialize,
    serde::Deserialize,
)]
#[serde(rename_all = "kebab-case")]
pub enum InferenceProvider {
    #[default]
    #[serde(rename = "openai")]
    OpenAI,
    Anthropic,
    Bedrock,
    Ollama,
    #[serde(rename = "gemini")]
    GoogleGemini,
    #[serde(untagged)]
    Named(CompactString),
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
            InferenceProvider::Ollama => {
                crate::endpoints::ollama::Ollama::iter()
                    .map(ApiEndpoint::Ollama)
                    .collect()
            }
            InferenceProvider::Bedrock => {
                crate::endpoints::bedrock::Bedrock::iter()
                    .map(ApiEndpoint::Bedrock)
                    .collect()
            }
            InferenceProvider::GoogleGemini => {
                crate::endpoints::google::Google::iter()
                    .map(ApiEndpoint::Google)
                    .collect()
            }
            InferenceProvider::Named(_) => {
                crate::endpoints::openai::OpenAI::iter()
                    .map(|endpoint| ApiEndpoint::OpenAICompatible {
                        provider: self.clone(),
                        openai_endpoint: endpoint,
                    })
                    .collect()
            }
        }
    }
}

impl FromStr for InferenceProvider {
    type Err = std::convert::Infallible;

    fn from_str(
        s: &str,
    ) -> ::core::result::Result<InferenceProvider, Self::Err> {
        match s {
            "openai" => Ok(InferenceProvider::OpenAI),
            "anthropic" => Ok(InferenceProvider::Anthropic),
            "bedrock" => Ok(InferenceProvider::Bedrock),
            "ollama" => Ok(InferenceProvider::Ollama),
            "gemini" => Ok(InferenceProvider::GoogleGemini),
            s => Ok(InferenceProvider::Named(s.into())),
        }
    }
}

impl AsRef<str> for InferenceProvider {
    fn as_ref(&self) -> &str {
        match self {
            InferenceProvider::Named(name) => name.as_ref(),
            InferenceProvider::OpenAI => "openai",
            InferenceProvider::Anthropic => "anthropic",
            InferenceProvider::Bedrock => "bedrock",
            InferenceProvider::Ollama => "ollama",
            InferenceProvider::GoogleGemini => "gemini",
        }
    }
}

impl std::fmt::Display for InferenceProvider {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InferenceProvider::Named(name) => write!(f, "{name}"),
            _ => write!(f, "{}", self.as_ref()),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ProviderKey {
    Secret(Secret<String>),
    AwsCredentials {
        access_key: Secret<String>,
        secret_key: Secret<String>,
    },
    NotRequired,
}

impl ProviderKey {
    #[must_use]
    pub fn as_secret(&self) -> Option<&Secret<String>> {
        match self {
            ProviderKey::Secret(key) => Some(key),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_aws_credentials(
        &self,
    ) -> (Option<&Secret<String>>, Option<&Secret<String>>) {
        match self {
            ProviderKey::AwsCredentials {
                access_key,
                secret_key,
            } => (Some(access_key), Some(secret_key)),
            _ => (None, None),
        }
    }

    #[must_use]
    pub fn from_env(provider: &InferenceProvider) -> Option<Self> {
        if *provider == InferenceProvider::Bedrock {
            if let (Ok(access_key), Ok(secret_key)) = (
                std::env::var("AWS_ACCESS_KEY"),
                std::env::var("AWS_SECRET_KEY"),
            ) {
                Some(ProviderKey::AwsCredentials {
                    access_key: Secret::from(access_key),
                    secret_key: Secret::from(secret_key),
                })
            } else {
                None
            }
        } else {
            let provider_str = provider.to_string().to_uppercase();
            let env_var = format!("{provider_str}_API_KEY");
            if let Ok(key) = std::env::var(&env_var) {
                Some(ProviderKey::Secret(Secret::from(key)))
            } else {
                None
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct ProviderKeys(Arc<HashMap<InferenceProvider, ProviderKey>>);

impl std::ops::Deref for ProviderKeys {
    type Target = HashMap<InferenceProvider, ProviderKey>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl ProviderKeys {
    pub fn from_env(router_config: &Arc<RouterConfig>) -> Self {
        tracing::debug!("Discovering provider keys");
        let balance_config = &router_config.load_balance;
        let mut keys = HashMap::default();
        let providers = balance_config.providers();

        for provider in providers {
            if provider == InferenceProvider::Ollama {
                // ollama doesn't require an API key
                continue;
            }
            if let Some(key) = ProviderKey::from_env(&provider) {
                keys.insert(provider.clone(), key);
            }
        }

        Self(Arc::new(keys))
    }

    pub fn from_env_direct_proxy(
        providers_config: &ProvidersConfig,
    ) -> Result<Self, ProviderError> {
        let keys = providers_config
            .iter()
            .filter_map(|(provider, _)| {
                ProviderKey::from_env(provider).map(|key| {
                    tracing::debug!(provider = %provider, "got llm provider key");
                    (provider.clone(), key)
                })
            })
            .collect();

        Ok(Self(Arc::new(keys)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inference_provider_as_ref() {
        let named_provider = InferenceProvider::Named("test".into());
        let named_provider_str = named_provider.as_ref();
        assert_eq!("test", named_provider_str);
    }

    #[test]
    fn inference_provider_to_string() {
        let named_provider = InferenceProvider::Named("test".into());
        let named_provider_str = named_provider.to_string();
        assert_eq!("test", named_provider_str);
    }
}
