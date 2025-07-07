use serde::{Deserialize, Serialize};
use url::Url;

use crate::types::secret::Secret;

#[derive(
    Default, Debug, Clone, Deserialize, Serialize, PartialEq, Eq, Hash,
)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub enum HeliconeFeatures {
    /// No features enabled
    ///
    /// **Note:** this means no authentication checks, so any request to the
    /// gateway will be able to use your provider API keys!
    #[default]
    None,
    /// Authentication only.
    Auth,
    /// Authentication and observability.
    Observability,
    /// Authentication and prompts.
    Prompts,
    /// Authentication, observability, and prompts.
    All,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "kebab-case", deny_unknown_fields)]
pub struct HeliconeConfig {
    /// The API key to authenticate the AI Gateway to the Helicone control
    /// plane.
    #[serde(default = "default_api_key")]
    pub api_key: Secret<String>,
    /// The base URL of Helicone.
    #[serde(default = "default_base_url")]
    pub base_url: Url,
    /// The websocket URL of the Helicone control plane.
    #[serde(default = "default_websocket_url")]
    pub websocket_url: Url,
    /// The mode of Helicone features to enable.
    #[serde(default)]
    pub features: HeliconeFeatures,
}

impl HeliconeConfig {
    #[must_use]
    pub fn is_auth_enabled(&self) -> bool {
        self.features != HeliconeFeatures::None
    }

    #[must_use]
    pub fn is_auth_disabled(&self) -> bool {
        self.features == HeliconeFeatures::None
    }

    #[must_use]
    pub fn is_observability_enabled(&self) -> bool {
        self.features == HeliconeFeatures::All
            || self.features == HeliconeFeatures::Observability
    }

    #[must_use]
    pub fn is_prompts_enabled(&self) -> bool {
        self.features == HeliconeFeatures::All
            || self.features == HeliconeFeatures::Prompts
    }
}

impl Default for HeliconeConfig {
    fn default() -> Self {
        Self {
            api_key: default_api_key(),
            base_url: default_base_url(),
            websocket_url: default_websocket_url(),
            features: HeliconeFeatures::None,
        }
    }
}

fn default_api_key() -> Secret<String> {
    Secret::from(
        std::env::var("HELICONE_CONTROL_PLANE_API_KEY")
            .unwrap_or("sk-helicone-...".to_string()),
    )
}

fn default_base_url() -> Url {
    "https://api.helicone.ai".parse().unwrap()
}

fn default_websocket_url() -> Url {
    "wss://api.helicone.ai/ws/v1/router/control-plane"
        .parse()
        .unwrap()
}

#[cfg(feature = "testing")]
impl crate::tests::TestDefault for HeliconeConfig {
    fn test_default() -> Self {
        Self {
            base_url: "http://localhost:8585".parse().unwrap(),
            websocket_url: "ws://localhost:8585/ws/v1/router/control-plane"
                .parse()
                .unwrap(),
            features: HeliconeFeatures::All,
            api_key: default_api_key(),
        }
    }
}

