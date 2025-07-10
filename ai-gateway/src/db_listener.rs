use futures::future::BoxFuture;
use meltdown::Token;
use serde::{Deserialize, Serialize};
use sqlx::{
    PgPool,
    postgres::{PgListener, PgPoolOptions},
};
use tracing::{debug, error, info};

use crate::{
    app_state::AppState,
    config::router::RouterConfig,
    error::{init::InitError, runtime::RuntimeError},
    types::router::RouterId,
};

/// A database listener service that handles LISTEN/NOTIFY functionality.
/// This service runs in the background and can be registered with meltdown.
#[derive(Debug, Clone)]
pub struct DatabaseListener {
    pool: PgPool,
    app_state: AppState,
}

#[derive(Debug, Deserialize, Serialize)]
enum Op {
    #[serde(rename = "INSERT")]
    Insert,
    #[serde(rename = "UPDATE")]
    Update,
    #[serde(rename = "DELETE")]
    Delete,
    #[serde(rename = "TRUNCATE")]
    Truncate,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "event")]
enum ConnectedCloudGatewaysNotification {
    #[serde(rename = "router_config_updated")]
    RouterConfigUpdated {
        router_id: RouterId,
        router_config_id: String,
        organization_id: String,
        version: String,
        op: Op,
        config: Box<RouterConfig>,
    },
    #[serde(rename = "router_keys_updated")]
    RouterKeysUpdated {
        router_id: RouterId,
        organization_id: String,
        api_key_hash: String,
        op: Op,
    },
    #[serde(rename = "unknown")]
    Unknown {
        #[serde(flatten)]
        data: serde_json::Value,
    },
}

const MAX_CHANNEL_CAPACITY: usize = 100;

impl DatabaseListener {
    pub async fn new(app_state: AppState) -> Result<Self, InitError> {
        let config = app_state.0.config.database.clone();
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
        Ok(Self { pool, app_state })
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

        // Listen for notifications on a channel (you can customize this)
        listener.listen("connected_cloud_gateways").await.map_err(|e| {
            error!(error = %e, "failed to listen on database notification channel");
            RuntimeError::Internal(crate::error::internal::InternalError::Internal)
        })?;

        let (tx, rx) = tokio::sync::mpsc::channel(MAX_CHANNEL_CAPACITY);
        self.app_state.set_router_rx(rx).await;

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

        if notification.channel() == "connected_cloud_gateways" {
            let payload: ConnectedCloudGatewaysNotification =
                serde_json::from_str(notification.payload()).unwrap();

            match payload {
                ConnectedCloudGatewaysNotification::RouterConfigUpdated {
                    router_id,
                    router_config_id,
                    organization_id,
                    version,
                    op,
                    config,
                } => {
                    info!("Router configuration updated");
                    info!("router_id: {}", router_id);
                    info!("router_config_id: {}", router_config_id);
                    info!("organization_id: {}", organization_id);
                    info!("version: {}", version);
                    info!("op: {:?}", op);
                    info!("config: {:?}", config);
                    // TODO: Handle router configuration update
                }
                ConnectedCloudGatewaysNotification::RouterKeysUpdated {
                    router_id,
                    organization_id,
                    api_key_hash,
                    op,
                } => {
                    info!("Router keys updated");
                    info!("router_id: {}", router_id);
                    info!("organization_id: {}", organization_id);
                    info!("api_key_hash: {}", api_key_hash);
                    info!("op: {:?}", op);
                    // TODO: Handle router configuration deletion
                }
                ConnectedCloudGatewaysNotification::Unknown { data } => {
                    info!("Unknown notification event");
                    info!("data: {:?}", data);
                    // TODO: Handle unknown event
                }
            }
        } else {
            info!("received unknown notification");
        }

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
            Ok(())
        })
    }
}
