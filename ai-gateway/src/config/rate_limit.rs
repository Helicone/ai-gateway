use std::{num::NonZeroU32, time::Duration};

use axum_core::response::IntoResponse;
use http::StatusCode;
use serde::{Deserialize, Serialize};
use tower_governor::governor::{GovernorConfig, GovernorConfigBuilder};

use crate::{
    config::redis::RedisConfig,
    error::{
        api::{ErrorDetails, ErrorResponse},
        init::InitError,
    },
    middleware::{
        mapper::openai::SERVER_ERROR_TYPE,
        rate_limit::extractor::RateLimitKeyExtractor,
    },
    types::json::Json,
};

pub type RateLimiterConfig = GovernorConfig<
    RateLimitKeyExtractor,
    governor::middleware::StateInformationMiddleware,
>;

#[derive(Debug, Default, Clone, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case")]
pub struct RateLimitConfig {
    /// If not set, the store from the rate-limit-store config will be
    /// used.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub store: Option<RateLimitStore>,
    #[serde(default, flatten)]
    pub limits: LimitsConfig,
}

pub(crate) fn limiter_config(
    limits: &LimitsConfig,
) -> Result<RateLimiterConfig, InitError> {
    let gcra = &limits.per_api_key;
    let per_cell_duration = gcra
        .refill_frequency
        .checked_div(gcra.capacity.into())
        .unwrap_or_else(|| {
            tracing::warn!(
                "fill_frequency is too small for capacity, using default fill \
                 frequency"
            );
            default_refill_frequency()
        });

    GovernorConfigBuilder::default()
        .period(per_cell_duration)
        .burst_size(gcra.capacity.get())
        .use_headers()
        .key_extractor(RateLimitKeyExtractor)
        .error_handler(|mut e| match &e {
            tower_governor::GovernorError::TooManyRequests { .. } => {
                tracing::debug!("rate limite exceeded");
                let governor_response: http::Response<String> = e.as_response();
                let (parts, _) = governor_response.into_parts();
                let body = ErrorResponse {
                    error: ErrorDetails {
                        message: "Too many requests".to_string(),
                        r#type: Some("rate_limit_exceeded".to_string()),
                        param: None,
                        code: None,
                    },
                };
                let json = Json(body);
                let openai_response = json.into_response();
                let (_, body) = openai_response.into_parts();
                http::Response::from_parts(parts, body)
            }
            tower_governor::GovernorError::UnableToExtractKey => {
                tracing::warn!(
                    "unable to extract key, rate limiting enabled without \
                     enabling authentication!!!"
                );
                let body = ErrorResponse {
                    error: ErrorDetails {
                        message: "Internal error, server misconfigured"
                            .to_string(),
                        r#type: Some(SERVER_ERROR_TYPE.to_string()),
                        param: None,
                        code: None,
                    },
                };
                let json = Json(body);
                (StatusCode::INTERNAL_SERVER_ERROR, json).into_response()
            }
            tower_governor::GovernorError::Other { code, msg, .. } => {
                tracing::error!(
                    msg = msg.as_deref().unwrap_or("Unknown error"),
                    "Other error"
                );
                let body = ErrorResponse {
                    error: ErrorDetails {
                        message: "Internal error".to_string(),
                        r#type: Some(SERVER_ERROR_TYPE.to_string()),
                        param: None,
                        code: None,
                    },
                };
                let json = Json(body);
                (*code, json).into_response()
            }
        })
        .finish()
        .ok_or(InitError::InvalidRateLimitConfig(
            "either burst size or period interval are zero",
        ))
}

#[derive(Debug, Default, Clone, Deserialize, Serialize, Eq, PartialEq)]
#[serde(rename_all = "kebab-case", tag = "type")]
pub enum RateLimitStore {
    #[default]
    InMemory,
    Redis(RedisConfig),
}

fn default_capacity() -> NonZeroU32 {
    NonZeroU32::new(500).unwrap()
}

pub(crate) fn default_refill_frequency() -> Duration {
    Duration::from_secs(1)
}

#[cfg(feature = "testing")]
impl crate::tests::TestDefault for RateLimitConfig {
    fn test_default() -> Self {
        Self {
            store: None,
            limits: LimitsConfig::test_default(),
        }
    }
}

#[cfg(feature = "testing")]
#[must_use]
pub fn config_enabled_for_test() -> RateLimitConfig {
    use crate::tests::TestDefault;
    RateLimitConfig {
        limits: LimitsConfig::test_default(),
        store: Some(RateLimitStore::InMemory),
    }
}

#[cfg(feature = "testing")]
#[must_use]
pub fn store_enabled_for_test_in_memory() -> RateLimitStore {
    RateLimitStore::InMemory
}

#[cfg(feature = "testing")]
#[must_use]
pub fn store_enabled_for_test_redis() -> RateLimitStore {
    use crate::types::secret::Secret;
    RateLimitStore::Redis(RedisConfig {
        host_url: Secret::from(
            "redis://localhost:6340".parse::<url::Url>().unwrap(),
        ),
        connection_timeout: Duration::from_secs(1),
    })
}

#[derive(Debug, Default, Clone, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct LimitsConfig {
    pub per_api_key: GcraConfig,
}

#[cfg(feature = "testing")]
impl crate::tests::TestDefault for LimitsConfig {
    fn test_default() -> Self {
        Self {
            per_api_key: GcraConfig::test_default(),
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct GcraConfig {
    /// The duration it takes to refill the entire rate limit quota.
    #[serde(with = "humantime_serde", default = "default_refill_frequency")]
    pub refill_frequency: Duration,
    /// The rate limit quota capacity.
    #[serde(default = "default_capacity")]
    pub capacity: NonZeroU32,
}

impl Default for GcraConfig {
    fn default() -> Self {
        Self {
            refill_frequency: default_refill_frequency(),
            capacity: default_capacity(),
        }
    }
}

#[cfg(feature = "testing")]
impl crate::tests::TestDefault for GcraConfig {
    fn test_default() -> Self {
        Self {
            refill_frequency: Duration::from_millis(500),
            capacity: NonZeroU32::new(3).unwrap(),
        }
    }
}
