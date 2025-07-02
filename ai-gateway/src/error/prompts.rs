use displaydoc::Display;
use thiserror::Error;

/// Prompt-related errors
#[derive(Debug, Error, Display, strum::AsRefStr)]
pub enum PromptError {
    /// Invalid url: {0}
    InvalidUrl(#[from] url::ParseError),
    /// Failed to send request: {0}
    FailedToSendRequest(reqwest::Error),
    /// Failed to get prompt body from S3: {0}
    FailedToGetPromptBody(reqwest::Error),
    /// Failed to get production version from helicone: {0}
    FailedToGetProductionVersion(reqwest::Error),
    /// Unexpected response: {0}
    UnexpectedResponse(String),
}