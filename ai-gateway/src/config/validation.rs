use indexmap::IndexSet;
use thiserror::Error;

use crate::{
    config::{Config, router::RouterConfig},
    types::{
        model_id::{ModelId, ModelName},
        provider::InferenceProvider,
        router::RouterId,
    },
};

#[derive(Debug, Error)]
pub enum ModelMappingValidationError {
    #[error(
        "Provider {provider} referenced in router {router} balance config but \
         not found in global providers config"
    )]
    ProviderNotConfigured {
        router: RouterId,
        provider: InferenceProvider,
    },

    #[error(
        "No valid model mapping found for router {router}: model \
         {source_model} cannot be mapped to provider {target_provider}"
    )]
    NoValidMapping {
        router: RouterId,
        source_model: String,
        target_provider: InferenceProvider,
    },

    #[error("Model {model} in mapping config does not exist in any provider")]
    ModelNotFound { model: String },

    #[error("Model {model} in mapping config cannot be parsed as a model id")]
    ModelIdParseError { model: String },
}

impl Config {
    /// Validate that model mappings are complete for all possible routing
    /// scenarios
    pub fn validate_model_mappings(
        &self,
    ) -> Result<(), ModelMappingValidationError> {
        // Validate each router
        for (router_id, router_config) in self.routers.as_ref() {
            // Get all providers this router might use
            let router_providers = router_config.load_balance.providers();

            // Validate each provider exists in global config
            for provider in &router_providers {
                if !self.providers.contains_key(provider) {
                    return Err(
                        ModelMappingValidationError::ProviderNotConfigured {
                            router: router_id.clone(),
                            provider: provider.clone(),
                        },
                    );
                }
            }

            let all_models_offered_by_configured_providers: IndexSet<
                ModelName,
            > = router_providers
                .iter()
                .flat_map(|provider| {
                    self.providers[provider]
                        .models
                        .iter()
                        .map(|m| m.as_model_name())
                })
                .collect();

            // For each provider this router might route to
            for target_provider in &router_providers {
                let target_provider_config = &self.providers[target_provider];

                let target_models = target_provider_config
                    .models
                    .iter()
                    .map(|m| m.clone().with_latest_version())
                    .collect::<IndexSet<ModelId>>();

                for source_model in &all_models_offered_by_configured_providers
                {
                    self.can_map_model(
                        source_model,
                        target_provider.clone(),
                        &target_models,
                        router_id,
                        router_config,
                    )?;
                }
            }
        }

        Ok(())
    }

    /// Check if a model can be mapped to a target provider
    fn can_map_model(
        &self,
        source_model: &ModelName,
        target_provider: InferenceProvider,
        target_models: &IndexSet<ModelId>,
        router_id: &RouterId,
        router_config: &RouterConfig,
    ) -> Result<(), ModelMappingValidationError> {
        // 1. Direct support - target provider offers this model directly
        let source_model_id = ModelId::from_str_and_provider(
            target_provider.clone(),
            source_model.as_ref(),
        )
        .map_err(|_| {
            ModelMappingValidationError::ModelIdParseError {
                model: source_model.to_string(),
            }
        })?;
        if target_models.contains(&source_model_id) {
            return Ok(());
        }

        // 2. Router-specific mapping
        if let Some(router_mappings) = &router_config.model_mappings {
            if let Some(alternatives) =
                router_mappings.as_ref().get(source_model)
            {
                if alternatives.iter().any(|m| target_models.contains(m)) {
                    return Ok(());
                }
            }
        }

        // 3. Default mapping
        if let Some(alternatives) =
            self.default_model_mapping.as_ref().get(source_model)
        {
            if alternatives.iter().any(|m| target_models.contains(m)) {
                return Ok(());
            }
        }

        Err(ModelMappingValidationError::NoValidMapping {
            router: router_id.clone(),
            source_model: source_model.as_ref().to_string(),
            target_provider,
        })
    }
}

#[cfg(test)]
mod tests {
    use compact_str::CompactString;

    use super::*;

    #[test]
    fn default_config_passes_validation() {
        let config = Config::default();
        let result = config.validate_model_mappings();

        assert!(result.is_ok());
    }

    #[test]
    fn test_can_map_model_error_no_valid_mapping() {
        let config = Config::default();

        let router_config = RouterConfig {
            model_mappings: None,
            ..Default::default()
        };

        let target_models = indexmap::IndexSet::from([
            ModelId::from_str_and_provider(InferenceProvider::OpenAI, "gpt-4")
                .unwrap(),
            ModelId::from_str_and_provider(
                InferenceProvider::OpenAI,
                "gpt-3.5-turbo",
            )
            .unwrap(),
        ]);

        let source_model = ModelName::owned("claude-3-opus".to_string());

        let result = config.can_map_model(
            &source_model,
            InferenceProvider::OpenAI,
            &target_models,
            &RouterId::Named(CompactString::new("my-router")),
            &router_config,
        );

        assert!(matches!(
            result,
            Err(ModelMappingValidationError::NoValidMapping { .. })
        ));
    }
}
