use std::{
    str::FromStr,
    task::{Context, Poll},
};

use compact_str::CompactString;
use futures::future::BoxFuture;
use http::uri::PathAndQuery;
use regex::Regex;

use crate::{
    error::{
        api::ApiError, internal::InternalError,
        invalid_req::InvalidRequestError,
    },
    router::FORCED_ROUTING_HEADER,
    types::{
        extensions::{MapperContext, RequestKind},
        provider::InferenceProvider,
        request::Request,
        response::Response,
        router::RouterId,
    },
};

/// Unified regex that matches all three routing patterns:
/// - `/router/{id}[/path][?query]` - Router pattern
/// - `/ai[/path][?query]` - Unified API pattern
/// - `/{provider}[/path][?query]` - Direct proxy pattern
const UNIFIED_URL_REGEX: &str =
    r"^/(?P<first_segment>[^/?]+)(?P<rest>/[^?]*)?(?P<query>\?.*)?$";

/// Legacy regex for router-specific matching (kept for backward compatibility)
const ROUTER_URL_REGEX: &str =
    r"^/router/(?P<id>[A-Za-z0-9_-]{1,12})(?P<path>/[^?]*)?(?P<query>\?.*)?$";

pub struct RouterDetailsLayer {}

impl RouterDetailsLayer {
    pub fn new() -> Self {
        Self {}
    }
}

impl<S> tower::Layer<S> for RouterDetailsLayer {
    type Service = RouterDetailsService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        RouterDetailsService {
            inner,
            unified_url_regex: Regex::new(UNIFIED_URL_REGEX).unwrap(),
            router_url_regex: Regex::new(ROUTER_URL_REGEX).unwrap(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct RouterDetailsService<S> {
    inner: S,
    unified_url_regex: Regex,
    router_url_regex: Regex,
}

#[derive(Debug, Clone)]
pub enum RouteType {
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

impl<S> RouterDetailsService<S> {
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
                    path: extracted_api_path.trim_start_matches('/').into(),
                })
            } else if is_unified_api_request {
                Ok(RouteType::UnifiedApi {
                    path: rest_path.trim_start_matches('/').into(),
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

impl<S> tower::Service<Request> for RouterDetailsService<S>
where
    S: tower::Service<Request, Response = Response, Error = ApiError>,
    S::Future: Send + 'static,
{
    type Response = Response;
    type Error = ApiError;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx).map_err(Into::into)
    }

    fn call(&mut self, mut req: Request) -> Self::Future {
        let route = self.parse_route(&req);
        if let Ok(route_type) = route {
            match &route_type {
                RouteType::Router { id, path } => {
                    let extracted_path_and_query =
                        match extract_path_and_query(path, req.uri().query()) {
                            Ok(p) => p,
                            Err(e) => {
                                return Box::pin(async move { Err(e) });
                            }
                        };

                    req.extensions_mut().insert(extracted_path_and_query);
                    req.extensions_mut().insert(RequestKind::Router);
                    req.extensions_mut().insert(id.clone());
                }
                RouteType::UnifiedApi { path } => {
                    let extracted_path_and_query =
                        match extract_path_and_query(path, req.uri().query()) {
                            Ok(p) => p,
                            Err(e) => {
                                return Box::pin(async move { Err(e) });
                            }
                        };
                    req.extensions_mut().insert(extracted_path_and_query);
                    req.extensions_mut().insert(RequestKind::UnifiedApi);
                }
                RouteType::DirectProxy { path, .. } => {
                    let extracted_path_and_query =
                        match extract_path_and_query(path, req.uri().query()) {
                            Ok(p) => p,
                            Err(e) => {
                                return Box::pin(async move { Err(e) });
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
                }
            }
            req.extensions_mut().insert(route_type);
        }

        let future = self.inner.call(req);
        Box::pin(async move {
            let response = future.await?;
            Ok(response)
        })

        // match route {
        //     RouteType::Router { id, path } => {
        //         self.inner.call(req)
        //     }
        // }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_join() {
        let url = "https://api.groq.com/openai/";
        let url = url::Url::parse(url).unwrap();
        let url = url.join("v1/chat/completions").unwrap();
        assert_eq!(
            url.as_str(),
            "https://api.groq.com/openai/v1/chat/completions"
        );
    }

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
