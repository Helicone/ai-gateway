use std::{
    collections::HashMap,
    sync::Arc,
    task::{Context, Poll},
};

use futures::future::BoxFuture;
use rust_decimal::prelude::ToPrimitive;
use tokio::sync::mpsc::Receiver;
use tokio_stream::wrappers::ReceiverStream;
use tower::{Service, discover::Change};
use weighted_balance::weight::{HasWeight, Weight, WeightedDiscover};

use crate::{
    app_state::AppState,
    config::{balance::BalanceConfigInner, router::RouterConfig},
    discover::{
        ServiceMap,
        dispatcher::{DispatcherDiscovery, factory::DispatcherDiscoverFactory},
    },
    dispatcher::{Dispatcher, DispatcherService},
    endpoints::EndpointType,
    error::init::InitError,
    types::{model_id::ModelId, router::RouterId},
};

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct WeightedKey {
    pub model_id: ModelId,
    pub endpoint_type: EndpointType,
    pub weight: Weight,
}

impl WeightedKey {
    #[must_use]
    pub fn new(
        model_id: ModelId,
        endpoint_type: EndpointType,
        weight: Weight,
    ) -> Self {
        Self {
            model_id,
            endpoint_type,
            weight,
        }
    }
}

impl DispatcherDiscovery<WeightedKey> {
    pub async fn new_weighted_model(
        app_state: &AppState,
        router_id: &RouterId,
        router_config: &Arc<RouterConfig>,
        rx: Receiver<Change<WeightedKey, DispatcherService>>,
    ) -> Result<Self, InitError> {
        let mut service_map = HashMap::new();
        for (endpoint_type, balance_config) in
            router_config.load_balance.as_ref()
        {
            let weighted_balance_targets = match balance_config {
                BalanceConfigInner::ProviderWeighted { .. } => {
                    return Err(InitError::InvalidBalancer(
                        "Provider weighted balancer not supported for model \
                         weighted discovery"
                            .to_string(),
                    ));
                }
                BalanceConfigInner::ModelWeighted { models } => models,
                BalanceConfigInner::BalancedLatency { .. } => {
                    return Err(InitError::InvalidBalancer(
                        "P2C balancer not supported for weighted discovery"
                            .to_string(),
                    ));
                }
            };
            for target_model_id in weighted_balance_targets {
                let provider = target_model_id
                    .model
                    .inference_provider()
                    .ok_or_else(|| {
                        InitError::ModelIdNotRecognized(
                            target_model_id.model.to_string(),
                        )
                    })?;
                let weight =
                    Weight::from(target_model_id.weight.to_f64().ok_or_else(
                        || InitError::InvalidWeight(provider.clone()),
                    )?);
                let key = WeightedKey::new(
                    target_model_id.model.clone(),
                    *endpoint_type,
                    weight,
                );
                let dispatcher = Dispatcher::new_with_model_id(
                    app_state.clone(),
                    router_id,
                    router_config,
                    provider,
                    target_model_id.model.clone(),
                )
                .await?;
                service_map.insert(key, dispatcher);
            }
        }
        let events = ReceiverStream::new(rx);

        tracing::debug!("Created config provider discovery");
        Ok(Self {
            initial: ServiceMap::new(service_map),
            events,
        })
    }
}

impl HasWeight for WeightedKey {
    fn weight(&self) -> Weight {
        self.weight
    }
}

impl Service<Receiver<Change<WeightedKey, DispatcherService>>>
    for DispatcherDiscoverFactory
{
    type Response = WeightedDiscover<DispatcherDiscovery<WeightedKey>>;
    type Error = InitError;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(
        &mut self,
        _: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(
        &mut self,
        rx: Receiver<Change<WeightedKey, DispatcherService>>,
    ) -> Self::Future {
        let app_state = self.app_state.clone();
        let router_id = self.router_id.clone();
        let router_config = self.router_config.clone();
        Box::pin(async move {
            let discovery = DispatcherDiscovery::new_weighted_model(
                &app_state,
                &router_id,
                &router_config,
                rx,
            )
            .await?;
            let discovery = WeightedDiscover::new(discovery);
            Ok(discovery)
        })
    }
}