// This manual deserialize impl is only required for backwards compatibility so
// that we can support the old `authentication` and `observability` boolean
// fields.
#[allow(clippy::too_many_lines)]
impl<'de> Deserialize<'de> for HeliconeConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        use std::fmt;

        use serde::de::{self, MapAccess, Visitor};

        #[derive(Deserialize)]
        #[serde(field_identifier, rename_all = "kebab-case")]
        enum Field {
            ApiKey,
            BaseUrl,
            WebsocketUrl,
            Features,
            Authentication,
            Observability,
            Prompts,
        }

        struct HeliconeConfigVisitor;

        impl<'de> Visitor<'de> for HeliconeConfigVisitor {
            type Value = HeliconeConfig;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("struct HeliconeConfig")
            }

            fn visit_map<V>(
                self,
                mut map: V,
            ) -> Result<HeliconeConfig, V::Error>
            where
                V: MapAccess<'de>,
            {
                let mut api_key = None;
                let mut base_url = None;
                let mut websocket_url = None;
                let mut features = None;
                let mut authentication = None;
                let mut observability = None;
                let mut prompts = None;

                while let Some(key) = map.next_key()? {
                    match key {
                        Field::ApiKey => {
                            if api_key.is_some() {
                                return Err(de::Error::duplicate_field(
                                    "api_key",
                                ));
                            }
                            api_key = Some(map.next_value()?);
                        }
                        Field::BaseUrl => {
                            if base_url.is_some() {
                                return Err(de::Error::duplicate_field(
                                    "base_url",
                                ));
                            }
                            base_url = Some(map.next_value()?);
                        }
                        Field::WebsocketUrl => {
                            if websocket_url.is_some() {
                                return Err(de::Error::duplicate_field(
                                    "websocket_url",
                                ));
                            }
                            websocket_url = Some(map.next_value()?);
                        }
                        Field::Features => {
                            if features.is_some() {
                                return Err(de::Error::duplicate_field(
                                    "features",
                                ));
                            }
                            features = Some(map.next_value()?);
                        }
                        Field::Authentication => {
                            if authentication.is_some() {
                                return Err(de::Error::duplicate_field(
                                    "authentication",
                                ));
                            }
                            authentication = Some(map.next_value()?);
                        }
                        Field::Observability => {
                            if observability.is_some() {
                                return Err(de::Error::duplicate_field(
                                    "observability",
                                ));
                            }
                            observability = Some(map.next_value()?);
                        }
                        Field::Prompts => {
                            if prompts.is_some() {
                                return Err(de::Error::duplicate_field(
                                    "prompts",
                                ));
                            }
                            prompts = Some(map.next_value()?);
                        }
                    }
                }

                // Determine features precedence:
                // 1. If features is set, use it.
                // 2. Otherwise, use authentication/observability/prompts
                //    booleans.
                // 3. Otherwise, default to None.

                let features = if let Some(f) = features {
                    f
                } else {
                    match (authentication, observability, prompts) {
                        (_, Some(true), Some(true)) => HeliconeFeatures::All,
                        (_, Some(true), Some(false) | None) => {
                            HeliconeFeatures::Observability
                        }
                        (_, Some(false) | None, Some(true)) => {
                            HeliconeFeatures::Prompts
                        }
                        (
                            Some(true),
                            Some(false) | None,
                            Some(false) | None,
                        ) => HeliconeFeatures::Auth,
                        _ => HeliconeFeatures::None,
                    }
                };

                Ok(HeliconeConfig {
                    api_key: api_key.unwrap_or_else(default_api_key),
                    base_url: base_url.unwrap_or_else(default_base_url),
                    websocket_url: websocket_url
                        .unwrap_or_else(default_websocket_url),
                    features,
                })
            }
        }

        const FIELDS: &[&str] = &[
            "api_key",
            "base_url",
            "websocket_url",
            "features",
            "authentication",
            "observability",
            "prompts",
        ];
        deserializer.deserialize_struct(
            "HeliconeConfig",
            FIELDS,
            HeliconeConfigVisitor,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_deserialize_features_field_only() {
        let yaml = r#"
api-key: "sk-test-key"
base-url: "https://example.com"
websocket-url: "wss://example.com/ws"
features: "all"
"#;

        let config: HeliconeConfig = serde_yml::from_str(yaml).unwrap();
        assert_eq!(config.features, HeliconeFeatures::All);
    }

    #[test]
    fn test_deserialize_all_flags_true() {
        let yaml = r#"
api-key: "sk-test-key"
authentication: true
observability: true
prompts: true
"#;

        let config: HeliconeConfig = serde_yml::from_str(yaml).unwrap();
        assert_eq!(config.features, HeliconeFeatures::All);
    }

    #[test]
    fn test_deserialize_auth_true_others_false() {
        let yaml = r#"
api-key: "sk-test-key"
authentication: true
observability: false
prompts: false
"#;

        let config: HeliconeConfig = serde_yml::from_str(yaml).unwrap();
        assert_eq!(config.features, HeliconeFeatures::Auth);
    }

    #[test]
    fn test_deserialize_observability_true_others_false() {
        let yaml = r#"
api-key: "sk-test-key"
authentication: false
observability: true
prompts: false
"#;

        let config: HeliconeConfig = serde_yml::from_str(yaml).unwrap();
        assert_eq!(config.features, HeliconeFeatures::Observability);
    }

    #[test]
    fn test_deserialize_prompts_true_others_false() {
        let yaml = r#"
api-key: "sk-test-key"
authentication: false
observability: false
prompts: true
"#;

        let config: HeliconeConfig = serde_yml::from_str(yaml).unwrap();
        assert_eq!(config.features, HeliconeFeatures::Prompts);
    }

    #[test]
    fn test_deserialize_all_flags_false() {
        let yaml = r#"
api-key: "sk-test-key"
authentication: false
observability: false
prompts: false
"#;

        let config: HeliconeConfig = serde_yml::from_str(yaml).unwrap();
        assert_eq!(config.features, HeliconeFeatures::None);
    }

    #[test]
    fn test_deserialize_auth_true_only() {
        let yaml = r#"
api-key: "sk-test-key"
authentication: true
"#;

        let config: HeliconeConfig = serde_yml::from_str(yaml).unwrap();
        assert_eq!(config.features, HeliconeFeatures::Auth);
    }

    #[test]
    fn test_deserialize_observability_true_only() {
        let yaml = r#"
api-key: "sk-test-key"
observability: true
"#;

        let config: HeliconeConfig = serde_yml::from_str(yaml).unwrap();
        assert_eq!(config.features, HeliconeFeatures::Observability);
    }

    #[test]
    fn test_deserialize_prompts_true_only() {
        let yaml = r#"
api-key: "sk-test-key"
prompts: true
"#;

        let config: HeliconeConfig = serde_yml::from_str(yaml).unwrap();
        assert_eq!(config.features, HeliconeFeatures::Prompts);
    }

    #[test]
    fn test_deserialize_auth_false_only() {
        let yaml = r#"
api-key: "sk-test-key"
authentication: false
"#;

        let config: HeliconeConfig = serde_yml::from_str(yaml).unwrap();
        assert_eq!(config.features, HeliconeFeatures::None);
    }

    #[test]
    fn test_deserialize_observability_false_only() {
        let yaml = r#"
api-key: "sk-test-key"
observability: false
"#;

        let config: HeliconeConfig = serde_yml::from_str(yaml).unwrap();
        assert_eq!(config.features, HeliconeFeatures::None);
    }

    #[test]
    fn test_deserialize_prompts_false_only() {
        let yaml = r#"
api-key: "sk-test-key"
prompts: false
"#;

        let config: HeliconeConfig = serde_yml::from_str(yaml).unwrap();
        assert_eq!(config.features, HeliconeFeatures::None);
    }

    #[test]
    fn test_deserialize_no_feature_fields() {
        let yaml = r#"
api-key: "sk-test-key"
base-url: "https://example.com"
"#;

        let config: HeliconeConfig = serde_yml::from_str(yaml).unwrap();
        assert_eq!(config.features, HeliconeFeatures::None);
    }

    #[test]
    fn test_deserialize_features_takes_precedence() {
        let yaml = r#"
api-key: "sk-test-key"
features: "auth"
authentication: true
observability: true
prompts: true
"#;

        let config: HeliconeConfig = serde_yml::from_str(yaml).unwrap();
        // features field should take precedence over auth/observability/prompts
        assert_eq!(config.features, HeliconeFeatures::Auth);
    }

    #[test]
    fn test_deserialize_features_none_with_legacy_fields() {
        let yaml = r#"
api-key: "sk-test-key"
features: "none"
authentication: true
observability: true
prompts: true
"#;

        let config: HeliconeConfig = serde_yml::from_str(yaml).unwrap();
        // features field should take precedence
        assert_eq!(config.features, HeliconeFeatures::None);
    }

    #[test]
    fn test_deserialize_all_features_variants() {
        let test_cases = vec![
            ("none", HeliconeFeatures::None),
            ("auth", HeliconeFeatures::Auth),
            ("observability", HeliconeFeatures::Observability),
            ("prompts", HeliconeFeatures::Prompts),
            ("all", HeliconeFeatures::All),
        ];

        for (feature_str, expected_feature) in test_cases {
            let yaml = format!(
                r#"
api-key: "sk-test-key"
features: "{feature_str}"
"#
            );

            let config: HeliconeConfig = serde_yml::from_str(&yaml).unwrap();
            assert_eq!(
                config.features, expected_feature,
                "Failed for feature: {feature_str}"
            );
        }
    }

    #[test]
    fn test_helper_methods() {
        let auth_config = HeliconeConfig {
            features: HeliconeFeatures::Auth,
            ..Default::default()
        };
        assert!(auth_config.is_auth_enabled());
        assert!(!auth_config.is_auth_disabled());
        assert!(!auth_config.is_observability_enabled());

        let all_config = HeliconeConfig {
            features: HeliconeFeatures::All,
            ..Default::default()
        };
        assert!(all_config.is_auth_enabled());
        assert!(!all_config.is_auth_disabled());
        assert!(all_config.is_observability_enabled());

        let none_config = HeliconeConfig {
            features: HeliconeFeatures::None,
            ..Default::default()
        };
        assert!(!none_config.is_auth_enabled());
        assert!(none_config.is_auth_disabled());
        assert!(!none_config.is_observability_enabled());
    }
}
