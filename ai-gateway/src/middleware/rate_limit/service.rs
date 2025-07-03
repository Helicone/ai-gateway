use std::{
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use governor::middleware::StateInformationMiddleware;
use http::Response;
use tower_governor::{GovernorLayer, governor::GovernorConfigBuilder};

use super::extractor::RateLimitKeyExtractor;
use crate::{
    app_state::AppState,
    config::{
        rate_limit::{
            LimitsConfig, RateLimitStore, RateLimiterConfig,
            default_refill_frequency,
        },
        router::{RouterConfig, RouterRateLimitConfig},
    },
    error::init::InitError,
    middleware::rate_limit::redis_service::{
        RedisRateLimitLayer, RedisRateLimitService,
    },
    types::router::RouterId,
};

pub type OptionalGovernorLayer =
    Option<GovernorLayer<RateLimitKeyExtractor, StateInformationMiddleware>>;
pub type GovernorService<S> = tower_governor::governor::Governor<
    RateLimitKeyExtractor,
    StateInformationMiddleware,
    S,
>;

#[derive(Clone)]
pub enum InnerLayer {
    None,
    InMemory(GovernorLayer<RateLimitKeyExtractor, StateInformationMiddleware>),
    Redis(RedisRateLimitLayer),
}

#[derive(Clone)]
pub struct Layer {
    inner: InnerLayer,
}

impl Layer {
    /// Create a new rate limit layer to be applied globally.
    #[must_use]
    pub fn global(app_state: &AppState) -> Self {
        if let Some(rate_limit_config) = &app_state.0.config.global.rate_limit {
            if let RateLimitStore::Redis(redis_config) =
                &rate_limit_config.store
            {
                Self::new_redis_inner(
                    rate_limit_config.limits.clone(),
                    url::Url::parse(redis_config.url.expose())
                        .map_err(|_e| {
                            InitError::InvalidRateLimitConfig(
                                "Invalid redis url",
                            )
                        })
                        .unwrap(),
                )
            } else {
                Self::new_in_memory_inner(app_state.0.global_rate_limit.clone())
            }
        } else {
            Self {
                inner: InnerLayer::None,
            }
        }
    }

    #[must_use]
    fn new_redis_inner(rl: Option<LimitsConfig>, url: url::Url) -> Self {
        if let Some(rl) = rl
            && let Ok(layer) = RedisRateLimitLayer::new(Arc::new(rl), url, None)
        {
            Self {
                inner: InnerLayer::Redis(layer),
            }
        } else {
            Self {
                inner: InnerLayer::None,
            }
        }
    }

    #[must_use]
    fn new_in_memory_inner(rl: Option<Arc<RateLimiterConfig>>) -> Self {
        if let Some(rl) = rl {
            Self {
                inner: InnerLayer::InMemory(GovernorLayer { config: rl }),
            }
        } else {
            Self {
                inner: InnerLayer::None,
            }
        }
    }

    /// For when we statically know that rate limiting is disabled.
    #[must_use]
    pub fn disabled() -> Self {
        Self {
            inner: InnerLayer::None,
        }
    }

    pub async fn per_router(
        app_state: &AppState,
        router_id: RouterId,
        router_config: &RouterConfig,
    ) -> Result<Self, InitError> {
        match &router_config.rate_limit {
            RouterRateLimitConfig::None => Ok(Self {
                inner: InnerLayer::None,
            }),
            RouterRateLimitConfig::Custom { store, limits } => {
                if let Some(store) = store
                    && let RateLimitStore::Redis(redis_config) = store
                    && let Ok(layer) = RedisRateLimitLayer::new(
                        Arc::new(limits.clone()),
                        url::Url::parse(redis_config.url.expose())
                            .map_err(|_e| {
                                InitError::InvalidRateLimitConfig(
                                    "Invalid redis url",
                                )
                            })
                            .map_err(|e| {
                                tracing::error!(
                                    "error creating redis layer: {:?}",
                                    e
                                );
                                e
                            })?,
                        Some(router_id.clone()),
                    )
                {
                    return Ok(Self {
                        inner: InnerLayer::Redis(layer),
                    });
                }
                let gcra = &limits.per_api_key;
                let per_cell_duration = gcra
                    .refill_frequency
                    .checked_div(gcra.capacity.into())
                    .unwrap_or_else(|| {
                        tracing::warn!(
                            "fill_frequency is too small for capacity, using \
                             default fill frequency"
                        );
                        default_refill_frequency()
                    });

                let rl = GovernorConfigBuilder::default()
                    .period(per_cell_duration)
                    .burst_size(gcra.capacity.get())
                    .use_headers()
                    .key_extractor(RateLimitKeyExtractor)
                    .finish()
                    .ok_or(InitError::InvalidRateLimitConfig(
                        "Invalid rate limit config",
                    ))?;
                let rl = Arc::new(rl);
                add_rate_limit_to_app_state(app_state, router_id, rl.clone())
                    .await;

                Ok(Self {
                    inner: InnerLayer::InMemory(GovernorLayer {
                        config: rl.clone(),
                    }),
                })
            }
        }
    }
}

async fn add_rate_limit_to_app_state(
    app_state: &AppState,
    router_id: RouterId,
    rl_config: Arc<RateLimiterConfig>,
) {
    let mut write_guard = app_state.0.router_rate_limits.write().await;
    write_guard.insert(router_id, rl_config);
}

impl<S> tower::layer::Layer<S> for Layer {
    type Service = Service<S>;

    fn layer(&self, service: S) -> Self::Service {
        match &self.inner {
            InnerLayer::InMemory(inner) => Service::InMemory {
                service: inner.layer(service),
            },
            InnerLayer::Redis(inner) => Service::Redis {
                service: inner.layer(service),
            },
            InnerLayer::None => Service::Disabled { service },
        }
    }
}

#[derive(Debug, Clone)]
pub enum Service<S> {
    Disabled { service: S },
    InMemory { service: GovernorService<S> },
    Redis { service: RedisRateLimitService<S> },
}

pin_project_lite::pin_project! {
    #[derive(Debug)]
    #[project = EnumProj]
    pub enum ResponseFuture<InMemoryFuture, RedisFuture, DisabledFuture> {
        InMemory { #[pin] future: InMemoryFuture },
        Redis { #[pin] future: RedisFuture },
        Disabled { #[pin] future: DisabledFuture },
    }
}

// add a second to the retry after header to prevent rounding errors
fn increment_retry_after_header<ResponseBody>(
    res: &mut http::Response<ResponseBody>,
) {
    if let Some(retry_after) = res.headers().get("Retry-After") {
        if let Some(retry_after_value) = retry_after
            .to_str()
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
        {
            tracing::info!("the retry after is: {}", retry_after_value);
            let new_retry_after = retry_after_value + 1;
            tracing::info!("setting the retry after to: {}", new_retry_after);
            res.headers_mut().insert(
                "Retry-After",
                new_retry_after.to_string().parse().unwrap(),
            );
            res.headers_mut().insert(
                "X-Ratelimit-After",
                new_retry_after.to_string().parse().unwrap(),
            );
        }
    }
}

impl<InMemoryFuture, RedisFuture, DisabledFuture, ResponseBody, Error> Future
    for ResponseFuture<InMemoryFuture, RedisFuture, DisabledFuture>
where
    InMemoryFuture: Future<Output = Result<Response<ResponseBody>, Error>>,
    RedisFuture: Future<Output = Result<Response<ResponseBody>, Error>>,
    DisabledFuture: Future<Output = Result<Response<ResponseBody>, Error>>,
{
    type Output = Result<Response<ResponseBody>, Error>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.project() {
            EnumProj::InMemory { future } => {
                let result = std::task::ready!(future.poll(cx));
                if let Ok(mut res) = result {
                    increment_retry_after_header(&mut res);
                    Poll::Ready(Ok(res))
                } else {
                    Poll::Ready(result)
                }
            }
            EnumProj::Redis { future } => future.poll(cx),
            EnumProj::Disabled { future } => future.poll(cx),
        }
    }
}

impl<S, Request, ResponseBody> tower::Service<Request> for Service<S>
where
    S: tower::Service<Request, Response = Response<ResponseBody>>,
    GovernorService<S>: tower::Service<
            Request,
            Response = Response<ResponseBody>,
            Error = S::Error,
        >,
    RedisRateLimitService<S>: tower::Service<
            Request,
            Response = Response<ResponseBody>,
            Error = S::Error,
        >,
{
    type Response = Response<ResponseBody>;
    type Error = S::Error;
    type Future = ResponseFuture<
        <GovernorService<S> as tower::Service<Request>>::Future,
        <RedisRateLimitService<S> as tower::Service<Request>>::Future,
        S::Future,
    >;

    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        match self {
            Service::InMemory { service } => service.poll_ready(cx),
            Service::Redis { service } => service.poll_ready(cx),
            Service::Disabled { service } => service.poll_ready(cx),
        }
    }

    fn call(&mut self, request: Request) -> Self::Future {
        match self {
            Service::InMemory { service } => ResponseFuture::InMemory {
                future: service.call(request),
            },
            Service::Redis { service } => ResponseFuture::Redis {
                future: service.call(request),
            },
            Service::Disabled { service } => ResponseFuture::Disabled {
                future: service.call(request),
            },
        }
    }
}

#[cfg(all(test, feature = "testing"))]
mod tests {
    use std::{num::NonZeroU32, time::Duration};

    use super::*;
    use crate::{
        app_state::AppState,
        config::{
            Config,
            rate_limit::{
                GcraConfig, GlobalRateLimitConfig, LimitsConfig, RateLimitStore,
            },
            router::{RouterConfig, RouterRateLimitConfig},
        },
        tests::TestDefault,
        types::router::RouterId,
    };

    async fn create_test_app_state(
        rate_limit_config: GlobalRateLimitConfig,
    ) -> AppState {
        let mut config = Config::test_default();
        config.global.rate_limit = Some(rate_limit_config);
        let app = crate::app::App::new(config)
            .await
            .expect("failed to create app");
        app.state
    }

    fn create_test_limits() -> LimitsConfig {
        LimitsConfig {
            per_api_key: GcraConfig {
                capacity: NonZeroU32::new(10).unwrap(),
                refill_frequency: Duration::from_secs(1),
            },
        }
    }

    fn create_router_config(rate_limit: RouterRateLimitConfig) -> RouterConfig {
        RouterConfig {
            rate_limit,
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn global_app_with_none_router() {
        let app_state = create_test_app_state(GlobalRateLimitConfig {
            store: RateLimitStore::InMemory,
            limits: Some(create_test_limits()),
            cleanup_interval: Duration::from_secs(300),
        })
        .await;
        let router_config = create_router_config(RouterRateLimitConfig::None);

        let result =
            Layer::per_router(&app_state, RouterId::Default, &router_config)
                .await;
        assert!(result.is_ok());
        assert!(matches!(result.unwrap().inner, InnerLayer::None));
    }

    #[tokio::test]
    async fn global_app_with_custom_router() {
        let app_state = create_test_app_state(GlobalRateLimitConfig {
            store: RateLimitStore::InMemory,
            limits: Some(create_test_limits()),
            cleanup_interval: Duration::from_secs(300),
        })
        .await;
        let router_config =
            create_router_config(RouterRateLimitConfig::Custom {
                store: Some(RateLimitStore::InMemory),
                limits: create_test_limits(),
            });

        let result =
            Layer::per_router(&app_state, RouterId::Default, &router_config)
                .await;
        assert!(result.is_ok());
        assert!(matches!(result.unwrap().inner, InnerLayer::InMemory(_)));
    }

    #[tokio::test]
    async fn router_specific_app_with_custom_router() {
        let app_state = create_test_app_state(GlobalRateLimitConfig {
            store: RateLimitStore::InMemory,
            limits: None,
            cleanup_interval: Duration::from_secs(300),
        })
        .await;
        let router_config =
            create_router_config(RouterRateLimitConfig::Custom {
                store: Some(RateLimitStore::InMemory),
                limits: create_test_limits(),
            });

        let result =
            Layer::per_router(&app_state, RouterId::Default, &router_config)
                .await;
        assert!(result.is_ok());
        assert!(matches!(result.unwrap().inner, InnerLayer::InMemory(_)));
    }
}
