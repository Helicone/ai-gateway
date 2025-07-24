use std::{
    future::{Ready, ready},
    marker::PhantomData,
    task::{Context, Poll},
};

use axum_core::response::Response;
use futures::future::Either;
use http::{Method, Request};
use tower::{Layer, Service};

#[derive(Debug, Clone)]
pub struct HealthCheckLayer<ReqBody, E> {
    _marker: PhantomData<(ReqBody, E)>,
}

impl<ReqBody, E> HealthCheckLayer<ReqBody, E> {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            _marker: PhantomData,
        }
    }
}

impl<ReqBody, E> Default for HealthCheckLayer<ReqBody, E> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S, ReqBody, E> Layer<S> for HealthCheckLayer<ReqBody, E>
where
    S: tower::Service<http::Request<ReqBody>, Response = Response, Error = E>,
{
    type Service = HealthCheck<S, ReqBody, E>;

    fn layer(&self, inner: S) -> Self::Service {
        HealthCheck::new(inner)
    }
}

#[derive(Debug)]
pub struct HealthCheck<S, ReqBody, E> {
    inner: S,
    _marker: PhantomData<(ReqBody, E)>,
}

impl<S: Clone, ReqBody, E> Clone for HealthCheck<S, ReqBody, E> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            _marker: PhantomData,
        }
    }
}

impl<S, ReqBody, E> HealthCheck<S, ReqBody, E>
where
    S: tower::Service<http::Request<ReqBody>, Response = Response, Error = E>,
{
    pub const fn new(inner: S) -> Self {
        Self {
            inner,
            _marker: PhantomData,
        }
    }
}

impl<S, ReqBody, E> Service<Request<ReqBody>> for HealthCheck<S, ReqBody, E>
where
    S: Service<Request<ReqBody>, Response = Response, Error = E>
        + Send
        + 'static,
    S::Future: Send + 'static,
    ReqBody: Send + 'static,
{
    type Response = Response;
    type Error = S::Error;
    type Future = Either<Ready<Result<Self::Response, Self::Error>>, S::Future>;

    fn poll_ready(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        if (req.method() == Method::GET || req.method() == Method::HEAD)
            && req.uri().path() == "/health"
        {
            Either::Left(ready(Ok(healthy_response())))
        } else {
            Either::Right(self.inner.call(req))
        }
    }
}

fn healthy_response() -> Response {
    let body = axum_core::body::Body::empty();
    http::Response::builder()
        .status(http::StatusCode::OK)
        .body(body)
        .expect("always valid if tests pass")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_healthy_response() {
        let response = healthy_response();
        assert_eq!(response.status(), http::StatusCode::OK);
    }
}
