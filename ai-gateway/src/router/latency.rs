use std::{
    pin::{Pin, pin},
    str::FromStr,
    sync::Arc,
    task::{Context, Poll},
};

use bytes::Bytes;
use futures::{Future, ready};
use http_body_util::{BodyExt, combinators::Collect};
use pin_project_lite::pin_project;
use tokio::sync::mpsc::channel;
use tower::{Service, buffer::Buffer, load::PeakEwmaDiscover};

use crate::{
    app_state::AppState,
    config::router::RouterConfig,
    discover::{
        dispatcher::{DispatcherDiscovery, factory::DispatcherDiscoverFactory},
        model,
    },
    error::{
        api::ApiError, init::InitError, internal::InternalError,
        invalid_req::InvalidRequestError,
    },
    types::{
        model_id::{ModelId, ModelName},
        request::Request,
        response::Response,
        router::RouterId,
    },
};

const CHANNEL_CAPACITY: usize = 16;

type ConcreteLatencyRouter = latency_router::router::LatencyRouter<
    ModelName<'static>,
    PeakEwmaDiscover<DispatcherDiscovery<model::key::Key>>,
    axum_core::body::Body,
>;

type InnerService =
    Buffer<Request, <ConcreteLatencyRouter as Service<Request>>::Future>;

#[derive(Clone)]
pub struct LatencyRouter {
    inner: InnerService,
}

impl std::fmt::Debug for LatencyRouter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LatencyRouter").finish_non_exhaustive()
    }
}

impl LatencyRouter {
    pub async fn new(
        app_state: AppState,
        router_id: RouterId,
        router_config: Arc<RouterConfig>,
    ) -> Result<Self, InitError> {
        let (change_tx, change_rx) = channel(CHANNEL_CAPACITY);
        let (rate_limit_tx, rate_limit_rx) = channel(CHANNEL_CAPACITY);
        let discover_factory = DispatcherDiscoverFactory::new(
            app_state.clone(),
            router_id.clone(),
            router_config.clone(),
        );
        app_state
            .add_model_latency_router_health_monitor(
                router_id.clone(),
                router_config.clone(),
                change_tx.clone(),
            )
            .await;
        app_state
            .add_rate_limit_tx(router_id.clone(), rate_limit_tx)
            .await;
        app_state
            .add_rate_limit_rx(router_id.clone(), rate_limit_rx)
            .await;
        app_state
            .add_model_latency_router_rate_limit_monitor(
                router_id.clone(),
                router_config,
                change_tx,
            )
            .await;
        let mut factory =
            latency_router::router::MakeRouter::new(discover_factory);
        let inner = factory.call(change_rx).await?;
        let inner = Buffer::new(inner, CHANNEL_CAPACITY);
        Ok(Self { inner })
    }
}

impl tower::Service<Request> for LatencyRouter {
    type Response = Response;
    type Error = ApiError;
    type Future = ResponseFuture;

    #[inline]
    fn poll_ready(
        &mut self,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Result<(), Self::Error>> {
        self.inner
            .poll_ready(cx)
            .map_err(Into::into)
            .map_err(InternalError::PollReadyError)
            .map_err(Into::into)
    }

    fn call(&mut self, req: Request) -> Self::Future {
        let (parts, body) = req.into_parts();
        let mut inner = self.inner.clone();
        std::mem::swap(&mut self.inner, &mut inner);
        ResponseFuture::new(body.collect(), parts, inner)
    }
}

pin_project! {
    pub struct ResponseFuture {
        #[pin]
        state: State,
    }
}

impl ResponseFuture {
    pub fn new(
        collect_future: Collect<axum_core::body::Body>,
        parts: http::request::Parts,
        inner: InnerService,
    ) -> Self {
        Self {
            state: State::CollectBody {
                collect_future,
                parts: Some(parts),
                inner: Some(inner),
            },
        }
    }
}

pin_project! {
    #[project = StateProj]
    enum State {
        CollectBody {
            #[pin]
            collect_future: Collect<axum_core::body::Body>,
            parts: Option<http::request::Parts>,
            inner: Option<InnerService>,
        },
        DetermineModelName {
            collected_body: Option<Bytes>,
            parts: Option<http::request::Parts>,
            inner: Option<InnerService>,
        },
        CallRouter {
            #[pin]
            response_future: <InnerService as tower::Service<Request>>::Future,
        },
    }
}

impl Future for ResponseFuture {
    type Output = Result<Response, ApiError>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut this = self.project();
        loop {
            match this.state.as_mut().project() {
                StateProj::CollectBody {
                    collect_future,
                    parts,
                    inner,
                } => {
                    let collected = match ready!(pin!(collect_future).poll(cx))
                    {
                        Ok(collected) => collected,
                        Err(e) => {
                            return Poll::Ready(Err(
                                InternalError::CollectBodyError(e).into(),
                            ));
                        }
                    };
                    let parts =
                        parts.take().expect("future polled after completion");
                    let inner =
                        inner.take().expect("future polled after completion");

                    this.state.set(State::DetermineModelName {
                        collected_body: Some(collected.to_bytes()),
                        parts: Some(parts),
                        inner: Some(inner),
                    });
                }
                StateProj::DetermineModelName {
                    collected_body,
                    parts,
                    inner,
                } => {
                    let body = collected_body
                        .take()
                        .expect("future polled after completion");
                    let json: serde_json::Value = serde_json::from_slice(&body)
                        .map_err(InvalidRequestError::InvalidRequestBody)?;
                    let model_id = json
                        .get("model")
                        .ok_or(InvalidRequestError::MissingModelId)?;
                    let model_id = model_id
                        .as_str()
                        .ok_or(InvalidRequestError::MissingModelId)?;
                    let model_id = ModelId::from_str(model_id).map_err(|_| {
                        tracing::debug!(model_id = %model_id, "invalid model id");
                        InvalidRequestError::InvalidModelId
                    })?;
                    let model_name = model_id.as_model_name_owned();
                    let mut parts =
                        parts.take().expect("future polled after completion");
                    parts.extensions.insert(model_name);
                    let request = Request::from_parts(
                        parts,
                        axum_core::body::Body::from(body),
                    );
                    let mut inner =
                        inner.take().expect("future polled after completion");
                    this.state.set(State::CallRouter {
                        response_future: inner.call(request),
                    });
                }
                StateProj::CallRouter { response_future } => {
                    let response = ready!(response_future.poll(cx))
                        .map_err(InternalError::LoadBalancerError)?;
                    return Poll::Ready(Ok(response));
                }
            }
        }
    }
}
