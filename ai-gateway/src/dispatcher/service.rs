use std::{
    str::FromStr,
    sync::Arc,
    task::{Context, Poll},
    time::Duration,
};

use backon::{BackoffBuilder, ConstantBuilder, ExponentialBuilder, Retryable};
use bytes::Bytes;
use chrono::{DateTime, Utc};
use futures::{TryStreamExt, future::BoxFuture};
use http::{HeaderMap, HeaderName, HeaderValue, StatusCode, uri::PathAndQuery};
use http_body_util::BodyExt;
use opentelemetry::KeyValue;
use reqwest::RequestBuilder;
use rust_decimal::prelude::ToPrimitive;
use tokio::{
    sync::{mpsc::Sender, oneshot},
    time::Instant,
};
use tower::{Service, ServiceBuilder};
use tracing::{Instrument, info_span};

use crate::{
    app_state::AppState,
    config::{retry::RetryConfig, router::RouterConfig},
    discover::monitor::metrics::EndpointMetricsRegistry,
    dispatcher::{
        client::{Client, ProviderClient},
        extensions::ExtensionsCopier,
    },
    endpoints::ApiEndpoint,
    error::{api::ApiError, init::InitError, internal::InternalError},
    logger::service::LoggerService,
    metrics::tfft::TFFTFuture,
    middleware::{
        add_extension::{AddExtensions, AddExtensionsLayer},
        mapper::{model::ModelMapper, registry::EndpointConverterRegistry},
    },
    types::{
        body::BodyReader,
        extensions::{MapperContext, RequestContext},
        provider::InferenceProvider,
        rate_limit::RateLimitEvent,
        request::Request,
        router::RouterId,
    },
    utils::handle_error::{ErrorHandler, ErrorHandlerLayer},
};

pub type DispatcherFuture = BoxFuture<
    'static,
    Result<http::Response<crate::types::body::Body>, ApiError>,
>;
pub type DispatcherService =
    AddExtensions<ErrorHandler<crate::middleware::mapper::Service<Dispatcher>>>;
pub type DispatcherServiceWithoutMapper =
    AddExtensions<ErrorHandler<Dispatcher>>;

/// Leaf service that dispatches requests to the correct provider.
#[derive(Debug, Clone)]
pub struct Dispatcher {
    client: Client,
    app_state: AppState,
    provider: InferenceProvider,
    /// Is `Some` for load balanced routers, `None` for direct proxies.
    rate_limit_tx: Option<Sender<RateLimitEvent>>,
}

impl Dispatcher {
    pub async fn new(
        app_state: AppState,
        router_id: &RouterId,
        router_config: &Arc<RouterConfig>,
        provider: InferenceProvider,
    ) -> Result<DispatcherService, InitError> {
        let client =
            Client::new_for_router(&app_state, provider.clone(), router_id)
                .await?;
        let rate_limit_tx = app_state.get_rate_limit_tx(router_id).await?;

        let dispatcher = Self {
            client,
            app_state: app_state.clone(),
            provider: provider.clone(),
            rate_limit_tx: Some(rate_limit_tx),
        };
        let model_mapper = ModelMapper::new_for_router(
            app_state.clone(),
            router_config.clone(),
        );
        let converter_registry = EndpointConverterRegistry::new(&model_mapper);

        let extensions_layer = AddExtensionsLayer::builder()
            .inference_provider(provider.clone())
            .router_id(Some(router_id.clone()))
            .build();

        Ok(ServiceBuilder::new()
            .layer(extensions_layer)
            .layer(ErrorHandlerLayer::new(app_state))
            .layer(crate::middleware::mapper::Layer::new(converter_registry))
            // other middleware: rate limiting, logging, etc, etc
            // will be added here as well
            .service(dispatcher))
    }

    pub fn new_direct_proxy(
        app_state: AppState,
        provider: &InferenceProvider,
    ) -> Result<DispatcherService, InitError> {
        let client =
            Client::new_for_direct_proxy(&app_state, provider.clone())?;

        let dispatcher = Self {
            client,
            app_state: app_state.clone(),
            provider: provider.clone(),
            rate_limit_tx: None,
        };
        let model_mapper = ModelMapper::new(app_state.clone());
        let converter_registry = EndpointConverterRegistry::new(&model_mapper);

        let extensions_layer = AddExtensionsLayer::builder()
            .inference_provider(provider.clone())
            .router_id(None)
            .build();

        Ok(ServiceBuilder::new()
            .layer(extensions_layer)
            .layer(ErrorHandlerLayer::new(app_state))
            .layer(crate::middleware::mapper::Layer::new(converter_registry))
            // other middleware: rate limiting, logging, etc, etc
            // will be added here as well
            .service(dispatcher))
    }

