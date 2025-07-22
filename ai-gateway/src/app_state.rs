use std::{collections::HashSet, sync::Arc};

use opentelemetry::KeyValue;
use rustc_hash::FxHashMap as HashMap;
use sqlx::PgPool;
use tokio::sync::{
    RwLock,
    mpsc::{Receiver, Sender},
};
use tower::discover::Change;

use crate::{
    cache::CacheClient,
    config::{
        Config, rate_limit::RateLimiterConfig,
        response_headers::ResponseHeadersConfig, router::RouterConfig,
    },
    control_plane::{control_plane_state::StateWithMetadata, types::Key},
    discover::monitor::{
        health::provider::HealthMonitorMap, metrics::EndpointMetricsRegistry,
        rate_limit::RateLimitMonitorMap,
    },
    error::init::InitError,
    logger::service::JawnClient,
    metrics::Metrics,
    router::service::Router,
    store::{minio::BaseMinioClient, router::RouterStore},
    types::{
        org::OrgId,
        provider::ProviderKeys,
        rate_limit::{
            RateLimitEvent, RateLimitEventReceivers, RateLimitEventSenders,
        },
        router::RouterId,
    },
};

#[derive(Debug, Clone)]
pub struct AppState(pub Arc<InnerAppState>);

impl AppState {
    #[must_use]
    pub fn response_headers_config(&self) -> ResponseHeadersConfig {
        self.0.config.response_headers
    }

    #[must_use]
    pub fn config(&self) -> &Config {
        &self.0.config
    }
}

#[derive(Debug)]
pub struct InnerAppState {
    pub config: Config,
    pub minio: BaseMinioClient,
    pub router_store: Option<RouterStore>,
    pub pg_pool: Option<PgPool>,
    pub jawn_http_client: JawnClient,
    pub cache_manager: Option<CacheClient>,
    pub global_rate_limit: Option<Arc<RateLimiterConfig>>,
    pub router_rate_limits: RwLock<HashMap<RouterId, Arc<RateLimiterConfig>>>,
    /// Top level metrics which are exported to OpenTelemetry.
    pub metrics: Metrics,
    /// Metrics to track provider health and rate limits.
    /// Not used for OpenTelemetry, only used for the load balancer to be
    /// dynamically updated based on provider health and rate limits.
    pub endpoint_metrics: EndpointMetricsRegistry,
    pub health_monitors: HealthMonitorMap,
    pub rate_limit_monitors: RateLimitMonitorMap,
    pub rate_limit_senders: RateLimitEventSenders,
    pub rate_limit_receivers: RateLimitEventReceivers,
    pub router_tx: RwLock<Option<Sender<Change<RouterId, Router>>>>,

    pub control_plane_state: Arc<RwLock<StateWithMetadata>>,

    pub provider_keys: ProviderKeys,
    pub helicone_api_keys: RwLock<Option<HashSet<Key>>>,
    pub router_organization_map: RwLock<HashMap<RouterId, OrgId>>,
}

impl AppState {
    pub async fn get_rate_limit_tx(
        &self,
        router_id: &RouterId,
    ) -> Result<Sender<RateLimitEvent>, InitError> {
        let rate_limit_channels = self.0.rate_limit_senders.read().await;
        let rate_limit_tx =
            rate_limit_channels.get(router_id).ok_or_else(|| {
                InitError::RateLimitChannelsNotInitialized(router_id.clone())
            })?;
        Ok(rate_limit_tx.clone())
    }

    pub async fn add_rate_limit_tx(
        &self,
        router_id: RouterId,
        rate_limit_tx: Sender<RateLimitEvent>,
    ) {
        let mut rate_limit_channels = self.0.rate_limit_senders.write().await;
        rate_limit_channels.insert(router_id, rate_limit_tx);
    }

    pub async fn add_rate_limit_rx(
        &self,
        router_id: RouterId,
        rate_limit_rx: Receiver<RateLimitEvent>,
    ) {
        let mut rate_limit_channels = self.0.rate_limit_receivers.write().await;
        rate_limit_channels.insert(router_id, rate_limit_rx);
    }

    pub async fn get_router_tx(
        &self,
    ) -> Option<Sender<Change<RouterId, Router>>> {
        let router_tx = self.0.router_tx.read().await;
        router_tx.clone()
    }

    pub async fn set_router_tx(&self, tx: Sender<Change<RouterId, Router>>) {
        let mut router_tx = self.0.router_tx.write().await;
        *router_tx = Some(tx);
    }

    pub async fn get_router_api_keys(&self) -> Option<HashSet<Key>> {
        let router_api_keys = self.0.helicone_api_keys.read().await;
        router_api_keys.clone()
    }

    pub async fn check_helicone_api_key(
        &self,
        api_key_hash: &str,
    ) -> Option<Key> {
        let router_api_keys = self.0.helicone_api_keys.read().await;
        router_api_keys
            .as_ref()?
            .iter()
            .find(|k| k.key_hash == api_key_hash)
            .cloned()
    }

    pub async fn set_router_api_keys(&self, keys: Option<HashSet<Key>>) {
        let mut router_api_keys = self.0.helicone_api_keys.write().await;
        (*router_api_keys).clone_from(&keys);
    }

