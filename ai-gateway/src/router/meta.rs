use std::{
    future::{Ready, ready},
    str::FromStr,
    task::{Context, Poll},
};

use compact_str::CompactString;
use dynamic_router::router::DynamicRouter;
use http::uri::PathAndQuery;
use pin_project_lite::pin_project;
use regex::Regex;
use tower::{Service as _, ServiceBuilder};

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
        rate_limit,
    },
    router::{
        FORCED_ROUTING_HEADER,
        direct::{DirectProxiesWithoutMapper, DirectProxyServiceWithoutMapper},
        unified_api,
    },
    types::{
        extensions::{MapperContext, RequestKind},
        provider::InferenceProvider,
        request::Request,
        router::RouterId,
    },
    utils::handle_error::{ErrorHandler, ErrorHandlerLayer},
};

#[derive(Debug, Clone)]
enum RouteType {
    Router {
        id: RouterId,
        path: CompactString,
    },
    UnifiedApi {
        path: CompactString,
    },
    DirectProxy {
        provider: InferenceProvider,
        path: CompactString,
    },
}

/// Unified regex that matches all three routing patterns:
/// - `/router/{id}[/path][?query]` - Router pattern
/// - `/ai[/path][?query]` - Unified API pattern
/// - `/{provider}[/path][?query]` - Direct proxy pattern
const UNIFIED_URL_REGEX: &str =
    r"^/(?P<first_segment>[^/?]+)(?P<rest>/[^?]*)?(?P<query>\?.*)?$";

/// Legacy regex for router-specific matching (kept for backward compatibility)
const ROUTER_URL_REGEX: &str =
    r"^/router/(?P<id>[A-Za-z0-9_-]{1,12})(?P<path>/[^?]*)?(?P<query>\?.*)?$";

pub type UnifiedApiService =
    rate_limit::Service<CacheService<ErrorHandler<unified_api::Service>>>;

fn extract_path_and_query(
    path: &str,
    query: Option<&str>,
) -> Result<PathAndQuery, ApiError> {
    let path_and_query = if let Some(query_params) = query {
        PathAndQuery::from_str(&format!("{path}?{query_params}"))
    } else {
        PathAndQuery::from_str(path)
    };

    path_and_query.map_err(|e| {
        tracing::warn!(error = %e, "Failed to convert extracted path to PathAndQuery");
        ApiError::Internal(InternalError::Internal)
    })
}

#[derive(Debug)]
pub struct MetaRouter {
    dynamic_router: DynamicRouter<RouterDiscovery, axum_core::body::Body>,
    unified_api: UnifiedApiService,
    direct_proxies: DirectProxiesWithoutMapper,
    unified_url_regex: Regex,
    router_url_regex: Regex,
}

impl MetaRouter {
    pub async fn new(app_state: AppState) -> Result<Self, InitError> {
        let meta_router = match app_state.0.config.deployment_target {
            DeploymentTarget::Sidecar => Self::sidecar(app_state).await,
            DeploymentTarget::Cloud => Self::cloud(app_state).await,
        }?;
        Ok(meta_router)
    }

    pub async fn cloud(app_state: AppState) -> Result<Self, InitError> {
        let unified_url_regex =
            Regex::new(UNIFIED_URL_REGEX).expect("always valid if tests pass");
        let router_url_regex =
            Regex::new(ROUTER_URL_REGEX).expect("always valid if tests pass");

        let discovery_factory = RouterDiscoverFactory::new(app_state.clone());
        let mut router_factory =
            dynamic_router::router::make::MakeRouter::new(discovery_factory);
        let (tx, rx) = tokio::sync::mpsc::channel(100);
        app_state.set_router_tx(tx).await;
        let dynamic_router = router_factory.call(Some(rx)).await?;

        let unified_api = ServiceBuilder::new()
            .layer(rate_limit::Layer::unified_api(&app_state)?)
            .layer(CacheLayer::unified_api(&app_state))
            .layer(ErrorHandlerLayer::new(app_state.clone()))
            .service(unified_api::Service::new(&app_state)?);
        let direct_proxies = DirectProxiesWithoutMapper::new(&app_state)?;

        let meta_router = Self {
            dynamic_router,
            unified_api,
            direct_proxies,
            unified_url_regex,
            router_url_regex,
        };
        Ok(meta_router)
    }

