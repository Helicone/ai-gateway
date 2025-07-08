pub mod chat_completions;

use super::EndpointType;
pub use crate::endpoints::openai::chat_completions::ChatCompletions;
use crate::{
    endpoints::{Endpoint, EndpointRoute},
    error::invalid_req::InvalidRequestError,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::EnumIter)]
pub enum OpenAI {
    ChatCompletions(ChatCompletions),
}

impl OpenAI {
    #[must_use]
    pub fn path(&self) -> &str {
        match self {
            Self::ChatCompletions(_) => ChatCompletions::PATH,
        }
    }

    #[must_use]
    pub fn chat_completions() -> Self {
        Self::ChatCompletions(ChatCompletions)
    }

    #[must_use]
    pub fn endpoint_type(&self) -> EndpointType {
        match self {
            Self::ChatCompletions(_) => EndpointType::Chat,
        }
    }
}

impl TryFrom<&EndpointRoute> for OpenAI {
    type Error = InvalidRequestError;

    fn try_from(endpoint: &EndpointRoute) -> Result<Self, Self::Error> {
        match endpoint {
            EndpointRoute::ChatCompletions => {
                Ok(Self::ChatCompletions(ChatCompletions))
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct OpenAICompatibleChatCompletions;

impl Endpoint for OpenAICompatibleChatCompletions {
    const PATH: &'static str = "/v1/chat/completions";
    type RequestBody = OpenAICompatibleChatCompletionRequest;
    type ResponseBody = async_openai::types::CreateChatCompletionResponse;
    type StreamResponseBody =
        async_openai::types::CreateChatCompletionStreamResponse;
    type ErrorResponseBody = async_openai::error::WrappedError;
}

#[derive(
    Clone, serde::Serialize, Default, Debug, serde::Deserialize, PartialEq,
)]
pub struct OpenAICompatibleChatCompletionRequest {
    #[serde(skip)]
    pub(crate) provider: crate::types::provider::InferenceProvider,
    #[serde(flatten)]
    pub(crate) inner: async_openai::types::CreateChatCompletionRequest,
}

impl super::AiRequest for OpenAICompatibleChatCompletionRequest {
    fn is_stream(&self) -> bool {
        self.inner.stream.unwrap_or(false)
    }

    fn model(
        &self,
    ) -> Result<
        crate::types::model_id::ModelId,
        crate::error::mapper::MapperError,
    > {
        crate::types::model_id::ModelId::from_str_and_provider(
            self.provider.clone(),
            &self.inner.model,
        )
    }
}
