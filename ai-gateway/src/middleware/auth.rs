use axum_core::response::IntoResponse;
use futures::future::BoxFuture;
use http::Request;
use regex::Regex;
use tower_http::auth::AsyncAuthorizeRequest;

use crate::{
    app_state::AppState,
    config::DeploymentTarget,
    control_plane::types::hash_key,
    error::auth::AuthError,
    types::{extensions::AuthContext, router::RouterId, secret::Secret},
};

#[derive(Clone)]
pub struct AuthService {
    app_state: AppState,
}

const UNIFIED_URL_REGEX: &str =
    r"^/(?P<first_segment>[^/?]+)(?P<rest>/[^?]*)?(?P<query>\?.*)?$";

const ROUTER_URL_REGEX: &str =
    r"^/router/(?P<id>[A-Za-z0-9_-]{1,12})(?P<path>/[^?]*)?(?P<query>\?.*)?$";

impl AuthService {
    #[must_use]
    pub fn new(app_state: AppState) -> Self {
        Self { app_state }
    }

    async fn authenticate_request_inner(
        app_state: AppState,
        api_key: &str,
        path: &str,
    ) -> Result<AuthContext, AuthError> {
        let config = &app_state.0.control_plane_state.read().await.config;
        let api_key_without_bearer = api_key.replace("Bearer ", "");
        let computed_hash = hash_key(&api_key_without_bearer);

        match app_state.0.config.deployment_target {
            DeploymentTarget::Cloud => {
                // let allowed_keys = if let Some(captures) =
                //     Regex::new(UNIFIED_URL_REGEX).unwrap().captures(path)
                // {
                //     let first_segment = captures
                //         .name("first_segment")
                //         .ok_or_else(|| AuthError::MissingRouterId)?
                //         .as_str();

                //     if let Some(router_api_keys) =
                //         app_state.get_router_api_keys().await
                //     {
                //         if first_segment == "router"
                //             && router_api_keys.contains_key(&router_id)
                //         {
                //             router_api_keys.get(&router_id).unwrap()
                //         } else if first_segment == "ai"
                //             && router_api_keys.contains_key(&RouterId::Named(
                //                 "hcone_rsv_ai".into(),
                //             ))
                //         {
                //             router_api_keys
                //                 .get(&RouterId::Named("hcone_rsv_ai".into()))
                //                 .unwrap()
                //         } else {
                //             return Err(AuthError::InvalidCredentials);
                //         }
                //     }
                // };

                if let Some(captures) =
                    Regex::new(UNIFIED_URL_REGEX).unwrap().captures(path)
                {
                    let first_segment = captures
                        .name("first_segment")
                        .ok_or_else(|| AuthError::MissingRouterId)?
                        .as_str();

                    if first_segment == "router" {
                        let regex = Regex::new(ROUTER_URL_REGEX).unwrap();
                        let captures = regex.captures(path);
                        if captures.is_none() {
                            return Err(AuthError::MissingRouterId);
                        }
                        let id_str = captures
                            .unwrap()
                            .name("id")
                            .ok_or_else(|| AuthError::MissingRouterId)?
                            .as_str();
                        let router_id = RouterId::Named(id_str.into());
                        if let Some(router_api_keys) =
                            app_state.get_router_api_keys().await
                            && router_api_keys.contains_key(&router_id)
                        {
                            let allowed_keys =
                                router_api_keys.get(&router_id).unwrap();
                            let key = allowed_keys
                                .iter()
                                .find(|k| k.key_hash == computed_hash);
                            if let Some(key) = key {
                                return Ok(AuthContext {
                                    api_key: Secret::from(
                                        api_key_without_bearer,
                                    ),
                                    user_id: key
                                        .owner_id
                                        .as_str()
                                        .try_into()?,
                                    org_id: config
                                        .auth
                                        .organization_id
                                        .as_str()
                                        .try_into()?,
                                });
                            } else {
                                Err(AuthError::InvalidCredentials)
                            }
                        } else {
                            Err(AuthError::InvalidCredentials)
                        }
                    } else if first_segment == "ai" {
                        if let Some(router_api_keys) =
                            app_state.get_router_api_keys().await
                            && router_api_keys.contains_key(&RouterId::Named(
                                "hcone_rsv_ai".into(),
                            ))
                        {
                            let allowed_keys = router_api_keys
                                .get(&RouterId::Named("hcone_rsv_ai".into()))
                                .unwrap();
                            let key = allowed_keys
                                .iter()
                                .find(|k| k.key_hash == computed_hash);
                            if let Some(key) = key {
                                return Ok(AuthContext {
                                    api_key: Secret::from(
                                        api_key_without_bearer,
                                    ),
                                    user_id: key
                                        .owner_id
                                        .as_str()
                                        .try_into()?,
                                    org_id: config
                                        .auth
                                        .organization_id
                                        .as_str()
                                        .try_into()?,
                                });
                            } else {
                                Err(AuthError::InvalidCredentials)
                            }
                        } else {
                            Err(AuthError::InvalidCredentials)
                        }
                    } else {
                        Err(AuthError::InvalidCredentials)
                    }
                } else {
                    Err(AuthError::InvalidCredentials)
                }
            }
            DeploymentTarget::Sidecar => {
                let key = config.get_key_from_hash(&computed_hash);
                if let Some(key) = key {
                    Ok(AuthContext {
                        api_key: Secret::from(api_key_without_bearer),
                        user_id: key.owner_id.as_str().try_into()?,
                        org_id: config
                            .auth
                            .organization_id
                            .as_str()
                            .try_into()?,
                    })
                } else {
                    Err(AuthError::InvalidCredentials)
                }
            }
        }
    }
}

impl<B> AsyncAuthorizeRequest<B> for AuthService
where
    B: Send + 'static,
{
    type RequestBody = B;
    type ResponseBody = axum_core::body::Body;
    type Future = BoxFuture<
        'static,
        Result<Request<B>, http::Response<Self::ResponseBody>>,
    >;

    #[tracing::instrument(skip_all)]
    fn authorize(&mut self, mut request: Request<B>) -> Self::Future {
        let app_state = self.app_state.clone();
        Box::pin(async move {
            if app_state.0.config.helicone.is_auth_disabled() {
                tracing::trace!("auth middleware: auth disabled");
                return Ok(request);
            }
            tracing::trace!("auth middleware");
            let Some(api_key) = request
                .headers()
                .get("authorization")
                .and_then(|h| h.to_str().ok())
            else {
                return Err(
                    AuthError::MissingAuthorizationHeader.into_response()
                );
            };
            app_state.0.metrics.auth_attempts.add(1, &[]);

            // let Some(router_id) = request.extensions().get::<RouterId>() else
            // {     return
            // Err(AuthError::MissingRouterId.into_response()); };
            match Self::authenticate_request_inner(
                app_state.clone(),
                api_key,
                request.uri().path(),
            )
            .await
            {
                Ok(auth_ctx) => {
                    request.extensions_mut().insert(auth_ctx);
                    Ok(request)
                }
                Err(e) => {
                    match &e {
                        AuthError::MissingAuthorizationHeader
                        | AuthError::InvalidCredentials
                        | AuthError::MissingRouterId => {
                            app_state.0.metrics.auth_rejections.add(1, &[]);
                        }
                    }
                    Err(e.into_response())
                }
            }
        })
    }
}
