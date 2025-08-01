use std::{
    collections::HashSet,
    string::ToString,
    task::{Context, Poll},
};

use futures::future::BoxFuture;
use http_body_util::BodyExt;
use regex::Regex;
use serde_json::Value;
use tracing::{Instrument, info_span};

use crate::{
    app_state::AppState,
    error::{
        api::ApiError, internal::InternalError,
        invalid_req::InvalidRequestError, prompts::PromptError,
    },
    store::minio::MinioClient,
    types::{
        extensions::{AuthContext, PromptContext},
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

    if request_json.pointer("/prompt_id").is_none() {
        let req =
            Request::from_parts(parts, axum_core::body::Body::from(body_bytes));
        return Ok(req);
    }

    let Ok(mut prompt_ctx) = get_prompt_params(&request_json) else {
        let req =
            Request::from_parts(parts, axum_core::body::Body::from(body_bytes));
        return Ok(req);
    };
    // TODO: Insert to extensions later and process in RequestLog

    let auth_ctx = parts
        .extensions
        .get::<AuthContext>()
        .cloned()
        .ok_or(InternalError::ExtensionNotFound("AuthContext"))?;

    let version_id = if let Some(ref version_id) = prompt_ctx.prompt_version_id
    {
        version_id.clone()
    } else {
        let version_response = get_prompt_version(
            &app_state,
            &prompt_ctx.prompt_id,
            &auth_ctx,
        )
        .await?
        .data()
        .map_err(|e| {
            tracing::error!(error = %e, "failed to get production version");
            ApiError::Internal(InternalError::PromptError(
                PromptError::UnexpectedResponse(e),
            ))
        })?;
        prompt_ctx.prompt_version_id = Some(version_response.id.clone());
        version_response.id
    };

    let s3_client = if app_state.config().deployment_target.is_cloud() {
        MinioClient::cloud(&app_state.0.minio)
    } else {
        MinioClient::sidecar(&app_state.0.jawn_http_client)
    };

    let prompt_body_json = s3_client
        .pull_prompt_body(
            &app_state,
            &auth_ctx,
            &prompt_ctx.prompt_id,
            &version_id,
        )
        .await
        .map_err(|e| ApiError::Internal(InternalError::PromptError(e)))?;

    let merged_body =
        merge_prompt_with_request(prompt_body_json, &request_json)?;

    let processed_body = process_prompt_variables(merged_body, &prompt_ctx)?;

    let merged_bytes = serde_json::to_vec(&processed_body)
        .map_err(|_| ApiError::Internal(InternalError::Internal))?;

    let mut parts = parts;
    parts.extensions.insert(prompt_ctx);

    let req =
        Request::from_parts(parts, axum_core::body::Body::from(merged_bytes));
    Ok(req)
}

fn get_prompt_params(
    request_json: &Value,
) -> Result<PromptContext, InvalidRequestError> {
    let prompt_ctx = serde_json::from_value(request_json.clone())?;
    Ok(prompt_ctx)
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

fn process_prompt_variables(
    mut body: serde_json::Value,
    prompt_ctx: &PromptContext,
) -> Result<serde_json::Value, ApiError> {
    let Some(inputs) = &prompt_ctx.inputs else {
        return Ok(body);
    };

    let Some(body_obj) = body.as_object_mut() else {
        return Ok(body);
    };

    let variable_regex = Regex::new(r"\{\{\s*hc\s*:\s*([a-zA-Z_-][a-zA-Z0-9_-]*)\s*:\s*([a-zA-Z_-][a-zA-Z0-9_-]*)\s*\}\}")
        .map_err(|_| ApiError::Internal(InternalError::Internal))?;

    if let Some(messages_value) = body_obj.get_mut("messages")
        && let Some(messages_array) = messages_value.as_array_mut()
    {
        let mut validated_variables = HashSet::new();

        for message_value in messages_array {
            process_message_variables(
                message_value,
                inputs,
                &variable_regex,
                &mut validated_variables,
            )?;
        }
    }

    if let Some(response_format_value) = body_obj.get_mut("response_format") {
        let processed_response_format = process_prompt_schema(
            response_format_value.clone(),
            inputs,
            &variable_regex,
        )?;
        body_obj
            .insert("response_format".to_string(), processed_response_format);
    }

    if let Some(tools_value) = body_obj.get_mut("tools") {
        let processed_tools = process_prompt_schema(
            tools_value.clone(),
            inputs,
            &variable_regex,
        )?;
        body_obj.insert("tools".to_string(), processed_tools);
    }

    Ok(body)
}

fn process_prompt_schema(
    value: serde_json::Value,
    inputs: &std::collections::HashMap<String, serde_json::Value>,
    variable_regex: &Regex,
) -> Result<serde_json::Value, ApiError> {
    // Any KV in a tool or response schema can have a variable, with two cases:
    // "{{hc:name:type}}" or "{{hc:name:type}} world." If the former, then
    // we wholly replace it with the input, (which may an object, such as an
    // array). This allows objects as prompt inputs in schemas. The latter
    // is a partial match, and we perform regex replacement as we do normally.

    // Other than some specific cases for prompt input type validation, we allow
    // the provider to complain on invalid schemas.
    match value {
        serde_json::Value::String(s) => {
            if is_whole_variable_match(&s, variable_regex) {
                let variable_name =
                    get_variable_name_from_string(&s, variable_regex)?;
                if let Some(input_value) = inputs.get(&variable_name) {
                    return Ok(input_value.clone());
                }
            }
            let processed_text = replace_variables(
                &s,
                inputs,
                variable_regex,
                &mut HashSet::new(),
            )?;
            Ok(serde_json::Value::String(processed_text))
        }
        serde_json::Value::Array(arr) => {
            let mut processed_array = Vec::new();
            for item in arr {
                let processed_item =
                    process_prompt_schema(item, inputs, variable_regex)?;
                processed_array.push(processed_item);
            }
            Ok(serde_json::Value::Array(processed_array))
        }
        serde_json::Value::Object(obj) => {
            let mut processed_object = serde_json::Map::new();
            for (key, val) in obj {
                let processed_key =
                    if is_whole_variable_match(&key, variable_regex) {
                        let variable_name = get_variable_name_from_string(
                            &key,
                            variable_regex,
                        )?;
                        if let Some(input_value) = inputs.get(&variable_name) {
                            if let Some(string_value) = input_value.as_str() {
                                string_value.to_string()
                            } else {
                                return Err(ApiError::InvalidRequest(
                                    InvalidRequestError::InvalidPromptInputs(
                                        format!(
                                            "Variable '{variable_name}' in \
                                             object schema key must be a \
                                             string, got: {input_value}"
                                        ),
                                    ),
                                ));
                            }
                        } else {
                            key
                        }
                    } else {
                        replace_variables(
                            &key,
                            inputs,
                            variable_regex,
                            &mut HashSet::new(),
                        )?
                    };

                let processed_value =
                    process_prompt_schema(val, inputs, variable_regex)?;
                processed_object.insert(processed_key, processed_value);
            }
            Ok(serde_json::Value::Object(processed_object))
        }
        _ => Ok(value),
    }
}

fn is_whole_variable_match(text: &str, variable_regex: &Regex) -> bool {
    if let Some(captures) = variable_regex.captures(text)
        && let Some(full_match) = captures.get(0)
    {
        return full_match.as_str() == text;
    }
    false
}

fn get_variable_name_from_string(
    text: &str,
    variable_regex: &Regex,
) -> Result<String, ApiError> {
    if let Some(captures) = variable_regex.captures(text)
        && let Some(name_match) = captures.get(1)
    {
        return Ok(name_match.as_str().to_string());
    }
    Err(ApiError::InvalidRequest(
        InvalidRequestError::InvalidPromptInputs(format!(
            "Failed to extract variable name from: {text}"
        )),
    ))
}

fn process_message_variables(
    message_value: &mut serde_json::Value,
    inputs: &std::collections::HashMap<String, serde_json::Value>,
    variable_regex: &Regex,
    validated_variables: &mut HashSet<String>,
) -> Result<(), ApiError> {
    // We can do this without matching to role type (e.g specific types for
    // User/Assistant...) since they all follow the same structure.
    // Unsure whether or not we should match to all the types for very redundant
    // but technically better code.
    if let Some(content_value) = message_value.get_mut("content") {
        match content_value {
            serde_json::Value::String(text) => {
                let processed_text = replace_variables(
                    text,
                    inputs,
                    variable_regex,
                    validated_variables,
                )?;
                *content_value = serde_json::Value::String(processed_text);
            }
            serde_json::Value::Array(parts) => {
                for part in parts {
                    if let Some(text_value) = part.get_mut("text") {
                        if let Some(text_str) = text_value.as_str() {
                            let processed_text = replace_variables(
                                text_str,
                                inputs,
                                variable_regex,
                                validated_variables,
                            )?;
                            *text_value =
                                serde_json::Value::String(processed_text);
                        }
                    }
                }
            }
            _ => {}
        }
    }

    Ok(())
}

fn replace_variables(
    text: &str,
    inputs: &std::collections::HashMap<String, serde_json::Value>,
    variable_regex: &Regex,
    validated_variables: &mut std::collections::HashSet<String>,
) -> Result<String, ApiError> {
    for caps in variable_regex.captures_iter(text) {
        let variable_name =
            caps.get(1).ok_or(InvalidRequestError::InvalidPromptInputs(
                "Invalid variable name".to_string(),
            ))?;
        let variable_type =
            caps.get(2).ok_or(InvalidRequestError::InvalidPromptInputs(
                "Invalid variable type".to_string(),
            ))?;

        if validated_variables.contains(variable_name.as_str()) {
            continue;
        }

        if let Some(value) = inputs.get(variable_name.as_str()) {
            validate_variable_type(value, variable_type.as_str())?;
            validated_variables.insert(variable_name.as_str().to_string());
        }
    }

    let result = variable_regex.replace_all(text, |caps: &regex::Captures| {
        let variable_name = &caps[1];
        inputs.get(variable_name).map_or_else(
            || caps.get(0).unwrap().as_str().to_string(),
            std::string::ToString::to_string,
        )
    });

    Ok(result.to_string())
}

fn validate_variable_type(
    value: &serde_json::Value,
    expected_type: &str,
) -> Result<String, ApiError> {
    let value_string = value.to_string();

    match expected_type {
        "number" => {
            if matches!(value, serde_json::Value::Number(_)) {
                return Ok(value_string);
            }

            value_string
                .parse::<f64>()
                .map(|_| value_string.clone())
                .map_err(|_| {
                    ApiError::InvalidRequest(
                        InvalidRequestError::InvalidPromptInputs(format!(
                            "Variable value '{value_string}' cannot be \
                             converted to number"
                        )),
                    )
                })
        }
        "boolean" => {
            if matches!(value, serde_json::Value::Bool(_)) {
                return Ok(value_string);
            }

            let lowercase_value = value_string.to_lowercase();
            match lowercase_value.as_str() {
                "true" | "false" | "yes" | "no" => Ok(value_string),
                _ => Err(ApiError::InvalidRequest(
                    InvalidRequestError::InvalidPromptInputs(format!(
                        "Variable value '{value_string}' is not a valid \
                         boolean (expected: true, false, yes, no)"
                    )),
                )),
            }
        }
        _ => Ok(value_string),
    }
}
