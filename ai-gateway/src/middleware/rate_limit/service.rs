use std::{
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use governor::middleware::StateInformationMiddleware;
use http::Response;
use tower_governor::GovernorLayer;

use super::extractor::RateLimitKeyExtractor;
use crate::{
    app_state::AppState,
    config::{
        rate_limit::{
            LimitsConfig, RateLimitConfig, RateLimitStore, RateLimiterConfig,
        },
        router::RouterConfig,
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
    pub fn global(app_state: &AppState) -> Result<Self, InitError> {
        if let Some(rate_limit_config) = &app_state.config().global.rate_limit {
            let store_config =
                app_state.config().rate_limit_store.as_ref().ok_or(
                    InitError::InvalidRateLimitConfig("store not configured"),
                )?;
            if let RateLimitStore::Redis(redis_config) = &store_config {
                Ok(Self::new_redis_inner(
                    rate_limit_config.limits.clone(),
                    redis_config.host_url.expose().clone(),
                ))
            } else {
                Ok(Self::new_in_memory_inner(
                    app_state.0.global_rate_limit.clone(),
                ))
            }
        } else {
            Ok(Self {
                inner: InnerLayer::None,
            })
        }
    }

    /// Create a new rate limit layer to be applied to all requests to the
    /// unified api.
    pub fn unified_api(app_state: &AppState) -> Result<Self, InitError> {
        if let Some(rate_limit_config) =
            &app_state.config().unified_api.rate_limit
        {
            let store_config =
                app_state.config().rate_limit_store.as_ref().ok_or(
                    InitError::InvalidRateLimitConfig("store not configured"),
                )?;
            if let RateLimitStore::Redis(redis_config) = &store_config {
                Ok(Self::new_redis_inner(
                    rate_limit_config.limits.clone(),
                    redis_config.host_url.expose().clone(),
                ))
            } else {
                Ok(Self::new_in_memory_inner(
                    app_state.0.global_rate_limit.clone(),
                ))
            }
        } else {
            Ok(Self {
                inner: InnerLayer::None,
            })
        }
    }

    #[must_use]
    fn new_redis_inner(rl: LimitsConfig, url: url::Url) -> Self {
        if let Ok(layer) = RedisRateLimitLayer::new(Arc::new(rl), url, None) {
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
            None => Ok(Self {
                inner: InnerLayer::None,
            }),
            Some(RateLimitConfig { store, limits }) => {
                let ratelimit_store = store
                    .as_ref()
                    .or_else(|| app_state.config().rate_limit_store.as_ref())
                    .ok_or(InitError::InvalidRateLimitConfig(
                        "store not configured",
                    ))?;

                if let RateLimitStore::Redis(redis_config) = ratelimit_store
                    && let Ok(layer) = RedisRateLimitLayer::new(
                        Arc::new(limits.clone()),
                        redis_config.host_url.expose().clone(),
                        Some(router_id.clone()),
                    )
                {
                    return Ok(Self {
                        inner: InnerLayer::Redis(layer),
                    });
                }
                let rl = Arc::new(crate::config::rate_limit::limiter_config(
                    limits,
                )?);
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
    if let Some(retry_after) = res.headers().get("retry-after") {
        if let Some(retry_after_value) = retry_after
            .to_str()
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
        {
            let new_retry_after = retry_after_value + 1;
            res.headers_mut().insert(
                "retry-after",
                new_retry_after.to_string().parse().unwrap(),
            );
            res.headers_mut().insert(
                "x-ratelimit-after",
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
            Service::InMemory { service } => match service.poll_ready(cx) {
                Poll::Ready(Ok(())) => {
                    tracing::trace!("in memory rate limit ready");
                    Poll::Ready(Ok(()))
                }
                Poll::Ready(Err(e)) => Poll::Ready(Err(e)),
                Poll::Pending => Poll::Pending,
            },
            Service::Redis { service } => service.poll_ready(cx),
            Service::Disabled { service } => service.poll_ready(cx),
        }
    }

    #[tracing::instrument(name = "opt_rate_limit", skip_all)]
    fn call(&mut self, request: Request) -> Self::Future {
        match self {
            Service::InMemory { service } => {
                tracing::trace!(kind = "in_memory", "rate limit middleware");
                ResponseFuture::InMemory {
                    future: service.call(request),
                }
            }
            Service::Redis { service } => {
                tracing::trace!(kind = "redis", "rate limit middleware");
                ResponseFuture::Redis {
                    future: service.call(request),
                }
            }
            Service::Disabled { service } => ResponseFuture::Disabled {
                future: service.call(request),
            },
        }
    }
}

#[cfg(all(test, feature = "testing"))]
mod tests {
    use std::{num::NonZeroU32, time::Duration};

    use compact_str::CompactString;

    use super::*;
    use crate::{
        app_state::AppState,
        config::{
            Config,
            rate_limit::{
                GcraConfig, LimitsConfig, RateLimitConfig, RateLimitStore,
            },
            router::RouterConfig,
        },
        tests::TestDefault,
        types::router::RouterId,
    };

    async fn create_test_app_state(
        rate_limit_config: RateLimitConfig,
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

    fn create_router_config(
        rate_limit: Option<RateLimitConfig>,
    ) -> RouterConfig {
        RouterConfig {
            rate_limit,
            ..Default::default()
        }
    }

    #[tokio::test]
    async fn global_app_with_none_router() {
        let app_state = create_test_app_state(RateLimitConfig {
            store: None,
            limits: create_test_limits(),
        })
        .await;
        let router_config = create_router_config(None);

        let result = Layer::per_router(
            &app_state,
            RouterId::Named(CompactString::new("my-router")),
            &router_config,
        )
        .await;
        assert!(result.is_ok());
        assert!(matches!(result.unwrap().inner, InnerLayer::None));
    }

    #[tokio::test]
    async fn global_app_with_custom_router() {
        let app_state = create_test_app_state(RateLimitConfig {
            store: Some(RateLimitStore::InMemory),
            limits: create_test_limits(),
        })
        .await;
        let router_config = create_router_config(Some(RateLimitConfig {
            store: Some(RateLimitStore::InMemory),
            limits: create_test_limits(),
        }));

        let result = Layer::per_router(
            &app_state,
            RouterId::Named(CompactString::new("my-router")),
            &router_config,
        )
        .await;
        assert!(result.is_ok());
        assert!(matches!(result.unwrap().inner, InnerLayer::InMemory(_)));
    }

    #[tokio::test]
    async fn router_specific_app_with_custom_router() {
        let app_state = create_test_app_state(RateLimitConfig {
            store: None,
            limits: create_test_limits(),
        })
        .await;
        let router_config = create_router_config(Some(RateLimitConfig {
            store: Some(RateLimitStore::InMemory),
            limits: create_test_limits(),
        }));

        let result = Layer::per_router(
            &app_state,
            RouterId::Named(CompactString::new("my-router")),
            &router_config,
        )
        .await;
        assert!(result.is_ok());
        assert!(matches!(result.unwrap().inner, InnerLayer::InMemory(_)));
    }
}
