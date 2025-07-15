use std::collections::HashMap;

use derive_more::{AsRef, From};
use indexmap::IndexSet;
use nonempty_collections::{NESet, nes};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::{
    endpoints::EndpointType,
    types::{model_id::ModelId, provider::InferenceProvider},
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
    pub fn providers(&self) -> IndexSet<InferenceProvider> {
        self.0
            .values()
            .flat_map(BalanceConfigInner::providers)
            .collect()
    }
}

/// Configurations which drive the strategy used for the
/// routing/load balancing done by the
/// [`RoutingStrategyService`](crate::router::strategy::RoutingStrategyService).
///
/// See the rustdocs there for more details.
#[derive(Debug, Clone, Deserialize, Serialize, Eq, PartialEq)]
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
    ModelLatency { models: NESet<ModelId> },
}

impl BalanceConfigInner {
    #[must_use]
    pub fn providers(&self) -> IndexSet<InferenceProvider> {
        match self {
            Self::ProviderWeighted { providers } => {
                providers.iter().map(|t| t.provider.clone()).collect()
            }
            Self::BalancedLatency { providers } => {
                providers.iter().cloned().collect()
            }
            Self::ModelWeighted { models } => models
                .iter()
                .filter_map(|model| {
                    if let Some(provider) = model.model.inference_provider() { Some(provider) } else {
                        tracing::warn!(model = ?model.model, "Model has no inference provider");
                        None
                    }
                })
                .collect(),
            Self::ModelLatency { models } => models
                .iter()
                .filter_map(|model| {
                    if let Some(provider) = model.inference_provider() { Some(provider) } else {
                        tracing::warn!(model = ?model, "Model has no inference provider");
                        None
                    }
                })
                .collect(),
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
