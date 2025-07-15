use std::sync::Arc;

use derive_more::{AsRef, Deref, DerefMut};
use rustc_hash::{FxHashMap as HashMap, FxHashSet as HashSet};

use crate::{
    app_state::AppState,
    config::{model_mapping::ModelMappingConfig, router::RouterConfig},
    error::mapper::MapperError,
    types::{
        model_id::{ModelId, ModelIdWithoutVersion, ModelName},
        provider::InferenceProvider,
    },
};

#[derive(Debug, Clone, Eq, PartialEq, Deref, DerefMut, AsRef)]
struct ProviderModels(
    HashMap<InferenceProvider, HashSet<ModelIdWithoutVersion>>,
);

impl ProviderModels {
    fn new(app_state: &AppState) -> Self {
        let mut map = HashMap::default();
        for (provider, config) in app_state.config().providers.iter() {
            let models =
                config.models.iter().map(|m| m.clone().into()).collect();
            map.insert(provider.clone(), models);
        }
        Self(map)
    }
}

#[derive(Debug, Clone)]
pub struct ModelMapper {
    app_state: AppState,
    router_config: Option<Arc<RouterConfig>>,
    model_id: Option<ModelId>,
    provider_models: ProviderModels,
}

impl ModelMapper {
    #[must_use]
    pub fn new_for_router(
        app_state: AppState,
        router_config: Arc<RouterConfig>,
    ) -> Self {
        let provider_models = ProviderModels::new(&app_state);
        Self {
            app_state,
            router_config: Some(router_config),
            model_id: None,
            provider_models,
        }
    }

    #[must_use]
    pub fn new_with_model_id(
        app_state: AppState,
        router_config: Arc<RouterConfig>,
        model_id: ModelId,
    ) -> Self {
        let provider_models = ProviderModels::new(&app_state);
        Self {
            app_state,
            router_config: Some(router_config),
            model_id: Some(model_id),
            provider_models,
        }
    }

    #[must_use]
    pub fn new(app_state: AppState) -> Self {
        let provider_models = ProviderModels::new(&app_state);
        Self {
            app_state,
            router_config: None,
            model_id: None,
            provider_models,
        }
    }

    fn default_model_mapping(&self) -> &ModelMappingConfig {
        &self.app_state.0.config.default_model_mapping
    }

    /// Map a model to a new model name for a target provider.
    ///
    /// If the source model is offered by the target provider, return the source
    /// model name. Otherwise, use the model mapping from router config.
    /// If that doesn't have a mapping, use the default model mapping from the
    /// global config. (maybe we should put usage of the default mapping
    /// behind a flag so its up to the user,  although declaring mappings
    /// for _every_ model seems onerous)
    pub fn map_model(
        &self,
        source_model: &ModelId,
        target_provider: &InferenceProvider,
    ) -> Result<ModelId, MapperError> {
        // this model id comes from the router's configuration, e.g. weighted
        // model configuration
        if let Some(model_id) = self.model_id.clone() {
            return Ok(model_id);
        }
        let models_offered_by_target_provider =
            self.provider_models.0.get(target_provider).ok_or_else(|| {
                MapperError::NoProviderConfig(target_provider.clone())
            })?;

        let source_model_w_out_version =
            ModelIdWithoutVersion::from(source_model.clone());

        if models_offered_by_target_provider
            .contains(&source_model_w_out_version)
        {
            return Ok(source_model.clone());
        }

        let model_mapping_config = if let Some(router_model_mapping) =
            self.router_config.as_ref().and_then(|c| c.model_mappings())
        {
            router_model_mapping
        } else {
            self.default_model_mapping()
        };

        let source_model_name = ModelName::from_model(source_model);
        let possible_mappings = model_mapping_config
            .as_ref()
            .get(&source_model_name)
            .ok_or_else(|| {
                MapperError::NoModelMapping(
                    target_provider.clone(),
                    source_model_name.as_ref().to_string(),
                )
            })?;

        // get the first model from the router model mapping that the target
        // provider supports
        let target_model = possible_mappings
            .iter()
            .find(|m| {
                let possible_mapping = (*m).clone().into();
                models_offered_by_target_provider.contains(&possible_mapping)
                    && m.inference_provider() == Some(target_provider.clone())
            })
            .ok_or_else(|| {
                MapperError::NoModelMapping(
                    target_provider.clone(),
                    source_model_name.as_ref().to_string(),
                )
            })?
            .clone();

        Ok(target_model)
    }
}
