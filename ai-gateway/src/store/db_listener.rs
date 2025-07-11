use std::sync::Arc;

use futures::future::BoxFuture;
use meltdown::Token;
use serde::{Deserialize, Serialize};
use sqlx::{PgPool, postgres::PgListener};
use tokio::sync::mpsc::Sender;
use tower::discover::Change;
use tracing::{debug, error, info};

use crate::{
    app_state::AppState,
    config::router::RouterConfig,
    error::{init::InitError, runtime::RuntimeError},
    router::service::Router,
    types::router::RouterId,
};

/// A database listener service that handles LISTEN/NOTIFY functionality.
/// This service runs in the background and can be registered with meltdown.
#[derive(Debug, Clone)]
pub struct DatabaseListener {
    pg_pool: PgPool,
    app_state: AppState,
}

#[derive(Debug, Deserialize, Serialize, PartialEq)]
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
#[serde(tag = "event", rename_all = "snake_case")]
enum ConnectedCloudGatewaysNotification {
    RouterConfigUpdated {
        router_id: RouterId,
        router_config_id: String,
        organization_id: String,
        version: String,
        op: Op,
        config: Box<RouterConfig>,
    },
    RouterKeysUpdated {
        router_id: RouterId,
        organization_id: String,
        api_key_hash: String,
        op: Op,
    },
    Unknown {
        #[serde(flatten)]
        data: serde_json::Value,
    },
}

impl DatabaseListener {
    pub fn new(
        pg_pool: PgPool,
        app_state: AppState,
    ) -> Result<Self, InitError> {
        Ok(Self { pg_pool, app_state })
    }

    /// Runs the database listener service.
    /// This includes listening for notifications and handling
    /// connection health.
    async fn run_service(&mut self) -> Result<(), RuntimeError> {
        info!("starting database listener service");

        // Create listener for LISTEN/NOTIFY
        let mut listener =
            PgListener::connect_with(&self.pg_pool).await.map_err(|e| {
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

        let tx = self.app_state.get_router_tx().await;
        if tx.is_none() {
            return Err(RuntimeError::Internal(
                crate::error::internal::InternalError::Internal,
            ));
        }

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
                    Self::handle_notification(
                        &notification,
                        tx.as_ref().unwrap().clone(),
                        self.app_state.clone(),
                    )
                    .await?;
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
    async fn handle_notification(
        notification: &sqlx::postgres::PgNotification,
        tx: Sender<Change<RouterId, Router>>,
        app_state: AppState,
    ) -> Result<(), RuntimeError> {
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
                    match op {
                        Op::Insert => {
                            let router = Router::new(
                                router_id.clone(),
                                Arc::new(*config),
                                app_state.clone(),
                            )
                            .await?;

                            info!("sending router to tx");
                            let _ = tx
                                .send(Change::Insert(router_id, router))
                                .await;
                            info!("router inserted");
                            Ok(())
                        }
                        Op::Delete => {
                            let _ = tx.send(Change::Remove(router_id)).await;
                            info!("router removed");
                            Ok(())
                        }
                        _ => {
                            info!("skipping router insert");
                            Ok(())
                        }
                    }
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

                    Ok(())
                }
                ConnectedCloudGatewaysNotification::Unknown { data } => {
                    info!("Unknown notification event");
                    info!("data: {:?}", data);
                    // TODO: Handle unknown event
                    Ok(())
                }
            }
        } else {
            info!("received unknown notification");
            Ok(())
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
