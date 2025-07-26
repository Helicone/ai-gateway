use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::types::secret::Secret;

#[derive(Debug, Clone, Deserialize, Serialize, Eq, PartialEq, Hash)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct RedisConfig {
    #[serde(default = "default_url")]
    pub host_url: Secret<url::Url>,
    #[serde(with = "humantime_serde", default = "default_connection_timeout")]
    pub connection_timeout: Duration,
}

impl Default for RedisConfig {
    fn default() -> Self {
        Self {
            host_url: default_url(),
            connection_timeout: default_connection_timeout(),
        }
    }
}

fn default_url() -> Secret<url::Url> {
    Secret::from("redis://localhost:6379".parse::<url::Url>().unwrap())
}

fn default_connection_timeout() -> Duration {
    Duration::from_secs(1)
}
