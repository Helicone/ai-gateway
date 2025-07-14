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
    control_plane::types::Key,
    error::{init::InitError, runtime::RuntimeError},
    router::service::Router,
    types::{org::OrgId, router::RouterId},
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
        router_id: String,
        router_hash: RouterId,
        router_config_id: String,
        organization_id: String,
        version: String,
        op: Op,
        config: Box<RouterConfig>,
    },
    ApiKeyUpdated {
        owner_id: String,
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

    async fn handle_router_config_insert(
        router_hash: RouterId,
        router_config: RouterConfig,
        app_state: AppState,
        organization_id: OrgId,
        tx: Sender<Change<RouterId, Router>>,
    ) -> Result<(), RuntimeError> {
        let router = Router::new(
            router_hash.clone(),
            Arc::new(router_config),
            app_state.clone(),
        )
        .await?;

        info!("sending router to tx");
        let _ = tx.send(Change::Insert(router_hash.clone(), router)).await;
        info!("router inserted");
        app_state
            .set_router_organization(router_hash.clone(), organization_id)
            .await;

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
                    router_id: _,
                    router_hash,
                    router_config_id: _,
                    organization_id,
                    version: _,
                    op,
                    config,
                } => {
                    info!("Router configuration updated");
                    match op {
                        Op::Insert => {
                            let organization_id = OrgId::try_from(organization_id.as_str()).map_err(|e| {
                                error!(error = %e, "failed to convert organization id to OrgId");
                                RuntimeError::Internal(crate::error::internal::InternalError::Internal)
                            })?;
                            Self::handle_router_config_insert(
                                router_hash,
                                *config,
                                app_state,
                                organization_id,
                                tx,
                            )
                            .await
                        }
                        Op::Delete => {
                            let _ = tx.send(Change::Remove(router_hash)).await;
                            info!("router removed");
                            Ok(())
                        }
                        _ => {
                            info!("skipping router insert");
                            Ok(())
                        }
                    }
                }
                ConnectedCloudGatewaysNotification::ApiKeyUpdated {
                    owner_id,
                    organization_id,
                    api_key_hash,
                    op,
                } => match op {
                    Op::Insert => {
                        let organization_id = OrgId::try_from(organization_id.as_str()).map_err(|e| {
                                error!(error = %e, "failed to convert organization id to OrgId");
                                RuntimeError::Internal(crate::error::internal::InternalError::Internal)
                            })?;
                        let _ = app_state
                            .set_router_api_key(Key {
                                key_hash: api_key_hash,
                                owner_id,
                                organization_id,
                            })
                            .await;
                        info!("router key inserted");
                        Ok(())
                    }
                    Op::Delete => {
                        let _ =
                            app_state.remove_router_api_key(api_key_hash).await;
                        info!("router key removed");
                        Ok(())
                    }
                    _ => {
                        info!("skipping router key insert");
                        Ok(())
                    }
                },
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
