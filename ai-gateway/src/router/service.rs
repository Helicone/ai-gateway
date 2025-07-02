use std::{
    future::{Ready, ready},
    sync::Arc,
    task::{Context, Poll},
};

use futures::future::Either;
use http::uri::PathAndQuery;
use rustc_hash::FxHashMap as HashMap;
use tower::{ServiceBuilder, buffer, util::BoxCloneService};

use crate::{
    app::BUFFER_SIZE,
    app_state::AppState,
    balancer::provider::ProviderBalancer,
    config::DeploymentTarget,
    endpoints::{ApiEndpoint, EndpointType},
    error::{
        api::ApiError, init::InitError, internal::InternalError,
        invalid_req::InvalidRequestError,
    },
    middleware::{
        cache::CacheLayer, prompts::PromptLayer, rate_limit, request_context,
    },
    router::direct::DirectProxyService,
    types::router::RouterId,
    utils::handle_error::ErrorHandlerLayer,
};

pub type RouterService = BoxCloneService<
    crate::types::request::Request,
    crate::types::response::Response,
    ApiError,
>;

#[derive(Debug)]
pub struct Router {
    inner: HashMap<EndpointType, RouterService>,
}

impl Router {
    pub async fn new(
        id: RouterId,
        app_state: AppState,
    ) -> Result<Self, InitError> {
        let router_config = match &app_state.0.config.deployment_target {
            DeploymentTarget::Cloud | DeploymentTarget::Sidecar => {
                // Note: Cloud will eventually get router configs from the
                // database, but for not we are just allowing
                // the cloud to be deployed to start dogfooding
                let router_config = app_state
                    .0
                    .config
                    .routers
                    .as_ref()
                    .get(&id)
                    .ok_or(InitError::DefaultRouterNotFound)?
                    .clone();
                Arc::new(router_config)
            }
        };
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
        let prompt_layer = PromptLayer::new(app_state.clone())?;
        let cache_layer = CacheLayer::for_router(&app_state, &id)?;
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
    type Error = ApiError;
    type Future = Either<
        Ready<Result<crate::types::response::Response, ApiError>>,
        <RouterService as tower::Service<crate::types::request::Request>>::Future,
    >;

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
            return Either::Left(ready(Err(InternalError::ExtensionNotFound(
                "PathAndQuery",
            )
            .into())));
        };

        let api_endpoint = ApiEndpoint::new(extracted_path_and_query.path());
        match api_endpoint {
            Some(api_endpoint) => {
                let endpoint_type = api_endpoint.endpoint_type();
                if let Some(balancer) = self.inner.get_mut(&endpoint_type) {
                    req.extensions_mut().insert(api_endpoint);
                    Either::Right(balancer.call(req))
                } else {
                    Either::Left(ready(Err(InvalidRequestError::NotFound(
                        extracted_path_and_query.path().to_string(),
                    )
                    .into())))
                }
            }
            None => Either::Left(ready(Err(InvalidRequestError::NotFound(
                extracted_path_and_query.path().to_string(),
            )
            .into()))),
        }
    }
}