    pub async fn set_router_api_key(
        &self,
        api_key: Key,
    ) -> Result<Option<HashSet<Key>>, InitError> {
        tracing::debug!("setting router api key");
        let mut router_api_keys = self.0.helicone_api_keys.write().await;
        router_api_keys
            .as_mut()
            .ok_or_else(|| InitError::RouterApiKeysNotInitialized)?
            .insert(api_key.clone());
        Ok(router_api_keys.clone())
    }

    pub async fn remove_router_api_key(
        &self,
        api_key_hash: String,
    ) -> Result<Option<HashSet<Key>>, InitError> {
        let mut router_api_keys = self.0.helicone_api_keys.write().await;
        router_api_keys
            .as_mut()
            .ok_or_else(|| InitError::RouterApiKeysNotInitialized)?
            .retain(|k| k.key_hash != api_key_hash);
        Ok(router_api_keys.clone())
    }

    pub async fn set_router_organization_map(
        &self,
        map: HashMap<RouterId, OrgId>,
    ) {
        let mut router_organization_map =
            self.0.router_organization_map.write().await;
        router_organization_map.clone_from(&map);
    }

    pub async fn set_router_organization(
        &self,
        router_id: RouterId,
        organization_id: OrgId,
    ) {
        let mut router_organization_map =
            self.0.router_organization_map.write().await;
        router_organization_map.insert(router_id, organization_id);
    }

    pub async fn get_router_organization(
        &self,
        router_id: &RouterId,
    ) -> Option<OrgId> {
        let router_organization_map =
            self.0.router_organization_map.read().await;
        router_organization_map.get(router_id).copied()
    }

    pub fn increment_router_metrics(
        &self,
        router_id: &RouterId,
        router_config: &RouterConfig,
        organization_id: Option<OrgId>,
    ) {
        let metrics = &self.0.metrics;
        let org_id = organization_id
            .as_ref()
            .map_or_else(|| "unknown".to_string(), ToString::to_string);
        metrics.routers.routers.add(
            1,
            &[
                KeyValue::new("organization_id", org_id.clone()),
                KeyValue::new("router_id", router_id.to_string()),
            ],
        );
        for (endpoint_type, balance_config) in &router_config.load_balance.0 {
            metrics.routers.router_strategies.add(
                1,
                &[
                    KeyValue::new("organization_id", org_id.clone()),
                    KeyValue::new("router_id", router_id.to_string()),
                    KeyValue::new(
                        "endpoint_type",
                        endpoint_type.as_ref().to_string(),
                    ),
                    KeyValue::new(
                        "balance_config",
                        balance_config.as_ref().to_string(),
                    ),
                ],
            );
        }
        if router_config.model_mappings.is_some() {
            metrics
                .routers
                .model_mappings
                .add(1, &[KeyValue::new("router_id", router_id.to_string())]);
        }
        if router_config.cache.is_some() {
            metrics
                .routers
                .cache_enabled
                .add(1, &[KeyValue::new("router_id", router_id.to_string())]);
        }
        if router_config.retries.is_some() {
            metrics
                .routers
                .retries_enabled
                .add(1, &[KeyValue::new("router_id", router_id.to_string())]);
        }
        if router_config.rate_limit.is_some() {
            metrics
                .routers
                .rate_limit_enabled
                .add(1, &[KeyValue::new("router_id", router_id.to_string())]);
        }
    }

    pub fn decrement_router_metrics(
        &self,
        router_id: &RouterId,
        router_config: &RouterConfig,
        organization_id: Option<OrgId>,
    ) {
        let metrics = &self.0.metrics;
        let org_id = organization_id
            .as_ref()
            .map_or_else(|| "unknown".to_string(), ToString::to_string);
        metrics.routers.routers.add(
            -1,
            &[
                KeyValue::new("organization_id", org_id.clone()),
                KeyValue::new("router_id", router_id.to_string()),
            ],
        );
        for (endpoint_type, balance_config) in &router_config.load_balance.0 {
            metrics.routers.router_strategies.add(
                1,
                &[
                    KeyValue::new("organization_id", org_id.clone()),
                    KeyValue::new("router_id", router_id.to_string()),
                    KeyValue::new(
                        "endpoint_type",
                        endpoint_type.as_ref().to_string(),
                    ),
                    KeyValue::new(
                        "balance_config",
                        balance_config.as_ref().to_string(),
                    ),
                ],
            );
        }
        if router_config.model_mappings.is_some() {
            metrics
                .routers
                .model_mappings
                .add(1, &[KeyValue::new("router_id", router_id.to_string())]);
        }
        if router_config.cache.is_some() {
            metrics
                .routers
                .cache_enabled
                .add(1, &[KeyValue::new("router_id", router_id.to_string())]);
        }
        if router_config.retries.is_some() {
            metrics
                .routers
                .retries_enabled
                .add(1, &[KeyValue::new("router_id", router_id.to_string())]);
        }
        if router_config.rate_limit.is_some() {
            metrics
                .routers
                .rate_limit_enabled
                .add(1, &[KeyValue::new("router_id", router_id.to_string())]);
        }
    }
}
