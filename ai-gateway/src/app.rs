use std::{
    convert::Infallible,
    future::{Ready, ready},
    net::SocketAddr,
    sync::Arc,
    task::{Context, Poll},
};

use axum_server::{accept::NoDelayAcceptor, tls_rustls::RustlsConfig};
use futures::future::BoxFuture;
use http_cache::MokaManager;
use meltdown::Token;
use moka::future::Cache;
use opentelemetry::global;
use rustc_hash::FxHashMap as HashMap;
use telemetry::{make_span::SpanFactory, tracing::MakeRequestId};
use tokio::sync::RwLock;
use tower::{ServiceBuilder, buffer::BufferLayer, util::BoxCloneService};
use tower_http::{
    ServiceBuilderExt, add_extension::AddExtension,
    catch_panic::CatchPanicLayer, compression::CompressionLayer,
    normalize_path::NormalizePathLayer,
    sensitive_headers::SetSensitiveHeadersLayer, trace::TraceLayer,
};
use tracing::{Level, info};

use crate::{
    app_state::{AppState, InnerAppState},
    cache::{CacheClient, RedisCacheManager},
    cli,
    config::{Config, DeploymentTarget, cache::CacheStore, server::TlsConfig},
    control_plane::control_plane_state::ControlPlaneState,
    discover::monitor::{
        health::provider::HealthMonitorMap, metrics::EndpointMetricsRegistry,
        rate_limit::RateLimitMonitorMap,
    },
    error::{init::InitError, runtime::RuntimeError},
    logger::service::JawnClient,
    metrics::{self, Metrics, attribute_extractor::AttributeExtractor},
    middleware::response_headers::ResponseHeaderLayer,
    router::meta::MetaRouter,
    store::{connect, minio::BaseMinioClient, router::RouterStore},
    types::provider::ProviderKeys,
    utils::{
        catch_panic::PanicResponder, handle_error::ErrorHandlerLayer,
        health_check::HealthCheckLayer, timer::TimerLayer,
    },
};

pub(crate) const BUFFER_SIZE: usize = 1024;
const SERVICE_NAME: &str = "ai-gateway";

pub type AppResponseBody = tower_http::body::UnsyncBoxBody<
    bytes::Bytes,
    Box<
        (
            dyn std::error::Error
                + std::marker::Send
                + std::marker::Sync
                + 'static
        ),
    >,
>;
pub type AppResponse = http::Response<AppResponseBody>;

pub type BoxedServiceStack =
    BoxCloneService<crate::types::request::Request, AppResponse, Infallible>;

pub type BoxedHyperServiceStack = BoxCloneService<
    http::Request<hyper::body::Incoming>,
    AppResponse,
    Infallible,
>;

/// The top level app used to start the hyper server.
/// The middleware stack is as follows:
/// -- global --
/// 0. `CatchPanic`
/// 1. `HandleError`
/// 2. Authn/Authz
/// 3. Unauthenticated and authenticated rate limit layers
/// 4. `MetaRouter`
///
/// -- Router specific MW, must not require Clone on inner Service --
/// 5. Per User Rate Limit layer
/// 6. Per Org Rate Limit layer
/// 7. `RequestContext`
///    - Fetch dynamic request specific metadata
///    - Deserialize request body based on default provider
///    - Parse Helicone inputs
/// 8. Per model rate limit layer
///    - Based on request context, rate limit based on deserialized model target
///      from request context
/// 9. Request/Response cache
/// 10. Spend controls
/// 11. A/B testing between models and prompt versions
/// 12. Fallbacks
/// 13. `ProviderBalancer`
///
/// -- provider specific middleware --
/// 14. Per provider rate limit layer
/// 15. Mapper
///     - based on selected provider, map request body
/// 16. `ProviderRegionBalancer`
///
/// -- region specific middleware (none yet, just leaf service) --
/// 17. Dispatcher
///
/// For request processing, we need to use some dynamically added
/// request extensions. We try to aggregate most of this into the
/// `RequestContext` struct to keep things simple but for some things
/// we will use separate types to avoid needing to use `Option`s in
/// the `RequestContext` struct.
///
/// Required request extensions:
/// - `AuthContext`
///    - Added by the auth layer
///    - Removed by the request context layer and aggregated into the
///      `Arc<RequestContext>`
/// - `PathAndQuery`
///   - Added by the `MetaRouter`
///   - Used by the Mapper layer
/// - `ApiEndpoint`
///   - Added by the `Router`
///   - Used by the Mapper layer
/// - `Arc<RequestContext>`
///   - Added by the request context layer
///   - Used by many layers
/// - `RouterConfig`
///   - Added by the request context layer
///   - Used by the Mapper layer
/// - `MapperContext`
///   - Added by the `Mapper` layer
///   - Used by the Dispatcher layer
/// - `Provider`
///   - Added by the `AddExtensionLayer` in the dispatcher service stack
///   - Value is driven by the `Key` type used by the `Discovery` impl.
///   - Used by the Mapper layer
///
/// Required response extensions:
/// - Copied by the dispatcher from req to resp extensions
///   - `InferenceProvider`
///   - `Model`
///   - `RouterId`
///   - `PathAndQuery`
///   - `ApiEndpoint`
///   - `MapperContext`
///   - `AuthContext`
///   - `ProviderRequestId`
#[derive(Clone)]
pub struct App {
    pub state: AppState,
    pub service_stack: BoxedServiceStack,
}

