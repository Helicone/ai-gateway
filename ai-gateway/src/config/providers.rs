use std::fmt;

use derive_more::{AsRef, Deref, DerefMut};
use indexmap::{IndexMap, IndexSet};
use serde::{
    Deserialize, Deserializer, Serialize, Serializer,
    de::{self, MapAccess, Visitor},
};
use url::Url;

use crate::types::{model_id::ModelId, provider::InferenceProvider};

const PROVIDERS_YAML: &str =
    include_str!("../../config/embedded/providers.yaml");
pub(crate) const DEFAULT_ANTHROPIC_VERSION: &str = "2023-06-01";

/// Global configuration for providers, shared across all routers.
///
/// For router-specific provider configuration, see [`RouterProviderConfig`]
#[derive(Debug, Clone, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub struct GlobalProviderConfig {
    /// NOTE: In the future we can delete the `model` field and
    /// instead load the models from the provider's respective APIs
    pub models: IndexSet<ModelId>,
    pub base_url: Url,
    #[serde(default)]
    pub version: Option<String>,
}

/// Map of *ALL* supported providers.
///
/// In order to configure subsets of providers use
#[derive(Debug, Clone, Eq, PartialEq, Deref, DerefMut, AsRef)]
pub struct ProvidersConfig(IndexMap<InferenceProvider, GlobalProviderConfig>);

impl<'de> Deserialize<'de> for ProvidersConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        struct ProvidersConfigVisitor;
        // Helper struct for deserializing the raw config
        #[derive(Deserialize)]
        #[serde(rename_all = "kebab-case")]
        struct RawGlobalProviderConfig {
            models: IndexSet<String>,
            base_url: Url,
            #[serde(default)]
            version: Option<String>,
        }

        impl<'de> Visitor<'de> for ProvidersConfigVisitor {
            type Value = ProvidersConfig;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str(
                    "a map of inference providers to their configuration",
                )
            }

            fn visit_map<V>(
                self,
                mut map: V,
            ) -> Result<ProvidersConfig, V::Error>
            where
                V: MapAccess<'de>,
            {
                let mut providers = IndexMap::new();

                while let Some(provider) =
                    map.next_key::<InferenceProvider>()?
                {
                    let raw_config: RawGlobalProviderConfig =
                        map.next_value()?;

                    // Convert model strings to ModelId using the provider
                    // context
                    let models = raw_config
                        .models
                        .into_iter()
                        .map(|model_str| {
                            ModelId::from_str_and_provider(
                                provider.clone(),
                                &model_str,
                            )
                            .map_err(|e| {
                                de::Error::custom(format!(
                                    "Invalid model '{model_str}' for provider \
                                     {provider}: {e}"
                                ))
                            })
                        })
                        .collect::<Result<IndexSet<_>, _>>()?;

                    let config = GlobalProviderConfig {
                        models,
                        base_url: raw_config.base_url,
                        version: raw_config.version,
                    };

                    providers.insert(provider, config);
                }

                Ok(ProvidersConfig(providers))
            }
        }

        deserializer.deserialize_map(ProvidersConfigVisitor)
    }
}

impl Serialize for ProvidersConfig {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        use serde::ser::SerializeMap;
        #[derive(Serialize)]
        #[serde(rename_all = "kebab-case")]
        struct SerializedGlobalProviderConfig {
            models: IndexSet<String>,
            base_url: Url,
            #[serde(skip_serializing_if = "Option::is_none")]
            version: Option<String>,
        }

        let mut map = serializer.serialize_map(Some(self.0.len()))?;

        for (provider, config) in &self.0 {
            // Create a temporary config with string model representations
            let models_as_strings: IndexSet<String> =
                config.models.iter().map(ToString::to_string).collect();

            let serialized_config = SerializedGlobalProviderConfig {
                models: models_as_strings,
                base_url: config.base_url.clone(),
                version: config.version.clone(),
            };

            map.serialize_entry(provider, &serialized_config)?;
        }

        map.end()
    }
}

impl FromIterator<(InferenceProvider, GlobalProviderConfig)>
    for ProvidersConfig
{
    fn from_iter<T>(iter: T) -> Self
    where
        T: IntoIterator<Item = (InferenceProvider, GlobalProviderConfig)>,
    {
        Self(IndexMap::from_iter(iter))
    }
}

impl Default for ProvidersConfig {
    fn default() -> Self {
        serde_yml::from_str(PROVIDERS_YAML).expect("Always valid if tests pass")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_providers_config_loads_from_yaml_string() {
        let _default_config = ProvidersConfig::default();
        // just want to make sure we don't panic...
    }

    #[test]
    fn test_providers_config_custom_deserialize() {
        use chrono::TimeZone;
        let yaml = r#"
openai:
  models:
    - "gpt-4"
    - "gpt-4-turbo"
    - "gpt-4o"
    - "gpt-4o-mini"
  base-url: https://api.openai.com
anthropic:
  models:
    - "claude-3-opus-20240229"
    - "claude-3-sonnet-20240229"
  base-url: https://api.anthropic.com
  version: "2023-06-01"
"#;

        let config: ProvidersConfig = serde_yml::from_str(yaml).unwrap();

        // Check OpenAI provider
        let openai_config = config.get(&InferenceProvider::OpenAI).unwrap();
        assert_eq!(openai_config.models.len(), 4);
        assert_eq!(openai_config.base_url.as_str(), "https://api.openai.com/");

        // Verify models are properly prefixed internally
        let model_ids: Vec<ModelId> =
            openai_config.models.clone().into_iter().collect();
        assert_eq!(
            model_ids[0],
            ModelId::ModelIdWithVersion {
                provider: InferenceProvider::OpenAI,
                id: crate::types::model_id::ModelIdWithVersion {
                    model: "gpt-4".to_string(),
                    version: crate::types::model_id::Version::ImplicitLatest,
                },
            }
        );
        // Check Anthropic provider
        let anthropic_config =
            config.get(&InferenceProvider::Anthropic).unwrap();
        assert_eq!(anthropic_config.models.len(), 2);
        let model_ids: Vec<ModelId> =
            anthropic_config.models.clone().into_iter().collect();
        let date =
            chrono::NaiveDate::parse_from_str("20240229", "%Y%m%d").unwrap();
        let naive_dt = date.and_hms_opt(0, 0, 0).unwrap();
        let date = chrono::Utc.from_utc_datetime(&naive_dt);
        assert_eq!(
            model_ids[0],
            ModelId::ModelIdWithVersion {
                provider: InferenceProvider::Anthropic,
                id: crate::types::model_id::ModelIdWithVersion {
                    model: "claude-3-opus".to_string(),
                    version: crate::types::model_id::Version::Date {
                        date,
                        format: "%Y%m%d",
                    },
                },
            }
        );
    }
}
