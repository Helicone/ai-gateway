use axum_core::response::IntoResponse;
use futures::future::BoxFuture;
use http::Request;
use tower_http::auth::AsyncAuthorizeRequest;

use crate::{
    app_state::AppState,
    config::DeploymentTarget,
    control_plane::types::hash_key,
    error::{
        api::ApiError, auth::AuthError, internal::InternalError,
        invalid_req::InvalidRequestError,
    },
    types::{
        extensions::{AuthContext, RequestKind},
        router::RouterId,
        secret::Secret,
    },
};

#[derive(Clone)]
pub struct AuthService {
    app_state: AppState,
}

impl AuthService {
    #[must_use]
    pub fn new(app_state: AppState) -> Self {
        Self { app_state }
    }

    async fn authenticate_request_inner(
        app_state: AppState,
        api_key: &str,
        request_kind: Option<&RequestKind>,
        router_id: Option<&RouterId>,
    ) -> Result<AuthContext, ApiError> {
        let api_key_without_bearer = api_key.replace("Bearer ", "");
        let computed_hash = hash_key(&api_key_without_bearer);

        match app_state.0.config.deployment_target {
            DeploymentTarget::Cloud => {
                let Some(request_kind) = request_kind else {
                    return Err(InternalError::ExtensionNotFound(
                        "RequestKind",
                    )
                    .into());
                };
                let Some(key) =
                    app_state.check_helicone_api_key(&computed_hash).await
                else {
                    return Err(AuthError::InvalidCredentials.into());
                };

                match request_kind {
                    RequestKind::Router => {
                        let Some(router_id) = router_id else {
                            return Err(InternalError::ExtensionNotFound(
                                "RouterId",
                            )
                            .into());
                        };

                        let Some(router_organization_id) =
                            app_state.get_router_organization(router_id).await
                        else {
                            return Err(InvalidRequestError::NotFound(
                                "router not found".to_string(),
                            )
                            .into());
                        };

                        if router_organization_id == key.organization_id {
                            Ok(AuthContext {
                                api_key: Secret::from(api_key_without_bearer),
                                user_id: key.owner_id,
                                org_id: key.organization_id,
                            })
                        } else {
                            Err(AuthError::InvalidCredentials.into())
                        }
                    }
                    RequestKind::UnifiedApi | RequestKind::DirectProxy => {
                        Ok(AuthContext {
                            api_key: Secret::from(api_key_without_bearer),
                            user_id: key.owner_id,
                            org_id: key.organization_id,
                        })
                    }
                }
            }
            DeploymentTarget::Sidecar => {
                let Some(control_plane_state) =
                    &app_state.0.control_plane_state.read().await.state
                else {
                    return Err(InternalError::AuthDataNotReady.into());
                };
                let key = control_plane_state.get_key_from_hash(&computed_hash);
                if let Some(key) = key {
                    Ok(AuthContext {
                        api_key: Secret::from(api_key_without_bearer),
                        user_id: key.owner_id,
                        org_id: control_plane_state.auth.organization_id,
                    })
                } else {
                    Err(AuthError::InvalidCredentials.into())
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

            let request_kind = request.extensions().get::<RequestKind>();
            let router_id = request.extensions().get::<RouterId>();

            match Self::authenticate_request_inner(
                app_state.clone(),
                api_key,
                request_kind,
                router_id,
            )
            .await
            {
                Ok(auth_ctx) => {
                    request.extensions_mut().insert(auth_ctx);
                    Ok(request)
                }
                Err(e) => {
                    if let ApiError::Authentication(auth_error) = &e {
                        match auth_error {
                            AuthError::MissingAuthorizationHeader
                            | AuthError::InvalidCredentials
                            | AuthError::ProviderKeyNotFound => {
                                app_state.0.metrics.auth_rejections.add(1, &[]);
                            }
                        }
                    }
                    Err(e.into_response())
                }
            }
        })
    }
}
