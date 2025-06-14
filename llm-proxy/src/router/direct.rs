use std::sync::Arc;

use rustc_hash::FxHashMap as HashMap;
use tower::ServiceBuilder;

use crate::{
    app_state::AppState,
    dispatcher::{Dispatcher, DispatcherService},
    error::init::InitError,
    middleware::{rate_limit, request_context},
    types::provider::InferenceProvider,
};

pub type DirectProxyService =
    rate_limit::Service<request_context::Service<DispatcherService>>;

#[derive(Debug, Clone)]
pub struct DirectProxies(Arc<HashMap<InferenceProvider, DirectProxyService>>);

impl DirectProxies {
    pub fn new(app_state: &AppState) -> Result<Self, InitError> {
        let mut direct_proxies = HashMap::default();
        let provider_keys = app_state.0.direct_proxy_api_keys.clone();
        for (provider, _provider_config) in app_state
            .config()
            .providers
            .iter()
            .filter(|(_, config)| config.enabled)
        {
            let direct_proxy_dispatcher =
                Dispatcher::new_direct_proxy(app_state.clone(), *provider)?;

            let direct_proxy = ServiceBuilder::new()
                // global rate limiting is still applied earlier in the stack
                .layer(rate_limit::Layer::disabled())
                .layer(request_context::Layer::for_direct_proxy(
                    provider_keys.clone(),
                ))
                // other middleware: caching, etc, etc
                // will be added here as well from the router config
                // .map_err(|e| crate::error::api::Error::Box(e))
                .service(direct_proxy_dispatcher);

            direct_proxies.insert(*provider, direct_proxy);
        }
        Ok(Self(Arc::new(direct_proxies)))
    }
}

impl std::ops::Deref for DirectProxies {
    type Target = Arc<HashMap<InferenceProvider, DirectProxyService>>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
