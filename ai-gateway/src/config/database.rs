use std::time::Duration;

use futures::future::BoxFuture;
use meltdown::Token;
use serde::{Deserialize, Serialize};
use sqlx::{
    PgPool,
    postgres::{PgListener, PgPoolOptions},
};
use tracing::{debug, error, info};

use crate::error::{init::InitError, runtime::RuntimeError};

const DEFAULT_DATABASE_URL: &str =
    "postgres://postgres:postgres@localhost:54322/postgres";

#[derive(Debug, Clone, Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct DatabaseConfig {
    /// Database connection URL.
    /// set via env vars: `AI_GATEWAY__DATABASE__URL`
    #[serde(default = "default_url")]
    pub url: String,
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

fn default_url() -> String {
    DEFAULT_DATABASE_URL.to_string()
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
    Duration::from_secs(30)
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
        Self {
            url: DEFAULT_DATABASE_URL.to_string(),
            connection_timeout: Duration::from_secs(5),
            max_connections: 5,
            min_connections: 0,
            acquire_timeout: Duration::from_secs(10),
            idle_timeout: Duration::from_secs(300),
            max_lifetime: Duration::from_secs(900),
        }
    }
}

/// A database listener service that handles LISTEN/NOTIFY functionality.
/// This service runs in the background and can be registered with meltdown.
#[derive(Debug, Clone)]
pub struct DatabaseListener {
    pool: PgPool,
}

impl DatabaseListener {
    pub async fn new(config: DatabaseConfig) -> Result<Self, InitError> {
        let pool = PgPoolOptions::new()
            .max_connections(config.max_connections)
            .min_connections(config.min_connections)
            .acquire_timeout(config.acquire_timeout)
            .idle_timeout(config.idle_timeout)
            .max_lifetime(config.max_lifetime)
            .connect(&config.url)
            .await
            .map_err(|e| {
                error!(error = %e, "failed to create database pool");
                InitError::DatabaseConnection(e)
            })?;
        Ok(Self { pool })
    }

    /// Runs the database listener service.
    /// This includes listening for notifications and handling
    /// connection health.
    async fn run_service(&mut self) -> Result<(), RuntimeError> {
        info!("starting database listener service");

        // Create listener for LISTEN/NOTIFY
        let mut listener =
            PgListener::connect_with(&self.pool).await.map_err(|e| {
                error!(error = %e, "failed to create database listener");
                RuntimeError::Internal(
                    crate::error::internal::InternalError::Internal,
                )
            })?;

        info!("database listener initialized successfully");

        // Listen for notifications on a channel (you can customize this)
        listener.listen("connected_cloud_gateways").await.map_err(|e| {
            error!(error = %e, "failed to listen on database notification channel");
            RuntimeError::Internal(crate::error::internal::InternalError::Internal)
        })?;

        info!(
            "listening for database notifications on \
             'connected_cloud_gateways' channel"
        );

        // Process notifications
        loop {
            match listener.recv().await {
                Ok(notification) => {
                    debug!(
                        channel = notification.channel(),
                        payload = notification.payload(),
                        "received database notification"
                    );

                    // Handle the notification here
                    Self::handle_notification(&notification);
                }
                Err(e) => {
                    error!(error = %e, "error receiving database notification");
                    break;
                }
            }
        }

        Ok(())
    }

    /// Handles incoming database notifications.
    fn handle_notification(notification: &sqlx::postgres::PgNotification) {
        // Customize this method to handle different types of notifications
        info!(
            channel = notification.channel(),
            payload = notification.payload(),
            "processing notification"
        );

        // Example: You could dispatch to different handlers based on the
        // channel
        // TODO: Implement handle db listener
    }
}

impl meltdown::Service for DatabaseListener {
    type Future = BoxFuture<'static, Result<(), RuntimeError>>;

    fn run(mut self, mut token: Token) -> Self::Future {
        Box::pin(async move {
            tokio::select! {
                result = self.run_service() => {
                    if let Err(e) = result {
                        error!(error = %e, "database listener service encountered error, shutting down");
                    } else {
                        debug!("database listener service shut down successfully");
                    }
                    token.trigger();
                }
                () = &mut token => {
                    debug!("database listener service shutdown signal received");
                }
            }

            info!("database listener service shutting down");
            Ok(())
        })
    }
}