    pub fn new_without_mapper(
        app_state: AppState,
        provider: &InferenceProvider,
    ) -> Result<DispatcherServiceWithoutMapper, InitError> {
        let client = Client::new_for_unified_api(&app_state, provider.clone())?;

        let dispatcher = Self {
            client,
            app_state: app_state.clone(),
            provider: provider.clone(),
            rate_limit_tx: None,
        };

        let extensions_layer = AddExtensionsLayer::builder()
            .inference_provider(provider.clone())
            .router_id(None)
            .build();

        Ok(ServiceBuilder::new()
            .layer(extensions_layer)
            .layer(ErrorHandlerLayer::new(app_state))
            // other middleware: rate limiting, logging, etc, etc
            // will be added here as well
            .service(dispatcher))
    }
}

impl Service<Request> for Dispatcher {
    type Response = http::Response<crate::types::body::Body>;
    type Error = ApiError;
    type Future = DispatcherFuture;

    fn poll_ready(
        &mut self,
        _cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    #[tracing::instrument(name = "dispatcher", skip_all)]
    fn call(&mut self, req: Request) -> Self::Future {
        // see: https://docs.rs/tower/latest/tower/trait.Service.html#be-careful-when-cloning-inner-services
        let this = self.clone();
        let this = std::mem::replace(self, this);
        tracing::trace!(provider = ?this.provider, "dispatcher received request");
        Box::pin(async move { this.dispatch(req).await })
    }
}

impl Dispatcher {
    /// Extracts request context and extensions from the request
    fn extract_request_context(
        req: &mut Request,
    ) -> Result<
        (
            MapperContext,
            Arc<RequestContext>,
            Option<ApiEndpoint>,
            PathAndQuery,
            InferenceProvider,
            Option<RouterId>,
            Instant,
            DateTime<Utc>,
        ),
        ApiError,
    > {
        let mapper_ctx = req
            .extensions_mut()
            .remove::<MapperContext>()
            .ok_or(InternalError::ExtensionNotFound("MapperContext"))?;
        let req_ctx = req
            .extensions_mut()
            .remove::<Arc<RequestContext>>()
            .ok_or(InternalError::ExtensionNotFound("RequestContext"))?;
        let api_endpoint = req.extensions().get::<ApiEndpoint>().cloned();
        let extracted_path_and_query = req
            .extensions_mut()
            .remove::<PathAndQuery>()
            .ok_or(ApiError::Internal(InternalError::ExtensionNotFound(
                "PathAndQuery",
            )))?;
        let inference_provider = req
            .extensions()
            .get::<InferenceProvider>()
            .cloned()
            .ok_or(InternalError::ExtensionNotFound("InferenceProvider"))?;
        let router_id = req.extensions().get::<RouterId>().cloned();
        let start_instant = req
            .extensions()
            .get::<Instant>()
            .copied()
            .unwrap_or_else(|| {
                tracing::warn!(
                    "did not find expected Instant in req extensions"
                );
                Instant::now()
            });
        let start_time = req
            .extensions()
            .get::<DateTime<Utc>>()
            .copied()
            .unwrap_or_else(|| {
                tracing::warn!(
                    "did not find expected DateTime<Utc> in req extensions"
                );
                Utc::now()
            });

        Ok((
            mapper_ctx,
            req_ctx,
            api_endpoint,
            extracted_path_and_query,
            inference_provider,
            router_id,
            start_instant,
            start_time,
        ))
    }

    /// Builds the target URL from base URL and path
    fn build_target_url(
        &self,
        extracted_path_and_query: &PathAndQuery,
    ) -> Result<url::Url, ApiError> {
        let config = self.app_state.config();
        let provider_config =
            config.providers.get(&self.provider).ok_or_else(|| {
                InternalError::ProviderNotConfigured(self.provider.clone())
            })?;
        let base_url = provider_config.base_url.clone();

        Ok(base_url
            .join(extracted_path_and_query.as_str())
            .expect("PathAndQuery joined with valid url will always succeed"))
    }

