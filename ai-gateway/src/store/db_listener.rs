use std::sync::Arc;

use chrono::{DateTime, Utc};
use futures::future::BoxFuture;
use meltdown::Token;
use rustc_hash::FxHashMap as HashMap;
use serde::{Deserialize, Serialize};
use sqlx::postgres::PgListener;
use tokio::{
    sync::mpsc::Sender,
    time::{Duration, MissedTickBehavior, interval},
};
use tower::discover::Change;
use tracing::{debug, error, info};

use crate::{
    app_state::AppState,
    config::{deployment_target::DeploymentTarget, router::RouterConfig},
    control_plane::types::Key,
    error::{init::InitError, internal::InternalError, runtime::RuntimeError},
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
    router_store: RouterStore,
    tx: Sender<Change<RouterId, Router>>,
    /// Track last seen router config versions to detect missed events
    last_router_config_versions: HashMap<String, DateTime<Utc>>,
    /// Track last seen API key `created_at` timestamps to detect missed events
    last_api_key_created_at: HashMap<String, DateTime<Utc>>,
    /// Polling interval for database queries
    poll_interval: Duration,
    /// Last time we polled the database
    last_poll_time: Option<DateTime<Utc>>,
    /// Interval for reconnecting the listener
    listener_reconnect_interval: Duration,
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

/// Service state to correctly handle cancellation safety
enum ServiceState {
    Idle,
    PollingDatabase,
    Reconnecting,
    HandlingNotification(sqlx::postgres::PgNotification),
}

impl DatabaseListener {
    pub async fn new(
        database_url: &str,
        app_state: AppState,
    ) -> Result<Self, InitError> {
        let pg_listener =
            PgListener::connect(database_url).await.map_err(|e| {
                error!(error = %e, "failed to create database listener");
                InitError::DatabaseConnection(e)
            })?;

        // Retry getting router_tx for up to 1 seconds
        let tx = tokio::time::timeout(Duration::from_secs(1), async {
            loop {
                if let Some(tx) = app_state.get_router_tx().await {
                    break tx;
                }
                debug!("router_tx not available, retrying...");
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        })
        .await
        .map_err(|_| InitError::RouterTxNotSet)?;

        let DeploymentTarget::Cloud {
            db_poll_interval,
            listener_reconnect_interval,
        } = app_state.config().deployment_target
        else {
            return Err(InitError::DatabaseListenerOnlyCloud);
        };

        let router_store = app_state
            .0
            .router_store
            .as_ref()
            .ok_or(InitError::StoreNotConfigured("router_store"))?
            .clone();

        Ok(Self {
            app_state,
            pg_listener,
            router_store,
            tx,
            last_router_config_versions: HashMap::default(),
            last_api_key_created_at: HashMap::default(),
            poll_interval: db_poll_interval,
            last_poll_time: None,
            listener_reconnect_interval,
        })
    }

    /// Poll the database for changes since last poll
    #[allow(clippy::too_many_lines)]
    async fn poll_database(&mut self) -> Result<(), RuntimeError> {
        let start = Utc::now();
        info!("polling database for changes");

        // Query for API key changes using RouterStore methods
        let new_api_keys = if let Some(last_poll) = self.last_poll_time {
            self.router_store
                .get_all_db_helicone_api_keys_updated_after(last_poll)
                .await
                .inspect(|keys| {
                    debug!(
                        "polling found {} new helicone api keys",
                        keys.len()
                    );
                })
                .inspect_err(|e| {
                    error!(error = %e, "failed to poll api keys");
                })?
        } else {
            // First poll - get all active API keys
            self.router_store
                .get_all_db_helicone_api_keys()
                .await
                .inspect(|keys| {
                    info!(
                        "polling initialized with {} helicone api keys",
                        keys.len()
                    );
                })
                .inspect_err(|e| {
                    error!(error = %e, "failed to poll api keys");
                })?
        };

        // Process API key changes
        for api_key in new_api_keys {
            let soft_delete = api_key.soft_delete.unwrap_or(false);
            // Use updated_at if available, otherwise fall back to created_at
            let key_timestamp =
                api_key.updated_at.unwrap_or(api_key.created_at);

            let should_process =
                match self.last_api_key_created_at.get(&api_key.key_hash) {
                    None => true,
                    Some(last_seen) => key_timestamp > *last_seen,
                };

            if should_process {
                if soft_delete {
                    self.app_state
                        .remove_helicone_api_key(api_key.key_hash.clone())
                        .await?;
                    // Remove from tracking map when soft deleted
                    self.last_api_key_created_at.remove(&api_key.key_hash);
                } else {
                    self.app_state
                        .set_helicone_api_key(Key {
                            key_hash: api_key.key_hash.clone(),
                            owner_id: UserId::new(api_key.owner_id),
                            organization_id: OrgId::new(
                                api_key.organization_id,
                            ),
                        })
                        .await?;
                    self.last_api_key_created_at
                        .insert(api_key.key_hash, key_timestamp);
                }
            }
        }

        // Query for router config changes using RouterStore methods
        let new_routers = if let Some(last_poll) = self.last_poll_time {
            self.router_store
                .get_routers_created_after(last_poll)
                .await
                .inspect(|routers| {
                    debug!("polling found {} new routers", routers.len());
                })
                .inspect_err(|e| {
                    error!(error = %e, "failed to poll router configs");
                })?
        } else {
            // First poll - get all routers
            self.router_store
                .get_all_routers()
                .await
                .inspect(|routers| {
                    info!("polling initialized with {} routers", routers.len());
                })
                .inspect_err(|e| {
                    error!(error = %e, "failed to poll router configs");
                })?
        };

        // Process router changes
        for db_router in new_routers {
            let should_process = match self
                .last_router_config_versions
                .get(&db_router.router_hash)
            {
                None => true,
                Some(last_seen) => db_router.created_at > *last_seen,
            };

            if should_process {
                match serde_json::from_value::<RouterConfig>(db_router.config) {
                    Ok(config) => {
                        info!(router_hash = %db_router.router_hash, "polling found new/updated router");
                        self.handle_router_config_insert(
                            RouterId::Named(
                                db_router.router_hash.clone().into(),
                            ),
                            config,
                            OrgId::new(db_router.organization_id),
                            self.tx.clone(),
                        )
                        .await?;

                        self.last_router_config_versions.insert(
                            db_router.router_hash,
                            db_router.created_at,
                        );
                    }
                    Err(e) => {
                        error!(error = %e, router_hash = %db_router.router_hash, "failed to parse router config");
                    }
                }
            }
        }

        let end = Utc::now();
        self.last_poll_time = Some(end);
        info!(
            poll_duration_ms = (end - start).num_milliseconds(),
            "database polling complete"
        );
        Ok(())
    }

    /// Runs the database listener service.
    /// This includes listening for notifications and handling
    /// connection health.
    async fn run_service(&mut self) -> Result<(), RuntimeError> {
        info!("performing initial database poll");
        // Do an initial poll to populate the state
        if let Err(e) = self.poll_database().await {
            error!(error = %e, "error during initial database poll");
        }

        self.pg_listener
            .listen("connected_cloud_gateways")
            .await
            .map_err(|e| {
                error!(error = %e, "failed to listen on database notification channel");
                InitError::DatabaseConnection(e)
            })?;

        let mut poll_interval = interval(self.poll_interval);
        poll_interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

        let mut reconnect_interval = interval(self.listener_reconnect_interval);
        reconnect_interval.set_missed_tick_behavior(MissedTickBehavior::Skip);

        let mut state = ServiceState::Idle;

        // Process notifications and polls
        loop {
            match state {
                ServiceState::Idle => {
                    tokio::select! {
                        biased;
                        notification_result = self.pg_listener.recv() => {
                            match notification_result {
                                Ok(notification) => {
                                    state = ServiceState::HandlingNotification(notification);
                                }
                                Err(e) => {
                                    error!(error = %e, "error receiving from listener, continuing");
                                    // we will continue to receive updates as the next call to recv() will
                                    // reconnect for us eagerly, additionally we have the db polling and
                                    // the periodic reconnection that will catch up on any missed events
                                }
                            }
                        }

                        _ = poll_interval.tick() => {
                            state = ServiceState::PollingDatabase;
                        }

                        _ = reconnect_interval.tick() => {
                            state = ServiceState::Reconnecting;
                        }
                    }
                }
                ServiceState::PollingDatabase => {
                    // This runs outside select!, so it can't be cancelled by
                    // other branches
                    if let Err(e) = self.poll_database().await {
                        error!(error = %e, "error polling database");
                    }
                    state = ServiceState::Idle;
                }
                ServiceState::HandlingNotification(notification) => {
                    // This runs outside select!, so it can't be cancelled by
                    // other branches
                    if let Err(e) = self
                        .handle_notification(&notification, self.tx.clone())
                        .await
                    {
                        error!(error = %e, "failed to handle db listener notification, continuing");
                    }
                    state = ServiceState::Idle;
                }
                ServiceState::Reconnecting => {
                    info!("periodic reconnection");
                    // This runs outside select!, so it can't be cancelled by
                    // other branches
                    if let Err(e) = self.pg_listener.unlisten_all().await {
                        error!(error = %e, "failed to unlisten all channels");
                    }
                    if let Err(e) = self
                        .pg_listener
                        .listen("connected_cloud_gateways")
                        .await
                    {
                        error!(error = %e, "failed to listen on channel after reconnection");
                    } else {
                        info!(
                            "successfully reconnected and listening on channel"
                        );
                    }
                    state = ServiceState::Idle;
                }
            }
        }
    }

    async fn handle_router_config_insert(
        &self,
        router_hash: RouterId,
        router_config: RouterConfig,
        organization_id: OrgId,
        tx: Sender<Change<RouterId, Router>>,
    ) -> Result<(), RuntimeError> {
        let router = Router::new(
            router_hash.clone(),
            Arc::new(router_config),
            self.app_state.clone(),
        )
        .await?;

        // TODO(eng-2471)
        tx.send(Change::Insert(router_hash.clone(), router))
            .await
            .map_err(|e| {
                error!(error = %e, "failed to send router insert to tx");
                RuntimeError::Internal(InternalError::Internal)
            })?;
        self.app_state
            .set_router_organization(router_hash.clone(), organization_id)
            .await;

        let provider_keys = self
            .router_store
            .get_org_provider_keys(organization_id)
            .await?;
        self.app_state
            .0
            .provider_keys
            .set_org_provider_keys(organization_id, provider_keys)
            .await;

        Ok(())
    }

    /// Handles incoming database notifications.
    #[allow(clippy::too_many_lines)]
    async fn handle_notification(
        &mut self,
        notification: &sqlx::postgres::PgNotification,
        tx: Sender<Change<RouterId, Router>>,
    ) -> Result<(), RuntimeError> {
        info!(channel = notification.channel(), "processing notification");

        if notification.channel() == "connected_cloud_gateways" {
            let payload = serde_json::from_str::<
                ConnectedCloudGatewaysNotification,
            >(notification.payload()).map_err(|e| {
                error!(error = %e, "failed to parse connected_cloud_gateways payload");
                InternalError::Deserialize {
                    ty: "ConnectedCloudGatewaysNotification",
                    error: e,
                }
            })?;

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
                    info!(
                        router_hash = %router_hash,
                        organization_id = %organization_id,
                        "router configuration created/updated"
                    );
                    match op {
                        Op::Insert => {
                            // TODO: metrics might be incorrect if this is just
                            // a config update
                            self.app_state.increment_router_metrics(
                                &router_hash,
                                &config,
                                Some(organization_id),
                            );
                            self.handle_router_config_insert(
                                router_hash.clone(),
                                *config,
                                organization_id,
                                tx,
                            )
                            .await
                            .map_err(|e| {
                                error!(error = %e, "failed to handle router config insert");
                                e
                            })?;

                            self.last_router_config_versions
                                .insert(router_hash.to_string(), Utc::now());

                            Ok(())
                        }
                        Op::Delete => {
                            self.app_state.decrement_router_metrics(
                                &router_hash,
                                &config,
                                Some(organization_id),
                            );
                            tx
                                .send(Change::Remove(router_hash.clone()))
                                .await
                                .map_err(|e| {
                                    error!(error = %e, "failed to send router remove to tx");
                                    RuntimeError::Internal(InternalError::Internal)
                                })?;
                            info!(
                                router_hash = %router_hash,
                                organization_id = %organization_id,
                                "router removed"
                            );
                            // Remove from state tracking
                            self.last_router_config_versions
                                .remove(&router_hash.to_string());
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
                        self.app_state
                            .set_helicone_api_key(Key {
                                key_hash: api_key_hash.clone(),
                                owner_id,
                                organization_id,
                            })
                            .await
                            .map_err(|e| {
                                error!(error = %e, "failed to set helicone api key");
                                e
                            })?;
                        info!(
                            owner_id = %owner_id,
                            organization_id = %organization_id,
                            "helicone api key inserted"
                        );
                        // Update state tracking
                        self.last_api_key_created_at
                            .insert(api_key_hash, Utc::now());
                        Ok(())
                    }
                    Op::Delete => {
                        // This case should theoretically never happen, since we
                        // update the soft delete flag when we delete an api
                        // key. However, we'll handle it
                        // just in case.
                        self.app_state
                            .remove_helicone_api_key(api_key_hash.clone())
                            .await
                            .map_err(|e| {
                                error!(error = %e, "failed to remove helicone api key");
                                e
                            })?;
                        info!(
                            owner_id = %owner_id,
                            organization_id = %organization_id,
                            "helicone api key removed"
                        );
                        // Remove from state tracking
                        self.last_api_key_created_at.remove(&api_key_hash);
                        Ok(())
                    }
                    Op::Update => {
                        if soft_delete {
                            self.app_state
                                .remove_helicone_api_key(api_key_hash.clone())
                                .await
                                .map_err(|e| {
                                    error!(error = %e, "failed to remove helicone api key");
                                    e
                                })?;
                            info!(
                                owner_id = %owner_id,
                                organization_id = %organization_id,
                                "helicone api key soft deleted"
                            );
                            // Remove from state tracking when soft deleted
                            self.last_api_key_created_at.remove(&api_key_hash);
                        } else {
                            // Update state tracking for non-soft-delete updates
                            self.last_api_key_created_at
                                .insert(api_key_hash, Utc::now());
                        }
                        Ok(())
                    }
                    Op::Truncate => {
                        debug!("skipping helicone api key truncate");
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
    }
}

impl meltdown::Service for DatabaseListener {
    type Future = BoxFuture<'static, Result<(), RuntimeError>>;

    fn run(mut self, mut token: Token) -> Self::Future {
        Box::pin(async move {
            tokio::select! {
                biased;
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
