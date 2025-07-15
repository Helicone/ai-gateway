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
    types::{provider::InferenceProvider, router::RouterId},
};

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct WeightedKey {
    pub provider: InferenceProvider,
    pub endpoint_type: EndpointType,
    pub weight: Weight,
}

impl WeightedKey {
    #[must_use]
    pub fn new(
        provider: InferenceProvider,
        endpoint_type: EndpointType,
        weight: Weight,
    ) -> Self {
        Self {
            provider,
            endpoint_type,
            weight,
        }
    }
}

impl DispatcherDiscovery<WeightedKey> {
    pub async fn new_weighted_provider(
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
                BalanceConfigInner::ProviderWeighted { providers } => providers,
                BalanceConfigInner::ModelWeighted { .. } => {
                    return Err(InitError::InvalidBalancer(
                        "Model weighted balancer not supported for provider \
                         weighted discovery"
                            .to_string(),
                    ));
                }
                BalanceConfigInner::BalancedLatency { .. } => {
                    return Err(InitError::InvalidBalancer(
                        "P2C balancer not supported for weighted discovery"
                            .to_string(),
                    ));
                }
                BalanceConfigInner::ModelLatency { .. } => {
                    return Err(InitError::InvalidBalancer(
                        "Model latency balancer not supported for provider \
                         weighted discovery"
                            .to_string(),
                    ));
                }
            };
            for target in weighted_balance_targets {
                let weight =
                    Weight::from(target.weight.to_f64().ok_or_else(|| {
                        InitError::InvalidWeight(target.provider.clone())
                    })?);
                let key = WeightedKey::new(
                    target.provider.clone(),
                    *endpoint_type,
                    weight,
                );
                let dispatcher = Dispatcher::new(
                    app_state.clone(),
                    router_id,
                    router_config,
                    target.provider.clone(),
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
            let discovery = DispatcherDiscovery::new_weighted_provider(
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
