use anthropic_ai_sdk::types::message::{
    self, CreateMessageParams, CreateMessageResponse,
};
use serde::{Deserialize, Serialize};

use crate::{
    endpoints::{AiRequest, Endpoint},
    error::mapper::MapperError,
    types::{model_id::ModelId, provider::InferenceProvider},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Messages;

impl Endpoint for Messages {
    const PATH: &'static str = "/v1/messages";
    type RequestBody = CreateMessageParams;
    type ResponseBody = CreateMessageResponse;
    type StreamResponseBody = message::StreamEvent;
    type ErrorResponseBody = AnthropicApiError;
}

impl AiRequest for CreateMessageParams {
    fn is_stream(&self) -> bool {
        self.stream.unwrap_or(false)
    }

    fn model(&self) -> Result<ModelId, MapperError> {
        ModelId::from_str_and_provider(
            InferenceProvider::Anthropic,
            &self.model,
        )
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AnthropicApiError {
    pub error: ErrorDetails,
    #[serde(rename = "type")]
    pub kind: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ErrorDetails {
    pub message: String,
    #[serde(rename = "type")]
    pub kind: String,
}
