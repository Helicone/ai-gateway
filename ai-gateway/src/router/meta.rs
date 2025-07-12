use std::{
    convert::Infallible,
    future::{Ready, ready},
    task::{Context, Poll},
};

use dynamic_router::router::DynamicRouter;
use pin_project_lite::pin_project;
use tower::{
    Service as _, ServiceBuilder, buffer::BufferLayer, util::BoxCloneService,
};
use tower_http::auth::AsyncRequireAuthorizationLayer;

use crate::{
    app_state::AppState,
    config::DeploymentTarget,
    discover::router::{
        discover::RouterDiscovery, factory::RouterDiscoverFactory,
    },
    error::{
        api::ApiError, init::InitError, internal::InternalError,
        invalid_req::InvalidRequestError,
    },
    middleware::{
        cache::{CacheLayer, CacheService},
        rate_limit::service::{
            Layer as RateLimitLayer, Service as RateLimitService,
        },
    },
    router::{
        direct::{DirectProxiesWithoutMapper, DirectProxyServiceWithoutMapper},
        router_details::{RouteType, RouterDetailsLayer},
        unified_api,
    },
    types::{provider::InferenceProvider, router::RouterId},
    utils::handle_error::{ErrorHandler, ErrorHandlerLayer},
};

const BUFFER_SIZE: usize = 256;

pub type UnifiedApiService =
    RateLimitService<CacheService<ErrorHandler<unified_api::Service>>>;

#[derive(Debug)]
pub struct MetaRouter {
    dynamic_router: DynamicRouter<RouterDiscovery, axum_core::body::Body>,
    unified_api: UnifiedApiService,
    direct_proxies: DirectProxiesWithoutMapper,
}

pub type MetaRouterService = BoxCloneService<
    crate::types::request::Request,
    crate::types::response::Response,
    Infallible,
>;

impl MetaRouter {
    pub async fn build(
        app_state: AppState,
    ) -> Result<MetaRouterService, InitError> {
        let meta_router = match app_state.0.config.deployment_target {
            DeploymentTarget::Sidecar => Self::sidecar(app_state.clone()).await,
            DeploymentTarget::Cloud => Self::cloud(app_state.clone()).await,
        }?;
        let service_stack = ServiceBuilder::new()
            // NOTE: not sure if there is perf impact from Auth layer coming
            // before buffer layer, but required due to Clone bound.
            .layer(ErrorHandlerLayer::new(app_state.clone()))
            .layer(RouterDetailsLayer::new())
            .layer(AsyncRequireAuthorizationLayer::new(
                crate::middleware::auth::AuthService::new(app_state.clone()),
            ))
            .layer(RateLimitLayer::global(&app_state)?)
            .layer(CacheLayer::global(&app_state))
            .layer(ErrorHandlerLayer::new(app_state.clone()))
            .map_err(crate::error::internal::InternalError::BufferError)
            .layer(BufferLayer::new(BUFFER_SIZE))
            .service(meta_router);
        Ok(BoxCloneService::new(service_stack))
    }

    pub async fn cloud(app_state: AppState) -> Result<Self, InitError> {
        let discovery_factory = RouterDiscoverFactory::new(app_state.clone());
        let mut router_factory =
            dynamic_router::router::make::MakeRouter::new(discovery_factory);
        let (tx, rx) = tokio::sync::mpsc::channel(100);
        app_state.set_router_tx(tx).await;
        let dynamic_router = router_factory.call(Some(rx)).await?;

        let unified_api = ServiceBuilder::new()
            .layer(RateLimitLayer::unified_api(&app_state)?)
            .layer(CacheLayer::unified_api(&app_state))
            .layer(ErrorHandlerLayer::new(app_state.clone()))
            .service(unified_api::Service::new(&app_state)?);
        let direct_proxies = DirectProxiesWithoutMapper::new(&app_state)?;

        let meta_router = Self {
            dynamic_router,
            unified_api,
            direct_proxies,
        };
        Ok(meta_router)
    }

    pub async fn sidecar(app_state: AppState) -> Result<Self, InitError> {
        let discovery_factory = RouterDiscoverFactory::new(app_state.clone());
        let mut router_factory =
            dynamic_router::router::make::MakeRouter::new(discovery_factory);
        let dynamic_router = router_factory.call(None).await?;
        let unified_api = ServiceBuilder::new()
            .layer(RateLimitLayer::unified_api(&app_state)?)
            .layer(CacheLayer::unified_api(&app_state))
            .layer(ErrorHandlerLayer::new(app_state.clone()))
            .service(unified_api::Service::new(&app_state)?);
        let direct_proxies = DirectProxiesWithoutMapper::new(&app_state)?;
        let meta_router = Self {
            dynamic_router,
            unified_api,
            direct_proxies,
        };
        Ok(meta_router)
    }