    /// Transforms the request by cleaning headers and collecting body
    async fn transform_request(
        &self,
        req: Request,
    ) -> Result<(http::Method, HeaderMap, Bytes), ApiError> {
        let mut req = req;
        {
            let h = req.headers_mut();
            h.remove(http::header::HOST);
            h.remove(http::header::AUTHORIZATION);
            h.remove(http::header::CONTENT_LENGTH);
            h.remove(HeaderName::from_str("helicone-api-key").unwrap());
            // TODO: properly support accept encoding
            h.remove(http::header::ACCEPT_ENCODING);
            h.insert(
                http::header::ACCEPT_ENCODING,
                HeaderValue::from_static("identity"),
            );
        }
        let method = req.method().clone();
        let headers = req.headers().clone();

        // TODO: could change request type of dispatcher to
        // http::Request<reqwest::Body>
        // to avoid collecting the body twice
        let req_body_bytes = req
            .into_body()
            .collect()
            .await
            .map_err(|e| InternalError::RequestBodyError(Box::new(e)))?
            .to_bytes();

        Ok((method, headers, req_body_bytes))
    }

    /// Processes the response by setting headers and extensions
    fn process_response(
        &self,
        mut client_response: http::Response<crate::types::body::Body>,
        inference_provider: InferenceProvider,
        router_id: Option<RouterId>,
        auth_ctx: Option<&crate::types::extensions::AuthContext>,
        mapper_ctx: &MapperContext,
        api_endpoint: Option<ApiEndpoint>,
        extracted_path_and_query: PathAndQuery,
    ) -> http::Response<crate::types::body::Body> {
        let provider_request_id = {
            let headers = client_response.headers_mut();
            headers.remove(http::header::CONTENT_LENGTH);
            headers.remove("x-request-id")
        };
        tracing::debug!(provider_req_id = ?provider_request_id, status = %client_response.status(), "received response");
        let extensions_copier = ExtensionsCopier::builder()
            .inference_provider(inference_provider)
            .router_id(router_id)
            .auth_context(auth_ctx.cloned())
            .provider_request_id(provider_request_id)
            .mapper_ctx(mapper_ctx.clone())
            .build();
        extensions_copier.copy_extensions(client_response.extensions_mut());
        client_response.extensions_mut().insert(mapper_ctx.clone());
        if let Some(api_endpoint) = api_endpoint {
            client_response.extensions_mut().insert(api_endpoint);
        }
        client_response
            .extensions_mut()
            .insert(extracted_path_and_query);

        client_response
    }

    /// Handles logging logic for both observability and metrics
    fn handle_logging(
        &self,
        req_ctx: &RequestContext,
        start_time: DateTime<Utc>,
        start_instant: Instant,
        target_url: url::Url,
        headers: HeaderMap,
        req_body_bytes: Bytes,
        client_response: &http::Response<crate::types::body::Body>,
        response_body_for_logger: BodyReader,
        tfft_rx: oneshot::Receiver<()>,
        mapper_ctx: &MapperContext,
    ) {
        if self.app_state.config().helicone.is_observability_enabled() {
            if let Some(auth_ctx) = req_ctx.auth_context.clone() {
                let response_logger = LoggerService::builder()
                    .app_state(self.app_state.clone())
                    .auth_ctx(auth_ctx)
                    .start_time(start_time)
                    .start_instant(start_instant)
                    .target_url(target_url)
                    .request_headers(headers)
                    .request_body(req_body_bytes)
                    .response_status(client_response.status())
                    .response_body(response_body_for_logger)
                    .provider(self.provider.clone())
                    .tfft_rx(tfft_rx)
                    .mapper_ctx(mapper_ctx.clone())
                    .build();

                let app_state = self.app_state.clone();
                tokio::spawn(
                    async move {
                        if let Err(e) = response_logger.log().await {
                            let error_str = e.as_ref().to_string();
                            app_state
                                .0
                                .metrics
                                .error_count
                                .add(1, &[KeyValue::new("type", error_str)]);
                        }
                    }
                    .instrument(tracing::Span::current()),
                );
            }
        } else {
            let app_state = self.app_state.clone();
            let model = mapper_ctx.model.as_ref().map_or_else(
                || "unknown".to_string(),
                std::string::ToString::to_string,
            );
            let path = target_url.path().to_string();
            let provider_string = self.provider.to_string();
            tokio::spawn(
                async move {
                    let tfft_future = TFFTFuture::new(start_instant, tfft_rx);
                    let collect_future = response_body_for_logger.collect();
                    let (_response_body, tfft_duration) = tokio::join!(collect_future, tfft_future);
                    if let Ok(tfft_duration) = tfft_duration {
                        tracing::trace!(tfft_duration = ?tfft_duration, "tfft_duration");
                        let attributes = [
                            KeyValue::new("provider", provider_string),
                            KeyValue::new("model", model),
                            KeyValue::new("path", path),
                        ];
                        #[allow(clippy::cast_precision_loss)]
                        app_state.0.metrics.tfft_duration.record(tfft_duration.as_millis() as f64, &attributes);
                    } else { tracing::error!("Failed to get TFFT signal") }
                }
                .instrument(tracing::Span::current()),
            );
        }
    }

