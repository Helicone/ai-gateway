use std::{
    collections::HashMap,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use futures::Stream;
use pin_project_lite::pin_project;
use tower::discover::Change;

use crate::{
    app_state::AppState, discover::ServiceMap, error::init::InitError,
    router::service::Router, types::router::RouterId,
};

pin_project! {
    /// Reads available routers from the config file
    #[derive(Debug)]
    pub struct ConfigDiscovery {
        #[pin]
        initial: ServiceMap<RouterId, Router>,
    }
}

impl ConfigDiscovery {
    pub async fn new(app_state: &AppState) -> Result<Self, InitError> {
        let mut service_map: HashMap<RouterId, Router> = HashMap::new();
        for (router_id, router_config) in app_state.0.config.routers.as_ref() {
            let key = router_id.clone();
            let router = Router::new(
                key.clone(),
                Arc::new(router_config.clone()),
                app_state.clone(),
            )
            .await?;
            service_map.insert(key, router);
        }

        tracing::debug!("created config router discoverer");
        Ok(Self {
            initial: ServiceMap::new(service_map),
        })
    }
}

impl Stream for ConfigDiscovery {
    type Item = Change<RouterId, Router>;

    fn poll_next(
        self: Pin<&mut Self>,
        ctx: &mut Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        let mut this = self.project();
        if let Poll::Ready(Some(change)) = this.initial.as_mut().poll_next(ctx)
        {
            return handle_change(change);
        }

        Poll::Ready(None)
    }
}

fn handle_change(
    change: Change<RouterId, Router>,
) -> Poll<Option<Change<RouterId, Router>>> {
    match change {
        Change::Insert(key, service) => {
            tracing::debug!(key = ?key, "Discovered new router");
            Poll::Ready(Some(Change::Insert(key, service)))
        }
        Change::Remove(key) => {
            tracing::debug!(key = ?key, "Removed router");
            Poll::Ready(Some(Change::Remove(key)))
        }
    }
}
