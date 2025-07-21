use std::{
    collections::HashMap,
    sync::Arc,
    task::{Context, Poll},
};

use futures::future::BoxFuture;
use tokio::sync::mpsc::Receiver;
use tokio_stream::wrappers::ReceiverStream;
use tower::{
    Service,
    discover::Change,
    load::{CompleteOnResponse, PeakEwmaDiscover},
};

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
    types::{
        model_id::{ModelId, ModelName},
        router::RouterId,
    },
};

#[derive(Debug, Clone, Eq, PartialEq, Hash)]
pub struct Key {
    pub model_id: ModelId,
    pub endpoint_type: EndpointType,
}

impl Key {
    #[must_use]
    pub fn new(model_id: ModelId, endpoint_type: EndpointType) -> Self {
        Self {
            model_id,
            endpoint_type,
        }
    }
}

impl From<Key> for ModelName<'_> {
    fn from(key: Key) -> Self {
        key.model_id.as_model_name_owned()
    }
}

impl DispatcherDiscovery<Key> {
    pub async fn new_model(
        app_state: &AppState,
        router_id: &RouterId,
        router_config: &Arc<RouterConfig>,
        rx: Receiver<Change<Key, DispatcherService>>,
    ) -> Result<Self, InitError> {
        let events = ReceiverStream::new(rx);
        let mut service_map: HashMap<Key, DispatcherService> = HashMap::new();
        for (endpoint_type, balance_config) in
            router_config.load_balance.as_ref()
        {
            let BalanceConfigInner::ModelLatency { models } = balance_config
            else {
                return Err(InitError::InvalidBalancer(
                    "incorrect dispatch discovery type used with model \
                     latency config"
                        .to_string(),
                ));
            };
            for model in models {
                let key = Key::new(model.clone(), *endpoint_type);
                let provider = model.inference_provider().ok_or_else(|| {
                    InitError::ModelIdNotRecognized(model.to_string())
                })?;
                let dispatcher = Dispatcher::new_with_model_id(
                    app_state.clone(),
                    router_id,
                    router_config,
                    provider,
                    model.clone(),
                )
                .await?;
                service_map.insert(key, dispatcher);
            }
        }

        tracing::debug!("created model discoverer");
        Ok(Self {
            initial: ServiceMap::new(service_map),
            events,
        })
    }
}

impl Service<Receiver<Change<Key, DispatcherService>>>
    for DispatcherDiscoverFactory
{
    type Response = PeakEwmaDiscover<DispatcherDiscovery<Key>>;
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
        rx: Receiver<Change<Key, DispatcherService>>,
    ) -> Self::Future {
        let app_state = self.app_state.clone();
        let router_id = self.router_id.clone();
        let router_config = self.router_config.clone();
        Box::pin(async move {
            let discovery = DispatcherDiscovery::new_model(
                &app_state,
                &router_id,
                &router_config,
                rx,
            )
            .await?;
            let discovery = PeakEwmaDiscover::new(
                discovery,
                app_state.0.config.discover.default_rtt,
                app_state.0.config.discover.discover_decay,
                CompleteOnResponse::default(),
            );

            Ok(discovery)
        })
    }
}
