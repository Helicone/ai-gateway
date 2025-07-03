use async_openai::types::{
    CreateChatCompletionResponse, CreateChatCompletionStreamResponse,
};

use crate::endpoints::openai::OpenAICompatibleChatCompletionRequest;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct GenerateContents;

impl crate::endpoints::Endpoint for GenerateContents {
    // https://ai.google.dev/gemini-api/docs/openai
    const PATH: &'static str = "/v1beta/openai/chat/completions";
    type RequestBody = OpenAICompatibleChatCompletionRequest;
    type ResponseBody = CreateChatCompletionResponse;
    type StreamResponseBody = CreateChatCompletionStreamResponse;
    type ErrorResponseBody = async_openai::error::WrappedError;
}
