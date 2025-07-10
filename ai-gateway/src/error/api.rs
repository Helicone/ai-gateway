use axum_core::response::IntoResponse;
use displaydoc::Display;
use http::StatusCode;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use utoipa::ToSchema;

use super::{
    ErrorMetric,
    auth::{AuthError, AuthErrorMetric},
    internal::{InternalError, InternalErrorMetric},
    invalid_req::{InvalidRequestError, InvalidRequestErrorMetric},
};
use crate::{
    error::stream::{StreamError, StreamErrorMetric},
    middleware::mapper::openai::SERVER_ERROR_TYPE,
    types::json::Json,
};

/// Common API errors
#[derive(Debug, Error, Display, strum::AsRefStr)]
pub enum ApiError {
    /// Invalid request: {0}
    InvalidRequest(#[from] InvalidRequestError),
    /// Authentication error: {0}
    Authentication(#[from] AuthError),
    /// Internal error: {0}
    Internal(#[from] InternalError),
    /// Stream error: {0}
    StreamError(#[from] StreamError),
    /// Service panicked: {0}
    Panic(String),
}

impl From<dynamic_router::router::Error> for ApiError {
    fn from(error: dynamic_router::router::Error) -> Self {
        match error {
            dynamic_router::router::Error::NotFound(path) => {
                Self::InvalidRequest(InvalidRequestError::NotFound(path))
            }
            dynamic_router::router::Error::InnerService(error) => {
                Self::Internal(InternalError::DynamicRouterInnerServiceError(
                    error,
                ))
            }
            dynamic_router::router::Error::Discover(error) => Self::Internal(
                InternalError::DynamicRouterDiscoveryError(error),
            ),
        }
    }
}

#[derive(Debug, Deserialize, Serialize, ToSchema)]
pub struct ErrorResponse {
    pub error: ErrorDetails,
}

/// This type is intended to mirror the error type returned by the `OpenAI` API.
#[derive(Debug, Deserialize, Serialize, ToSchema)]
pub struct ErrorDetails {
    pub message: String,
    pub r#type: Option<String>,
    pub param: Option<String>,
    pub code: Option<String>,
}

impl IntoResponse for ApiError {
    fn into_response(self) -> axum_core::response::Response {
        match self {
            ApiError::InvalidRequest(error) => error.into_response(),
            ApiError::Authentication(error) => error.into_response(),
            ApiError::Internal(error) => error.into_response(),
            ApiError::StreamError(error) => error.into_response(),
            ApiError::Panic(error) => {
                tracing::error!(error = %error, "Internal server error");
                (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    Json(ErrorResponse {
                        error: ErrorDetails {
                            message: "Internal server error".to_string(),
                            r#type: Some(SERVER_ERROR_TYPE.to_string()),
                            param: None,
                            code: None,
                        },
                    }),
                )
                    .into_response()
            }
        }
    }
}

/// Top level metric type that reduces cardinality such that
/// we can use the error type in metrics.
#[derive(Debug, Error, Display, strum::AsRefStr)]
pub enum ApiErrorMetric {
    /// Invalid request
    InvalidRequest(#[from] InvalidRequestErrorMetric),
    /// Authentication
    Authentication(#[from] AuthErrorMetric),
    /// Internal
    Internal(#[from] InternalErrorMetric),
    /// Stream error
    StreamError(#[from] StreamErrorMetric),
    /// Panic
    Panic,
}

impl From<&ApiError> for ApiErrorMetric {
    fn from(error: &ApiError) -> Self {
        match error {
            ApiError::InvalidRequest(invalid_request_error) => {
                Self::InvalidRequest(InvalidRequestErrorMetric::from(
                    invalid_request_error,
                ))
            }
            ApiError::Authentication(auth_error) => {
                Self::Authentication(AuthErrorMetric::from(auth_error))
            }
            ApiError::Internal(internal_error) => {
                Self::Internal(InternalErrorMetric::from(internal_error))
            }
            ApiError::StreamError(error) => match error {
                StreamError::StreamError(boxed_error) if matches!(**boxed_error, reqwest_eventsource::Error::InvalidStatusCode(status_code, _) if status_code.is_client_error()) => {
                    Self::InvalidRequest(
                        InvalidRequestErrorMetric::InvalidRequest,
                    )
                }
                _ => Self::StreamError(StreamErrorMetric::from(error)),
            },
            ApiError::Panic(_error) => Self::Panic,
        }
    }
}

impl ErrorMetric for ApiErrorMetric {
    fn error_metric(&self) -> String {
        match self {
            Self::InvalidRequest(error) => {
                format!("InvalidRequest:{}", error.as_ref())
            }
            Self::Authentication(error) => {
                format!("Authentication:{}", error.as_ref())
            }
            Self::Internal(error) => {
                if let InternalErrorMetric::MapperError(e) = error {
                    format!("InternalError:MapperError:{}", e.as_ref())
                } else {
                    format!("InternalError:{}", error.as_ref())
                }
            }
            Self::StreamError(error) => {
                format!("StreamError:{}", error.as_ref())
            }
            Self::Panic => String::from("Panic"),
        }
    }
}

impl ErrorMetric for ApiError {
    fn error_metric(&self) -> String {
        ApiErrorMetric::from(self).error_metric()
    }
}