impl tower::Service<crate::types::request::Request> for App {
    type Response = AppResponse;
    type Error = Infallible;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    #[inline]
    #[tracing::instrument(skip_all)]
    fn poll_ready(
        &mut self,
        ctx: &mut Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.service_stack.poll_ready(ctx)
    }

    #[inline]
    fn call(&mut self, req: crate::types::request::Request) -> Self::Future {
        tracing::trace!(uri = %req.uri(), method = %req.method(), version = ?req.version(), "app received request");
        self.service_stack.call(req)
    }
}

impl App {
    pub async fn new(config: Config) -> Result<Self, InitError> {
        tracing::debug!("creating app");
        let app_state = Self::build_app_state(config).await?;
        let service_stack =
            Self::build_service_stack(app_state.clone()).await?;

        let app = Self {
            state: app_state,
            service_stack,
        };

        Ok(app)
    }

    /// Initializes all the clients, managers, and other stateful components
    /// that are shared across the application. This includes setting up
    /// metrics, monitoring, caching, and API keys.
    async fn build_app_state(config: Config) -> Result<AppState, InitError> {
        let minio = BaseMinioClient::new(config.minio.clone())?;
        let (pg_pool, router_store) =
            if config.deployment_target == DeploymentTarget::Cloud {
                let pg_pool = connect(&config.database).await?;
                let router_store = RouterStore::new(pg_pool.clone())?;
                (Some(pg_pool), Some(router_store))
            } else {
                (None, None)
            };
        let jawn_http_client = JawnClient::new()?;

        let meter = global::meter(SERVICE_NAME);
        let metrics = metrics::Metrics::new(&meter);
        let endpoint_metrics = EndpointMetricsRegistry::new(&config);
        let health_monitor = HealthMonitorMap::default();
        let rate_limit_monitor = RateLimitMonitorMap::default();

        let global_rate_limit = config
            .global
            .rate_limit
            .as_ref()
            .map(|rl| {
                crate::config::rate_limit::limiter_config(&rl.limits)
                    .map(Arc::new)
            })
            .transpose()?;

        let cache_manager = setup_cache(&config, metrics.clone())?;

        let router_api_keys = if config.deployment_target
            == DeploymentTarget::Cloud
            && let Some(router_store_ref) = router_store.as_ref()
        {
            let router_api_keys =
                router_store_ref.get_all_router_keys().await?;

            Some(router_api_keys)
        } else {
            None
        };
        let provider_keys = ProviderKeys::new(&config);

        let app_state = AppState(Arc::new(InnerAppState {
            config,
            minio,
            router_store,
            pg_pool,
            jawn_http_client,
            control_plane_state: Arc::new(RwLock::new(
                ControlPlaneState::default(),
            )),
            provider_keys,
            global_rate_limit,
            router_rate_limits: RwLock::new(HashMap::default()),
            metrics,
            endpoint_metrics,
            health_monitors: health_monitor,
            rate_limit_monitors: rate_limit_monitor,
            rate_limit_senders: RwLock::new(HashMap::default()),
            rate_limit_receivers: RwLock::new(HashMap::default()),
            cache_manager,
            router_tx: RwLock::new(None),
            helicone_api_keys: RwLock::new(router_api_keys),
            router_organization_map: RwLock::new(HashMap::default()),
        }));

        Ok(app_state)
    }

