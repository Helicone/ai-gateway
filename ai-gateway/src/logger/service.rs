use std::time::Duration;

use bytes::Bytes;
use chrono::{DateTime, Utc};
use http::{HeaderMap, StatusCode};
use http_body_util::BodyExt;
use indexmap::IndexMap;
use opentelemetry::KeyValue;
use reqwest::Client;
use tokio::{sync::oneshot, time::Instant};
use typed_builder::TypedBuilder;
use url::Url;
use uuid::Uuid;

use crate::{
    app_state::AppState,
    config::deployment_target::DeploymentTarget,
    error::{init::InitError, logger::LoggerError},
    metrics::tfft::TFFTFuture,
    store::minio::MinioClient,
    types::{
        body::BodyReader,
        extensions::{AuthContext, MapperContext, PromptContext},
        logger::{
            HeliconeLogMetadata, Log, LogMessage, RequestLog, ResponseLog,
        },
        provider::InferenceProvider,
        router::RouterId,
    },
};

const JAWN_CONNECT_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug)]
pub struct JawnClient {
    pub request_client: Client,
}

impl JawnClient {
    pub fn new() -> Result<Self, InitError> {
        Ok(Self {
            request_client: Client::builder()
                .tcp_nodelay(true)
                .connect_timeout(JAWN_CONNECT_TIMEOUT)
                .build()
                .map_err(InitError::CreateReqwestClient)?,
        })
    }
}

#[derive(Debug, TypedBuilder)]
pub struct LoggerService {
    app_state: AppState,
    auth_ctx: AuthContext,
    start_time: DateTime<Utc>,
    start_instant: Instant,
    response_body: BodyReader,
    request_body: Bytes,
    target_url: Url,
    request_headers: HeaderMap,
    response_status: StatusCode,
    provider: InferenceProvider,
    mapper_ctx: MapperContext,
    router_id: Option<RouterId>,
    deployment_target: DeploymentTarget,
    tfft_rx: oneshot::Receiver<()>,
    request_id: Uuid,
    #[builder(default)]
    cache_enabled: Option<bool>,
    #[builder(default)]
    cache_bucket_max_size: Option<u8>,
    #[builder(default)]
    cache_control: Option<String>,
    #[builder(default)]
    cache_reference_id: Option<String>,
    #[builder(default)]
    prompt_ctx: Option<PromptContext>,
}

impl LoggerService {
    #[tracing::instrument(skip_all)]
    #[allow(clippy::cast_precision_loss, clippy::too_many_lines)]
    pub async fn log(mut self) -> Result<(), LoggerError> {
        tracing::trace!("logging request");
        let tfft_future = TFFTFuture::new(self.start_instant, self.tfft_rx);
        let collect_future = self.response_body.collect();
        let (response_body, tfft_duration) =
            tokio::join!(collect_future, tfft_future);
        let response_body = response_body
            .inspect_err(|_| tracing::error!("infallible errored"))
            .expect("infallible never errors")
            .to_bytes();
        let tfft_duration = tfft_duration.unwrap_or_else(|_| {
            tracing::error!("Failed to get TFFT signal");
            Duration::from_secs(0)
        });
        tracing::trace!(tfft_duration = ?tfft_duration, "tfft_duration");
        let req_body_len = self.request_body.len();
        let resp_body_len = response_body.len();
        let s3_client = if self.app_state.config().deployment_target.is_cloud()
        {
            MinioClient::cloud(&self.app_state.0.minio)
        } else {
            MinioClient::sidecar(&self.app_state.0.jawn_http_client)
        };
        s3_client
            .log_bodies(
                &self.app_state,
                &self.auth_ctx,
                self.request_id,
                self.request_body,
                response_body,
            )
            .await?;

        let model = self
            .mapper_ctx
            .model
            .as_ref()
            .map_or_else(|| "unknown".to_string(), ToString::to_string);
        let attributes = [
            KeyValue::new("provider", self.provider.to_string()),
            KeyValue::new("model", model),
            KeyValue::new("path", self.target_url.path().to_string()),
        ];
        self.app_state
            .0
            .metrics
            .tfft_duration
            .record(tfft_duration.as_millis() as f64, &attributes);

        let helicone_metadata = HeliconeLogMetadata::from_headers(
            &mut self.request_headers,
            self.router_id,
            &self.deployment_target,
            self.prompt_ctx,
        )?;
        let req_path = self.target_url.path().to_string();
        let provider = match self.provider {
            InferenceProvider::Ollama => "CUSTOM".to_string(),
            InferenceProvider::GoogleGemini => "GOOGLE".to_string(),
            provider => provider.to_string().to_uppercase(),
        };

        // Extract helicone-properties-* headers
        let mut properties = IndexMap::new();
        for (name, value) in &self.request_headers {
            if name.as_str().starts_with("helicone-property-") {
                if let Ok(value_str) = value.to_str() {
                    properties.insert(name.to_string(), value_str.to_string());
                }
            }
        }

        let request_log = RequestLog::builder()
            .id(self.request_id)
            .user_id(self.auth_ctx.user_id)
            .properties(properties)
            .target_url(self.target_url)
            .provider(provider)
            .body_size(req_body_len as f64)
            .path(req_path)
            .request_created_at(self.start_time)
            .is_stream(self.mapper_ctx.is_stream)
            .cache_enabled(self.cache_enabled)
            .cache_bucket_max_size(self.cache_bucket_max_size)
            .cache_control(self.cache_control)
            .cache_reference_id(self.cache_reference_id)
            .build();
        let response_log = ResponseLog::builder()
            .id(self.request_id)
            .status(f64::from(self.response_status.as_u16()))
            .body_size(resp_body_len as f64)
            .response_created_at(Utc::now())
            .delay_ms(tfft_duration.as_millis() as f64)
            .build();
        let log = Log::new(request_log, response_log);
        let log_message = LogMessage::builder()
            .authorization(self.auth_ctx.api_key.expose().to_string())
            .helicone_meta(helicone_metadata)
            .log(log)
            .build();

        let helicone_url = self
            .app_state
            .config()
            .helicone
            .base_url
            .join("/v1/log/request")?;

        let _helicone_response = self
            .app_state
            .0
            .jawn_http_client
            .request_client
            .post(helicone_url)
            .json(&log_message)
            .header(
                "authorization",
                format!("Bearer {}", self.auth_ctx.api_key.expose()),
            )
            .send()
            .await
            .map_err(|e| {
                tracing::debug!(error = %e, "failed to send request to helicone");
                LoggerError::FailedToSendRequest(e)
            })?
            .error_for_status()
            .map_err(|e| {
                tracing::error!(error = %e, "failed to log request to helicone");
                LoggerError::ResponseError(e)
            })?;

        tracing::debug!("successfully logged request");
        Ok(())
    }
}
