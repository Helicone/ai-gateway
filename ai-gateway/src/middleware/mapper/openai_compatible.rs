use std::str::FromStr;

use http::response::Parts;

use super::{TryConvertStreamData, model::ModelMapper};
use crate::{
    endpoints::openai::OpenAICompatibleChatCompletionRequest,
    error::mapper::MapperError,
    middleware::mapper::{TryConvert, TryConvertError},
    types::{model_id::ModelId, provider::InferenceProvider},
};

pub struct OpenAICompatibleConverter {
    provider: InferenceProvider,
    model_mapper: ModelMapper,
}

impl OpenAICompatibleConverter {
    #[must_use]
    pub fn new(provider: InferenceProvider, model_mapper: ModelMapper) -> Self {
        Self {
            provider,
            model_mapper,
        }
    }
}

impl
    TryConvert<
        async_openai::types::CreateChatCompletionRequest,
        OpenAICompatibleChatCompletionRequest,
    > for OpenAICompatibleConverter
{
    type Error = MapperError;
    fn try_convert(
        &self,
        mut value: async_openai::types::CreateChatCompletionRequest,
    ) -> Result<OpenAICompatibleChatCompletionRequest, Self::Error> {
        let source_model = ModelId::from_str(&value.model)?;
        let target_model =
            self.model_mapper.map_model(&source_model, &self.provider)?;
        tracing::trace!(source_model = ?source_model, target_model = ?target_model, "mapped model");
        value.model = target_model.to_string();

        Ok(OpenAICompatibleChatCompletionRequest {
            provider: self.provider,
            inner: value,
        })
    }
}

impl
    TryConvert<
        async_openai::types::CreateChatCompletionResponse,
        async_openai::types::CreateChatCompletionResponse,
    > for OpenAICompatibleConverter
{
    type Error = MapperError;
    fn try_convert(
        &self,
        value: async_openai::types::CreateChatCompletionResponse,
    ) -> Result<async_openai::types::CreateChatCompletionResponse, Self::Error>
    {
        Ok(value)
    }
}

impl
    TryConvertStreamData<
        async_openai::types::CreateChatCompletionStreamResponse,
        async_openai::types::CreateChatCompletionStreamResponse,
    > for OpenAICompatibleConverter
{
    type Error = MapperError;

    fn try_convert_chunk(
        &self,
        value: async_openai::types::CreateChatCompletionStreamResponse,
    ) -> Result<
        Option<async_openai::types::CreateChatCompletionStreamResponse>,
        Self::Error,
    > {
        Ok(Some(value))
    }
}

impl
    TryConvertError<
        async_openai::error::WrappedError,
        async_openai::error::WrappedError,
    > for OpenAICompatibleConverter
{
    type Error = MapperError;

    fn try_convert_error(
        &self,
        _resp_parts: &Parts,
        value: async_openai::error::WrappedError,
    ) -> Result<async_openai::error::WrappedError, Self::Error> {
        Ok(value)
    }
}
