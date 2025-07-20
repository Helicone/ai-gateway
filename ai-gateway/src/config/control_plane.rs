use std::time::Duration;

use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::config::retry::RetryConfig;

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields, rename_all = "kebab-case")]
pub struct ControlPlaneConfig {
    pub retry: RetryConfig,
}

impl Default for ControlPlaneConfig {
    fn default() -> Self {
        Self {
            retry: RetryConfig::Exponential {
                min_delay: Duration::from_secs(2),
                max_delay: Duration::from_secs(60),
                max_retries: 15,
                factor: Decimal::from(2),
            },
        }
    }
}