    /// Handles error responses and rate limiting
    async fn handle_error_and_rate_limiting(
        &self,
        response_status: StatusCode,
        response_headers: &HeaderMap,
        api_endpoint: Option<ApiEndpoint>,
    ) -> Result<(), ApiError> {
        if response_status.is_server_error() {
            if let Some(api_endpoint) = api_endpoint {
                let endpoint_metrics = self
                    .app_state
                    .0
                    .endpoint_metrics
                    .health_metrics(api_endpoint)?;
                endpoint_metrics.incr_remote_internal_error_count();
            }
        } else if response_status == StatusCode::TOO_MANY_REQUESTS {
            if let Some(ref api_endpoint) = api_endpoint {
                let retry_after = extract_retry_after(response_headers);
                tracing::info!(
                    provider = ?self.provider,
                    api_endpoint = ?api_endpoint,
                    retry_after = ?retry_after,
                    "Provider rate limited, signaling monitor"
                );

                if let Some(rate_limit_tx) = &self.rate_limit_tx {
                    if let Err(e) = rate_limit_tx
                        .send(RateLimitEvent::new(
                            api_endpoint.clone(),
                            retry_after,
                        ))
                        .await
                    {
                        tracing::error!(error = %e, "failed to send rate limit event");
                    }
                }
            }
        }
        Ok(())
    }

    /// Main dispatch function that orchestrates the request processing
    async fn dispatch(
        &self,
        mut req: Request,
    ) -> Result<http::Response<crate::types::body::Body>, ApiError> {
        // Extract request context and extensions
        let (
            mapper_ctx,
            req_ctx,
            api_endpoint,
            extracted_path_and_query,
            inference_provider,
            router_id,
            start_instant,
            start_time,
        ) = Self::extract_request_context(&mut req)?;

        let auth_ctx = req_ctx.auth_context.as_ref();

        // Build target URL
        let target_url = self.build_target_url(&extracted_path_and_query)?;

        // Transform request
        let (method, headers, req_body_bytes) =
            self.transform_request(req).await?;

        // Build request builder with correct target URL
        let request_builder = self
            .client
            .as_ref()
            .request(method.clone(), target_url.clone())
            .headers(headers.clone());

        let request_builder = self
            .client
            .extract_and_sign_aws_headers(request_builder, &req_body_bytes)?;

        // Update metrics
        let metrics_for_stream = self.app_state.0.endpoint_metrics.clone();
        if let Some(ref api_endpoint) = api_endpoint {
            let endpoint_metrics = self
                .app_state
                .0
                .endpoint_metrics
                .health_metrics(api_endpoint.clone())?;
            endpoint_metrics.incr_req_count();
        }

        // Dispatch request (stream or sync)
        let (client_response, response_body_for_logger, tfft_rx): (
            http::Response<crate::types::body::Body>,
            crate::types::body::BodyReader,
            oneshot::Receiver<()>,
        ) = if mapper_ctx.is_stream {
            tracing::debug!(method = %method, target_url = %target_url, "dispatching stream request");
            dispatch_stream_with_retry(
                &self.app_state,
                request_builder,
                req_body_bytes.clone(),
                api_endpoint.clone(),
                metrics_for_stream,
                &req_ctx,
            )
            .await?
        } else {
            tracing::debug!(method = %method, target_url = %target_url, "dispatching sync request");
            self.dispatch_sync_with_retry(
                request_builder,
                req_body_bytes.clone(),
                &req_ctx,
            )
            .instrument(info_span!("dispatch_sync"))
            .await?
        };

        // Handle error and rate limiting (do this before processing response to avoid borrowing issues)
        let response_status = client_response.status();
        let response_headers = client_response.headers().clone();
        self.handle_error_and_rate_limiting(
            response_status,
            &response_headers,
            api_endpoint.clone(),
        )
        .await?;

        // Process response
        let client_response = self.process_response(
            client_response,
            inference_provider,
            router_id,
            auth_ctx,
            &mapper_ctx,
            api_endpoint.clone(),
            extracted_path_and_query,
        );

        // Handle logging
        self.handle_logging(
            &req_ctx,
            start_time,
            start_instant,
            target_url,
            headers,
            req_body_bytes,
            &client_response,
            response_body_for_logger,
            tfft_rx,
            &mapper_ctx,
        );

        Ok(client_response)
    }

