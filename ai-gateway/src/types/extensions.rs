use std::{collections::HashMap, sync::Arc};

use derive_more::{AsRef, From, Into};

use super::{model_id::ModelId, org::OrgId, user::UserId};
use crate::{config::router::RouterConfig, types::secret::Secret};

#[derive(Debug, Clone, AsRef, From, Into)]
pub struct ProviderRequestId(pub(crate) http::HeaderValue);

#[derive(Debug, Clone)]
pub struct AuthContext {
    pub api_key: Secret<String>,
    pub user_id: UserId,
    pub org_id: OrgId,
}

#[derive(Debug)]
pub struct RequestContext {
    /// If `None`, the request was for a direct proxy.
    /// If `Some`, the request was for a load balanced router.
    pub router_config: Option<Arc<RouterConfig>>,
    /// If `None`, the router is configured to not require auth for requests,
    /// disabling some features.
    pub auth_context: Option<AuthContext>,
}

#[derive(Debug, Clone)]
pub struct MapperContext {
    pub is_stream: bool,
    /// If `None`, the request was for an endpoint without
    /// first class support for mapping between different provider
    /// models.
    pub model: Option<ModelId>,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct PromptContext {
    pub prompt_id: String,
    pub prompt_version_id: Option<String>,
    pub inputs: Option<HashMap<String, serde_json::Value>>,
}

#[derive(Debug, Clone, Copy)]
pub enum RequestKind {
    Router,
    UnifiedApi,
    DirectProxy,
}
