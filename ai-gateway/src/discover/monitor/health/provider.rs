//! Dynamically remove inference providers that fail health checks
use std::sync::Arc;

use futures::future::{self, BoxFuture};
use meltdown::Token;
use opentelemetry::KeyValue;
use rust_decimal::prelude::ToPrimitive;
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};
use tokio::{
    sync::{RwLock, mpsc::Sender},
    task::JoinSet,
    time,
};
use tower::discover::Change;
use tracing::{Instrument, debug, error, trace};
use weighted_balance::weight::Weight;

use crate::{
    app_state::AppState,
    config::{
        balance::BalanceConfigInner, monitor::GracePeriod, router::RouterConfig,
    },
    discover::{
        model::{
            key::Key as ModelKey, weighted_key::WeightedKey as ModelWeightedKey,
        },
        provider::{
            key::Key as ProviderKey,
            weighted_key::WeightedKey as ProviderWeightedKey,
        },
    },
    dispatcher::{Dispatcher, DispatcherService},
    error::{
        init::InitError,
        internal::InternalError,
        runtime::{self, RuntimeError},
    },
    types::{provider::InferenceProvider, router::RouterId},
};

pub type HealthMonitorMap =
    Arc<RwLock<HashMap<RouterId, ProviderHealthMonitor>>>;

#[derive(Debug, Clone)]
pub enum ProviderHealthMonitor {
    ProviderWeighted(ProviderMonitorInner<ProviderWeightedKey>),
    ModelWeighted(ProviderMonitorInner<ModelWeightedKey>),
    ProviderLatency(ProviderMonitorInner<ProviderKey>),
    ModelLatency(ProviderMonitorInner<ModelKey>),
}

impl ProviderHealthMonitor {
    fn provider_weighted(
        tx: Sender<Change<ProviderWeightedKey, DispatcherService>>,
        router_id: RouterId,
        router_config: Arc<RouterConfig>,
        app_state: AppState,
    ) -> Self {
        Self::ProviderWeighted(ProviderMonitorInner::new(
            tx,
            router_id,
            router_config,
            app_state,
        ))
    }

    fn model_weighted(
        tx: Sender<Change<ModelWeightedKey, DispatcherService>>,
        router_id: RouterId,
        router_config: Arc<RouterConfig>,
        app_state: AppState,
    ) -> Self {
        Self::ModelWeighted(ProviderMonitorInner::new(
            tx,
            router_id,
            router_config,
            app_state,
        ))
    }

    fn provider_latency(
        tx: Sender<Change<ProviderKey, DispatcherService>>,
        router_id: RouterId,
        router_config: Arc<RouterConfig>,
        app_state: AppState,
    ) -> Self {
        Self::ProviderLatency(ProviderMonitorInner::new(
            tx,
            router_id,
            router_config,
            app_state,
        ))
    }

    fn model_latency(
        tx: Sender<Change<ModelKey, DispatcherService>>,
        router_id: RouterId,
        router_config: Arc<RouterConfig>,
        app_state: AppState,
    ) -> Self {
        Self::ModelLatency(ProviderMonitorInner::new(
            tx,
            router_id,
            router_config,
            app_state,
        ))
    }

    async fn check_monitor(&mut self) -> Result<(), runtime::RuntimeError> {
        match self {
            ProviderHealthMonitor::ProviderWeighted(inner) => {
                check_provider_weighted_monitor(inner).await
            }
            ProviderHealthMonitor::ModelWeighted(inner) => {
                check_model_weighted_monitor(inner).await
            }
            ProviderHealthMonitor::ProviderLatency(inner) => {
                check_provider_latency_monitor(inner).await
            }
            ProviderHealthMonitor::ModelLatency(inner) => {
                check_model_latency_monitor(inner).await
            }
        }
    }
}

