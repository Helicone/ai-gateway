use std::task::{Context, Poll};

use futures::future::BoxFuture;
use http_body_util::BodyExt;
use tracing::{Instrument, info_span};
use std::string::ToString;
use crate::{
    app_state::AppState,
    config::DeploymentTarget,
    error::{
        api::ApiError, internal::InternalError,
        invalid_req::InvalidRequestError, prompts::PromptError,
    },
    s3::S3Client,
    types::{
        extensions::AuthContext,
        request::Request,
        response::{JawnResponse, Response},
    },
};

#[derive(Debug, Clone)]
pub struct PromptLayer {
    app_state: AppState,
}

impl PromptLayer {
    pub fn new(app_state: AppState) -> PromptLayer {
        Self { app_state }
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
struct Prompt2025Version {
    id: String,
}

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

    let request_json: serde_json::Value = serde_json::from_slice(&body_bytes)
        .map_err(|e| {
        ApiError::InvalidRequest(InvalidRequestError::InvalidRequestBody(e))
    })?;

    let Some(prompt_id) = request_json
        .get("promptId")
        .and_then(|v| v.as_str())
        .map(ToString::to_string)
    else {
        let req =
            Request::from_parts(parts, axum_core::body::Body::from(body_bytes));
        return Ok(req);
    };

    let auth_ctx = parts
        .extensions
        .get::<AuthContext>()
        .cloned()
        .ok_or(InternalError::ExtensionNotFound("AuthContext"))?;

    let version_response =
        get_prompt_version(&app_state, &prompt_id, &auth_ctx)
            .await?
            .data()
            .map_err(|e| {
                tracing::error!(error = %e, "failed to get production version");
                ApiError::Internal(InternalError::PromptError(
                    PromptError::UnexpectedResponse(e),
                ))
            })?;

    let s3_client = match app_state.config().deployment_target {
        DeploymentTarget::Cloud => S3Client::cloud(&app_state.0.minio),
        DeploymentTarget::Sidecar => {
            S3Client::sidecar(&app_state.0.jawn_http_client)
        }
    };

    let prompt_body_json = s3_client
        .pull_prompt_body(
            &app_state,
            &auth_ctx,
            &prompt_id,
            &version_response.id,
        )
        .await
        .map_err(|e| ApiError::Internal(InternalError::PromptError(e)))?;

    tracing::debug!(
        "Prompt body from S3: {}",
        serde_json::to_string_pretty(&prompt_body_json).unwrap_or_default()
    );

    let merged_body =
        merge_prompt_with_request(prompt_body_json, &request_json)?;

    tracing::debug!(
        "Merged body: {}",
        serde_json::to_string_pretty(&merged_body).unwrap_or_default()
    );

    let merged_bytes = serde_json::to_vec(&merged_body)
        .map_err(|_| ApiError::Internal(InternalError::Internal))?;

    let req =
        Request::from_parts(parts, axum_core::body::Body::from(merged_bytes));
    Ok(req)
}

async fn get_prompt_version(
    app_state: &AppState,
    prompt_id: &str,
    auth_ctx: &AuthContext,
) -> Result<JawnResponse<Prompt2025Version>, ApiError> {
    let endpoint_url = app_state
        .config()
        .helicone
        .base_url
        .join("/v1/prompt-2025/query/production-version")
        .map_err(|_| InternalError::Internal)?;

    let response = app_state
        .0
        .jawn_http_client
        .request_client
        .post(endpoint_url)
        .json(&serde_json::json!({ "promptId": prompt_id }))
        .header(
            "authorization",
            format!("Bearer {}", auth_ctx.api_key.expose()),
        )
        .send()
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "failed to get prompt version");
            ApiError::Internal(InternalError::PromptError(
                PromptError::FailedToGetProductionVersion(e),
            ))
        })?
        .error_for_status()
        .map_err(|e| {
            ApiError::Internal(InternalError::PromptError(
                PromptError::FailedToGetProductionVersion(e),
            ))
        })?;

    response
        .json::<JawnResponse<Prompt2025Version>>()
        .await
        .map_err(|e| {
            ApiError::Internal(InternalError::PromptError(
                PromptError::FailedToGetProductionVersion(e),
            ))
        })
}

// TODO: Better serialization handling for messages types
// TODO: Message templating with inputs/variables.
fn merge_prompt_with_request(
    mut prompt_body: serde_json::Value,
    request_body: &serde_json::Value,
) -> Result<serde_json::Value, ApiError> {
    let Some(prompt_obj) = prompt_body.as_object_mut() else {
        return Err(ApiError::Internal(InternalError::Internal));
    };

    let Some(request_obj) = request_body.as_object() else {
        return Err(ApiError::Internal(InternalError::Internal));
    };

    let Some(prompt_messages) =
        prompt_obj.get("messages").and_then(|m| m.as_array())
    else {
        return Err(ApiError::Internal(InternalError::Internal));
    };

    let Some(request_messages) =
        request_obj.get("messages").and_then(|m| m.as_array())
    else {
        return Err(ApiError::Internal(InternalError::Internal));
    };

    let mut merged_messages = prompt_messages.clone();
    merged_messages.extend(request_messages.iter().cloned());

    prompt_obj.insert(
        "messages".to_string(),
        serde_json::Value::Array(merged_messages),
    );

    for (key, value) in request_obj {
        if key != "messages" {
            prompt_obj.insert(key.clone(), value.clone());
        }
    }

    Ok(prompt_body)
}
