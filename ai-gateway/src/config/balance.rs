use std::collections::HashMap;

use derive_more::{AsRef, From};
use indexmap::IndexSet;
use nonempty_collections::{NEMap, NESet, nes};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::{
    endpoints::EndpointType,
    error::init::InitError,
    types::{
        model_id::{ModelId, ModelName},
        provider::InferenceProvider,
    },
};

/// A registry of balance configs for each endpoint type,
/// since a separate load balancer is used for each endpoint type.
#[derive(Debug, Clone, Deserialize, Serialize, Eq, PartialEq, AsRef, From)]
pub struct BalanceConfig(pub HashMap<EndpointType, BalanceConfigInner>);

impl Default for BalanceConfig {
    fn default() -> Self {
        Self(HashMap::from([(
            EndpointType::Chat,
            BalanceConfigInner::BalancedLatency {
                providers: nes![
                    InferenceProvider::OpenAI,
                    InferenceProvider::Anthropic,
                    InferenceProvider::GoogleGemini,
                ],
            },
        )]))
    }
}

impl BalanceConfig {
    #[cfg(any(test, feature = "testing"))]
    #[must_use]
    pub fn openai_chat() -> Self {
        Self(HashMap::from([(
            EndpointType::Chat,
            BalanceConfigInner::ProviderWeighted {
                providers: nes![WeightedProvider {
                    provider: InferenceProvider::OpenAI,
                    weight: Decimal::from(1),
                }],
            },
        )]))
    }

    #[cfg(any(test, feature = "testing"))]
    #[must_use]
    pub fn anthropic_chat() -> Self {
        Self(HashMap::from([(
            EndpointType::Chat,
            BalanceConfigInner::ProviderWeighted {
                providers: nes![WeightedProvider {
                    provider: InferenceProvider::Anthropic,
                    weight: Decimal::from(1),
                }],
            },
        )]))
    }

    #[cfg(any(test, feature = "testing"))]
    #[must_use]
    pub fn google_gemini() -> Self {
        Self(HashMap::from([(
            EndpointType::Chat,
            BalanceConfigInner::ProviderWeighted {
                providers: nes![WeightedProvider {
                    provider: InferenceProvider::GoogleGemini,
                    weight: Decimal::from(1),
                }],
            },
        )]))
    }

    #[cfg(any(test, feature = "testing"))]
    #[must_use]
    pub fn ollama_chat() -> Self {
        Self(HashMap::from([(
            EndpointType::Chat,
            BalanceConfigInner::ProviderWeighted {
                providers: nes![WeightedProvider {
                    provider: InferenceProvider::Ollama,
                    weight: Decimal::from(1),
                }],
            },
        )]))
    }

    #[cfg(any(test, feature = "testing"))]
    #[must_use]
    pub fn bedrock() -> Self {
        Self(HashMap::from([(
            EndpointType::Chat,
            BalanceConfigInner::ProviderWeighted {
                providers: nes![WeightedProvider {
                    provider: InferenceProvider::Bedrock,
                    weight: Decimal::from(1),
                }],
            },
        )]))
    }

    #[cfg(any(test, feature = "testing"))]
    #[must_use]
    pub fn mistral() -> Self {
        Self(HashMap::from([(
            EndpointType::Chat,
            BalanceConfigInner::ProviderWeighted {
                providers: nes![WeightedProvider {
                    provider: InferenceProvider::Named("mistral".into()),
                    weight: Decimal::from(1),
                }],
            },
        )]))
    }

    #[must_use]
    pub fn providers(&self) -> Result<IndexSet<InferenceProvider>, InitError> {
        let mut all_providers = IndexSet::new();
        for config in self.0.values() {
            let providers = config.providers()?;
            all_providers.extend(providers);
        }
        Ok(all_providers)
    }
}

/// Configurations which drive the strategy used for the
/// routing/load balancing done by the
/// [`RoutingStrategyService`](crate::router::strategy::RoutingStrategyService).
///
/// See the rustdocs there for more details.
#[derive(
    Debug, Clone, Deserialize, Serialize, Eq, PartialEq, strum::AsRefStr,
)]
#[strum(serialize_all = "kebab-case")]
#[serde(rename_all = "kebab-case", tag = "strategy")]
pub enum BalanceConfigInner {
    /// Distributes and load balances requests among a set of providers.
    #[serde(alias = "weighted")]
    ProviderWeighted { providers: NESet<WeightedProvider> },
    /// Distributes and load balances requests among a set of providers.
    /// This means there is an element of randomness in the selection of the
    /// provider, so generally requests will go to the provider with lowest
    /// latency, but not always.
    #[serde(alias = "latency")]
    BalancedLatency { providers: NESet<InferenceProvider> },
    /// Distributes and load balances requests among a set of (providers,model).
    ModelWeighted { models: NESet<WeightedModel> },
    /// Distributes and load balances requests among a set of (providers,model).
    ModelLatency {
        models: NEMap<ModelName<'static>, NESet<ModelId>>,
    },
}

impl BalanceConfigInner {
    #[must_use]
    pub fn providers(&self) -> Result<IndexSet<InferenceProvider>, InitError> {
        match self {
            Self::ProviderWeighted { providers } => {
                Ok(providers.iter().map(|t| t.provider.clone()).collect())
            }
            Self::BalancedLatency { providers } => {
                Ok(providers.iter().cloned().collect())
            }
            Self::ModelWeighted { models } => {
                let mut providers = IndexSet::new();
                for model in models {
                    if let Some(provider) = model.model.inference_provider() {
                        providers.insert(provider);
                    } else {
                        return Err(InitError::ModelIdNotRecognized(
                            model.model.to_string(),
                        ));
                    }
                }
                Ok(providers)
            }
            Self::ModelLatency { models } => {
                let mut providers = IndexSet::new();
                for (_model_name, model_ids) in models {
                    for model_id in model_ids {
                        if let Some(provider) = model_id.inference_provider() {
                            providers.insert(provider);
                        } else {
                            return Err(InitError::ModelIdNotRecognized(
                                model_id.to_string(),
                            ));
                        }
                    }
                }
                Ok(providers)
            }
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Eq, Hash, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub struct WeightedProvider {
    pub provider: InferenceProvider,
    pub weight: Decimal,
}

#[derive(Debug, Clone, Deserialize, Serialize, Eq, Hash, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub struct WeightedModel {
    pub model: ModelId,
    pub weight: Decimal,
}
