use std::task::{Context, Poll};

use futures::future::BoxFuture;
use tokio::sync::mpsc::Receiver;
use tower::{Service, discover::Change};
use weighted_balance::weight::{HasWeight, Weight, WeightedDiscover};

use crate::{
    discover::dispatcher::{
        DispatcherDiscovery, factory::DispatcherDiscoverFactory,
    },
    dispatcher::DispatcherService,
    endpoints::EndpointType,
    error::init::InitError,
    types::provider::InferenceProvider,
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
            let discovery = DispatcherDiscovery::new_weighted(
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
