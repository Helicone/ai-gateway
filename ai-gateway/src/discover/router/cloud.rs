use std::{
    collections::HashMap,
    pin::Pin,
    sync::Arc,
    task::{Context, Poll},
};

use compact_str::CompactString;
use futures::Stream;
use pin_project_lite::pin_project;
use tokio::sync::mpsc::Receiver;
use tokio_stream::wrappers::ReceiverStream;
use tower::discover::Change;

use crate::{
    app_state::AppState, config::router::RouterConfig,
    discover::provider::config::ServiceMap, error::init::InitError,
    router::service::Router, types::router::RouterId,
};

pin_project! {
  /// Reads available models and providers from the config file.
  ///
  /// We can additionally dynamically remove providers from the balancer
  /// if they hit certain failure thresholds by using a layer like:
  ///
  /// ```rust,ignore
  /// #[derive(Clone)]
  /// pub struct FailureWatcherLayer {
  ///     key: usize,
  ///     registry: tokio::sync::watch::Sender<HashMap<usize, DispatcherService>>,
  ///     failure_limit: u32,
  ///     window: Duration,
  /// }
  /// ```
  ///
  /// the layer would then send `Change::Remove` events to this discovery struct
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
        rx: Option<Receiver<Change<RouterId, Router>>>,
    ) -> Result<Self, InitError> {
        if let Some(rx) = rx {
            let mut service_map: HashMap<RouterId, Router> = HashMap::new();
            let database = &app_state.0.database;
            let routers = database
                .as_ref()
                .unwrap()
                .get_all_routers()
                .await
                .map_err(|e| {
                    tracing::error!(error = %e, "failed to get all routers");
                    InitError::DefaultRouterNotFound
                })?;
            for router in routers {
                let router_id = RouterId::Named(CompactString::from(
                    router.router_id.to_string(),
                ));
                let router_config = serde_json::from_value::<RouterConfig>(
                router.config.clone(),
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
            }

            tracing::debug!("Created config router discovery");
            Ok(Self {
                initial: ServiceMap::new(service_map),
                events: ReceiverStream::new(rx),
            })
        } else {
            //  BETTER ERROR LATER
            Err(InitError::RouterRxNotConfigured)
        }
    }
}

impl Stream for CloudDiscovery {
    type Item = Change<RouterId, Router>;

    fn poll_next(
        self: Pin<&mut Self>,
        ctx: &mut Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        let mut this = self.project();

        // 1) one‑time inserts, once the ServiceMap returns `Poll::Ready(None)`,
        //    then the service map is empty
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