    /// Constructs the application's service stack, including all middleware
    /// layers and the main router.
    async fn build_service_stack(
        app_state: AppState,
    ) -> Result<BoxedServiceStack, InitError> {
        let meter = global::meter(SERVICE_NAME);
        let otel_metrics_layer =
            tower_otel_http_metrics::HTTPMetricsLayerBuilder::builder()
                .with_meter(meter)
                .with_response_extractor::<_, axum_core::body::Body>(
                    AttributeExtractor,
                )
                .build()?;

        let router = MetaRouter::build(app_state.clone()).await?;

        let compression_layer = CompressionLayer::new()
            .gzip(true)
            .br(true)
            .deflate(true)
            .zstd(true);

        // global middleware is applied here
        let service_stack = ServiceBuilder::new()
            .layer(CatchPanicLayer::custom(PanicResponder))
            .layer(SetSensitiveHeadersLayer::new(std::iter::once(
                http::header::AUTHORIZATION,
            )))
            .layer(
                TraceLayer::new_for_http()
                    .make_span_with(SpanFactory::new(
                        Level::INFO,
                        app_state.config().telemetry.propagate,
                    ))
                    .on_body_chunk(())
                    .on_eos(()),
            )
            .layer(otel_metrics_layer)
            .set_x_request_id(MakeRequestId)
            .propagate_x_request_id()
            .layer(NormalizePathLayer::trim_trailing_slash())
            .layer(metrics::request_count::Layer::new(app_state.clone()))
            .layer(compression_layer)
            .layer(HealthCheckLayer::new())
            .layer(TimerLayer::new())
            .layer(ErrorHandlerLayer::new(app_state.clone()))
            .layer(ResponseHeaderLayer::new(
                app_state.response_headers_config(),
            ))
            .map_err(crate::error::internal::InternalError::BufferError)
            .layer(BufferLayer::new(BUFFER_SIZE))
            .layer(ErrorHandlerLayer::new(app_state.clone()))
            .service(router);

        Ok(BoxCloneService::new(service_stack))
    }
}

impl meltdown::Service for App {
    type Future = BoxFuture<'static, Result<(), RuntimeError>>;

    fn run(self, token: Token) -> Self::Future {
        Box::pin(async move {
            let app_state = self.state.clone();
            let config = app_state.config();
            let addr =
                SocketAddr::from((config.server.address, config.server.port));
            info!(address = %addr, tls = %config.server.tls, "server starting");

            let handle = axum_server::Handle::new();
            let app_factory = AppFactory::new_hyper_app(self);
            // sleep so that the banner is not printed before the server is
            // ready
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            cli::helpers::show_welcome_banner(&addr);

            match &config.server.tls {
                TlsConfig::Enabled { cert, key } => {
                    let tls_config =
                        RustlsConfig::from_pem_file(cert.clone(), key.clone())
                            .await
                            .map_err(InitError::Tls)?;

                    tokio::select! {
                        biased;
                        server_output = axum_server::bind_rustls(addr, tls_config)
                            // Why `NoDelayAcceptor`? See:
                            // https://brooker.co.za/blog/2024/05/09/nagle.html
                            .acceptor(NoDelayAcceptor)
                            .handle(handle.clone())
                            .serve(app_factory) => server_output.map_err(RuntimeError::Serve)?,
                        () = token => {
                            handle.graceful_shutdown(Some(config.server.shutdown_timeout));
                        }
                    };
                }
                TlsConfig::Disabled => {
                    tokio::select! {
                        biased;
                        server_output = axum_server::bind(addr)
                            .handle(handle.clone())
                            .serve(app_factory) => server_output.map_err(RuntimeError::Serve)?,
                        () = token => {
                            handle.graceful_shutdown(Some(config.server.shutdown_timeout));
                        }
                    };
                }
            }
            Ok(())
        })
    }
}

