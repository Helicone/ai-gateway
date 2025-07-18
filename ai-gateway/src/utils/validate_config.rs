use std::{
    future::{Ready, ready},
    marker::PhantomData,
    task::{Context, Poll},
};

use axum_core::response::Response;
use futures::future::{BoxFuture, Either};
use http::{Method, Request};
use http_body_util::BodyExt;
use serde::Serialize;
use tower::{Layer, Service};

use crate::{config::router::RouterConfig, error::internal::InternalError};

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
    // type Future = BoxFuture<'static, Result<Response, S::Error>>;
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
            tracing::info!("validating router config");
            let fut = async move {
                let config = req.into_body().collect().await;
                tracing::info!("config");
                let invalid_response_body =
                    serde_json::to_string(&ValidateRouterConfigResponse {
                        valid: false,
                    })
                    .unwrap();
                let valid_response_body =
                    serde_json::to_string(&ValidateRouterConfigResponse {
                        valid: true,
                    })
                    .unwrap();

                match config {
                    Ok(config_body) => {
                        let config = serde_json::from_slice::<RouterConfig>(
                            &config_body.to_bytes(),
                        );
                        if config.is_err() {
                            return Ok(http::Response::builder()
                                .status(http::StatusCode::OK)
                                .header(
                                    http::header::CONTENT_TYPE,
                                    "application/json",
                                )
                                .body(axum_core::body::Body::from(
                                    invalid_response_body,
                                ))
                                .expect("always valid if tests pass"));
                        }

                        let config = config.unwrap();

                        if config.validate().is_err() {
                            return Ok(http::Response::builder()
                                .status(http::StatusCode::OK)
                                .header(
                                    http::header::CONTENT_TYPE,
                                    "application/json",
                                )
                                .body(axum_core::body::Body::from(
                                    invalid_response_body,
                                ))
                                .expect("always valid if tests pass"));
                        }

                        Ok(http::Response::builder()
                            .status(http::StatusCode::OK)
                            .header(
                                http::header::CONTENT_TYPE,
                                "application/json",
                            )
                            .body(axum_core::body::Body::from(
                                valid_response_body,
                            ))
                            .expect("always valid if tests pass"))
                    }
                    Err(_e) => {
                        return Ok(http::Response::builder()
                            .status(http::StatusCode::OK)
                            .header(
                                http::header::CONTENT_TYPE,
                                "application/json",
                            )
                            .body(axum_core::body::Body::from(
                                invalid_response_body,
                            ))
                            .expect("always valid if tests pass"));
                    }
                }
            };
            Either::Left(Box::pin(fut))
        } else {
            Either::Right(self.inner.call(req))
        }
    }
}

/*
    fn call(&mut self, req: Request<ReqBody>) -> Self::Future {
        if req.method() == Method::POST
            && req.uri().path() == "/validate-router-config"
        {
            let fut = async move {
                let config = req.body().await;
                let config = serde_json::from_slice::<RouterConfig>(&config);
                if config.is_err() {
                    return Ok(http::Response::builder()
                        .status(http::StatusCode::OK)
                        .body(axum_core::body::Body::from(false))
                        .expect("always valid if tests pass"));
                }

                Ok(http::Response::builder()
                    .status(http::StatusCode::OK)
                    .body(axum_core::body::Body::from(true))
                    .expect("always valid if tests pass"))
            };

            Either::Left(Box::pin(fut))
            // let this = self.clone();
            // let this = std::mem::replace(self, this);
            // Box::pin(async move {
            //     Either::Left(ready(Ok(validate_config_response(
            //         req.body().await,
            //     ))))
            // })
        } else {
            Either::Right(self.inner.call(req))
        }
    }

*/

// #[cfg(test)]
// mod tests {
//     use super::*;

//     #[test]
//     fn test_healthy_response() {
//         let response = healthy_response();
//         assert_eq!(response.status(), http::StatusCode::OK);
//     }
// }
