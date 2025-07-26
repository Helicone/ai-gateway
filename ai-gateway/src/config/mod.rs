pub mod balance;
pub mod cache;
pub mod control_plane;
pub mod database;
pub mod deployment_target;
pub mod discover;
pub mod dispatcher;
pub mod helicone;
pub mod minio;
pub mod model_mapping;
pub mod monitor;
pub mod providers;
pub mod rate_limit;
pub mod redis;
pub mod response_headers;
pub mod retry;
pub mod router;
pub mod server;
pub mod validation;
use std::path::PathBuf;

use config::ConfigError;
use displaydoc::Display;
use json_patch::merge;
use regex::Regex;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use url::Url;

use crate::{
    error::init::InitError,
    types::{provider::InferenceProvider, secret::Secret},
};

const ROUTER_ID_REGEX: &str = r"^[A-Za-z0-9_-]{1,12}$";
const DEFAULT_CONFIG_PATH: &str = "/etc/ai-gateway/config.yaml";

#[derive(Debug, Error, Display)]
pub enum Error {
    /// error collecting config sources: {0}
    Source(#[from] ConfigError),
    /// deserialization error for input config: {0}
    InputConfigDeserialization(#[from] serde_path_to_error::Error<ConfigError>),
    /// deserialization error for merged config: {0}
    MergedConfigDeserialization(
        #[from] serde_path_to_error::Error<serde_json::Error>,
    ),
    /// URL parsing error: {0}
    UrlParse(#[from] url::ParseError),
}

#[derive(Debug, Default, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct MiddlewareConfig {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache: Option<self::cache::CacheConfig>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rate_limit: Option<self::rate_limit::RateLimitConfig>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub retries: Option<self::retry::RetryConfig>,
}

#[derive(Debug, Default, Deserialize, Serialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct Config {
    pub telemetry: telemetry::Config,
    pub server: self::server::ServerConfig,
    pub minio: self::minio::Config,
    pub database: self::database::DatabaseConfig,
    pub dispatcher: self::dispatcher::DispatcherConfig,
    pub discover: self::discover::DiscoverConfig,
    pub response_headers: self::response_headers::ResponseHeadersConfig,
    pub deployment_target: self::deployment_target::DeploymentTarget,
    pub control_plane: self::control_plane::ControlPlaneConfig,

    /// If a request is made with a model that is not in the `RouterConfig`
    /// model mapping, then we fallback to this.
    pub default_model_mapping: self::model_mapping::ModelMappingConfig,
    pub helicone: self::helicone::HeliconeConfig,
    /// *ALL* supported providers, independent of router configuration.
    pub providers: self::providers::ProvidersConfig,

    #[serde(skip_serializing_if = "Option::is_none")]
    pub cache_store: Option<self::cache::CacheStore>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rate_limit_store: Option<self::rate_limit::RateLimitStore>,
    /// Global middleware configuration, e.g. rate limiting, caching, etc.
    ///
    /// This configuration will be for middleware that is applied to ALL
    /// routes on the application, and will run before any other
    /// router or unified-api-specific middleware.
    pub global: MiddlewareConfig,
    /// Middleware configuration for the unified API.
    ///
    /// This configuration will be for middleware that is applied to ALL
    /// requests to the unified API (`/ai`)
    pub unified_api: MiddlewareConfig,
    pub routers: self::router::RouterConfigs,
}

impl Config {
    pub fn try_read(
        config_file_path: Option<PathBuf>,
    ) -> Result<Self, Box<Error>> {
        let mut default_config = serde_json::to_value(Self::default())
            .expect("default config is serializable");
        let mut builder = config::Config::builder();
        if let Some(path) = config_file_path {
            builder = builder.add_source(config::File::from(path));
        } else if std::fs::exists(DEFAULT_CONFIG_PATH).unwrap_or_default() {
            builder = builder.add_source(config::File::from(PathBuf::from(
                DEFAULT_CONFIG_PATH,
            )));
        }
        builder = builder.add_source(
            config::Environment::with_prefix("AI_GATEWAY")
                .try_parsing(true)
                .separator("__")
                .convert_case(config::Case::Kebab),
        );
        let input_config: serde_json::Value = builder
            .build()
            .map_err(Error::from)
            .map_err(Box::new)?
            .try_deserialize()
            .map_err(Error::from)
            .map_err(Box::new)?;
        merge(&mut default_config, &input_config);

        let mut config: Config =
            serde_path_to_error::deserialize(default_config)
                .map_err(Error::from)
                .map_err(Box::new)?;

        // HACK: for secret fields in the **`Config`** struct that don't follow
        // the       `AI_GATEWAY` prefix + the double underscore
        // separator (`__`) format.
        //
        //       Right now, that only applies to
        // `HELICONE_CONTROL_PLANE_API_KEY`,       provider keys also
        // have their own format, but **they are not fields in       the
        // `Config` struct**.
        //
        //       It was intentional to allow the
        // `HELICONE_CONTROL_PLANE_API_KEY` naming       for better
        // clarity, as the `AI_GATEWAY__HELICONE_OBSERVABILITY__API_KEY`
        //       version is very verbose and confusing.
        //
        //       The bug here is that due to the Serialize impl, when we do
        //       `serde_json::to_value(Self::default())` above, we get the
        // literal value       `*****` rather than the secret we want.
        //
        //       The fix therefore is just to re-read the value from the
        // environment       after serializing. This is only needed to
        // be done here in this one place,       there aren't any other
        // functions where we merge configs like we do here.
        if let Ok(helicone_control_plane_api_key) =
            std::env::var("HELICONE_CONTROL_PLANE_API_KEY")
        {
            config.helicone.api_key =
                Secret::from(helicone_control_plane_api_key);
        }

        if let Ok(bedrock_region) = std::env::var("AWS_REGION")
            && let Some(bedrock_provider) =
                config.providers.get_mut(&InferenceProvider::Bedrock)
        {
            let bedrock_url = format!(
                "https://bedrock-runtime.{bedrock_region}.amazonaws.com"
            );
            bedrock_provider.base_url =
                Url::parse(&bedrock_url).map_err(Error::UrlParse)?;
        }

        Ok(config)
    }