async fn check_provider_weighted_monitor(
    inner: &mut ProviderMonitorInner<ProviderWeightedKey>,
) -> Result<(), runtime::RuntimeError> {
    for (endpoint_type, balance_config) in
        inner.router_config.load_balance.as_ref()
    {
        match balance_config {
            BalanceConfigInner::ProviderWeighted { providers } => {
                for target in providers {
                    let provider = &target.provider;
                    let weight = Weight::from(
                        target.weight.to_f64().ok_or_else(|| {
                            InitError::InvalidWeight(target.provider.clone())
                        })?,
                    );

                    let key = ProviderWeightedKey::new(
                        provider.clone(),
                        *endpoint_type,
                        weight,
                    );
                    let is_healthy = inner.check_health(provider)?;
                    let was_unhealthy = inner.unhealthy_keys.contains(&key);

                    if !is_healthy && !was_unhealthy {
                        trace!(provider = ?provider, endpoint_type = ?endpoint_type, "Provider became unhealthy, removing");
                        if let Err(e) =
                            inner.tx.send(Change::Remove(key.clone())).await
                        {
                            error!(error = ?e, "Failed to send remove event for unhealthy provider");
                        }
                        inner.unhealthy_keys.insert(key);
                    } else if is_healthy && was_unhealthy {
                        trace!(provider = ?provider, endpoint_type = ?endpoint_type, "Provider became healthy, adding back");
                        inner.unhealthy_keys.remove(&key);

                        let service = Dispatcher::new(
                            inner.app_state.clone(),
                            &inner.router_id,
                            &inner.router_config,
                            provider.clone(),
                        )
                        .await?;

                        if let Err(e) =
                            inner.tx.send(Change::Insert(key, service)).await
                        {
                            error!(error = ?e, "Failed to send insert event for healthy provider");
                        }
                    }

                    let metric_attributes =
                        [KeyValue::new("provider", provider.to_string())];
                    if is_healthy {
                        inner
                            .app_state
                            .0
                            .metrics
                            .provider_health
                            .record(1, &metric_attributes);
                    } else {
                        inner
                            .app_state
                            .0
                            .metrics
                            .provider_health
                            .record(0, &metric_attributes);
                    }
                }
            }
            BalanceConfigInner::ModelWeighted { .. } => {
                tracing::error!(
                    "Model weighted entries in a provider weighted monitor"
                );
                return Err(InternalError::Internal.into());
            }
            BalanceConfigInner::BalancedLatency { .. } => {
                tracing::error!("P2C entries in a weighted monitor");
                return Err(InternalError::Internal.into());
            }
            BalanceConfigInner::ModelLatency { .. } => {
                tracing::error!(
                    "Model latency entries in a provider weighted monitor"
                );
                return Err(InternalError::Internal.into());
            }
        }
    }

    Ok(())
}