#[derive(Clone)]
pub struct HyperApp {
    pub state: AppState,
    pub service_stack: BoxedHyperServiceStack,
}

impl HyperApp {
    #[must_use]
    pub fn new(app: App) -> Self {
        let state = app.state.clone();
        let service_stack = ServiceBuilder::new()
            .map_request(|req: http::Request<hyper::body::Incoming>| {
                req.map(axum_core::body::Body::new)
            })
            .service(app);
        Self {
            state,
            service_stack: BoxCloneService::new(service_stack),
        }
    }
}

impl tower::Service<http::Request<hyper::body::Incoming>> for HyperApp {
    type Response = AppResponse;
    type Error = Infallible;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    #[inline]
    fn poll_ready(
        &mut self,
        ctx: &mut Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.service_stack.poll_ready(ctx)
    }

    #[inline]
    fn call(
        &mut self,
        req: http::Request<hyper::body::Incoming>,
    ) -> Self::Future {
        self.service_stack.call(req)
    }
}

#[derive(Clone)]
pub struct AppFactory<S> {
    pub state: AppState,
    pub inner: S,
}

impl<S> AppFactory<S> {
    pub fn new(state: AppState, inner: S) -> Self {
        Self { state, inner }
    }
}

impl AppFactory<HyperApp> {
    #[must_use]
    pub fn new_hyper_app(app: App) -> Self {
        Self {
            state: app.state.clone(),
            inner: HyperApp::new(app),
        }
    }
}

impl<S> tower::Service<SocketAddr> for AppFactory<S>
where
    S: Clone,
{
    type Response = AddExtension<S, SocketAddr>;
    type Error = Infallible;
    type Future = Ready<Result<Self::Response, Self::Error>>;

    #[inline]
    fn poll_ready(
        &mut self,
        _ctx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, socket: SocketAddr) -> Self::Future {
        // see: https://docs.rs/tower/latest/tower/trait.Service.html#be-careful-when-cloning-inner-services
        let mut inner = self.inner.clone();
        std::mem::swap(&mut self.inner, &mut inner);
        let svc = ServiceBuilder::new()
            .layer(tower_http::add_extension::AddExtensionLayer::new(socket))
            .service(inner);
        ready(Ok(svc))
    }
}

fn setup_moka_cache(capacity: usize, metrics: Metrics) -> MokaManager {
    let listener = move |_k, _v, cause| {
        use moka::notification::RemovalCause;
        // RemovalCause::Size means that the cache reached its maximum
        // capacity and had to evict an entry.
        //
        // For other causes, please see:
        // https://docs.rs/moka/*/moka/notification/enum.RemovalCause.html
        if cause == RemovalCause::Size {
            metrics.cache.evictions.add(1, &[]);
        }
    };

    let cache = Cache::builder()
        .max_capacity(u64::try_from(capacity).unwrap_or(u64::MAX))
        .eviction_listener(listener)
        .build();
    MokaManager::new(cache)
}

fn setup_redis_cache(
    host_url: url::Url,
) -> std::result::Result<RedisCacheManager, InitError> {
    RedisCacheManager::new(host_url)
}

fn setup_cache(
    config: &Config,
    metrics: Metrics,
) -> std::result::Result<Option<CacheClient>, InitError> {
    match &config.cache_store {
        Some(CacheStore::InMemory { max_size }) => {
            tracing::debug!("Using in-memory cache");
            let moka_manager = setup_moka_cache(*max_size, metrics);
            Ok(Some(CacheClient::Moka(moka_manager)))
        }
        Some(CacheStore::Redis { host_url }) => {
            tracing::debug!("Using redis cache");
            let redis_manager = setup_redis_cache(host_url.clone())?;
            Ok(Some(CacheClient::Redis(redis_manager)))
        }
        None => Ok(None),
    }
}