    pub fn validate(&self) -> Result<(), InitError> {
        let router_id_regex =
            Regex::new(ROUTER_ID_REGEX).expect("always valid if tests pass");
        for (router_id, router_config) in self.routers.as_ref() {
            router_config.validate()?;
            if !router_id_regex.is_match(router_id.as_ref()) {
                return Err(InitError::InvalidRouterId(router_id.to_string()));
            }
        }
        // TODO: merged configs make this brittle. bring it back after we've
        // improved that self.validate_model_mappings()?;
        Ok(())
    }
}

#[cfg(feature = "testing")]
impl crate::tests::TestDefault for Config {
    fn test_default() -> Self {
        let telemetry = telemetry::Config {
            exporter: telemetry::Exporter::Stdout,
            level: "info,ai_gateway=trace".to_string(),
            ..Default::default()
        };
        Config {
            telemetry,
            server: self::server::ServerConfig::test_default(),
            minio: self::minio::Config::test_default(),
            database: self::database::DatabaseConfig::test_default(),
            dispatcher: self::dispatcher::DispatcherConfig::test_default(),
            control_plane: self::control_plane::ControlPlaneConfig::default(),
            default_model_mapping:
                self::model_mapping::ModelMappingConfig::default(),
            global: MiddlewareConfig::default(),
            unified_api: MiddlewareConfig::default(),
            providers: self::providers::ProvidersConfig::default(),
            helicone: self::helicone::HeliconeConfig::test_default(),
            deployment_target:
                self::deployment_target::DeploymentTarget::Sidecar,
            discover: self::discover::DiscoverConfig::test_default(),
            cache_store: Some(self::cache::CacheStore::default()),
            rate_limit_store: Some(self::rate_limit::RateLimitStore::default()),
            routers: self::router::RouterConfigs::test_default(),
            response_headers:
                self::response_headers::ResponseHeadersConfig::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use super::*;
    use crate::config::deployment_target::DeploymentTarget;

    #[test]
    fn router_id_regex_is_valid() {
        assert!(Regex::new(ROUTER_ID_REGEX).is_ok());
    }

    #[test]
    fn default_config_is_serializable() {
        // if it doesn't panic, it's good
        let _config = serde_json::to_string(&Config::default())
            .expect("default config is serializable");
    }

    #[test]
    fn deployment_target_round_trip() {
        let config = DeploymentTarget::Sidecar;
        let serialized = serde_json::to_string(&config).unwrap();
        let deserialized =
            serde_json::from_str::<DeploymentTarget>(&serialized).unwrap();
        assert_eq!(config, deserialized);

        // Test Cloud variant
        let cloud_config = DeploymentTarget::Cloud {
            db_poll_interval: Duration::from_secs(60),
        };
        let serialized = serde_json::to_string(&cloud_config).unwrap();
        let deserialized =
            serde_json::from_str::<DeploymentTarget>(&serialized).unwrap();
        assert_eq!(cloud_config, deserialized);
    }

    #[test]
    fn router_id_regex_positive_cases() {
        let regex = Regex::new(ROUTER_ID_REGEX).unwrap();
        let valid_ids = [
            "a",
            "Z",
            "abc",
            "ABC",
            "A1B2",
            "A-1",
            "a_b",
            "abc_def",
            "0123456789",
            "123456789012", // 12 chars
            "a-b-c-d",
        ];
        for id in valid_ids {
            assert!(
                regex.is_match(id),
                "expected '{id}' to be valid according to ROUTER_ID_REGEX"
            );
        }
    }

    #[test]
    fn router_id_regex_negative_cases() {
        let regex = Regex::new(ROUTER_ID_REGEX).unwrap();
        let invalid_ids = [
            "",
            "with space",
            "special$",
            "1234567890123", // 13 chars
            "mixed*chars",
        ];
        for id in invalid_ids {
            assert!(
                !regex.is_match(id),
                "expected '{id}' to be invalid according to ROUTER_ID_REGEX"
            );
        }
    }

    // Individual field round trip tests
    #[test]
    fn telemetry_round_trip() {
        let config = Config::default();
        let serialized = serde_json::to_string(&config.telemetry).unwrap();
        let deserialized =
            serde_json::from_str::<telemetry::Config>(&serialized).unwrap();
        assert_eq!(config.telemetry, deserialized);
    }

    #[test]
    fn server_round_trip() {
        let config = Config::default();
        let serialized = serde_json::to_string(&config.server).unwrap();
        let deserialized =
            serde_json::from_str::<self::server::ServerConfig>(&serialized)
                .unwrap();
        assert_eq!(config.server, deserialized);
    }

    #[test]
    fn dispatcher_round_trip() {
        let config = Config::default();
        let serialized = serde_json::to_string(&config.dispatcher).unwrap();
        let deserialized = serde_json::from_str::<
            self::dispatcher::DispatcherConfig,
        >(&serialized)
        .unwrap();
        assert_eq!(config.dispatcher, deserialized);
    }

    #[test]
    fn discover_round_trip() {
        let config = Config::default();
        let serialized = serde_json::to_string(&config.discover).unwrap();
        let deserialized =
            serde_json::from_str::<self::discover::DiscoverConfig>(&serialized)
                .unwrap();
        assert_eq!(config.discover, deserialized);
    }

    #[test]
    fn response_headers_round_trip() {
        let config = Config::default();
        let serialized =
            serde_json::to_string(&config.response_headers).unwrap();
        let deserialized = serde_json::from_str::<
            self::response_headers::ResponseHeadersConfig,
        >(&serialized)
        .unwrap();
        assert_eq!(config.response_headers, deserialized);
    }

    #[test]
    fn deployment_target_field_round_trip() {
        let config = Config::default();
        let serialized =
            serde_json::to_string(&config.deployment_target).unwrap();
        let deserialized = serde_json::from_str::<
            self::deployment_target::DeploymentTarget,
        >(&serialized)
        .unwrap();
        assert_eq!(config.deployment_target, deserialized);
    }

    #[test]
    fn control_plane_round_trip() {
        let config = Config::default();
        let serialized = serde_json::to_string(&config.control_plane).unwrap();
        let deserialized = serde_json::from_str::<
            self::control_plane::ControlPlaneConfig,
        >(&serialized)
        .unwrap();
        assert_eq!(config.control_plane, deserialized);
    }

    #[test]
    fn default_model_mapping_round_trip() {
        let config = Config::default();
        let serialized =
            serde_json::to_string(&config.default_model_mapping).unwrap();
        let deserialized = serde_json::from_str::<
            self::model_mapping::ModelMappingConfig,
        >(&serialized)
        .unwrap();
        assert_eq!(config.default_model_mapping, deserialized);
    }

    #[test]
    fn providers_round_trip() {
        let config = Config::default();
        let serialized = serde_json::to_string(&config.providers).unwrap();
        let deserialized = serde_json::from_str::<
            self::providers::ProvidersConfig,
        >(&serialized)
        .unwrap();
        assert_eq!(config.providers, deserialized);
    }

    #[test]
    fn cache_store_round_trip() {
        let config = Config::default();
        let serialized = serde_json::to_string(&config.cache_store).unwrap();
        let deserialized = serde_json::from_str::<
            Option<self::cache::CacheStore>,
        >(&serialized)
        .unwrap();
        assert_eq!(config.cache_store, deserialized);
    }

    #[test]
    fn rate_limit_store_round_trip() {
        let config = Config::default();
        let serialized =
            serde_json::to_string(&config.rate_limit_store).unwrap();
        let deserialized = serde_json::from_str::<
            Option<self::rate_limit::RateLimitStore>,
        >(&serialized)
        .unwrap();
        assert_eq!(config.rate_limit_store, deserialized);
    }

    #[test]
    fn global_middleware_round_trip() {
        let config = Config::default();
        let serialized = serde_json::to_string(&config.global).unwrap();
        let deserialized =
            serde_json::from_str::<MiddlewareConfig>(&serialized).unwrap();
        assert_eq!(config.global, deserialized);
    }

    #[test]
    fn unified_api_middleware_round_trip() {
        let config = Config::default();
        let serialized = serde_json::to_string(&config.unified_api).unwrap();
        let deserialized =
            serde_json::from_str::<MiddlewareConfig>(&serialized).unwrap();
        assert_eq!(config.unified_api, deserialized);
    }

    #[test]
    fn routers_round_trip() {
        let config = Config::default();
        let serialized = serde_json::to_string(&config.routers).unwrap();
        let deserialized =
            serde_json::from_str::<self::router::RouterConfigs>(&serialized)
                .unwrap();
        assert_eq!(config.routers, deserialized);
    }

    #[test]
    fn secret_serialization_behavior() {
        // This test demonstrates why configs with Secret fields fail round-trip
        // serialization
        use crate::types::secret::Secret;

        #[derive(Debug, PartialEq, serde::Serialize, serde::Deserialize)]
        struct TestConfig {
            secret_field: Secret<String>,
        }

        let original = TestConfig {
            secret_field: Secret::from("my-secret-value".to_string()),
        };

        // Serialize the config
        let serialized = serde_json::to_string(&original).unwrap();
        println!("Serialized: {}", serialized);

        // The serialized form will be: {"secret_field":"*****"}
        assert!(serialized.contains("*****"));

        // Deserializing succeeds but with "*****" as the new value
        let deserialized =
            serde_json::from_str::<TestConfig>(&serialized).unwrap();

        // The values won't be equal because the secret is now "*****" instead
        // of "my-secret-value"
        assert_ne!(
            original, deserialized,
            "Round-trip fails because secret value is lost"
        );

        // To verify, let's check the exposed value
        assert_eq!(deserialized.secret_field.expose(), "*****");
        assert_ne!(
            original.secret_field.expose(),
            deserialized.secret_field.expose()
        );
    }
}
