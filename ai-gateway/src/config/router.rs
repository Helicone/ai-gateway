use std::collections::HashMap;

use derive_more::{AsMut, AsRef};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use super::{
    balance::{BalanceConfig, BalanceConfigInner},
    model_mapping::ModelMappingConfig,
    retry::RetryConfig,
};
use crate::{
    config::{cache::CacheConfig, rate_limit::RateLimitConfig},
    error::init::InitError,
    types::router::RouterId,
};

#[derive(
    Debug, Default, Clone, Deserialize, Serialize, Eq, PartialEq, AsRef, AsMut,
)]
pub struct RouterConfigs(HashMap<RouterId, RouterConfig>);

impl RouterConfigs {
    #[must_use]
    pub fn new(configs: HashMap<RouterId, RouterConfig>) -> Self {
        Self(configs)
    }
}

impl std::ops::Deref for RouterConfigs {
    type Target = HashMap<RouterId, RouterConfig>;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[derive(Debug, Default, Clone, Deserialize, Serialize, Eq, PartialEq)]
#[serde(default, rename_all = "kebab-case")]
pub struct RouterConfig {
    pub load_balance: BalanceConfig,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub model_mappings: Option<ModelMappingConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache: Option<CacheConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retries: Option<RetryConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate_limit: Option<RateLimitConfig>,
}

impl RouterConfig {
    pub fn validate(&self) -> Result<(), InitError> {
        for balance_config in self.load_balance.0.values() {
            match balance_config {
                BalanceConfigInner::ProviderWeighted { providers } => {
                    let total =
                        providers.iter().map(|t| t.weight).sum::<Decimal>();
                    if total != Decimal::from(1) {
                        return Err(InitError::InvalidBalancer(format!(
                            "Balance weights dont sum to 1: {total}"
                        )));
                    }
                }
                BalanceConfigInner::ModelWeighted { models } => {
                    let total =
                        models.iter().map(|m| m.weight).sum::<Decimal>();
                    if total != Decimal::from(1) {
                        return Err(InitError::InvalidBalancer(format!(
                            "Balance weights dont sum to 1: {total}"
                        )));
                    }
                }
                BalanceConfigInner::BalancedLatency { .. }
                | BalanceConfigInner::ModelLatency { .. } => {}
            }
        }

        Ok(())
    }

    #[must_use]
    pub fn model_mappings(&self) -> Option<&ModelMappingConfig> {
        self.model_mappings.as_ref()
    }
}

#[cfg(feature = "testing")]
impl crate::tests::TestDefault for RouterConfigs {
    fn test_default() -> Self {
        Self(HashMap::from([(
            RouterId::Named(compact_str::CompactString::new("my-router")),
            RouterConfig {
                model_mappings: None,
                cache: None,
                load_balance: BalanceConfig(HashMap::from([(
                    crate::endpoints::EndpointType::Chat,
                    BalanceConfigInner::BalancedLatency {
                        providers: nonempty_collections::nes![
                            crate::types::provider::InferenceProvider::OpenAI
                        ],
                    },
                )])),
                retries: None,
                rate_limit: None,
            },
        )]))
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::config::cache::CacheConfig;

    fn test_router_config() -> RouterConfig {
        let cache = CacheConfig {
            directive: Some("max-age=3600, max-stale=1800".to_string()),
            buckets: 10,
            seed: Some("test-seed".to_string()),
        };

        let balance = BalanceConfig::default();
        let retries = RetryConfig::Exponential {
            min_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(10),
            max_retries: 3,
            factor: Decimal::from(2),
        };

        RouterConfig {
            model_mappings: None,
            cache: Some(cache),
            load_balance: balance,
            retries: Some(retries),
            rate_limit: None,
        }
    }

    #[test]
    fn router_config_round_trip() {
        let config = test_router_config();
        let serialized = serde_json::to_string(&config).unwrap();
        let deserialized =
            serde_json::from_str::<RouterConfig>(&serialized).unwrap();
        assert_eq!(config, deserialized);
    }

    #[test]
    fn router_configs_round_trip() {
        let config = RouterConfigs::default();
        let serialized = serde_json::to_string(&config).unwrap();
        let deserialized =
            serde_json::from_str::<RouterConfigs>(&serialized).unwrap();
        assert_eq!(config, deserialized);
    }
}
