use std::{
    marker::PhantomData,
    task::{Context, Poll},
};

use axum_core::response::{IntoResponse, Response};
use futures::future::{BoxFuture, Either};
use http::{Method, Request};
use http_body_util::BodyExt;
use serde::Serialize;
use tower::{Layer, Service};

use crate::{
    config::router::RouterConfig,
    error::{api::ApiError, internal::InternalError},
    types::json::Json,
};

#[derive(Debug, Clone)]
pub struct ValidateRouterConfigLayer<ReqBody> {
    _marker: PhantomData<ReqBody>,
}

impl<ReqBody> ValidateRouterConfigLayer<ReqBody> {
    #[must_use]
    pub const fn new() -> Self {
        Self {
            _marker: PhantomData,
        }
    }
}

impl<ReqBody> Default for ValidateRouterConfigLayer<ReqBody> {
    fn default() -> Self {
        Self::new()
    }
}

impl<S, ReqBody> Layer<S> for ValidateRouterConfigLayer<ReqBody>
where
    S: tower::Service<http::Request<ReqBody>, Response = Response>,
{
    type Service = ValidateRouterConfig<S, ReqBody>;

    fn layer(&self, inner: S) -> Self::Service {
        ValidateRouterConfig::new(inner)
    }
}

#[derive(Debug)]
pub struct ValidateRouterConfig<S, ReqBody> {
    inner: S,
    _marker: PhantomData<ReqBody>,
}

impl<S: Clone, ReqBody> Clone for ValidateRouterConfig<S, ReqBody> {
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            _marker: PhantomData,
        }
    }
}

impl<S, ReqBody> ValidateRouterConfig<S, ReqBody>
where
    S: tower::Service<http::Request<ReqBody>, Response = Response>,
{
    pub const fn new(inner: S) -> Self {
        Self {
            inner,
            _marker: PhantomData,
        }
    }
}

#[derive(Serialize)]
pub struct ValidateRouterConfigResponse {
    pub valid: bool,
    pub error: Option<String>,
}

impl<S, ReqBody> Service<Request<ReqBody>> for ValidateRouterConfig<S, ReqBody>
where
    S: Service<Request<ReqBody>, Response = Response> + Send + Clone + 'static,
    S::Future: Send + 'static,
    ReqBody: http_body::Body + Send + 'static,
    ReqBody::Data: Send,
{
    type Response = Response;
    type Error = S::Error;
    type Future =
        Either<BoxFuture<'static, Result<Response, S::Error>>, S::Future>;

    fn poll_ready(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        if req.method() == Method::POST
            && req.uri().path() == "/validate-router-config"
        {
            let fut = async move {
                let config = match req.into_body().collect().await {
                    Ok(body) => body.to_bytes(),
                    Err(_e) => {
                        tracing::warn!("failed to collect request body");
                        let error = ApiError::Internal(InternalError::Internal);
                        return Ok(error.into_response());
                    }
                };

                let config =
                    match serde_json::from_slice::<RouterConfig>(&config) {
                        Ok(config) => config,
                        Err(e) => {
                            let body = Json(ValidateRouterConfigResponse {
                                valid: false,
                                error: Some(e.to_string()),
                            });
                            return Ok(body.into_response());
                        }
                    };

                let validate_result = config.validate();
                if let Err(e) = validate_result {
                    let body = Json(ValidateRouterConfigResponse {
                        valid: false,
                        error: Some(e.to_string()),
                    });
                    Ok(body.into_response())
                } else {
                    let body = Json(ValidateRouterConfigResponse {
                        valid: true,
                        error: None,
                    });
                    Ok(body.into_response())
                }
            };
            Either::Left(Box::pin(fut))
        } else {
            Either::Right(self.inner.call(req))
        }
    }
}