    /// We take a `&RequestBuilder` so that `dispatch_stream` implements `FnMut`
    /// so we can use the [`backon`] crate for retries.
    async fn dispatch_stream(
        request_builder: &RequestBuilder,
        req_body_bytes: Bytes,
        api_endpoint: Option<ApiEndpoint>,
        metrics_registry: EndpointMetricsRegistry,
    ) -> Result<
        (
            http::Response<crate::types::body::Body>,
            crate::types::body::BodyReader,
            oneshot::Receiver<()>,
        ),
        ApiError,
    > {
        let request_builder = request_builder.try_clone().ok_or_else(|| {
            // in theory, this should never happen, as we'll have already
            // collected the request body
            tracing::error!(
                "failed to clone request builder, cannot dispatch stream"
            );
            ApiError::Internal(InternalError::Internal)
        })?;
        let response_stream = Client::sse_stream(
            request_builder,
            req_body_bytes,
            api_endpoint,
            &metrics_registry,
        )
        .await?;
        let mut resp_builder = http::Response::builder();
        *resp_builder.headers_mut().unwrap() = stream_response_headers();
        resp_builder = resp_builder.status(StatusCode::OK);

        let (user_resp_body, body_reader, tfft_rx) =
            BodyReader::wrap_stream(response_stream, true);

        let response = resp_builder
            .body(user_resp_body)
            .map_err(InternalError::HttpError)?;
        Ok((response, body_reader, tfft_rx))
    }

    async fn dispatch_sync(
        request_builder: &RequestBuilder,
        req_body_bytes: Bytes,
    ) -> Result<
        (
            http::Response<crate::types::body::Body>,
            crate::types::body::BodyReader,
            oneshot::Receiver<()>,
        ),
        ApiError,
    > {
        let request_builder = request_builder.try_clone().ok_or_else(|| {
            // in theory, this should never happen, as we'll have already
            // collected the request body
            tracing::error!(
                "failed to clone request builder, cannot dispatch stream"
            );
            ApiError::Internal(InternalError::Internal)
        })?;
        let response: reqwest::Response = request_builder
            .body(req_body_bytes)
            .send()
            .await
            .map_err(InternalError::ReqwestError)?;

        let status = response.status();
        let mut resp_builder = http::Response::builder().status(status);
        *resp_builder.headers_mut().unwrap() = response.headers().clone();

        // this is compiled out in release builds
        #[cfg(debug_assertions)]
        if status.is_server_error() || status.is_client_error() {
            let body =
                response.text().await.map_err(InternalError::ReqwestError)?;
            tracing::debug!(status_code = %status, error_resp = %body, "received error response");
            let bytes = bytes::Bytes::from(body);
            let stream = futures::stream::once(futures::future::ok::<
                _,
                ApiError,
            >(bytes));
            let (error_body, error_reader, tfft_rx) =
                BodyReader::wrap_stream(stream, false);
            let response = resp_builder
                .body(error_body)
                .map_err(InternalError::HttpError)?;

            return Ok((response, error_reader, tfft_rx));
        }

        let (user_resp_body, body_reader, tfft_rx) = BodyReader::wrap_stream(
            response
                .bytes_stream()
                .map_err(|e| InternalError::ReqwestError(e).into()),
            false,
        );
        let response = resp_builder
            .body(user_resp_body)
            .map_err(InternalError::HttpError)?;
        Ok((response, body_reader, tfft_rx))
    }