    fn handle_router_request(
        &mut self,
        req: crate::types::request::Request,
        router_id: &RouterId,
        extracted_api_path: &str,
    ) -> ResponseFuture {
        tracing::trace!(
            router_id = %router_id,
            api_path = extracted_api_path,
            "received /router request"
        );
        ResponseFuture::RouterRequest {
            future: self.dynamic_router.call(req),
        }
    }

    fn handle_unified_api_request(
        &mut self,
        req: crate::types::request::Request,
        rest: &str,
    ) -> ResponseFuture {
        tracing::trace!(api_path = rest, "received /ai request");
        // assumes request is from OpenAI compatible client
        // and uses the model name to determine the provider.
        ResponseFuture::UnifiedApi {
            future: self.unified_api.call(req),
        }
    }

    fn handle_direct_proxy_request(
        &mut self,
        req: crate::types::request::Request,
        provider: InferenceProvider,
    ) -> ResponseFuture {
        tracing::trace!(
            provider = %provider,
            "received /{{provider}} request"
        );

        let Some(mut direct_proxy) =
            self.direct_proxies.get(&provider).cloned()
        else {
            tracing::warn!(provider = %provider, "requested provider is not configured for direct proxy");
            return ResponseFuture::Ready {
                future: ready(Err(ApiError::InvalidRequest(
                    InvalidRequestError::UnsupportedProvider(provider),
                ))),
            };
        };
        ResponseFuture::DirectProxy {
            future: direct_proxy.call(req),
        }
    }
}

impl tower::Service<crate::types::request::Request> for MetaRouter {
    type Response = crate::types::response::Response;
    type Error = ApiError;
    type Future = ResponseFuture;

    fn poll_ready(
        &mut self,
        ctx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        let mut any_pending = false;

        if self.dynamic_router.poll_ready(ctx).is_pending() {
            any_pending = true;
        }

        if self.unified_api.poll_ready(ctx).is_pending() {
            any_pending = true;
        }
        // we don't need to poll the direct proxies since they
        // always return `Poll::Ready(Ok(()))`. However, if this
        // were to change, we would need to poll them here.
        if any_pending {
            Poll::Pending
        } else {
            Poll::Ready(Ok(()))
        }
    }

    fn call(&mut self, req: crate::types::request::Request) -> Self::Future {
        let route_type = req.extensions().get::<RouteType>().cloned();
        match route_type {
            Some(RouteType::Router { id, path }) => {
                self.handle_router_request(req, &id, &path)
            }
            Some(RouteType::UnifiedApi { path }) => {
                self.handle_unified_api_request(req, &path)
            }
            Some(RouteType::DirectProxy { provider, .. }) => {
                self.handle_direct_proxy_request(req, provider.clone())
            }
            None => ResponseFuture::Ready {
                future: ready(Err(ApiError::InvalidRequest(
                    InvalidRequestError::NotFound(req.uri().path().to_string()),
                ))),
            },
        }
    }
}

pin_project! {
    #[project = ResponseFutureProj]
    pub enum ResponseFuture {
        Ready {
            #[pin]
            future: Ready<Result<crate::types::response::Response, ApiError>>,
        },
        RouterRequest {
            #[pin]
            future: <DynamicRouter<RouterDiscovery, axum_core::body::Body> as tower::Service<crate::types::request::Request>>::Future,
        },
        UnifiedApi {
            #[pin]
            future: <UnifiedApiService as tower::Service<crate::types::request::Request>>::Future,
        },
        DirectProxy {
            #[pin]
            future: <DirectProxyServiceWithoutMapper as tower::Service<crate::types::request::Request>>::Future,
        },
    }
}

impl std::future::Future for ResponseFuture {
    type Output = Result<crate::types::response::Response, ApiError>;

    fn poll(
        self: std::pin::Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Self::Output> {
        match self.project() {
            ResponseFutureProj::Ready { future } => future.poll(cx),
            ResponseFutureProj::RouterRequest { future } => {
                future.poll(cx).map_err(Into::into)
            }
            ResponseFutureProj::UnifiedApi { future } => future.poll(cx),
            ResponseFutureProj::DirectProxy { future } => future
                .poll(cx)
                .map_err(|_| ApiError::Internal(InternalError::Internal)),
        }
    }
}
