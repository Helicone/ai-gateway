use axum_core::response::{IntoResponse, Response};
use displaydoc::Display;
use http::StatusCode;
use thiserror::Error;

use super::api::ErrorResponse;
use crate::{
    error::api::ErrorDetails,
    middleware::mapper::openai::INVALID_REQUEST_ERROR_TYPE, types::json::Json,
};

#[derive(Debug, strum::AsRefStr, Error, Display)]
pub enum AuthError {
    /// Missing authorization header
    MissingAuthorizationHeader,
    /// Invalid credentials
    InvalidCredentials,
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        match self {
            Self::MissingAuthorizationHeader => (
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse {
                    error: ErrorDetails {
                        message: Self::MissingAuthorizationHeader.to_string(),
                        r#type: Some(INVALID_REQUEST_ERROR_TYPE.to_string()),
                        param: None,
                        code: Some("invalid_api_key".to_string()),
                    },
                }),
            )
                .into_response(),
            Self::InvalidCredentials => (
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse {
                    error: ErrorDetails {
                        message: Self::InvalidCredentials.to_string(),
                        r#type: Some(INVALID_REQUEST_ERROR_TYPE.to_string()),
                        param: None,
                        code: Some("invalid_api_key".to_string()),
                    },
                }),
            )
                .into_response(),
        }
    }
}

/// Auth errors for metrics. This is a special type
/// that avoids including dynamic information to limit cardinality
/// such that we can use this type in metrics.
#[derive(Debug, Error, Display, strum::AsRefStr)]
pub enum AuthErrorMetric {
    /// Missing authorization header
    MissingAuthorizationHeader,
    /// Invalid credentials
    InvalidCredentials,
}

impl From<&AuthError> for AuthErrorMetric {
    fn from(error: &AuthError) -> Self {
        match error {
            AuthError::MissingAuthorizationHeader => {
                Self::MissingAuthorizationHeader
            }
            AuthError::InvalidCredentials => Self::InvalidCredentials,
        }
    }
}