    pub async fn sidecar(app_state: AppState) -> Result<Self, InitError> {
        let unified_url_regex =
            Regex::new(UNIFIED_URL_REGEX).expect("always valid if tests pass");
        let router_url_regex =
            Regex::new(ROUTER_URL_REGEX).expect("always valid if tests pass");
        let discovery_factory = RouterDiscoverFactory::new(app_state.clone());
        let mut router_factory =
            dynamic_router::router::make::MakeRouter::new(discovery_factory);
        let dynamic_router = router_factory.call(None).await?;
        let unified_api = ServiceBuilder::new()
            .layer(rate_limit::Layer::unified_api(&app_state)?)
            .layer(CacheLayer::unified_api(&app_state))
            .layer(ErrorHandlerLayer::new(app_state.clone()))
            .service(unified_api::Service::new(&app_state)?);
        let direct_proxies = DirectProxiesWithoutMapper::new(&app_state)?;
        let meta_router = Self {
            dynamic_router,
            unified_api,
            direct_proxies,
            unified_url_regex,
            router_url_regex,
        };
        Ok(meta_router)
    }

    fn parse_route(&self, request: &Request) -> Result<RouteType, ApiError> {
        let path = request.uri().path();
        if let Some(captures) = self.unified_url_regex.captures(path) {
            let first_segment = captures
                .name("first_segment")
                .ok_or_else(|| {
                    ApiError::InvalidRequest(InvalidRequestError::NotFound(
                        path.to_string(),
                    ))
                })?
                .as_str();

            let is_router_request = first_segment == "router";
            let is_unified_api_request = first_segment == "ai";

            let rest_path = captures
                .name("rest")
                .map(|m| m.as_str())
                .unwrap_or_default();
            if let Some(forced_routing) =
                request.headers().get(FORCED_ROUTING_HEADER)
                && let Ok(forced_routing) = forced_routing.to_str()
                && (is_router_request || is_unified_api_request)
            {
                let Ok(provider) = InferenceProvider::from_str(forced_routing);
                return Ok(RouteType::DirectProxy {
                    provider,
                    path: rest_path.trim_start_matches('/').into(),
                });
            }

            if is_router_request {
                // Use the router-specific regex for detailed parsing
                let (router_id, extracted_api_path) =
                    extract_router_id_and_path(&self.router_url_regex, path)?;
                Ok(RouteType::Router {
                    id: router_id,
                    path: extracted_api_path.into(),
                })
            } else if is_unified_api_request {
                Ok(RouteType::UnifiedApi {
                    path: rest_path.into(),
                })
            } else {
                let Ok(provider) = InferenceProvider::from_str(first_segment);
                Ok(RouteType::DirectProxy {
                    provider,
                    path: rest_path.trim_start_matches('/').into(),
                })
            }
        } else {
            Err(ApiError::InvalidRequest(InvalidRequestError::NotFound(
                path.to_string(),
            )))
        }
    }

    fn handle_router_request(
        &mut self,
        mut req: crate::types::request::Request,
        router_id: &RouterId,
        extracted_api_path: &str,
    ) -> ResponseFuture {
        tracing::trace!(
            router_id = %router_id,
            api_path = extracted_api_path,
            "received /router request"
        );
        let extracted_path_and_query =
            match extract_path_and_query(extracted_api_path, req.uri().query())
            {
                Ok(p) => p,
                Err(e) => {
                    return ResponseFuture::Ready {
                        future: ready(Err(e)),
                    };
                }
            };

        req.extensions_mut().insert(extracted_path_and_query);
        req.extensions_mut().insert(RequestKind::Router);
        req.extensions_mut().insert(router_id.clone());
        ResponseFuture::RouterRequest {
            future: self.dynamic_router.call(req),
        }
    }

