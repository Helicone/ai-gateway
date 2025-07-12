use std::task::{Context, Poll};

use futures::future::BoxFuture;
use tokio::sync::mpsc::Receiver;
use tower::{Service, discover::Change};

use crate::{
    app_state::AppState, discover::router::discover::RouterDiscovery,
    error::init::InitError, router::service::Router, types::router::RouterId,
};

#[derive(Debug)]
pub struct RouterDiscoverFactory {
    pub(crate) app_state: AppState,
}

impl RouterDiscoverFactory {
    #[must_use]
    pub fn new(app_state: AppState) -> Self {
        Self { app_state }
    }
}

impl Service<Option<Receiver<Change<RouterId, Router>>>>
    for RouterDiscoverFactory
{
    type Response = RouterDiscovery;
    type Error = InitError;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(
        &mut self,
        _: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(
        &mut self,
        rx: Option<Receiver<Change<RouterId, Router>>>,
    ) -> Self::Future {
        let app_state = self.app_state.clone();
        Box::pin(async move {
            let discovery = RouterDiscovery::new(&app_state, rx).await?;

            Ok(discovery)
        })
    }
}
