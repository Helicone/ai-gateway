use std::{
    convert::Infallible,
    future::Future,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use axum_core::response::IntoResponse;
use http::uri::PathAndQuery;
use pin_project_lite::pin_project;
use rustc_hash::FxHashMap as HashMap;
use tower::{ServiceBuilder, buffer, util::BoxCloneService};

use crate::{
    app::BUFFER_SIZE,
    app_state::AppState,
    balancer::provider::ProviderBalancer,
    config::router::RouterConfig,
    endpoints::{ApiEndpoint, EndpointType},
    error::{
        api::ApiError, init::InitError, internal::InternalError,
        invalid_req::InvalidRequestError,
    },
    middleware::{
        cache::CacheLayer, prompts::PromptLayer, rate_limit, request_context,
    },
    types::router::RouterId,
    utils::handle_error::ErrorHandlerLayer,
};

pub type RouterService = BoxCloneService<
    crate::types::request::Request,
    crate::types::response::Response,
    Infallible,
>;

#[derive(Debug)]
pub struct Router {
    inner: HashMap<EndpointType, RouterService>,
}

impl Router {
    pub async fn new(
        id: RouterId,
        router_config: Arc<RouterConfig>,
        app_state: AppState,
    ) -> Result<Self, InitError> {
        router_config.validate()?;

        let provider_keys = app_state
            .add_provider_keys_for_router(id.clone(), &router_config)
            .await;

        let mut inner = HashMap::default();
        let rl_layer = rate_limit::Layer::per_router(
            &app_state,
            id.clone(),
            &router_config,
        )
        .await?;
        let prompt_layer = PromptLayer::new(&app_state)?;
        let cache_layer = CacheLayer::for_router(&app_state, &router_config)?;
        let request_context_layer = request_context::Layer::for_router(
            router_config.clone(),
            provider_keys.clone(),
        );
        for (endpoint_type, balance_config) in
            router_config.load_balance.as_ref()
        {
            let balancer = ProviderBalancer::new(
                app_state.clone(),
                id.clone(),
                router_config.clone(),
                balance_config,
            )
            .await?;
            let service_stack = ServiceBuilder::new()
                .layer(ErrorHandlerLayer::new(app_state.clone()))
                .layer(prompt_layer.clone())
                .layer(cache_layer.clone())
                .layer(ErrorHandlerLayer::new(app_state.clone()))
                .layer(rl_layer.clone())
                .map_err(|e| ApiError::from(InternalError::BufferError(e)))
                .layer(buffer::BufferLayer::new(BUFFER_SIZE))
                .layer(request_context_layer.clone())
                .service(balancer);

            inner.insert(*endpoint_type, BoxCloneService::new(service_stack));
        }

        tracing::info!(id = %id, "router created");

        Ok(Self { inner })
    }
}

impl tower::Service<crate::types::request::Request> for Router {
    type Response = crate::types::response::Response;
    type Error = Infallible;
    type Future = ResponseFuture;

    #[inline]
    fn poll_ready(
        &mut self,
        ctx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        let mut any_pending = false;
        for balancer in self.inner.values_mut() {
            if balancer.poll_ready(ctx).is_pending() {
                any_pending = true;
            }
        }
        if any_pending {
            Poll::Pending
        } else {
            Poll::Ready(Ok(()))
        }
    }

    #[inline]
    #[tracing::instrument(level = "debug", name = "router", skip_all)]
    fn call(
        &mut self,
        mut req: crate::types::request::Request,
    ) -> Self::Future {
        let Some(extracted_path_and_query) =
            req.extensions().get::<PathAndQuery>()
        else {
            let api_error = ApiError::Internal(
                InternalError::ExtensionNotFound("PathAndQuery"),
            );
            let response = api_error.into_response();
            return ResponseFuture::Ready {
                response: Some(response),
            };
        };

        let api_endpoint = ApiEndpoint::new(extracted_path_and_query.path());
        match api_endpoint {
            Some(api_endpoint) => {
                let endpoint_type = api_endpoint.endpoint_type();
                if let Some(balancer) = self.inner.get_mut(&endpoint_type) {
                    req.extensions_mut().insert(api_endpoint);
                    ResponseFuture::Inner {
                        future: balancer.call(req),
                    }
                } else {
                    let api_error = ApiError::InvalidRequest(
                        InvalidRequestError::NotFound(
                            extracted_path_and_query.path().to_string(),
                        ),
                    );
                    let response = api_error.into_response();
                    return ResponseFuture::Ready {
                        response: Some(response),
                    };
                }
            }
            None => {
                let api_error =
                    ApiError::InvalidRequest(InvalidRequestError::NotFound(
                        extracted_path_and_query.path().to_string(),
                    ));
                let response = api_error.into_response();
                return ResponseFuture::Ready {
                    response: Some(response),
                };
            }
        }
    }
}

pin_project! {
    #[project = ResponseFutureProj]
    pub enum ResponseFuture
    {
        Ready {
            response: Option<crate::types::response::Response>,
        },
        Inner {
            #[pin]
            future: <RouterService as tower::Service<crate::types::request::Request>>::Future,
        },
    }
}

impl Future for ResponseFuture {
    type Output = Result<crate::types::response::Response, Infallible>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.project() {
            ResponseFutureProj::Ready { response } => Poll::Ready(Ok(response
                .take()
                .expect("future polled after completion"))),
            ResponseFutureProj::Inner { future } => {
                match futures::ready!(future.poll(cx)) {
                    Ok(res) => Poll::Ready(Ok(res)),
                    // never happens due to `Infallible` bound
                    Err(e) => match e {},
                }
            }
        }
    }
}
