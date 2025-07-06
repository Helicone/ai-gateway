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
        service::record_stream_err_metrics,
    },
    endpoints::ApiEndpoint,
    error::{
        api::ApiError, init::InitError, internal::InternalError,
        stream::StreamError,
    },
    types::{
        provider::{InferenceProvider, ProviderKey},
        router::RouterId,
    },
};

pub trait ProviderClient {
    fn extract_and_sign_aws_headers(
        &self,
        request_builder: reqwest::RequestBuilder,
        req_body_bytes: &bytes::Bytes,
    ) -> Result<reqwest::RequestBuilder, ApiError>;
}

impl ProviderClient for Client {
    fn extract_and_sign_aws_headers(
        &self,
        request_builder: reqwest::RequestBuilder,
        req_body_bytes: &bytes::Bytes,
    ) -> Result<reqwest::RequestBuilder, ApiError> {
        match self {
            Client::Bedrock(inner) => inner
                .extract_and_sign_aws_headers(request_builder, req_body_bytes),
            _ => Ok(request_builder),
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
            sse_stream(event_source, api_endpoint, metrics_registry).await?;
        Ok(stream)
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
            | InferenceProvider::Mistral => {
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

    pub(crate) async fn new_for_router(
        app_state: &AppState,
        inference_provider: InferenceProvider,
        router_id: &RouterId,
    ) -> Result<Self, InitError> {
        if inference_provider == InferenceProvider::Ollama {
            return Self::new_inner(app_state, inference_provider, None);
        }
        let api_key = &app_state
            .get_provider_api_key_for_router(router_id, &inference_provider)
            .await?;

        Self::new_inner(app_state, inference_provider, api_key.as_ref())
    }

    pub(crate) fn new_for_direct_proxy(
        app_state: &AppState,
        inference_provider: InferenceProvider,
    ) -> Result<Self, InitError> {
        if inference_provider == InferenceProvider::Ollama {
            return Self::new_inner(app_state, inference_provider, None);
        }
        let api_key = &app_state
            .get_provider_api_key_for_direct_proxy(&inference_provider)?;

        Self::new_inner(app_state, inference_provider, api_key.as_ref())
    }

    pub(crate) fn new_for_unified_api(
        app_state: &AppState,
        inference_provider: InferenceProvider,
    ) -> Result<Self, InitError> {
        if inference_provider == InferenceProvider::Ollama {
            return Self::new_inner(app_state, inference_provider, None);
        }
        // we're cheating here but this will be changed soon for cloud hosted
        // version
        let api_key = &app_state
            .get_provider_api_key_for_direct_proxy(&inference_provider)?;

        Self::new_inner(app_state, inference_provider, api_key.as_ref())
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
    metrics_registry: &EndpointMetricsRegistry,
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
            let stream_error = StreamError::StreamError(Box::new(e));
            record_stream_err_metrics(
                &stream_error,
                api_endpoint,
                metrics_registry,
            );
            return Err(stream_error);
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

                        if let Err(e) = handle_stream_error(tx.clone(), e).await {
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

async fn handle_stream_error(
    tx: tokio::sync::mpsc::UnboundedSender<Result<Bytes, ApiError>>,
    error: reqwest_eventsource::Error,
) -> Result<(), InternalError> {
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
