use axum_core::response::{IntoResponse, Response};
use displaydoc::Display;
use http::StatusCode;
use thiserror::Error;

use super::api::ErrorResponse;
use crate::{
    error::api::ErrorDetails,
    middleware::mapper::openai::{
        INVALID_REQUEST_ERROR_TYPE, SERVER_ERROR_TYPE,
    },
    types::json::Json,
};

#[derive(Debug, strum::AsRefStr, Error, Display)]
pub enum StreamError {
    /// Stream error: {0}
    StreamError(#[from] Box<reqwest_eventsource::Error>),
    /// Body error: {0}
    BodyError(axum_core::Error),
}

impl StreamError {
    #[must_use]
    pub fn is_retryable(&self) -> bool {
        match self {
            StreamError::StreamError(error) => match &**error {
                reqwest_eventsource::Error::Utf8(_)
                | reqwest_eventsource::Error::Parser(_)
                | reqwest_eventsource::Error::Transport(_) => true,
                reqwest_eventsource::Error::InvalidStatusCode(
                    status_code,
                    _response,
                ) => status_code.is_server_error(),

                reqwest_eventsource::Error::InvalidLastEventId(_)
                | reqwest_eventsource::Error::InvalidContentType(_, _)
                | reqwest_eventsource::Error::StreamEnded => false,
            },
            StreamError::BodyError(_error) => false,
        }
    }
}

impl IntoResponse for StreamError {
    fn into_response(self) -> Response {
        match self {
            Self::StreamError(error) => {
                if let reqwest_eventsource::Error::InvalidStatusCode(
                    status_code,
                    _response,
                ) = &*error
                {
                    if status_code.is_server_error() {
                        tracing::error!(error = %error, "upstream server error in stream");
                        (
                            *status_code,
                            Json(ErrorResponse {
                                error: ErrorDetails {
                                    message: error.to_string(),
                                    r#type: Some(SERVER_ERROR_TYPE.to_string()),
                                    param: None,
                                    code: None,
                                },
                            }),
                        )
                            .into_response()
                    } else if status_code.is_client_error() {
                        tracing::debug!(error = %error, "invalid request error in stream");
                        (
                            *status_code,
                            Json(ErrorResponse {
                                error: ErrorDetails {
                                    message: error.to_string(),
                                    r#type: Some(
                                        INVALID_REQUEST_ERROR_TYPE.to_string(),
                                    ),
                                    param: None,
                                    code: None,
                                },
                            }),
                        )
                            .into_response()
                    } else {
                        (
                            StatusCode::INTERNAL_SERVER_ERROR,
                            Json(ErrorResponse {
                                error: ErrorDetails {
                                    message: error.to_string(),
                                    r#type: Some(SERVER_ERROR_TYPE.to_string()),
                                    param: None,
                                    code: None,
                                },
                            }),
                        )
                            .into_response()
                    }
                } else {
                    tracing::error!(error = %error, "internal error in stream");
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(ErrorResponse {
                            error: ErrorDetails {
                                message: error.to_string(),
                                r#type: Some(SERVER_ERROR_TYPE.to_string()),
                                param: None,
                                code: None,
                            },
                        }),
                    )
                        .into_response()
                }
            }
            Self::BodyError(error) => (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: ErrorDetails {
                        message: error.to_string(),
                        r#type: Some(SERVER_ERROR_TYPE.to_string()),
                        param: None,
                        code: None,
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
pub enum StreamErrorMetric {
    /// Event stream error
    StreamError,
    /// Body error
    BodyError,
}

impl From<&StreamError> for StreamErrorMetric {
    fn from(error: &StreamError) -> Self {
        match error {
            StreamError::StreamError(_) => Self::StreamError,
            StreamError::BodyError(_) => Self::BodyError,
        }
    }
}
