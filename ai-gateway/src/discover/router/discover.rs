use std::{
    convert::Infallible,
    pin::Pin,
    task::{Context, Poll},
};

use futures::Stream;
use pin_project_lite::pin_project;
use tokio::sync::mpsc::Receiver;
use tower::discover::Change;

use crate::{
    app_state::AppState,
    config::DeploymentTarget,
    discover::router::{cloud::CloudDiscovery, config::ConfigDiscovery},
    error::init::InitError,
    router::service::Router,
    types::router::RouterId,
};

pin_project! {
    /// Discover routers.
    #[derive(Debug)]
    #[project = DiscoveryProj]
    pub enum RouterDiscovery {
        Config {
            #[pin]
            inner: ConfigDiscovery,
        },
        Cloud {
            #[pin]
            inner: CloudDiscovery,
        },
    }
}

impl RouterDiscovery {
    pub async fn new(
        app_state: &AppState,
        rx: Option<Receiver<Change<RouterId, Router>>>,
    ) -> Result<Self, InitError> {
        match app_state.0.config.deployment_target {
            DeploymentTarget::Sidecar => Ok(Self::Config {
                inner: ConfigDiscovery::new(app_state).await?,
            }),
            DeploymentTarget::Cloud => {
                let rx = rx.ok_or(InitError::RouterRxNotConfigured)?;
                Ok(Self::Cloud {
                    inner: CloudDiscovery::new(app_state, rx).await?,
                })
            }
        }
    }
}

impl Stream for RouterDiscovery {
    type Item = Result<Change<RouterId, Router>, Infallible>;

    fn poll_next(
        self: Pin<&mut Self>,
        ctx: &mut Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        match self.project() {
            DiscoveryProj::Config { inner } => {
                inner.poll_next(ctx).map(|p| p.map(Result::Ok))
            }
            DiscoveryProj::Cloud { inner } => {
                inner.poll_next(ctx).map(|p| p.map(Result::Ok))
            }
        }
    }
}
