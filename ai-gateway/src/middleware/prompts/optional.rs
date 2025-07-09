use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

use crate::{
    app_state::AppState,
    error::{api::ApiError, init::InitError},
    middleware::prompts::service::{PromptLayer, PromptService},
    types::{request::Request, response::Response},
};

#[derive(Debug, Clone)]
pub struct Layer {
    inner: Option<PromptLayer>,
}

impl Layer {
    pub fn new(app_state: &AppState) -> Result<Self, InitError> {
        if !app_state.config().helicone.is_prompts_enabled() {
            return Ok(Self { inner: None });
        }

        let layer = PromptLayer::new(app_state.clone());
        Ok(Self { inner: Some(layer) })
    }

    /// For when we statically know that prompts are disabled.
    #[must_use]
    pub fn disabled() -> Self {
        Self { inner: None }
    }
}

impl<S> tower::Layer<S> for Layer {
    type Service = Service<S>;

    fn layer(&self, service: S) -> Self::Service {
        if let Some(inner) = &self.inner {
            Service::Enabled {
                service: inner.layer(service),
            }
        } else {
            Service::Disabled { service }
        }
    }
}

#[derive(Debug, Clone)]
pub enum Service<S> {
    Enabled { service: PromptService<S> },
    Disabled { service: S },
}

pin_project_lite::pin_project! {
    #[derive(Debug)]
    #[project = EnumProj]
    pub enum ResponseFuture<EnabledFuture, DisabledFuture> {
        Enabled { #[pin] future: EnabledFuture },
        Disabled { #[pin] future: DisabledFuture },
    }
}

impl<EnabledFuture, DisabledFuture, Response> Future
    for ResponseFuture<EnabledFuture, DisabledFuture>
where
    EnabledFuture: Future<Output = Result<Response, ApiError>>,
    DisabledFuture: Future<Output = Result<Response, ApiError>>,
{
    type Output = Result<Response, ApiError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.project() {
            EnumProj::Enabled { future } => future.poll(cx),
            EnumProj::Disabled { future } => future.poll(cx),
        }
    }
}

impl<S> tower::Service<Request> for Service<S>
where
    S: tower::Service<Request, Response = Response, Error = ApiError>
        + Send
        + Clone
        + 'static,
    S::Future: Send + 'static,
{
    type Response = Response;
    type Error = ApiError;
    type Future = ResponseFuture<
        <PromptService<S> as tower::Service<Request>>::Future,
        S::Future,
    >;

    fn poll_ready(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        match self {
            Service::Enabled { service, .. } => service.poll_ready(cx),
            Service::Disabled { service } => service.poll_ready(cx),
        }
    }

    fn call(&mut self, req: Request) -> Self::Future {
        match self {
            Service::Enabled { service } => ResponseFuture::Enabled {
                future: service.call(req),
            },
            Service::Disabled { service } => ResponseFuture::Disabled {
                future: service.call(req),
            },
        }
    }
}
