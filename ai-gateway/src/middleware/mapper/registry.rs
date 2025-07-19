use std::sync::Arc;

use rustc_hash::FxHashMap as HashMap;

use super::{
    EndpointConverter, TypedEndpointConverter, anthropic::AnthropicConverter,
    model::ModelMapper, openai::OpenAIConverter,
    openai_compatible::OpenAICompatibleConverter,
};
use crate::{
    endpoints::{
        self, ApiEndpoint, anthropic::Anthropic, bedrock::Bedrock,
        google::Google, ollama::Ollama, openai::OpenAI,
    },
    middleware::mapper::{bedrock::BedrockConverter, ollama::OllamaConverter},
    types::provider::InferenceProvider,
};

#[derive(Debug, Default, Clone)]
pub struct EndpointConverterRegistry(Arc<EndpointConverterRegistryInner>);

impl EndpointConverterRegistry {
    #[must_use]
    pub fn new(model_mapper: &ModelMapper) -> Self {
        let inner = EndpointConverterRegistryInner::new(model_mapper);
        Self(Arc::new(inner))
    }

    #[must_use]
    pub fn get_converter(
        &self,
        source_endpoint: &ApiEndpoint,
        target_endpoint: &ApiEndpoint,
    ) -> Option<&(dyn EndpointConverter + Send + Sync + 'static)> {
        self.0
            .converters
            .get(&RegistryKey::new(
                source_endpoint.clone(),
                target_endpoint.clone(),
            ))
            .map(|v| &**v)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct RegistryKey {
    source_endpoint: ApiEndpoint,
    target_endpoint: ApiEndpoint,
}

impl RegistryKey {
    fn new(source_endpoint: ApiEndpoint, target_endpoint: ApiEndpoint) -> Self {
        Self {
            source_endpoint,
            target_endpoint,
        }
    }
}

#[derive(Default)]
struct EndpointConverterRegistryInner {
    /// In the future when we support other APIs beside just chat completion
    /// we'll want to add another level here.
    converters: HashMap<
        RegistryKey,
        Box<dyn EndpointConverter + Send + Sync + 'static>,
    >,
}

impl std::fmt::Debug for EndpointConverterRegistryInner {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut debug = f.debug_struct("EndpointConverterRegistryInner");
        debug.field("converters", &self.converters.keys().collect::<Vec<_>>());
        debug.finish()
    }
}

impl EndpointConverterRegistryInner {
    #[allow(clippy::too_many_lines)]
    fn new(model_mapper: &ModelMapper) -> Self {
        let mut registry = Self {
            converters: HashMap::default(),
        };

        let key = RegistryKey::new(
            ApiEndpoint::OpenAI(OpenAI::chat_completions()),
            ApiEndpoint::Anthropic(Anthropic::messages()),
        );
        let converter =
            TypedEndpointConverter::<
                endpoints::openai::ChatCompletions,
                endpoints::anthropic::Messages,
                AnthropicConverter,
            >::new(AnthropicConverter::new(model_mapper.clone()));
        registry.register_converter(key, converter);

        let key = RegistryKey::new(
            ApiEndpoint::OpenAI(OpenAI::chat_completions()),
            ApiEndpoint::Google(Google::generate_contents()),
        );
        let converter = TypedEndpointConverter::<
            endpoints::openai::ChatCompletions,
            endpoints::google::GenerateContents,
            OpenAICompatibleConverter,
        >::new(OpenAICompatibleConverter::new(
            InferenceProvider::GoogleGemini,
            model_mapper.clone(),
        ));
        registry.register_converter(key, converter);

        let key = RegistryKey::new(
            ApiEndpoint::OpenAI(OpenAI::chat_completions()),
            ApiEndpoint::OpenAI(OpenAI::chat_completions()),
        );
        let converter =
            TypedEndpointConverter::<
                endpoints::openai::ChatCompletions,
                endpoints::openai::ChatCompletions,
                OpenAIConverter,
            >::new(OpenAIConverter::new(model_mapper.clone()));
        registry.register_converter(key, converter);

        let key = RegistryKey::new(
            ApiEndpoint::OpenAI(OpenAI::chat_completions()),
            ApiEndpoint::Ollama(Ollama::chat_completions()),
        );
        let converter =
            TypedEndpointConverter::<
                endpoints::openai::ChatCompletions,
                endpoints::ollama::chat_completions::ChatCompletions,
                OllamaConverter,
            >::new(OllamaConverter::new(model_mapper.clone()));
        registry.register_converter(key, converter);

        let key = RegistryKey::new(
            ApiEndpoint::OpenAI(OpenAI::chat_completions()),
            ApiEndpoint::Bedrock(Bedrock::converse()),
        );

        let converter =
            TypedEndpointConverter::<
                endpoints::openai::ChatCompletions,
                endpoints::bedrock::Converse,
                BedrockConverter,
            >::new(BedrockConverter::new(model_mapper.clone()));

        registry.register_converter(key, converter);

        let key = RegistryKey::new(
            ApiEndpoint::OpenAI(OpenAI::chat_completions()),
            ApiEndpoint::OpenAICompatible {
                provider: InferenceProvider::Named("mistral".into()),
                openai_endpoint: OpenAI::chat_completions(),
            },
        );
        let converter = TypedEndpointConverter::<
            endpoints::openai::ChatCompletions,
            endpoints::openai::OpenAICompatibleChatCompletions,
            OpenAICompatibleConverter,
        >::new(OpenAICompatibleConverter::new(
            InferenceProvider::Named("mistral".into()),
            model_mapper.clone(),
        ));
        registry.register_converter(key, converter);

        let key = RegistryKey::new(
            ApiEndpoint::OpenAI(OpenAI::chat_completions()),
            ApiEndpoint::OpenAICompatible {
                provider: InferenceProvider::Named("groq".into()),
                openai_endpoint: OpenAI::chat_completions(),
            },
        );
        let converter = TypedEndpointConverter::<
            endpoints::openai::ChatCompletions,
            endpoints::openai::OpenAICompatibleChatCompletions,
            OpenAICompatibleConverter,
        >::new(OpenAICompatibleConverter::new(
            InferenceProvider::Named("groq".into()),
            model_mapper.clone(),
        ));
        registry.register_converter(key, converter);

        let key = RegistryKey::new(
            ApiEndpoint::OpenAI(OpenAI::chat_completions()),
            ApiEndpoint::OpenAICompatible {
                provider: InferenceProvider::Named("deepseek".into()),
                openai_endpoint: OpenAI::chat_completions(),
            },
        );
        let converter = TypedEndpointConverter::<
            endpoints::openai::ChatCompletions,
            endpoints::openai::OpenAICompatibleChatCompletions,
            OpenAICompatibleConverter,
        >::new(OpenAICompatibleConverter::new(
            InferenceProvider::Named("deepseek".into()),
            model_mapper.clone(),
        ));
        registry.register_converter(key, converter);

        let key = RegistryKey::new(
            ApiEndpoint::OpenAI(OpenAI::chat_completions()),
            ApiEndpoint::OpenAICompatible {
                provider: InferenceProvider::Named("xai".into()),
                openai_endpoint: OpenAI::chat_completions(),
            },
        );
        let converter = TypedEndpointConverter::<
            endpoints::openai::ChatCompletions,
            endpoints::openai::OpenAICompatibleChatCompletions,
            OpenAICompatibleConverter,
        >::new(OpenAICompatibleConverter::new(
            InferenceProvider::Named("xai".into()),
            model_mapper.clone(),
        ));
        registry.register_converter(key, converter);

        let key = RegistryKey::new(
            ApiEndpoint::OpenAI(OpenAI::chat_completions()),
            ApiEndpoint::OpenAICompatible {
                provider: InferenceProvider::Named("hyperbolic".into()),
                openai_endpoint: OpenAI::chat_completions(),
            },
        );
        let converter = TypedEndpointConverter::<
            endpoints::openai::ChatCompletions,
            endpoints::openai::OpenAICompatibleChatCompletions,
            OpenAICompatibleConverter,
        >::new(OpenAICompatibleConverter::new(
            InferenceProvider::Named("hyperbolic".into()),
            model_mapper.clone(),
        ));
        registry.register_converter(key, converter);

        registry
    }

    fn register_converter<C>(&mut self, key: RegistryKey, converter: C)
    where
        C: EndpointConverter + Send + Sync + 'static,
    {
        self.converters.insert(key, Box::new(converter));
    }
}