#[allow(clippy::too_many_lines)]
async fn check_model_weighted_monitor(
    inner: &mut ProviderMonitorInner<ModelWeightedKey>,
) -> Result<(), runtime::RuntimeError> {
    for (endpoint_type, balance_config) in
        inner.router_config.load_balance.as_ref()
    {
        match balance_config {
            BalanceConfigInner::ModelWeighted { models } => {
                for target in models {
                    let model = &target.model;
                    let provider =
                        model.inference_provider().ok_or_else(|| {
                            InitError::ModelIdNotRecognized(model.to_string())
                        })?;
                    let weight =
                        Weight::from(target.weight.to_f64().ok_or_else(
                            || InitError::InvalidWeight(provider.clone()),
                        )?);

                    let key = ModelWeightedKey::new(
                        model.clone(),
                        *endpoint_type,
                        weight,
                    );
                    let is_healthy = inner.check_health(&provider)?;
                    let was_unhealthy = inner.unhealthy_keys.contains(&key);

                    if !is_healthy && !was_unhealthy {
                        trace!(provider = ?provider, endpoint_type = ?endpoint_type, "Provider became unhealthy, removing");
                        let all_models_of_unhealthy_provider = models
                            .iter()
                            .filter(|m| {
                                m.model.inference_provider().as_ref()
                                    == Some(&provider)
                            })
                            .collect::<Vec<_>>();

                        // Send removal changes for all models of the unhealthy
                        // provider concurrently
                        let mut join_set = JoinSet::new();
                        for unhealthy_model in all_models_of_unhealthy_provider
                        {
                            let weight = Weight::from(
                                unhealthy_model.weight.to_f64().ok_or_else(
                                    || {
                                        InitError::InvalidWeight(
                                            provider.clone(),
                                        )
                                    },
                                )?,
                            );
                            let unhealthy_key = ModelWeightedKey::new(
                                unhealthy_model.model.clone(),
                                *endpoint_type,
                                weight,
                            );
                            let tx = inner.tx.clone();

                            inner.unhealthy_keys.insert(unhealthy_key.clone());
                            join_set.spawn(async move {
                                tx.send(Change::Remove(unhealthy_key)).await
                            });
                        }

                        // we can't use join_all because we want to avoid panics
                        while let Some(task_result) = join_set.join_next().await
                        {
                            match task_result {
                                Ok(send_result) => {
                                    if let Err(e) = send_result {
                                        error!(error = ?e, model = ?model, "Failed to send remove event for unhealthy provider model");
                                    }
                                }
                                Err(e) => {
                                    error!(error = ?e, "Task failed while sending remove event for unhealthy provider model");
                                    return Err(e.into());
                                }
                            }
                        }
                    } else if is_healthy && was_unhealthy {
                        trace!(provider = ?provider, endpoint_type = ?endpoint_type, "Provider became healthy, adding back");
                        let all_models_of_now_healthy_provider = models
                            .iter()
                            .filter(|m| {
                                m.model.inference_provider().as_ref()
                                    == Some(&provider)
                            })
                            .collect::<Vec<_>>();
                        inner.unhealthy_keys.remove(&key);

                        for healthy_model in all_models_of_now_healthy_provider
                        {
                            let weight = Weight::from(
                                healthy_model.weight.to_f64().ok_or_else(
                                    || {
                                        InitError::InvalidWeight(
                                            provider.clone(),
                                        )
                                    },
                                )?,
                            );
                            let key = ModelWeightedKey::new(
                                healthy_model.model.clone(),
                                *endpoint_type,
                                weight,
                            );
                            let service = Dispatcher::new(
                                inner.app_state.clone(),
                                &inner.router_id,
                                &inner.router_config,
                                provider.clone(),
                            )
                            .await?;
                            if let Err(e) = inner
                                .tx
                                .send(Change::Insert(key, service))
                                .await
                            {
                                error!(error = ?e, "Failed to send insert event for healthy provider");
                            }
                        }
                    }

                    let metric_attributes =
                        [KeyValue::new("provider", provider.to_string())];
                    if is_healthy {
                        inner
                            .app_state
                            .0
                            .metrics
                            .provider_health
                            .record(1, &metric_attributes);
                    } else {
                        inner
                            .app_state
                            .0
                            .metrics
                            .provider_health
                            .record(0, &metric_attributes);
                    }
                }
            }
            BalanceConfigInner::ProviderWeighted { .. } => {
                tracing::error!(
                    "Provider weighted entries in a model weighted monitor"
                );
                return Err(InternalError::Internal.into());
            }
            BalanceConfigInner::BalancedLatency { .. } => {
                tracing::error!("P2C entries in a weighted monitor");
                return Err(InternalError::Internal.into());
            }
            BalanceConfigInner::ModelLatency { .. } => {
                tracing::error!(
                    "Model latency entries in a model weighted monitor"
                );
                return Err(InternalError::Internal.into());
            }
        }
    }

    Ok(())
}

