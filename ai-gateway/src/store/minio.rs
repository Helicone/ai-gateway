use std::time::Duration;

use bytes::Bytes;
use reqwest::Client;
use rusty_s3::{
    Bucket, Credentials, S3Action,
    actions::{GetObject, PutObject},
};
use serde::{Deserialize, Serialize};
use url::Url;
use uuid::Uuid;

use crate::{
    app_state::AppState,
    config::minio::Config,
    error::{init::InitError, logger::LoggerError, prompts::PromptError},
    logger::service::JawnClient,
    types::{extensions::AuthContext, logger::S3Log, response::JawnResponse},
};

const DEFAULT_MINIO_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug)]
pub struct BaseMinioClient {
    pub bucket: Bucket,
    pub client: Client,
    pub credentials: Credentials,
}

impl BaseMinioClient {
    pub fn new(config: Config) -> Result<Self, InitError> {
        let bucket = Bucket::new(
            config.host,
            config.url_style.into(),
            config.bucket_name,
            config.region,
        )?;
        let client = Client::builder()
            .connect_timeout(DEFAULT_MINIO_TIMEOUT)
            .tcp_nodelay(true)
            .build()
            .map_err(InitError::CreateReqwestClient)?;
        let credentials = Credentials::new(
            config.access_key.expose(),
            config.secret_key.expose(),
        );
        Ok(Self {
            bucket,
            client,
            credentials,
        })
    }

    #[must_use]
    pub fn put_object<'obj, 'client>(
        &'client self,
        object: &'obj str,
    ) -> PutObject<'obj>
    where
        'client: 'obj,
    {
        PutObject::new(&self.bucket, Some(&self.credentials), object)
    }

    #[must_use]
    pub fn get_object<'obj, 'client>(
        &'client self,
        object: &'obj str,
    ) -> GetObject<'obj>
    where
        'client: 'obj,
    {
        GetObject::new(&self.bucket, Some(&self.credentials), object)
    }
}

const PUT_OBJECT_SIGN_DURATION: Duration = Duration::from_secs(120);
const GET_OBJECT_SIGN_DURATION: Duration = Duration::from_secs(120);

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SignedUrlRequest {
    request_id: Uuid,
    payload_size: usize,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct SignedGetUrlRequest<'a> {
    prompt_id: &'a str,
    version_id: &'a str,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SignedUrlResponse {
    url: Url,
}

