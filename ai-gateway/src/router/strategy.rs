use std::{
    sync::Arc,
    task::{Context, Poll},
};

use futures::{Future, ready};
use pin_project_lite::pin_project;
use tokio::sync::mpsc::channel;
use tower::{Service, balance::p2c::Balance, load::PeakEwmaDiscover};
use weighted_balance::{balance::WeightedBalance, weight::WeightedDiscover};

use crate::{
    app_state::AppState,
    config::{balance::BalanceConfigInner, router::RouterConfig},
    discover::{
        dispatcher::{DispatcherDiscovery, factory::DispatcherDiscoverFactory},
        model, provider,
    },
    error::{api::ApiError, init::InitError, internal::InternalError},
    types::{request::Request, response::Response, router::RouterId},
};

const CHANNEL_CAPACITY: usize = 16;

#[derive(Debug)]
pub enum RoutingStrategyService {
    /// Strategy:
    /// 1. receive request
    /// 2. pick two random providers
    /// 3. compare their latency, pick the lower one
    /// 4. if provider with lowest latency does not have requested model, map it
    ///    to a model offered by the target provider.
    /// 5. send request
    ProviderLatencyPeakEwmaP2C(
        Balance<
            PeakEwmaDiscover<DispatcherDiscovery<provider::key::Key>>,
            Request,
        >,
    ),
    /// Strategy:
    /// 1. receive request
    /// 2. according to configured weighted distribution, randomly sample a
    ///    single provider from the set of providers.
    /// 3. if the provider does not have requested model, map it to a model
    ///    offered by the target provider.
    /// 4. send request
    WeightedProvider(
        WeightedBalance<
            WeightedDiscover<
                DispatcherDiscovery<provider::weighted_key::WeightedKey>,
            >,
            Request,
        >,
    ),
    /// Strategy:
    /// 1. receive request
    /// 2. according to configured weighted distribution, randomly sample a
    ///    single (provider, model) from the set of (provider, model) pairs.
    /// 3. send request
    WeightedModel(
        WeightedBalance<
            WeightedDiscover<
                DispatcherDiscovery<model::weighted_key::WeightedKey>,
            >,
            Request,
        >,
    ),
}

impl RoutingStrategyService {
    pub async fn new(
        app_state: AppState,
        router_id: RouterId,
        router_config: Arc<RouterConfig>,
        balance_config: &BalanceConfigInner,
    ) -> Result<RoutingStrategyService, InitError> {
        match balance_config {
            BalanceConfigInner::ProviderWeighted { .. } => {
                Self::provider_weighted(app_state, router_id, router_config)
                    .await
            }
            BalanceConfigInner::BalancedLatency { .. } => {
                Self::peak_ewma(app_state, router_id, router_config).await
            }
            BalanceConfigInner::ModelWeighted { .. } => {
                Self::model_weighted(app_state, router_id, router_config).await
            }
        }
    }

    async fn provider_weighted(
        app_state: AppState,
        router_id: RouterId,
        router_config: Arc<RouterConfig>,
    ) -> Result<RoutingStrategyService, InitError> {
        tracing::debug!("Creating provider weighted balancer");
        let (change_tx, change_rx) = channel(CHANNEL_CAPACITY);
        let (rate_limit_tx, rate_limit_rx) = channel(CHANNEL_CAPACITY);
        let discover_factory = DispatcherDiscoverFactory::new(
            app_state.clone(),
            router_id.clone(),
            router_config.clone(),
        );
        app_state
            .add_provider_weighted_router_health_monitor(
                router_id.clone(),
                router_config.clone(),
                change_tx.clone(),
            )
            .await;
        app_state
            .add_rate_limit_tx(router_id.clone(), rate_limit_tx)
            .await;
        app_state
            .add_rate_limit_rx(router_id.clone(), rate_limit_rx)
            .await;
        app_state
            .add_provider_weighted_router_rate_limit_monitor(
                router_id.clone(),
                router_config,
                change_tx,
            )
            .await;
        let mut balance_factory =
            weighted_balance::balance::make::MakeBalance::new(discover_factory);
        let balance = balance_factory.call(change_rx).await?;
        let provider_balancer =
            RoutingStrategyService::WeightedProvider(balance);

        Ok(provider_balancer)
    }