async fn check_provider_latency_monitor(
    inner: &mut ProviderMonitorInner<ProviderKey>,
) -> Result<(), runtime::RuntimeError> {
    for (endpoint_type, balance_config) in
        inner.router_config.load_balance.as_ref()
    {
        match balance_config {
            BalanceConfigInner::BalancedLatency { providers } => {
                for provider in providers {
                    let key =
                        ProviderKey::new(provider.clone(), *endpoint_type);
                    let is_healthy = inner.check_health(provider)?;
                    let was_unhealthy = inner.unhealthy_keys.contains(&key);

                    if !is_healthy && !was_unhealthy {
                        trace!(provider = ?provider, endpoint_type = ?endpoint_type, "Provider became unhealthy, removing");
                        if let Err(e) =
                            inner.tx.send(Change::Remove(key.clone())).await
                        {
                            error!(error = ?e, "Failed to send remove event for unhealthy provider");
                        }
                        inner.unhealthy_keys.insert(key);
                    } else if is_healthy && was_unhealthy {
                        trace!(provider = ?provider, endpoint_type = ?endpoint_type, "Provider became healthy, adding back");
                        inner.unhealthy_keys.remove(&key);

                        let service = Dispatcher::new(
                            inner.app_state.clone(),
                            &inner.router_id,
                            &inner.router_config,
                            provider.clone(),
                        )
                        .await?;

                        if let Err(e) =
                            inner.tx.send(Change::Insert(key, service)).await
                        {
                            error!(error = ?e, "Failed to send insert event for healthy provider");
                        }
                    }

                    let metric_attributes =
                        [KeyValue::new("provider", provider.to_string())];
                    if is_healthy {
                        inner
                            .app_state
                            .0
                            .metrics
                            .provider_health
                            .record(1, &metric_attributes);
                    } else {
                        inner
                            .app_state
                            .0
                            .metrics
                            .provider_health
                            .record(0, &metric_attributes);
                    }
                }
            }
            BalanceConfigInner::ModelWeighted { .. } => {
                tracing::error!("Model weighted entries in a P2C monitor");
                return Err(InternalError::Internal.into());
            }
            BalanceConfigInner::ProviderWeighted { .. } => {
                tracing::error!("Weighted entries in a P2C monitor");
                return Err(InternalError::Internal.into());
            }
            BalanceConfigInner::ModelLatency { .. } => {
                tracing::error!("Model latency entries in a P2C monitor");
                return Err(InternalError::Internal.into());
            }
        }
    }

    Ok(())
}

