use std::sync::Arc;

use crate::{
    app_state::AppState, config::router::RouterConfig, types::router::RouterId,
};

#[derive(Debug)]
pub struct DispatcherDiscoverFactory {
    pub(crate) app_state: AppState,
    pub(crate) router_id: RouterId,
    pub(crate) router_config: Arc<RouterConfig>,
}

impl DispatcherDiscoverFactory {
    #[must_use]
    pub fn new(
        app_state: AppState,
        router_id: RouterId,
        router_config: Arc<RouterConfig>,
    ) -> Self {
        Self {
            app_state,
            router_id,
            router_config,
        }
    }
}
