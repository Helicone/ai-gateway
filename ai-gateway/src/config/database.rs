use std::time::Duration;

use serde::{Deserialize, Serialize};

use crate::types::secret::Secret;

const DEFAULT_DATABASE_URL: &str =
    "postgres://postgres:postgres@localhost:54322/postgres";

#[derive(Debug, Clone, Deserialize, Serialize, Eq, PartialEq, Hash)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct DatabaseConfig {
    /// Database connection URL.
    /// set via env vars: `AI_GATEWAY__DATABASE__URL`
    #[serde(default = "default_url")]
    pub url: Secret<String>,
    /// Connection timeout for database operations.
    /// set via env vars: `AI_GATEWAY__DATABASE__CONNECTION_TIMEOUT`
    #[serde(with = "humantime_serde", default = "default_connection_timeout")]
    pub connection_timeout: Duration,
    /// Maximum number of connections in the pool.
    /// set via env vars: `AI_GATEWAY__DATABASE__MAX_CONNECTIONS`
    #[serde(default = "default_max_connections")]
    pub max_connections: u32,
    /// Minimum number of connections in the pool.
    /// set via env vars: `AI_GATEWAY__DATABASE__MIN_CONNECTIONS`
    #[serde(default = "default_min_connections")]
    pub min_connections: u32,
    /// Timeout for acquiring a connection from the pool.
    /// set via env vars: `AI_GATEWAY__DATABASE__ACQUIRE_TIMEOUT`
    #[serde(with = "humantime_serde", default = "default_acquire_timeout")]
    pub acquire_timeout: Duration,
    /// Timeout for idle connections in the pool.
    /// set via env vars: `AI_GATEWAY__DATABASE__IDLE_TIMEOUT`
    #[serde(with = "humantime_serde", default = "default_idle_timeout")]
    pub idle_timeout: Duration,
    /// Maximum lifetime of connections in the pool.
    /// set via env vars: `AI_GATEWAY__DATABASE__MAX_LIFETIME`
    #[serde(with = "humantime_serde", default = "default_max_lifetime")]
    pub max_lifetime: Duration,
}

impl Default for DatabaseConfig {
    fn default() -> Self {
        Self {
            url: default_url(),
            connection_timeout: default_connection_timeout(),
            max_connections: default_max_connections(),
            min_connections: default_min_connections(),
            acquire_timeout: default_acquire_timeout(),
            idle_timeout: default_idle_timeout(),
            max_lifetime: default_max_lifetime(),
        }
    }
}

fn default_url() -> Secret<String> {
    Secret::from(DEFAULT_DATABASE_URL.to_string())
}

fn default_connection_timeout() -> Duration {
    Duration::from_secs(10)
}

fn default_max_connections() -> u32 {
    10
}

fn default_min_connections() -> u32 {
    0
}

fn default_acquire_timeout() -> Duration {
    Duration::from_secs(5)
}

fn default_idle_timeout() -> Duration {
    Duration::from_secs(600) // 10 minutes
}

fn default_max_lifetime() -> Duration {
    Duration::from_secs(1800) // 30 minutes
}

#[cfg(feature = "testing")]
impl crate::tests::TestDefault for DatabaseConfig {
    fn test_default() -> Self {
        use crate::types::secret::Secret;

        Self {
            url: Secret::from(DEFAULT_DATABASE_URL.to_string()),
            connection_timeout: Duration::from_secs(5),
            max_connections: 5,
            min_connections: 0,
            acquire_timeout: Duration::from_secs(10),
            idle_timeout: Duration::from_secs(300),
            max_lifetime: Duration::from_secs(900),
        }
    }
}
