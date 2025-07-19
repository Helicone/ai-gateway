use std::collections::HashSet;

use rustc_hash::FxHashMap;
use sqlx::PgPool;
use tracing::{error, info};
use uuid::Uuid;

use crate::{
    control_plane::types::Key,
    error::{init::InitError, provider::ProviderError},
    types::{
        org::OrgId,
        provider::{InferenceProvider, ProviderKey, ProviderKeyMap},
        secret::Secret,
    },
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

#[derive(Debug, sqlx::FromRow)]
pub struct DBProviderKey {
    pub provider_name: String,
    pub decrypted_provider_key: String,
    pub org_id: Uuid,
    pub config: serde_json::Value,
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

    pub async fn get_all_provider_keys(
        &self,
    ) -> Result<FxHashMap<OrgId, ProviderKeyMap>, InitError> {
        let res = sqlx::query_as::<_, DBProviderKey>(
            "SELECT decrypted_provider_keys.provider_name, \
             decrypted_provider_keys.decrypted_provider_key, \
             decrypted_provider_keys.org_id, decrypted_provider_keys.config \
             FROM decrypted_provider_keys WHERE soft_delete = false",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| {
            error!(error = %e, "failed to get all provider keys");
            InitError::DatabaseConnection(e)
        })?;
        let mut provider_keys: FxHashMap<
            OrgId,
            FxHashMap<InferenceProvider, ProviderKey>,
        > = FxHashMap::default();
        for key in res {
            let provider_key =
                ProviderKey::Secret(Secret::from(key.decrypted_provider_key));
            let inference_provider =
                match InferenceProvider::from_helicone_provider_name(
                    &key.provider_name,
                ) {
                    Ok(provider) => provider,
                    Err(e) => {
                        error!(error = %e, provider_name = %key.provider_name, "Failed to parse inference provider, skipping");
                        continue;
                    }
                };
            let existing_provider_keys =
                provider_keys.entry(OrgId::new(key.org_id)).or_default();
            existing_provider_keys.insert(inference_provider, provider_key);
        }

        let mut final_provider_keys = FxHashMap::default();
        for (org_id, provider_keys) in provider_keys.drain() {
            let provider_key_map =
                ProviderKeyMap::from_db(provider_keys.clone());
            final_provider_keys.insert(org_id, provider_key_map);
        }

        Ok(final_provider_keys)
    }

    pub async fn get_org_provider_keys(
        &self,
        org_id: OrgId,
    ) -> Result<ProviderKeyMap, InitError> {
        let res = sqlx::query_as::<_, DBProviderKey>(
            "SELECT decrypted_provider_keys.provider_name, \
             decrypted_provider_keys.decrypted_provider_key, \
             decrypted_provider_keys.org_id, decrypted_provider_keys.config \
             FROM decrypted_provider_keys WHERE org_id = $1 AND soft_delete = \
             false",
        )
        .bind(org_id.as_ref())
        .fetch_all(&self.pool)
        .await
        .map_err(|e| {
            error!(error = %e, "failed to get organization provider keys");
            InitError::DatabaseConnection(e)
        })?;
        let mut provider_keys = FxHashMap::default();
        for key in res {
            let provider_key =
                ProviderKey::Secret(Secret::from(key.decrypted_provider_key));
            let inference_provider =
                match InferenceProvider::from_helicone_provider_name(
                    &key.provider_name,
                ) {
                    Ok(provider) => provider,
                    Err(e) => {
                        error!(error = %e, provider_name = %key.provider_name, "Failed to parse inference provider, skipping");
                        continue;
                    }
                };
            provider_keys.insert(inference_provider, provider_key);
        }
        Ok(ProviderKeyMap::from_db(provider_keys))
    }
}
