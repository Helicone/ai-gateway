use axum_core::response::{IntoResponse, Response};
use http::StatusCode;
use tracing::error;

use super::api::ErrorResponse;
use crate::types::json::Json;

#[derive(Debug, strum::AsRefStr, thiserror::Error)]
pub enum AuthError {
    #[error(transparent)]
    Database(#[from] sqlx::Error),

    #[error(transparent)]
    Reqwest(#[from] reqwest::Error),

    #[error(transparent)]
    TaskJoin(#[from] tokio::task::JoinError),

    #[error("Invalid credentials")]
    InvalidCredentials,

    #[error("Internal server error")]
    InternalServerError,
}

impl IntoResponse for AuthError {
    fn into_response(self) -> Response {
        if let Self::InvalidCredentials = self {
            (
                StatusCode::UNAUTHORIZED,
                Json(ErrorResponse {
                    error: "Invalid credentials".to_string(),
                }),
            )
                .into_response()
        } else {
            error!(error = %self, "authentication error");
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(ErrorResponse {
                    error: "Internal server error".to_string(),
                }),
            )
                .into_response()
        }
    }
}