    fn handle_unified_api_request(
        &mut self,
        mut req: crate::types::request::Request,
        rest: &str,
    ) -> ResponseFuture {
        tracing::trace!(api_path = rest, "received /ai request");
        let extracted_path_and_query =
            match extract_path_and_query(rest, req.uri().query()) {
                Ok(p) => p,
                Err(e) => {
                    return ResponseFuture::Ready {
                        future: ready(Err(e)),
                    };
                }
            };
        req.extensions_mut().insert(extracted_path_and_query);
        req.extensions_mut().insert(RequestKind::UnifiedApi);
        // assumes request is from OpenAI compatible client
        // and uses the model name to determine the provider.
        ResponseFuture::UnifiedApi {
            future: self.unified_api.call(req),
        }
    }

    fn handle_direct_proxy_request(
        &mut self,
        mut req: crate::types::request::Request,
        provider: InferenceProvider,
        rest: &str,
    ) -> ResponseFuture {
        tracing::trace!(
            provider = %provider,
            "received /{{provider}} request"
        );
        let extracted_path_and_query =
            match extract_path_and_query(rest, req.uri().query()) {
                Ok(p) => p,
                Err(e) => {
                    return ResponseFuture::Ready {
                        future: ready(Err(e)),
                    };
                }
            };
        req.extensions_mut().insert(extracted_path_and_query);
        req.extensions_mut().insert(RequestKind::DirectProxy);
        // for the passthrough endpoints, we don't want to
        // collect/deserialize the request
        // body, and thus we must assume the request is not a stream
        // request and cannot support streaming.
        let mapper_ctx = MapperContext {
            is_stream: false,
            model: None,
        };
        req.extensions_mut().insert(mapper_ctx);

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
        match self.parse_route(&req) {
            Ok(RouteType::Router { id, path }) => {
                self.handle_router_request(req, &id, &path)
            }
            Ok(RouteType::UnifiedApi { path }) => {
                self.handle_unified_api_request(req, &path)
            }
            Ok(RouteType::DirectProxy { provider, path }) => {
                self.handle_direct_proxy_request(req, provider, &path)
            }
            Err(e) => ResponseFuture::Ready {
                future: ready(Err(e)),
            },
        }
    }
}

