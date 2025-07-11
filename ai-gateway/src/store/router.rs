use std::collections::HashSet;

use sqlx::PgPool;
use tracing::{error, info};
use uuid::Uuid;

use crate::{
    control_plane::types::Key, error::init::InitError, types::org::OrgId,
};

#[derive(Debug)]
pub struct RouterStore {
    pub pool: PgPool,
}

#[derive(Debug, sqlx::FromRow)]
pub struct DBRouterConfig {
    pub router_hash: String,
    pub organization_id: Uuid,
    pub config: serde_json::Value,
}

#[derive(Debug, sqlx::FromRow)]
pub struct DBApiKey {
    pub key_hash: String,
    pub owner_id: Uuid,
    pub organization_id: Uuid,
}

impl RouterStore {
    pub fn new(pool: PgPool) -> Result<Self, InitError> {
        Ok(Self { pool })
    }

    pub async fn get_all_routers(
        &self,
    ) -> Result<Vec<DBRouterConfig>, InitError> {
        let res = sqlx::query_as::<_, DBRouterConfig>(
            "SELECT DISTINCT ON (routers.hash) routers.hash as router_hash, \
             routers.organization_id as organization_id, config FROM \
             router_config_versions INNER JOIN routers on \
             router_config_versions.router_id = routers.id ORDER BY \
             routers.hash, router_config_versions.created_at DESC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| {
            error!(error = %e, "failed to get all routers");
            InitError::DatabaseConnection(e)
        })?;
        Ok(res)
    }

    pub async fn get_all_router_keys(&self) -> Result<HashSet<Key>, InitError> {
        let res = sqlx::query_as::<_, DBApiKey>(
            "SELECT helicone_api_keys.api_key_hash as key_hash, \
             helicone_api_keys.user_id as owner_id, \
             helicone_api_keys.organization_id as organization_id FROM \
             helicone_api_keys WHERE helicone_api_keys.soft_delete = false",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| {
            error!(error = %e, "failed to get all router keys");
            InitError::DatabaseConnection(e)
        })?;
        info!("found {} router keys", res.len());

        let keys = res
            .into_iter()
            .map(|k| Key {
                key_hash: k.key_hash,
                owner_id: k.owner_id.to_string(),
                organization_id: OrgId::new(k.organization_id),
            })
            .collect();

        Ok(keys)
    }

    pub async fn get_organization_keys(
        &self,
        organization_id: &str,
    ) -> Result<HashSet<Key>, InitError> {
        let org_id = Uuid::parse_str(organization_id).map_err(|e| {
            error!(error = %e, "failed to parse organization id");
            InitError::InvalidOrganizationId(organization_id.to_string())
        })?;
        let res = sqlx::query_as::<_, DBApiKey>(
            "SELECT helicone_api_keys.api_key_hash as key_hash, \
             helicone_api_keys.user_id as owner_id, \
             helicone_api_keys.organization_id as organization_id FROM \
             helicone_api_keys WHERE helicone_api_keys.organization_id = $1 \
             AND helicone_api_keys.soft_delete = false",
        )
        .bind(org_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| {
            error!(error = %e, "failed to get organization keys");
            InitError::DatabaseConnection(e)
        })?;
        let keys = res
            .into_iter()
            .map(|k| Key {
                key_hash: k.key_hash,
                owner_id: k.owner_id.to_string(),
                organization_id: OrgId::new(k.organization_id),
            })
            .collect();

        Ok(keys)
    }
}
