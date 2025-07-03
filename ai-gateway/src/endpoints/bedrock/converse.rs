use aws_sdk_bedrockruntime::{
    operation::converse::{ConverseInput, ConverseOutput},
    types::ConverseStreamOutput,
};
use serde::{Deserialize, Serialize};

use crate::{
    endpoints::{AiRequest, Endpoint},
    error::mapper::MapperError,
    types::{model_id::ModelId, provider::InferenceProvider},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Converse;

impl Endpoint for Converse {
    const PATH: &'static str = "/model/{model_id}/converse";
    type RequestBody = ConverseInput;
    type ResponseBody = ConverseOutput;
    type StreamResponseBody = ConverseStreamOutput;
    type ErrorResponseBody = ConverseError;
}

impl AiRequest for ConverseInput {
    fn is_stream(&self) -> bool {
        false
    }

    fn model(&self) -> Result<ModelId, MapperError> {
        let model =
            self.model_id.as_ref().ok_or(MapperError::InvalidRequest)?;
        ModelId::from_str_and_provider(InferenceProvider::Bedrock, model)
    }
}

// The AWS SDK does not document the error format so instead we use a unit
// struct and simply rely on the http status codes to map to the OpenAI error.
#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
pub struct ConverseError;
