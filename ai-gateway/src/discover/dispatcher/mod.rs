pub mod factory;

use std::{
    collections::HashMap,
    convert::Infallible,
    hash::Hash,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use futures::Stream;
use pin_project_lite::pin_project;
use rust_decimal::prelude::ToPrimitive;
use tokio::sync::mpsc::Receiver;
use tokio_stream::wrappers::ReceiverStream;
use tower::discover::Change;
use weighted_balance::weight::Weight;

use crate::{
    app_state::AppState,
    config::{balance::BalanceConfigInner, router::RouterConfig},
    discover::{ServiceMap, provider::weighted_key::WeightedKey},
    dispatcher::{Dispatcher, DispatcherService},
    error::init::InitError,
    types::router::RouterId,
};

pin_project! {
    /// Reads available models and providers from the config file.
    ///
    /// We can additionally dynamically remove providers from the balancer
    /// if they hit certain failure thresholds by using a layer like:
    ///
    /// ```rust,ignore
    /// #[derive(Clone)]
    /// pub struct FailureWatcherLayer {
    ///     key: usize,
    ///     registry: tokio::sync::watch::Sender<HashMap<usize, DispatcherService>>,
    ///     failure_limit: u32,
    ///     window: Duration,
    /// }
    /// ```
    ///
    /// the layer would then send `Change::Remove` events to this discovery struct
    #[derive(Debug)]
    pub struct DispatcherDiscovery<K> {
        #[pin]
        pub(super) initial: ServiceMap<K, DispatcherService>,
        #[pin]
        pub(super) events: ReceiverStream<Change<K, DispatcherService>>,
    }
}

impl DispatcherDiscovery<WeightedKey> {
    pub async fn new_weighted(
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
                BalanceConfigInner::Weighted { providers } => providers,
                BalanceConfigInner::Latency { .. } => {
                    return Err(InitError::InvalidWeightedBalancer(
                        "P2C balancer not supported for weighted discovery"
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

impl<K> Stream for DispatcherDiscovery<K>
where
    K: Hash + Eq + Clone + std::fmt::Debug,
{
    type Item = Result<Change<K, DispatcherService>, Infallible>;

    fn poll_next(
        self: Pin<&mut Self>,
        ctx: &mut Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        let mut this = self.project();

        // 1) one‑time inserts, once the ServiceMap returns `Poll::Ready(None)`,
        //    then the service map is empty
        if let Poll::Ready(Some(change)) = this.initial.as_mut().poll_next(ctx)
        {
            return handle_change(change);
        }

        // 2) live events (removals / re‑inserts)
        match this.events.as_mut().poll_next(ctx) {
            Poll::Ready(Some(change)) => handle_change(change),
            Poll::Pending => Poll::Pending,
            Poll::Ready(None) => Poll::Ready(None),
        }
    }
}

fn handle_change<K>(
    change: Change<K, DispatcherService>,
) -> Poll<Option<Result<Change<K, DispatcherService>, Infallible>>>
where
    K: std::fmt::Debug,
{
    match change {
        Change::Insert(key, service) => {
            tracing::debug!(key = ?key, "Discovered new provider");
            Poll::Ready(Some(Ok(Change::Insert(key, service))))
        }
        Change::Remove(key) => {
            tracing::debug!(key = ?key, "Removed provider");
            Poll::Ready(Some(Ok(Change::Remove(key))))
        }
    }
}