pub enum MinioClient<'a> {
    SelfSigned(&'a BaseMinioClient),
    SignedByJawn(&'a JawnClient),
}

impl<'a> MinioClient<'a> {
    #[must_use]
    pub fn cloud(minio: &'a BaseMinioClient) -> Self {
        Self::SelfSigned(minio)
    }

    #[must_use]
    pub fn sidecar(jawn_client: &'a JawnClient) -> Self {
        Self::SignedByJawn(jawn_client)
    }

    #[tracing::instrument(skip_all)]
    pub async fn log_bodies(
        &self,
        app_state: &AppState,
        auth_ctx: &AuthContext,
        request_id: Uuid,
        request_body: Bytes,
        response_body: Bytes,
    ) -> Result<(), LoggerError> {
        let (signed_url, s3_log) = match self {
            Self::SelfSigned(minio) => {
                let object_path = format!(
                    "organizations/{}/requests/{}/raw_request_response_body",
                    auth_ctx.org_id.as_ref(),
                    request_id
                );
                let action = minio.put_object(&object_path);
                let signed_url = action.sign(PUT_OBJECT_SIGN_DURATION);
                let request_body = String::from_utf8(request_body.to_vec())?;
                let response_body = String::from_utf8(response_body.to_vec())?;

                tracing::trace!("got signed url for self hosted minio");
                let s3_log = S3Log::new(request_body, response_body);
                (signed_url, s3_log)
            }
            Self::SignedByJawn(client) => {
                let signed_request_url =
                    app_state
                        .config()
                        .helicone
                        .base_url
                        .join("/v1/router/control-plane/sign-s3-url")?;
                let request_body = String::from_utf8(request_body.to_vec())?;
                let response_body = String::from_utf8(response_body.to_vec())?;

                let s3_log = S3Log::new(request_body, response_body);
                let bytes = serde_json::to_vec(&s3_log).map_err(|e| {
                    tracing::error!(error = %e, "failed to serialize s3 log");
                    LoggerError::InvalidLogMessage
                })?;

                let signed_url = client
                  .request_client
                  .post(signed_request_url)
                  .json(&SignedUrlRequest { request_id, payload_size: bytes.len() })
                  .header(
                    "authorization",
                    format!("Bearer {}", auth_ctx.api_key.expose()),
                  )
                  .send()
                  .await
                  .map_err(|e| {
                    tracing::error!(error = %e, "failed to send request for signed url");
                    LoggerError::FailedToSendRequest(e)
                  })?
                  .error_for_status()
                  .map_err(|e| {
                    tracing::error!(error = %e, "failed to get signed url");
                    LoggerError::ResponseError(e)
                  })?;

                let signed_url = signed_url.json::<JawnResponse<SignedUrlResponse>>().await.map_err(|e| {
                    tracing::error!(error = %e, "failed to deserialize signed url response");
                    LoggerError::ResponseError(e)
                })?.data().map_err(|e| {
                    tracing::error!(error = %e, "failed to get signed url");
                    LoggerError::UnexpectedResponse(e)
                })?;
                tracing::trace!("got signed url for sidecar");

                (signed_url.url, s3_log)
            }
        };

        let _resp = app_state
            .0
            .minio
            .client
            .put(signed_url)
            .json(&s3_log)
            .send()
            .await
            .map_err(|e| {
                tracing::debug!(error = %e, "failed to send request to S3");
                LoggerError::FailedToSendRequest(e)
            })?
            .error_for_status()
            .map_err(|e| {
                tracing::error!(error = %e, "failed to log bodies in S3");
                LoggerError::ResponseError(e)
            })?;
        Ok(())
    }

    #[tracing::instrument(skip_all)]
    pub async fn pull_prompt_body(
        &self,
        app_state: &AppState,
        auth_ctx: &AuthContext,
        prompt_id: &str,
        version_id: &str,
    ) -> Result<serde_json::Value, PromptError> {
        let object_path = format!(
            "organizations/{}/prompts/{}/versions/{}/prompt_body",
            auth_ctx.org_id.as_ref(),
            prompt_id,
            version_id,
        );

        let signed_url = match self {
            Self::SelfSigned(minio) => {
                let action = minio.get_object(&object_path);
                action.sign(GET_OBJECT_SIGN_DURATION)
            }
            Self::SignedByJawn(client) => {
                let signed_request_url =
                    app_state
                        .config()
                        .helicone
                        .base_url
                        .join("/v1/router/control-plane/sign-s3-get-url")?;

                let signed_url = client
                    .request_client
                    .post(signed_request_url)
                    .json(&SignedGetUrlRequest {
                        prompt_id,
                        version_id,
                    })
                    .header(
                        "authorization",
                        format!("Bearer {}", auth_ctx.api_key.expose()),
                    )
                    .send()
                    .await
                    .map_err(|e| {
                        tracing::error!(error = %e, "failed to send request for signed get url");
                        PromptError::FailedToSendRequest(e)
                    })?
                    .error_for_status()
                    .map_err(|e| {
                        tracing::error!(error = %e, "failed to get signed get url");
                        PromptError::FailedToGetPromptBody(e)
                    })?;

                let signed_url = signed_url.json::<JawnResponse<SignedUrlResponse>>().await.map_err(|e| {
                    tracing::error!(error = %e, "failed to deserialize signed get url response");
                    PromptError::FailedToGetPromptBody(e)
                })?.data().map_err(|e| {
                    tracing::error!(error = %e, "failed to get signed get url");
                    PromptError::UnexpectedResponse(e)
                })?;
                tracing::trace!("got signed get url for sidecar");

                signed_url.url
            }
        };

        let response = app_state
            .0
            .minio
            .client
            .get(signed_url)
            .send()
            .await
            .map_err(|e| {
                tracing::debug!(error = %e, "failed to send request to S3 for prompt body");
                PromptError::FailedToSendRequest(e)
            })?
            .error_for_status()
            .map_err(|e| {
                tracing::error!(error = %e, "failed to get prompt body from S3");
                PromptError::FailedToGetPromptBody(e)
            })?;

        let response_bytes = response.bytes().await.map_err(|e| {
            tracing::error!(error = %e, "failed to read prompt body bytes");
            PromptError::FailedToGetPromptBody(e)
        })?;

        serde_json::from_slice(&response_bytes).map_err(|e| {
            tracing::error!(error = %e, "failed to deserialize prompt body JSON");
            PromptError::UnexpectedResponse(e.to_string())
        })
    }
}
