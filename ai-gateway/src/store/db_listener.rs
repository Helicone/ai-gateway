use std::sync::Arc;

use futures::future::BoxFuture;
use meltdown::Token;
use serde::{Deserialize, Serialize};
use sqlx::postgres::PgListener;
use tokio::sync::mpsc::Sender;
use tower::discover::Change;
use tracing::{debug, error, info};

use crate::{
    app_state::AppState,
    config::router::RouterConfig,
    control_plane::types::Key,
    error::{init::InitError, runtime::RuntimeError},
    router::service::Router,
    store::router::RouterStore,
    types::{org::OrgId, router::RouterId, user::UserId},
};

/// A database listener service that handles LISTEN/NOTIFY functionality.
/// This service runs in the background and can be registered with meltdown.
#[derive(Debug)]
pub struct DatabaseListener {
    app_state: AppState,
    pg_listener: PgListener,
    tx: Sender<Change<RouterId, Router>>,
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
        organization_id: OrgId,
        version: String,
        op: Op,
        config: Box<RouterConfig>,
    },
    ApiKeyUpdated {
        owner_id: UserId,
        organization_id: OrgId,
        api_key_hash: String,
        soft_delete: bool,
        op: Op,
    },
    Unknown {
        #[serde(flatten)]
        data: serde_json::Value,
    },
}

impl DatabaseListener {
    pub async fn new(
        database_url: &str,
        app_state: AppState,
    ) -> Result<Self, InitError> {
        let listener =
            PgListener::connect(database_url).await.map_err(|e| {
                error!(error = %e, "failed to create database listener");
                InitError::DatabaseConnection(e)
            })?;

        // Retry getting router_tx for up to 5 seconds
        let start = tokio::time::Instant::now();
        let timeout = tokio::time::Duration::from_secs(1);
        let tx = loop {
            if let Some(tx) = app_state.get_router_tx().await {
                break tx;
            }

            if start.elapsed() >= timeout {
                error!("failed to get router_tx after 5 seconds");
                return Err(InitError::RouterTxNotSet);
            }

            debug!("router_tx not available, retrying...");
            tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
        };

        Ok(Self {
            app_state,
            pg_listener: listener,
            tx,
        })
    }

    /// Runs the database listener service.
    /// This includes listening for notifications and handling
    /// connection health.
    async fn run_service(&mut self) -> Result<(), RuntimeError> {
        info!("starting database listener service");

        // Create listener for LISTEN/NOTIFY
        let listener = &mut self.pg_listener;
        let router_store = self
            .app_state
            .0
            .router_store
            .as_ref()
            .ok_or(InitError::StoreNotConfigured("router_store"))?;

        // Listen for notifications on a channel (you can customize this)
        listener.listen("connected_cloud_gateways").await.map_err(|e| {
            error!(error = %e, "failed to listen on database notification channel");
            RuntimeError::Internal(crate::error::internal::InternalError::Internal)
        })?;

        // Process notifications
        loop {
            while let Ok(Some(notification)) = listener.try_recv().await {
                debug!(
                    channel = notification.channel(),
                    payload = notification.payload(),
                    "received database notification"
                );
                // Handle the notification here
                Self::handle_notification(
                    &notification,
                    self.tx.clone(),
                    self.app_state.clone(),
                )
                .await?;
            }

            // connection lost, reset the api keys and router state
            Self::reset_state(
                router_store,
                self.tx.clone(),
                self.app_state.clone(),
            )
            .await?;
        }
    }

    async fn reset_state(
        router_store: &RouterStore,
        tx: Sender<Change<RouterId, Router>>,
        app_state: AppState,
    ) -> Result<(), RuntimeError> {
        let routers = router_store.get_all_routers().await?;
        for router in routers {
            let _ = tx
                .send(Change::Remove(RouterId::Named(
                    router.router_hash.into(),
                )))
                .await;
        }
        let provider_keys = router_store.get_all_provider_keys().await?;
        app_state
            .0
            .provider_keys
            .set_all_provider_keys(provider_keys)
            .await;
        let api_keys = router_store.get_all_router_keys().await?;
        app_state.set_router_api_keys(Some(api_keys)).await;
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

        debug!("sending router to tx");
        let _ = tx.send(Change::Insert(router_hash.clone(), router)).await;
        debug!("router inserted");
        app_state
            .set_router_organization(router_hash.clone(), organization_id)
            .await;

        let router_store = app_state
            .0
            .router_store
            .as_ref()
            .ok_or(InitError::StoreNotConfigured("router_store"))?;
        let provider_keys =
            router_store.get_org_provider_keys(organization_id).await?;
        app_state
            .0
            .provider_keys
            .set_org_provider_keys(organization_id, provider_keys)
            .await;

        Ok(())
    }

    /// Handles incoming database notifications.
    #[allow(clippy::too_many_lines)]
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
            let Ok(payload) = serde_json::from_str::<
                ConnectedCloudGatewaysNotification,
            >(notification.payload()) else {
                error!("failed to parse db_listener notification payload");
                return Ok(());
            };

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
                    debug!("Router configuration updated");
                    match op {
                        Op::Insert => {
                            app_state.increment_router_metrics(
                                &router_hash,
                                &config,
                                Some(organization_id),
                            );
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
                            app_state.decrement_router_metrics(
                                &router_hash,
                                &config,
                                Some(organization_id),
                            );
                            if let Err(e) =
                                tx.send(Change::Remove(router_hash)).await
                            {
                                error!(error = %e, "failed to send router remove to tx");
                            } else {
                                debug!("router removed");
                            }
                            Ok(())
                        }
                        _ => {
                            debug!("skipping router insert");
                            Ok(())
                        }
                    }
                }
                ConnectedCloudGatewaysNotification::ApiKeyUpdated {
                    owner_id,
                    organization_id,
                    api_key_hash,
                    soft_delete,
                    op,
                } => match op {
                    Op::Insert => {
                        let _ = app_state
                            .set_router_api_key(Key {
                                key_hash: api_key_hash,
                                owner_id,
                                organization_id,
                            })
                            .await;
                        debug!("router key inserted");
                        Ok(())
                    }
                    Op::Delete => {
                        // This case should never happen, since we update the
                        // soft delete flag when we delete an api key.
                        let _ =
                            app_state.remove_router_api_key(api_key_hash).await;
                        debug!("router key removed");
                        Ok(())
                    }
                    Op::Update => {
                        if soft_delete {
                            let _ = app_state
                                .remove_router_api_key(api_key_hash)
                                .await;
                            debug!("router key removed");
                        }
                        Ok(())
                    }
                    Op::Truncate => {
                        debug!("skipping router key truncate");
                        Ok(())
                    }
                },
                ConnectedCloudGatewaysNotification::Unknown { data } => {
                    debug!("Unknown notification event");
                    debug!("data: {:?}", data);
                    // TODO: Handle unknown event
                    Ok(())
                }
            }
        } else {
            debug!("received unknown notification");
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