    #[allow(clippy::too_many_lines)]
    async fn dispatch_sync_with_retry(
        &self,
        request_builder: RequestBuilder,
        req_body_bytes: Bytes,
        req_ctx: &RequestContext,
    ) -> Result<
        (
            http::Response<crate::types::body::Body>,
            crate::types::body::BodyReader,
            oneshot::Receiver<()>,
        ),
        ApiError,
    > {
        let retry_config =
            if let Some(router_config) = req_ctx.router_config.as_ref() {
                router_config.retries.as_ref()
            } else {
                self.app_state.config().global.retries.as_ref()
            };

        if let Some(retry_config) = retry_config {
            match retry_config {
                RetryConfig::Exponential {
                    min_delay,
                    max_delay,
                    max_retries,
                    factor,
                } => {
                    let retry_strategy = ExponentialBuilder::default()
                        .with_max_delay(*max_delay)
                        .with_min_delay(*min_delay)
                        .with_max_times(usize::from(*max_retries))
                        .with_factor(factor.to_f32().unwrap_or(
                            crate::config::retry::DEFAULT_RETRY_FACTOR,
                        ))
                        .with_jitter()
                        .build();
                    let future_fn = || async {
                        let result = Self::dispatch_sync(
                            &request_builder,
                            req_body_bytes.clone(),
                        )
                        .await?;

                        Ok(result)
                    };

                    crate::utils::retry::RetryWithResult::new(future_fn, retry_strategy)
                    .when(|result: &Result<_, _>| match result {
                        Ok(response) => response.0.status().is_server_error(),
                        Err(e) => match e {
                            ApiError::Internal(InternalError::ReqwestError(
                                reqwest_error,
                            )) => reqwest_error.is_connect() || reqwest_error.status().is_some_and(|s| s.is_server_error()),
                            _ => false,
                        },
                    })
                    .notify(|result: &Result<_, _>, dur: Duration| match result {
                        Ok(result) if result.0.status().is_server_error() => {
                                tracing::warn!(
                                    error = %result.0.status(),
                                    retry_in = ?dur,
                                    "got error dispatching sync request, retrying...",
                                );
                        }
                        Err(ApiError::Internal(InternalError::ReqwestError(
                            reqwest_error,
                        ))) if reqwest_error.is_connect() || reqwest_error.status().is_some_and(|s| s.is_server_error()) => {
                                tracing::warn!(
                                    error = %reqwest_error,
                                    retry_in = ?dur,
                                    "got error dispatching sync request, retrying...",
                                );
                            }
                        _ => {}
                    })
                    .await
                }
                RetryConfig::Constant { delay, max_retries } => {
                    let retry_strategy = ConstantBuilder::default()
                        .with_delay(*delay)
                        .with_max_times(usize::from(*max_retries))
                        .with_jitter()
                        .build();
                    let future_fn = || async {
                        Self::dispatch_sync(
                            &request_builder,
                            req_body_bytes.clone(),
                        )
                        .await
                    };

                    crate::utils::retry::RetryWithResult::new(future_fn, retry_strategy)
                    .when(|result: &Result<_, _>| match result {
                        Ok(response) => response.0.status().is_server_error(),
                        Err(e) => match e {
                            ApiError::Internal(InternalError::ReqwestError(
                                reqwest_error,
                            )) => reqwest_error.is_connect() || reqwest_error.status().is_some_and(|s| s.is_server_error()),
                            _ => false,
                        },
                    })
                    .notify(|result: &Result<_, _>, dur: Duration| match result {
                        Ok(result) if result.0.status().is_server_error() => {
                                tracing::warn!(
                                    error = %result.0.status(),
                                    retry_in = ?dur,
                                    "got error dispatching sync request, retrying...",
                                );
                        }
                        Err(ApiError::Internal(InternalError::ReqwestError(
                            reqwest_error,
                        ))) if reqwest_error.is_connect() || reqwest_error.status().is_some_and(|s| s.is_server_error()) => {
                                tracing::warn!(
                                    error = %reqwest_error,
                                    retry_in = ?dur,
                                    "got error dispatching sync request, retrying...",
                                );
                            }
                        _ => {}
                    })
                    .await
                }
            }
        } else {
            Self::dispatch_sync(&request_builder, req_body_bytes.clone()).await
        }
    }
}

async fn dispatch_stream_with_retry(
    app_state: &AppState,
    request_builder: RequestBuilder,
    req_body_bytes: Bytes,
    api_endpoint: Option<ApiEndpoint>,
    metrics_registry: EndpointMetricsRegistry,
    request_ctx: &RequestContext,
) -> Result<
    (
        http::Response<crate::types::body::Body>,
        crate::types::body::BodyReader,
        oneshot::Receiver<()>,
    ),
    ApiError,
> {
    let retry_config =
        if let Some(router_config) = request_ctx.router_config.as_ref() {
            router_config.retries.as_ref()
        } else {
            app_state.config().global.retries.as_ref()
        };

    if let Some(retry_config) = retry_config {
        match retry_config {
            RetryConfig::Exponential {
                min_delay,
                max_delay,
                max_retries,
                factor,
            } => {
                let retry_strategy =
                    ExponentialBuilder::default()
                        .with_max_delay(*max_delay)
                        .with_min_delay(*min_delay)
                        .with_max_times(usize::from(*max_retries))
                        .with_factor(factor.to_f32().unwrap_or(
                            crate::config::retry::DEFAULT_RETRY_FACTOR,
                        ))
                        .with_jitter()
                        .build();
                (|| async {
                    Dispatcher::dispatch_stream(
                        &request_builder,
                        req_body_bytes.clone(),
                        api_endpoint.clone(),
                        metrics_registry.clone(),
                    )
                    .await
                })
                .retry(retry_strategy)
                .sleep(tokio::time::sleep)
                .when(|e: &ApiError| match e {
                    ApiError::StreamError(s) => s.is_retryable(),
                    _ => false,
                })
                .notify(|err: &ApiError, dur: Duration| {
                    if let ApiError::StreamError(_s) = err {
                        tracing::warn!(
                            error = %err,
                            retry_in = ?dur,
                            "upstream server error in stream, retrying...",
                        );
                    }
                })
                .await
            }
            RetryConfig::Constant { delay, max_retries } => {
                let retry_strategy = ConstantBuilder::default()
                    .with_delay(*delay)
                    .with_max_times(usize::from(*max_retries))
                    .with_jitter()
                    .build();
                (|| async {
                    Dispatcher::dispatch_stream(
                        &request_builder,
                        req_body_bytes.clone(),
                        api_endpoint.clone(),
                        metrics_registry.clone(),
                    )
                    .await
                })
                .retry(retry_strategy)
                .sleep(tokio::time::sleep)
                .when(|e: &ApiError| match e {
                    ApiError::StreamError(s) => s.is_retryable(),
                    _ => false,
                })
                .notify(|err: &ApiError, dur: Duration| {
                    if let ApiError::StreamError(_s) = err {
                        tracing::warn!(
                            error = %err,
                            retry_in = ?dur,
                            "upstream server error in stream, retrying...",
                        );
                    }
                })
                .await
            }
        }
    } else {
        Dispatcher::dispatch_stream(
            &request_builder,
            req_body_bytes.clone(),
            api_endpoint,
            metrics_registry,
        )
        .await
    }
}

fn extract_retry_after(headers: &HeaderMap) -> Option<u64> {
    let retry_after_str = headers
        .get(http::header::RETRY_AFTER)
        .and_then(|v| v.to_str().ok())?;

    // First try to parse as seconds (u64)
    if let Ok(seconds) = retry_after_str.parse::<u64>() {
        // The value is in seconds, return seconds from now
        return Some(seconds);
    }

    // If that fails, try to parse as HTTP date format
    if let Ok(datetime) =
        DateTime::parse_from_str(retry_after_str, "%a, %d %b %Y %H:%M:%S GMT")
    {
        // Convert to seconds from now
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("epoch is always earlier than now")
            .as_secs();
        let target = u64::try_from(datetime.to_utc().timestamp()).unwrap_or(0);
        if target > now {
            return Some(target - now);
        }
    }

    None
}

fn stream_response_headers() -> HeaderMap {
    HeaderMap::from_iter([
        (
            http::header::CONTENT_TYPE,
            HeaderValue::from_str("text/event-stream; charset=utf-8").unwrap(),
        ),
        (
            http::header::CONNECTION,
            HeaderValue::from_str("keep-alive").unwrap(),
        ),
        (
            http::header::TRANSFER_ENCODING,
            HeaderValue::from_str("chunked").unwrap(),
        ),
    ])
}
