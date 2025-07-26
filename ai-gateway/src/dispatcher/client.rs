use bytes::Bytes;
use futures::StreamExt;
use http_body_util::BodyExt;
use reqwest::RequestBuilder;
use reqwest_eventsource::{Event, EventSource, RequestBuilderExt};
use tracing::{Instrument, info_span};

use crate::{
    app_state::AppState,
    discover::monitor::metrics::EndpointMetricsRegistry,
    dispatcher::{
        SSEStream, anthropic_client::Client as AnthropicClient,
        bedrock_client::Client as BedrockClient,
        ollama_client::Client as OllamaClient,
        openai_compatible_client::Client as OpenAICompatibleClient,
    },
    endpoints::ApiEndpoint,
    error::{
        api::ApiError, auth::AuthError, init::InitError,
        internal::InternalError, stream::StreamError,
    },
    types::{
        extensions::AuthContext,
        provider::{InferenceProvider, ProviderKey},
    },
};

pub trait ProviderClient {
    async fn authenticate(
        &self,
        app_state: &AppState,
        request_builder: reqwest::RequestBuilder,
        req_body_bytes: &bytes::Bytes,
        auth_ctx: Option<&AuthContext>,
        provider: InferenceProvider,
    ) -> Result<reqwest::RequestBuilder, ApiError>;
}

impl ProviderClient for Client {
    async fn authenticate(
        &self,
        app_state: &AppState,
        request_builder: reqwest::RequestBuilder,
        req_body_bytes: &bytes::Bytes,
        auth_ctx: Option<&AuthContext>,
        provider: InferenceProvider,
    ) -> Result<reqwest::RequestBuilder, ApiError> {
        match self {
            Client::Bedrock(inner) => inner
                .extract_and_sign_aws_headers(request_builder, req_body_bytes),
            Client::OpenAICompatible(_) | Client::Anthropic(_) => {
                self.authenticate_inner(
                    app_state,
                    request_builder,
                    auth_ctx,
                    provider,
                )
                .await
            }
            Client::Ollama(_) => Ok(request_builder),
        }
    }
}

#[derive(Debug, Clone)]
pub enum Client {
    OpenAICompatible(OpenAICompatibleClient),
    Anthropic(AnthropicClient),
    Ollama(OllamaClient),
    Bedrock(BedrockClient),
}

impl Client {
    async fn authenticate_inner(
        &self,
        app_state: &AppState,
        request_builder: reqwest::RequestBuilder,
        auth_ctx: Option<&AuthContext>,
        provider: InferenceProvider,
    ) -> Result<reqwest::RequestBuilder, ApiError> {
        if app_state.0.config.deployment_target.is_cloud() {
            if let Some(auth_ctx) = auth_ctx {
                let org_id = auth_ctx.org_id;

                let provider_key = app_state
                    .0
                    .provider_keys
                    .get_provider_key(&provider, Some(&org_id))
                    .await;

                if let Some(ProviderKey::Secret(key)) = provider_key
                    && key.expose() != ""
                {
                    let request_builder = match self {
                        Client::OpenAICompatible(_) => {
                            OpenAICompatibleClient::set_auth_header(
                                request_builder,
                                &key,
                            )
                        }
                        Client::Anthropic(_) => {
                            AnthropicClient::set_auth_header(
                                request_builder,
                                &key,
                            )
                        }
                        _ => request_builder,
                    };

                    return Ok(request_builder);
                }

                let refetched_org_provider_keys = app_state
                    .0
                    .router_store
                    .as_ref()
                    .ok_or(ApiError::Internal(InternalError::Internal))?
                    .get_org_provider_keys(org_id)
                    .await
                    .map_err(|_| ApiError::Internal(InternalError::Internal))?;

                let provider_key = refetched_org_provider_keys.get(&provider);

                app_state
                    .set_org_provider_keys(
                        org_id,
                        refetched_org_provider_keys.clone(),
                    )
                    .await;

                if let Some(ProviderKey::Secret(key)) = provider_key {
                    let request_builder = match self {
                        Client::OpenAICompatible(_) => {
                            OpenAICompatibleClient::set_auth_header(
                                request_builder,
                                key,
                            )
                        }
                        Client::Anthropic(_) => {
                            AnthropicClient::set_auth_header(
                                request_builder,
                                key,
                            )
                        }
                        _ => request_builder,
                    };

                    return Ok(request_builder);
                }

                return Err(ApiError::Authentication(
                    AuthError::ProviderKeyNotFound,
                ));
            }
            Err(ApiError::Authentication(AuthError::ProviderKeyNotFound))
        } else {
            Ok(request_builder)
        }
    }

