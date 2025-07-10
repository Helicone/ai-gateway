use axum_core::response::IntoResponse;
use displaydoc::Display;
use http::{HeaderMap, StatusCode};
use thiserror::Error;
use tracing::debug;

use crate::{
    error::api::{ErrorDetails, ErrorResponse},
    middleware::mapper::openai::INVALID_REQUEST_ERROR_TYPE,
    types::{json::Json, provider::InferenceProvider},
};

#[derive(Debug, Display)]
#[displaydoc("Retry after {retry_after}s.")]
pub struct TooManyRequestsError {
    /// Request limit
    pub ratelimit_limit: u64,
    /// Number of requests left for the time window
    pub ratelimit_remaining: u64,
    /// Number of seconds in which the API will become available again after
    /// its rate limit has been exceeded
    pub retry_after: u64,
}

/// User errors
#[derive(Debug, Error, Display, strum::AsRefStr)]
pub enum InvalidRequestError {
    /// Resource not found: {0}
    NotFound(String),
    /// Unsupported provider: {0}
    UnsupportedProvider(InferenceProvider),
    /// Unsupported endpoint: {0}
    UnsupportedEndpoint(String),
    /// Router id not found: {0}
    RouterIdNotFound(String),
    /// Missing router id in request path
    MissingRouterId,
    /// Invalid request: {0}
    InvalidRequest(http::Error),
    /// Invalid request url: {0}
    InvalidUrl(String),
    /// Invalid request body: {0}
    InvalidRequestBody(#[from] serde_json::Error),
    /// Upstream 4xx error: {0}
    Provider4xxError(StatusCode),
    /// Invalid cache config
    InvalidCacheConfig,
    /// Too many requests: {0}
    TooManyRequests(TooManyRequestsError),
    /// Invalid request header: {0}
    InvalidRequestHeader(http::header::ToStrError),
    /// Invalid prompt inputs: {0}
    InvalidPromptInputs(String),
}

impl IntoResponse for InvalidRequestError {
    fn into_response(self) -> axum_core::response::Response {
        debug!(error = %self, "Invalid request");
        let message = self.to_string();
        match self {
            Self::NotFound(_) | Self::RouterIdNotFound(_) => (
                StatusCode::NOT_FOUND,
                Json(ErrorResponse {
                    error: ErrorDetails {
                        message,
                        r#type: Some(INVALID_REQUEST_ERROR_TYPE.to_string()),
                        param: None,
                        code: None,
                    },
                }),
            )
                .into_response(),
            Self::Provider4xxError(status) => (
                status,
                Json(ErrorResponse {
                    error: ErrorDetails {
                        message,
                        r#type: Some(INVALID_REQUEST_ERROR_TYPE.to_string()),
                        param: None,
                        code: None,
                    },
                }),
            )
                .into_response(),
            Self::TooManyRequests(error) => {
                let mut headers = HeaderMap::new();
                headers.insert(
                    "retry-after",
                    error.retry_after.to_string().parse().unwrap(),
                );
                headers.insert(
                    "x-ratelimit-after",
                    error.retry_after.to_string().parse().unwrap(),
                );
                headers.insert(
                    "x-ratelimit-limit",
                    error.ratelimit_limit.to_string().parse().unwrap(),
                );
                headers.insert(
                    "x-ratelimit-remaining",
                    error.ratelimit_remaining.to_string().parse().unwrap(),
                );
                (
                    StatusCode::TOO_MANY_REQUESTS,
                    headers,
                    Json(ErrorResponse {
                        error: ErrorDetails {
                            message,
                            r#type: Some(
                                INVALID_REQUEST_ERROR_TYPE.to_string(),
                            ),
                            param: None,
                            code: None,
                        },
                    }),
                )
                    .into_response()
            }
            _ => (
                StatusCode::BAD_REQUEST,
                Json(ErrorResponse {
                    error: ErrorDetails {
                        message,
                        r#type: Some(INVALID_REQUEST_ERROR_TYPE.to_string()),
                        param: None,
                        code: None,
                    },
                }),
            )
                .into_response(),
        }
    }
}

/// User errors for metrics. This is a special type
/// that avoids including dynamic information to limit cardinality
/// such that we can use this type in metrics.
#[derive(Debug, Error, Display, strum::AsRefStr)]
pub enum InvalidRequestErrorMetric {
    /// Resource not found
    NotFound,
    /// Unsupported provider
    UnsupportedProvider,
    /// Invalid request
    InvalidRequest,
    /// Invalid request url
    InvalidUrl,
    /// Invalid request body
    InvalidRequestBody,
    /// Upstream 4xx error
    Provider4xxError,
    /// Too many requests
    TooManyRequests,
}

impl From<&InvalidRequestError> for InvalidRequestErrorMetric {
    fn from(error: &InvalidRequestError) -> Self {
        match error {
            InvalidRequestError::UnsupportedProvider(_) => {
                Self::UnsupportedProvider
            }
            InvalidRequestError::NotFound(_)
            | InvalidRequestError::RouterIdNotFound(_)
            | InvalidRequestError::MissingRouterId
            | InvalidRequestError::InvalidRequestHeader(_) => Self::NotFound,
            InvalidRequestError::InvalidRequest(_)
            | InvalidRequestError::UnsupportedEndpoint(_)
            | InvalidRequestError::InvalidCacheConfig
            | InvalidRequestError::InvalidPromptInputs(_) => {
                Self::InvalidRequest
            }
            InvalidRequestError::InvalidUrl(_) => Self::InvalidUrl,
            InvalidRequestError::InvalidRequestBody(_) => {
                Self::InvalidRequestBody
            }
            InvalidRequestError::Provider4xxError(_) => Self::Provider4xxError,
            InvalidRequestError::TooManyRequests(_) => Self::TooManyRequests,
        }
    }
}
