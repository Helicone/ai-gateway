use std::{
    collections::HashMap,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use compact_str::CompactString;
use futures::Stream;
use pin_project_lite::pin_project;
use rustc_hash::FxHashMap;
use tokio::sync::mpsc::Receiver;
use tokio_stream::wrappers::ReceiverStream;
use tower::discover::Change;

use crate::{
    app_state::AppState,
    config::router::RouterConfig,
    discover::ServiceMap,
    error::init::InitError,
    router::service::Router,
    types::{org::OrgId, router::RouterId},
};

pin_project! {
  /// Reads available routers from the database
  #[derive(Debug)]
  pub struct CloudDiscovery {
      #[pin]
      initial: ServiceMap<RouterId, Router>,
      #[pin]
      events: ReceiverStream<Change<RouterId, Router>>,
  }
}

impl CloudDiscovery {
    pub async fn new(
        app_state: &AppState,
        rx: Receiver<Change<RouterId, Router>>,
    ) -> Result<Self, InitError> {
        let mut service_map: HashMap<RouterId, Router> = HashMap::new();
        let router_store = app_state
            .0
            .router_store
            .as_ref()
            .ok_or(InitError::StoreNotConfigured("router_store"))?;
        let routers = router_store.get_all_routers().await?;
        let mut router_organisation_map = FxHashMap::default();
        for db_router in routers {
            let router_id = RouterId::Named(CompactString::from(
                db_router.router_hash.to_string(),
            ));
            let router_config = serde_json::from_value::<RouterConfig>(
                db_router.config.clone(),
            )
            .map_err(|e| {
                tracing::error!(error = %e, "failed to parse router config");
                InitError::DefaultRouterNotFound
            })?;

            let router = Router::new(
                router_id.clone(),
                Arc::new(router_config),
                app_state.clone(),
            )
            .await?;
            service_map.insert(router_id.clone(), router);
            router_organisation_map.insert(
                router_id.clone(),
                OrgId::new(db_router.organization_id),
            );
        }
        app_state
            .set_router_organization_map(router_organisation_map)
            .await;

        let provider_keys = router_store.get_all_provider_keys_by_ids().await?;
        app_state
            .0
            .provider_keys
            .set_all_provider_keys(provider_keys)
            .await;

        tracing::debug!("Created cloud router discoverer");
        Ok(Self {
            initial: ServiceMap::new(service_map),
            events: ReceiverStream::new(rx),
        })
    }
}

impl Stream for CloudDiscovery {
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
        match this.events.as_mut().poll_next(ctx) {
            Poll::Ready(Some(change)) => handle_change(change),
            Poll::Pending => Poll::Pending,
            Poll::Ready(None) => Poll::Ready(None),
        }
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