    async fn model_weighted(
        app_state: AppState,
        router_id: RouterId,
        router_config: Arc<RouterConfig>,
    ) -> Result<RoutingStrategyService, InitError> {
        tracing::debug!("Creating model weighted balancer");
        let (change_tx, change_rx) = channel(CHANNEL_CAPACITY);
        let (rate_limit_tx, rate_limit_rx) = channel(CHANNEL_CAPACITY);
        let discover_factory = DispatcherDiscoverFactory::new(
            app_state.clone(),
            router_id.clone(),
            router_config.clone(),
        );
        app_state
            .add_model_weighted_router_health_monitor(
                router_id.clone(),
                router_config.clone(),
                change_tx.clone(),
            )
            .await;
        app_state
            .add_rate_limit_tx(router_id.clone(), rate_limit_tx)
            .await;
        app_state
            .add_rate_limit_rx(router_id.clone(), rate_limit_rx)
            .await;
        app_state
            .add_model_weighted_router_rate_limit_monitor(
                router_id.clone(),
                router_config,
                change_tx,
            )
            .await;
        let mut balance_factory =
            weighted_balance::balance::make::MakeBalance::new(discover_factory);
        let balance = balance_factory.call(change_rx).await?;
        let provider_balancer = RoutingStrategyService::WeightedModel(balance);

        Ok(provider_balancer)
    }

    async fn peak_ewma(
        app_state: AppState,
        router_id: RouterId,
        router_config: Arc<RouterConfig>,
    ) -> Result<RoutingStrategyService, InitError> {
        tracing::debug!("Creating peak ewma p2c balancer");
        let (change_tx, change_rx) = channel(CHANNEL_CAPACITY);
        let (rate_limit_tx, rate_limit_rx) = channel(CHANNEL_CAPACITY);
        let discover_factory = DispatcherDiscoverFactory::new(
            app_state.clone(),
            router_id.clone(),
            router_config.clone(),
        );
        app_state
            .add_p2c_router_health_monitor(
                router_id.clone(),
                router_config.clone(),
                change_tx.clone(),
            )
            .await;
        app_state
            .add_rate_limit_tx(router_id.clone(), rate_limit_tx)
            .await;
        app_state
            .add_rate_limit_rx(router_id.clone(), rate_limit_rx)
            .await;
        app_state
            .add_p2c_router_rate_limit_monitor(
                router_id.clone(),
                router_config,
                change_tx,
            )
            .await;
        let mut balance_factory =
            tower::balance::p2c::MakeBalance::new(discover_factory);
        let balance = balance_factory.call(change_rx).await?;
        let provider_balancer =
            RoutingStrategyService::ProviderLatencyPeakEwmaP2C(balance);

        Ok(provider_balancer)
    }
}

impl tower::Service<Request> for RoutingStrategyService {
    type Response = Response;
    type Error = ApiError;
    type Future = ResponseFuture;

    #[inline]
    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        match self {
            RoutingStrategyService::ProviderLatencyPeakEwmaP2C(inner) => {
                inner.poll_ready(cx)
            }
            RoutingStrategyService::WeightedProvider(inner) => {
                inner.poll_ready(cx)
            }
            RoutingStrategyService::WeightedModel(inner) => {
                inner.poll_ready(cx)
            }
        }
        .map_err(InternalError::PollReadyError)
        .map_err(Into::into)
    }

    fn call(&mut self, req: Request) -> Self::Future {
        tracing::trace!("ProviderBalancer");
        match self {
            RoutingStrategyService::ProviderLatencyPeakEwmaP2C(inner) => {
                ResponseFuture::PeakEwma {
                    future: inner.call(req),
                }
            }
            RoutingStrategyService::WeightedProvider(inner) => {
                ResponseFuture::ProviderWeighted {
                    future: inner.call(req),
                }
            }
            RoutingStrategyService::WeightedModel(inner) => {
                ResponseFuture::ModelWeighted {
                    future: inner.call(req),
                }
            }
        }
    }
}

pin_project! {
    #[project = EnumProj]
    pub enum ResponseFuture {
        PeakEwma {
            #[pin]
            future: <
                Balance<PeakEwmaDiscover<DispatcherDiscovery<provider::key::Key>>, Request> as tower::Service<
                    Request,
                >
            >::Future,
        },
        ProviderWeighted {
            #[pin]
            future: <
                WeightedBalance<WeightedDiscover<DispatcherDiscovery<provider::weighted_key::WeightedKey>>, Request> as tower::Service<
                    Request,
                >
            >::Future,
        },
        ModelWeighted {
            #[pin]
            future: <
                WeightedBalance<WeightedDiscover<DispatcherDiscovery<model::weighted_key::WeightedKey>>, Request> as tower::Service<
                    Request,
                >
            >::Future,
        },
    }
}

impl Future for ResponseFuture {
    type Output = Result<Response, ApiError>;

    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Self::Output> {
        match self.project() {
            EnumProj::PeakEwma { future } => Poll::Ready(ready!(
                future
                    .poll(cx)
                    .map_err(InternalError::LoadBalancerError)
                    .map_err(Into::into)
            )),
            EnumProj::ProviderWeighted { future }
            | EnumProj::ModelWeighted { future } => Poll::Ready(ready!(
                future
                    .poll(cx)
                    .map_err(InternalError::LoadBalancerError)
                    .map_err(Into::into)
            )),
        }
    }
}