    pub(crate) async fn sse_stream<B>(
        request_builder: RequestBuilder,
        body: B,
        api_endpoint: Option<ApiEndpoint>,
        metrics_registry: &EndpointMetricsRegistry,
    ) -> Result<SSEStream, ApiError>
    where
        B: Into<reqwest::Body>,
    {
        let event_source = request_builder
            .body(body)
            .eventsource()
            .map_err(|_e| InternalError::Internal)?;
        let stream =
            sse_stream(event_source, api_endpoint, metrics_registry.clone())
                .await?;
        Ok(stream)
    }

    pub(crate) async fn new(
        app_state: &AppState,
        inference_provider: InferenceProvider,
    ) -> Result<Self, InitError> {
        if inference_provider == InferenceProvider::Ollama {
            return Self::new_inner(app_state, inference_provider, None);
        }
        let api_key = &app_state
            .0
            .provider_keys
            .get_provider_key(&inference_provider, None)
            .await;

        Self::new_inner(app_state, inference_provider, api_key.as_ref())
    }

    fn new_inner(
        app_state: &AppState,
        inference_provider: InferenceProvider,
        api_key: Option<&ProviderKey>,
    ) -> Result<Self, InitError> {
        // connection timeout, timeout, etc.
        let base_client = reqwest::Client::builder()
            .connect_timeout(app_state.0.config.dispatcher.connection_timeout)
            .timeout(app_state.0.config.dispatcher.timeout)
            .tcp_nodelay(true);

        match inference_provider {
            InferenceProvider::OpenAI
            | InferenceProvider::GoogleGemini
            | InferenceProvider::Named(_) => {
                let openai_compatible_client = OpenAICompatibleClient::new(
                    app_state,
                    base_client,
                    inference_provider,
                    api_key,
                )?;
                Ok(Self::OpenAICompatible(openai_compatible_client))
            }
            InferenceProvider::Anthropic => Ok(Self::Anthropic(
                AnthropicClient::new(app_state, base_client, api_key)?,
            )),
            InferenceProvider::Bedrock => Ok(Self::Bedrock(
                BedrockClient::new(app_state, base_client, api_key)?,
            )),
            InferenceProvider::Ollama => {
                Ok(Self::Ollama(OllamaClient::new(app_state, base_client)?))
            }
        }
    }
}

impl AsRef<reqwest::Client> for Client {
    fn as_ref(&self) -> &reqwest::Client {
        match self {
            Client::OpenAICompatible(client) => &client.0,
            Client::Anthropic(client) => &client.0,
            Client::Ollama(client) => &client.0,
            Client::Bedrock(client) => &client.inner,
        }
    }
}

/// Request which responds with SSE.
/// [server-sent events](https://developer.mozilla.org/en-US/docs/Web/API/Server-sent_events/Using_server-sent_events#event_stream_format)
pub(super) async fn sse_stream(
    mut event_source: EventSource,
    api_endpoint: Option<ApiEndpoint>,
    metrics_registry: EndpointMetricsRegistry,
) -> Result<SSEStream, StreamError> {
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    // we want to await the first event so that we can propagate errors
    match event_source.next().await {
        Some(Ok(event)) => match event {
            Event::Message(message) if message.data != "[DONE]" => {
                let data = Bytes::from(message.data);

                if let Err(_e) = tx.send(Ok(data)) {
                    tracing::trace!("rx dropped before stream ended");
                }
            }
            _ => {}
        },
        Some(Err(e)) => {
            handle_stream_error(e, api_endpoint.clone(), &metrics_registry)
                .await?;
        }
        None => {}
    }

    tokio::spawn(
        async move {
            while let Some(ev) = event_source.next().await {
                match ev {
                    Err(e) => {
                        if matches!(e, reqwest_eventsource::Error::StreamEnded) {
                            // `StreamEnded` is returned for valid stream end cases
                            // so we don't send the error in the channel
                            tracing::trace!("stream ended");
                            break;
                        }

                        if let Err(e) = handle_stream_error_with_tx(e, tx.clone(), api_endpoint.clone(), &metrics_registry).await {
                            tracing::error!(error = %e, "failed to handle stream error");
                            break;
                        }
                    }
                    Ok(event) => match event {
                        Event::Message(message) => {
                            if message.data == "[DONE]" {
                                break;
                            }

                            let data = Bytes::from(message.data);

                            if let Err(_e) = tx.send(Ok(data)) {
                                tracing::trace!(
                                    "rx dropped before stream ended"
                                );
                                break;
                            }
                        }
                        Event::Open => {}
                    },
                }
            }

            event_source.close();
        }
        .instrument(info_span!("sse_stream")),
    );

    Ok(Box::pin(
        tokio_stream::wrappers::UnboundedReceiverStream::new(rx),
    ))
}