fn extract_router_id_and_path<'a>(
    url_regex: &Regex,
    path: &'a str,
) -> Result<(RouterId, &'a str), ApiError> {
    // Attempt to match the incoming URI path against the provided regex
    if let Some(captures) = url_regex.captures(path) {
        // --- Determine the router id ---
        let id_str = captures
            .name("id")
            .ok_or_else(|| {
                ApiError::InvalidRequest(InvalidRequestError::NotFound(
                    path.to_string(),
                ))
            })?
            .as_str();

        // All router IDs are treated as named routers
        let router_id = RouterId::Named(CompactString::from(id_str));

        // Determine the API sub-path
        let api_path = captures
            .name("path")
            .map(|m| m.as_str())
            .unwrap_or_default();

        Ok((router_id, api_path))
    } else {
        // If the regex does not match at all, the request URI is considered
        // invalid.
        Err(ApiError::InvalidRequest(InvalidRequestError::NotFound(
            path.to_string(),
        )))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_unified_regex() {
        let regex =
            Regex::new(UNIFIED_URL_REGEX).expect("Regex should be valid");

        // --- Router patterns ---
        assert!(regex.is_match("/router/default"));
        assert!(regex.is_match("/router/default/chat/completions"));
        assert!(regex.is_match("/router/default?user=test"));
        assert!(regex.is_match("/router/my-router"));
        assert!(regex.is_match(
            "/router/my-router/v1/chat/completions?user=test&limit=10"
        ));

        // --- Unified API patterns ---
        assert!(regex.is_match("/ai"));
        assert!(regex.is_match("/ai/chat/completions"));
        assert!(regex.is_match("/ai/chat/completions?user=test"));

        // --- Direct proxy patterns ---
        assert!(regex.is_match("/openai"));
        assert!(regex.is_match("/openai/v1/chat/completions"));
        assert!(regex.is_match("/anthropic/v1/messages"));
        assert!(regex.is_match("/bedrock/converse"));

        // Note: The unified regex matches "/router" because it's a valid first
        // segment, but it will fail when parsed as a router request due
        // to missing ID
        assert!(regex.is_match("/router"));

        // --- Negative cases ---
        assert!(!regex.is_match("/"));
        assert!(!regex.is_match("//double-slash"));
    }

    #[test]
    fn test_router_regex() {
        let regex =
            Regex::new(ROUTER_URL_REGEX).expect("Regex should be valid");

        // --- Positive cases ---
        assert!(regex.is_match("/router/default"));
        assert!(regex.is_match("/router/default/chat/completions"));
        assert!(regex.is_match("/router/default?user=test"));
        assert!(regex.is_match("/router/my-router"));
        assert!(regex.is_match(
            "/router/my-router/v1/chat/completions?user=test&limit=10"
        ));

        // --- Negative cases ---
        assert!(!regex.is_match("/router"));
        assert!(!regex.is_match("/router/"));
        assert!(!regex.is_match(
            "/router/this-id-is-way-too-long-to-be-valid-as-a-router-id"
        ));
        assert!(!regex.is_match("/other/path"));
    }

    #[test]
    fn test_extract_router_id_and_path() {
        let url_regex = Regex::new(ROUTER_URL_REGEX).unwrap();

        // --- Default router id ---
        let path_default = "/router/my-router";
        let expected_api_path_default = "";
        assert_eq!(
            extract_router_id_and_path(&url_regex, path_default).unwrap(),
            (
                RouterId::Named(CompactString::from("my-router")),
                expected_api_path_default
            )
        );

        // Default router id with API path and query params
        let path_default_with_path_query =
            "/router/my-router/chat/completions?user=test";
        let expected_api_path_default_with_path_query = "/chat/completions";
        assert_eq!(
            extract_router_id_and_path(
                &url_regex,
                path_default_with_path_query
            )
            .unwrap(),
            (
                RouterId::Named(CompactString::from("my-router")),
                expected_api_path_default_with_path_query
            )
        );

        // --- Named router id ---
        let path_named = "/router/my-router";
        let expected_api_path_named = "";
        assert_eq!(
            extract_router_id_and_path(&url_regex, path_named).unwrap(),
            (
                RouterId::Named(CompactString::from("my-router")),
                expected_api_path_named
            )
        );

        // Named router id with additional API path
        let path_named_with_path = "/router/my-router/v1/chat/completions";
        let expected_api_path_named_with_path = "/v1/chat/completions";
        assert_eq!(
            extract_router_id_and_path(&url_regex, path_named_with_path)
                .unwrap(),
            (
                RouterId::Named(CompactString::from("my-router")),
                expected_api_path_named_with_path
            )
        );

        // Named router id with query params but no explicit API path
        let path_named_query_only = "/router/my-router?foo=bar";
        let expected_api_path_named_query_only = "";
        assert_eq!(
            extract_router_id_and_path(&url_regex, path_named_query_only)
                .unwrap(),
            (
                RouterId::Named(CompactString::from("my-router")),
                expected_api_path_named_query_only
            )
        );

        // --- Invalid cases ---
        let path_missing_id = "/router";
        assert!(matches!(
            extract_router_id_and_path(&url_regex, path_missing_id),
            Err(ApiError::InvalidRequest(_))
        ));

        let path_id_too_long =
            "/router/this-id-is-way-too-long-to-be-valid-as-a-router-id";
        assert!(matches!(
            extract_router_id_and_path(&url_regex, path_id_too_long),
            Err(ApiError::InvalidRequest(_))
        ));
    }
}