#[allow(clippy::too_many_lines)]
async fn check_model_latency_monitor(
    inner: &mut ProviderMonitorInner<ModelKey>,
) -> Result<(), runtime::RuntimeError> {
    for (endpoint_type, balance_config) in
        inner.router_config.load_balance.as_ref()
    {
        match balance_config {
            BalanceConfigInner::ModelLatency { models } => {
                for model in models {
                    let provider =
                        model.inference_provider().ok_or_else(|| {
                            InitError::ModelIdNotRecognized(model.to_string())
                        })?;
                    let key = ModelKey::new(model.clone(), *endpoint_type);
                    let is_healthy = inner.check_health(&provider)?;
                    let was_unhealthy = inner.unhealthy_keys.contains(&key);

                    if !is_healthy && !was_unhealthy {
                        trace!(provider = ?provider, endpoint_type = ?endpoint_type, "Provider became unhealthy, removing");
                        let all_models_of_unhealthy_provider = models
                            .iter()
                            .filter(|m| {
                                m.inference_provider().as_ref()
                                    == Some(&provider)
                            })
                            .collect::<Vec<_>>();

                        // Send removal changes for all models of the unhealthy
                        // provider concurrently
                        let mut join_set = JoinSet::new();
                        for unhealthy_model in all_models_of_unhealthy_provider
                        {
                            let unhealthy_key = ModelKey::new(
                                unhealthy_model.clone(),
                                *endpoint_type,
                            );
                            let tx = inner.tx.clone();

                            inner.unhealthy_keys.insert(unhealthy_key.clone());
                            join_set.spawn(async move {
                                tx.send(Change::Remove(unhealthy_key)).await
                            });
                        }

                        // we can't use join_all because we want to avoid panics
                        while let Some(task_result) = join_set.join_next().await
                        {
                            match task_result {
                                Ok(send_result) => {
                                    if let Err(e) = send_result {
                                        error!(error = ?e, model = ?model, "Failed to send remove event for unhealthy provider model");
                                    }
                                }
                                Err(e) => {
                                    error!(error = ?e, "Task failed while sending remove event for unhealthy provider model");
                                    return Err(e.into());
                                }
                            }
                        }
                    } else if is_healthy && was_unhealthy {
                        trace!(provider = ?provider, endpoint_type = ?endpoint_type, "Provider became healthy, adding back");
                        let all_models_of_now_healthy_provider = models
                            .iter()
                            .filter(|m| {
                                m.inference_provider().as_ref()
                                    == Some(&provider)
                            })
                            .collect::<Vec<_>>();
                        inner.unhealthy_keys.remove(&key);

                        for model in all_models_of_now_healthy_provider {
                            let key =
                                ModelKey::new(model.clone(), *endpoint_type);
                            let service = Dispatcher::new(
                                inner.app_state.clone(),
                                &inner.router_id,
                                &inner.router_config,
                                provider.clone(),
                            )
                            .await?;
                            if let Err(e) = inner
                                .tx
                                .send(Change::Insert(key, service))
                                .await
                            {
                                error!(error = ?e, "Failed to send insert event for healthy provider");
                            }
                        }
                    }

                    let metric_attributes =
                        [KeyValue::new("provider", provider.to_string())];
                    if is_healthy {
                        inner
                            .app_state
                            .0
                            .metrics
                            .provider_health
                            .record(1, &metric_attributes);
                    } else {
                        inner
                            .app_state
                            .0
                            .metrics
                            .provider_health
                            .record(0, &metric_attributes);
                    }
                }
            }
            BalanceConfigInner::ModelWeighted { .. } => {
                tracing::error!("Model weighted entries in a P2C monitor");
                return Err(InternalError::Internal.into());
            }
            BalanceConfigInner::ProviderWeighted { .. } => {
                tracing::error!("Weighted entries in a P2C monitor");
                return Err(InternalError::Internal.into());
            }
            BalanceConfigInner::BalancedLatency { .. } => {
                tracing::error!(
                    "provider latency entries in a model latency monitor"
                );
                return Err(InternalError::Internal.into());
            }
        }
    }

    Ok(())
}

/// Monitors health of provider APIs and emits Change events when providers
/// become unhealthy
#[derive(Debug, Clone)]
pub struct ProviderMonitorInner<K> {
    tx: Sender<Change<K, DispatcherService>>,
    router_id: RouterId,
    router_config: Arc<RouterConfig>,
    app_state: AppState,
    unhealthy_keys: HashSet<K>,
}

impl<K> ProviderMonitorInner<K> {
    fn new(
        tx: Sender<Change<K, DispatcherService>>,
        router_id: RouterId,
        router_config: Arc<RouterConfig>,
        app_state: AppState,
    ) -> Self {
        Self {
            tx,
            router_id,
            router_config,
            app_state,
            unhealthy_keys: HashSet::default(),
        }
    }

    fn check_health(
        &self,
        provider: &InferenceProvider,
    ) -> Result<bool, InternalError> {
        let provider_endpoints = provider.endpoints();
        let config = self.app_state.config();
        let grace_period = config.discover.monitor.grace_period();
        let mut all_healthy = true;
        for endpoint in provider_endpoints {
            let endpoint_metrics =
                self.app_state.0.endpoint_metrics.health_metrics(endpoint)?;
            let requests = endpoint_metrics.request_count.total();
            match grace_period {
                GracePeriod::Requests { min_requests } => {
                    if requests < *min_requests {
                        continue;
                    }
                }
            }

            let errors = endpoint_metrics.remote_internal_error_count.total();
            let error_ratio = f64::from(errors) / f64::from(requests);

            if error_ratio > config.discover.monitor.error_threshold() {
                all_healthy = false;
            }
        }

        Ok(all_healthy)
    }
}