async fn handle_stream_error_with_tx(
    error: reqwest_eventsource::Error,
    tx: tokio::sync::mpsc::UnboundedSender<Result<Bytes, ApiError>>,
    api_endpoint: Option<ApiEndpoint>,
    metrics_registry: &EndpointMetricsRegistry,
) -> Result<(), InternalError> {
    record_stream_err_metrics(&error, api_endpoint.clone(), metrics_registry);
    match error {
        reqwest_eventsource::Error::InvalidStatusCode(
            status_code,
            response,
        ) => {
            let http_resp = http::Response::from(response);
            let (_parts, body) = http_resp.into_parts();
            let body = body.collect().await?.to_bytes();

            cfg_if::cfg_if! {
                // this is compiled out in release builds
                if #[cfg(debug_assertions)] {
                    let text = String::from_utf8_lossy(&body);
                    tracing::debug!(status_code = %status_code, body = %text, "received error response in stream");
                } else {
                    if status_code.is_server_error() {
                        tracing::error!(status_code = %status_code, "received server error in stream");
                    } else if status_code.is_client_error() {
                        tracing::debug!(status_code = %status_code, "received client error in stream");
                    }
                }
            }

            if let Err(e) = tx.send(Ok(body)) {
                tracing::error!(error = %e, "rx dropped before stream ended");
            }
            Ok(())
        }
        e => {
            if let Err(e) = tx.send(Err(ApiError::StreamError(
                StreamError::StreamError(Box::new(e)),
            ))) {
                tracing::error!(error = %e, "rx dropped before stream ended");
            }
            Ok(())
        }
    }
}

async fn handle_stream_error(
    error: reqwest_eventsource::Error,
    api_endpoint: Option<ApiEndpoint>,
    metrics_registry: &EndpointMetricsRegistry,
) -> Result<(), StreamError> {
    record_stream_err_metrics(&error, api_endpoint.clone(), metrics_registry);
    match error {
        reqwest_eventsource::Error::InvalidStatusCode(
            status_code,
            response,
        ) => {
            cfg_if::cfg_if! {
                // this is compiled out in release builds
                if #[cfg(debug_assertions)] {
                    let http_resp = http::Response::from(response);
                    let (parts, body) = http_resp.into_parts();
                    let body = match body.collect().await {
                        Err(e) => {
                            let error =
                                axum_core::Error::new(InternalError::ReqwestError(e));
                            return Err(StreamError::BodyError(error));
                        }
                        Ok(body) => body.to_bytes(),
                    };
                    let text = String::from_utf8_lossy(&body);
                    tracing::debug!(status_code = %status_code, body = %text, "received error response in stream");
                    let response = http::Response::from_parts(parts, body);
                    Err(StreamError::StreamError(Box::new(reqwest_eventsource::Error::InvalidStatusCode(
                        status_code,
                        response.into(),
                    ))))
                } else {
                    if status_code.is_server_error() {
                        tracing::error!(status_code = %status_code, "received server error in stream");
                    } else if status_code.is_client_error() {
                        tracing::debug!(status_code = %status_code, "received client error in stream");
                    }


                    Err(StreamError::StreamError(Box::new(reqwest_eventsource::Error::InvalidStatusCode(
                        status_code,
                        response,
                    ))))
                }
            }
        }
        e => {
            tracing::error!(error = %e, "received error in stream");
            Err(StreamError::StreamError(Box::new(e)))
        }
    }
}

fn record_stream_err_metrics(
    stream_error: &reqwest_eventsource::Error,
    api_endpoint: Option<ApiEndpoint>,
    metrics_registry: &EndpointMetricsRegistry,
) {
    if let Some(api_endpoint) = api_endpoint {
        metrics_registry.health_metrics(api_endpoint).map(|metrics| {
            metrics.incr_for_stream_error(stream_error);
        }).inspect_err(|e| {
            tracing::error!(error = %e, "failed to increment stream error metrics");
        }).ok();
    }
}
