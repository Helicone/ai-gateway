use std::{
    task::{Context, Poll},
    time::Duration,
};

use futures::future::BoxFuture;
use tracing::{Instrument, info_span};
use http_body_util::BodyExt;
use rusty_s3::S3Action;

use crate::{
    app_state::AppState,
    error::{
        init::InitError,
        prompts::PromptError,
        api::ApiError,
        internal::InternalError,
    },

    types::{
        extensions::AuthContext,
        request::Request,
        response::Response,
    }
};

#[derive(Debug, Clone)]
pub struct PromptLayer {
    app_state: AppState,
}

impl PromptLayer {
    pub fn new(
        app_state: AppState,
    ) -> Result<Self, InitError> {
        Ok(Self { app_state })
    }
}

impl<S> tower::Layer<S> for PromptLayer {
    type Service = PromptService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        PromptService {
            inner,
            app_state: self.app_state.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct PromptService<S> {
    inner: S,
    app_state: AppState,
}

impl<S> tower::Service<Request> for PromptService<S>
where
    S: tower::Service<
            Request,
            Response = http::Response<crate::types::body::Body>,
            Error = ApiError,
        > + Clone
        + Send
        + 'static,
    S::Future: Send + 'static,
{
    type Response = Response;
    type Error = ApiError;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    #[inline]
    fn poll_ready(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    #[tracing::instrument(name = "prompt", skip_all)]
    fn call(&mut self, req: Request) -> Self::Future {
        let mut inner = self.inner.clone();
        let app_state = self.app_state.clone();
        std::mem::swap(&mut self.inner, &mut inner);
        Box::pin(async move {
            let req = tokio::task::spawn_blocking(move || async move {
                build_prompt_request(app_state, req)
                .instrument(info_span!("build_prompt_request"))
                .await
            })
            .await
            .map_err(InternalError::PromptTaskError)?
            .await?;
            let response = inner.call(req).await?;
            Ok(response)
        })
    }
}

#[derive(Debug, serde::Deserialize)]
struct Prompt2025VersionResponse {
    data: Prompt2025Version,
}

#[derive(Debug, serde::Deserialize)]
struct Prompt2025Version {
    id: String,
}

// 0. Use ai-gateway/src/config/minio.rs for s3 client
// 0. create wrapper type around openai request type that includes new fields for prompt inputs

// 1. use boxfuture as future type
// 2. receive request
// 3. deserialize to openai request type
// 4. do prompt templating
//   - draw the rest of the owl
// 5. call inner service
// 7. propagate response to client
async fn build_prompt_request(
    app_state: AppState,
    req: Request,   
) -> Result<Request, ApiError> {
    let (parts, body) = req.into_parts();
    let body_bytes = body
        .collect()
        .await
        .map_err(InternalError::CollectBodyError)?
        .to_bytes();

    if app_state.config().helicone.is_observability_enabled() {
        // Should return an error here, since prompts requires authentication
        // if !app_state.config().helicone.authentication {
        //     tracing::warn!("Authentication is disabled, ??? cannot get prompt body");
        //     // ???
        // }
        let auth_ctx =
            parts.extensions.get::<AuthContext>().cloned().ok_or(
                InternalError::ExtensionNotFound("AuthContext"),
            )?;

        // PARSE BODY TO GET PROMPT ID (AND OPTIONALLY VERSION ID)


        // GIVEN PROMPT ID, FIND PRODUCTION VERSION FROM PG DB
        let prompt_id = "ROyT8F"; // placeholder
        let endpoint_url = app_state
            .config()
            .helicone
            .base_url
            .join("/v1/prompt-2025/query/production-version")
            .map_err(|_| InternalError::Internal)?;
        
        tracing::info!("fetching production version from: {}", endpoint_url);

        let jawn_response = app_state
            .0
            .jawn_http_client
            .request_client
            .post(endpoint_url)
            .json(&serde_json::json!({
                "promptId": prompt_id
            }))
            .header(
                "authorization",
                format!("Bearer {}", auth_ctx.api_key.expose()),
            )
            .send()
            .await
            .map_err(|e| {
                tracing::debug!(error = %e, "failed to send request to helicone");
                ApiError::Internal(InternalError::PromptError(PromptError::FailedToSendRequest(e)))
            })?
            .error_for_status()
            .map_err(|e| {
                tracing::error!(error = %e, "failed to get production version from helicone");
                ApiError::Internal(InternalError::PromptError(PromptError::FailedToGetProductionVersion(e)))
            })?;
        
        let version_response = jawn_response.json::<Prompt2025VersionResponse>().await
            .map_err(|e| {
                tracing::error!(error = %e, "failed to parse version response");
                ApiError::Internal(InternalError::PromptError(PromptError::FailedToGetProductionVersion(e)))
            })?;

        tracing::info!("got version response: {:?}", version_response);

        // PULL PROMPT BODY
        let object_path = format!(
            "organizations/{}/prompts/{}/versions/{}/prompt_body",
            auth_ctx.org_id.as_ref(),
            prompt_id,
            version_response.data.id,
        );
        tracing::info!("using S3 object path: {}", object_path);

        let minio = &app_state.0.minio;
        let signed_url = minio.get_object(&object_path).sign(Duration::from_secs(120));
        tracing::info!("generated signed S3 URL: {}", signed_url);

        let s3_response = minio.client
            .get(signed_url)
            .send()
            .await
            .map_err(|e| {
                tracing::debug!(error = %e, "failed to send request to S3");
                ApiError::Internal(InternalError::PromptError(PromptError::FailedToSendRequest(e)))
            })?;

        tracing::info!("s3 response status: {}", s3_response.status());
        
        let s3_response = s3_response
            .error_for_status()
            .map_err(|e| {
                tracing::error!(error = %e, "failed to get prompt body from S3");
                ApiError::Internal(InternalError::PromptError(PromptError::FailedToGetPromptBody(e)))
            })?;

        let response_bytes = s3_response.bytes().await
            .map_err(|e| ApiError::Internal(InternalError::PromptError(PromptError::FailedToGetPromptBody(e))))?;

        tracing::info!("S3 response bytes length: {}", response_bytes.len());

        // TEMPORARY CODE TO DEBUG S3 PROMPT BODY
        
        let decompressed_bytes = if response_bytes.len() >= 2 && response_bytes[0] == 31 && response_bytes[1] == 139 {
            tracing::info!("Detected gzip compression, decompressing...");
            use std::io::Read;
            let mut decoder = flate2::read::GzDecoder::new(&response_bytes[..]);
            let mut decompressed = Vec::new();
            decoder.read_to_end(&mut decompressed)
                .map_err(|e| {
                    tracing::error!(error = %e, "failed to decompress gzip data");
                    ApiError::Internal(InternalError::Internal)
                })?;
            decompressed
        } else {
            response_bytes.to_vec()
        };

        match serde_json::from_slice::<serde_json::Value>(&decompressed_bytes) {
            Ok(json_value) => {
                tracing::info!("Successfully parsed as JSON: {}", serde_json::to_string_pretty(&json_value).unwrap_or_default());
            }
            Err(_) => {
                match String::from_utf8(decompressed_bytes) {
                    Ok(text) => {
                        tracing::info!("Content as text: {}", text);
                    }
                    Err(_) => {
                        tracing::info!("Content is still binary after decompression");
                    }
                }
            }
        }
    }

    // fail the request here? or pass it through, since it will fail later?
    // remove this, we should reassemble the request body using the prompt body
    let req = Request::from_parts(parts, axum_core::body::Body::from(body_bytes));
    Ok(req)
}