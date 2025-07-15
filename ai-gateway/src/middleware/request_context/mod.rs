use std::{
    sync::Arc,
    task::{Context, Poll},
};

use crate::{
    config::router::RouterConfig,
    types::{
        extensions::{AuthContext, RequestContext},
        request::Request,
        response::Response,
    },
};

#[derive(Debug, Clone)]
pub struct Service<S> {
    inner: S,
    /// If `None`, this service is for a direct proxy.
    /// If `Some`, this service is for a load balanced router.
    router_config: Option<Arc<RouterConfig>>,
}

impl<S> Service<S> {
    pub fn new(inner: S, router_config: Option<Arc<RouterConfig>>) -> Self {
        Self {
            inner,
            router_config,
        }
    }
}

impl<S> tower::Service<Request> for Service<S>
where
    S: tower::Service<Request, Response = Response> + Send + 'static,
    S::Future: Send + 'static,
{
    type Response = S::Response;
    type Error = S::Error;
    type Future = S::Future;

    #[inline]
    fn poll_ready(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx).map_err(Into::into)
    }

    #[tracing::instrument(level = "debug", name = "request_context", skip_all)]
    fn call(&mut self, mut req: Request) -> Self::Future {
        let router_config = self.router_config.clone();
        let auth_context = req.extensions_mut().remove::<AuthContext>();
        let req_ctx = RequestContext {
            router_config,
            auth_context,
        };
        req.extensions_mut().insert(Arc::new(req_ctx));
        self.inner.call(req)
    }
}

#[derive(Debug, Clone)]
pub struct Layer {
    router_config: Option<Arc<RouterConfig>>,
}

impl Layer {
    #[must_use]
    pub fn for_router(router_config: Arc<RouterConfig>) -> Self {
        Self {
            router_config: Some(router_config),
        }
    }

    #[must_use]
    pub fn for_direct_proxy() -> Self {
        Self {
            router_config: None,
        }
    }
}

impl<S> tower::Layer<S> for Layer {
    type Service = Service<S>;

    fn layer(&self, inner: S) -> Self::Service {
        Service::new(inner, self.router_config.clone())
    }
}