#[derive(Debug, Clone)]
pub struct HealthMonitor {
    app_state: AppState,
}

impl HealthMonitor {
    #[must_use]
    pub fn new(app_state: AppState) -> Self {
        Self { app_state }
    }

    pub async fn run_forever(self) -> Result<(), runtime::RuntimeError> {
        tracing::info!("starting health and uptime monitors");

        let interval_duration =
            self.app_state.config().discover.monitor.health_interval();
        let mut interval = time::interval(interval_duration);

        loop {
            interval.tick().await;
            let mut monitors = self.app_state.0.health_monitors.write().await;
            let mut check_futures = Vec::new();
            for (router_id, monitor) in monitors.iter_mut() {
                let span = tracing::info_span!("health_monitor", router_id = ?router_id);
                let check_future = async move {
                    let result = monitor.check_monitor().await;
                    if let Err(e) = &result {
                        error!(router_id = ?router_id, error = ?e, "Provider health monitor check failed");
                    }
                    result
                }.instrument(span);

                check_futures.push(check_future);
            }

            if let Err(e) = future::try_join_all(check_futures).await {
                error!(error = ?e, "Provider health monitor encountered an error");
                return Err(e);
            }
        }
    }
}

impl meltdown::Service for HealthMonitor {
    type Future = BoxFuture<'static, Result<(), RuntimeError>>;

    fn run(self, mut token: Token) -> Self::Future {
        Box::pin(async move {
            tokio::select! {
                result = self.run_forever() => {
                    if let Err(e) = result {
                        error!(name = "provider-health-monitor-task", error = ?e, "Monitor encountered error, shutting down");
                    } else {
                        debug!(name = "provider-health-monitor-task", "Monitor shut down successfully");
                    }
                    token.trigger();
                }
                () = &mut token => {
                    debug!(name = "provider-health-monitor-task", "task shut down successfully");
                }
            }
            Ok(())
        })
    }
}

impl AppState {
    pub async fn add_provider_weighted_router_health_monitor(
        &self,
        router_id: RouterId,
        router_config: Arc<RouterConfig>,
        tx: Sender<Change<ProviderWeightedKey, DispatcherService>>,
    ) {
        self.0.health_monitors.write().await.insert(
            router_id.clone(),
            ProviderHealthMonitor::provider_weighted(
                tx,
                router_id,
                router_config,
                self.clone(),
            ),
        );
    }

    pub async fn add_model_weighted_router_health_monitor(
        &self,
        router_id: RouterId,
        router_config: Arc<RouterConfig>,
        tx: Sender<Change<ModelWeightedKey, DispatcherService>>,
    ) {
        self.0.health_monitors.write().await.insert(
            router_id.clone(),
            ProviderHealthMonitor::model_weighted(
                tx,
                router_id,
                router_config,
                self.clone(),
            ),
        );
    }

    pub async fn add_provider_latency_router_health_monitor(
        &self,
        router_id: RouterId,
        router_config: Arc<RouterConfig>,
        tx: Sender<Change<ProviderKey, DispatcherService>>,
    ) {
        self.0.health_monitors.write().await.insert(
            router_id.clone(),
            ProviderHealthMonitor::provider_latency(
                tx,
                router_id,
                router_config,
                self.clone(),
            ),
        );
    }

    pub async fn add_model_latency_router_health_monitor(
        &self,
        router_id: RouterId,
        router_config: Arc<RouterConfig>,
        tx: Sender<Change<ModelKey, DispatcherService>>,
    ) {
        self.0.health_monitors.write().await.insert(
            router_id.clone(),
            ProviderHealthMonitor::model_latency(
                tx,
                router_id,
                router_config,
                self.clone(),
            ),
        );
    }
}
