use std::collections::HashSet;

use chrono::{DateTime, Utc};
use rustc_hash::FxHashMap;
use sqlx::PgPool;
use tracing::{error, warn};
use uuid::Uuid;

use crate::{
    control_plane::types::Key,
    error::{init::InitError, internal::InternalError},
    types::{
        org::OrgId,
        provider::{InferenceProvider, ProviderKey, ProviderKeyMap},
        secret::Secret,
        user::UserId,
    },
};

#[derive(Debug, Clone)]
pub struct RouterStore {
    pub pool: PgPool,
}

#[derive(Debug, sqlx::FromRow)]
pub struct DbRouterConfig {
    pub router_hash: String,
    pub organization_id: Uuid,
    pub config: serde_json::Value,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, sqlx::FromRow)]
pub struct DbApiKey {
    pub key_hash: String,
    pub owner_id: Uuid,
    pub organization_id: Uuid,
    pub created_at: DateTime<Utc>,
    #[sqlx(default)]
    pub updated_at: Option<DateTime<Utc>>,
    #[sqlx(default)]
    pub soft_delete: Option<bool>,
}

#[derive(Debug, sqlx::FromRow)]
pub struct DbProviderKey {
    pub provider_name: String,
    pub decrypted_provider_key: String,
    pub org_id: Uuid,
    pub config: Option<serde_json::Value>,
}

impl RouterStore {
    pub fn new(pool: PgPool) -> Result<Self, InitError> {
        Ok(Self { pool })
    }

    pub async fn get_all_routers(
        &self,
    ) -> Result<Vec<DbRouterConfig>, InternalError> {
        let res = sqlx::query_as::<_, DbRouterConfig>(
            r"SELECT DISTINCT ON (routers.id)
                     routers.hash as router_hash,
                     routers.organization_id as organization_id,
                     router_config_versions.config,
                     router_config_versions.created_at
             FROM router_config_versions
             INNER JOIN routers ON router_config_versions.router_id = routers.id
             ORDER BY routers.id, router_config_versions.created_at DESC",
        )
        .fetch_all(&self.pool)
        .await
        .inspect_err(|e| {
            error!(error = %e, "failed to get all routers");
        })?;
        Ok(res)
    }

    pub async fn get_routers_created_after(
        &self,
        created_at: DateTime<Utc>,
    ) -> Result<Vec<DbRouterConfig>, InternalError> {
        let res = sqlx::query_as::<_, DbRouterConfig>(
            r"SELECT DISTINCT ON (routers.id)
                     routers.hash as router_hash,
                     routers.organization_id as organization_id,
                     router_config_versions.config,
                     router_config_versions.created_at
             FROM router_config_versions
             INNER JOIN routers ON router_config_versions.router_id = routers.id
             WHERE router_config_versions.created_at > $1
             ORDER BY routers.id, router_config_versions.created_at DESC",
        )
        .bind(created_at)
        .fetch_all(&self.pool)
        .await
        .inspect_err(|e| {
            error!(error = %e, "failed to get routers created after");
        })?;
        Ok(res)
    }

    pub async fn get_all_helicone_api_keys(
        &self,
    ) -> Result<HashSet<Key>, InternalError> {
        let res = self.get_all_db_helicone_api_keys().await?;
        let keys = res
            .into_iter()
            .map(|k| Key {
                key_hash: k.key_hash,
                owner_id: UserId::new(k.owner_id),
                organization_id: OrgId::new(k.organization_id),
            })
            .collect();

        Ok(keys)
    }

    pub async fn get_all_db_helicone_api_keys(
        &self,
    ) -> Result<Vec<DbApiKey>, InternalError> {
        let res = sqlx::query_as::<_, DbApiKey>(
            r"SELECT helicone_api_keys.api_key_hash as key_hash,
             helicone_api_keys.user_id as owner_id,
             helicone_api_keys.organization_id as organization_id,
             helicone_api_keys.created_at as created_at,
             helicone_api_keys.updated_at as updated_at
             FROM helicone_api_keys
             WHERE helicone_api_keys.soft_delete = false",
        )
        .fetch_all(&self.pool)
        .await
        .inspect_err(|e| {
            error!(error = %e, "failed to get all helicone api keys with timestamp");
        })?;

        Ok(res)
    }

    pub async fn get_all_db_helicone_api_keys_updated_after(
        &self,
        updated_at: DateTime<Utc>,
    ) -> Result<Vec<DbApiKey>, InternalError> {
        let res = sqlx::query_as::<_, DbApiKey>(
            r"SELECT helicone_api_keys.api_key_hash as key_hash,
             helicone_api_keys.user_id as owner_id,
             helicone_api_keys.organization_id as organization_id,
             helicone_api_keys.created_at as created_at,
             helicone_api_keys.updated_at as updated_at,
             helicone_api_keys.soft_delete as soft_delete
             FROM helicone_api_keys
             WHERE helicone_api_keys.updated_at > $1 
             OR helicone_api_keys.created_at > $1",
        )
        .bind(updated_at)
        .fetch_all(&self.pool)
        .await
        .inspect_err(|e| {
            error!(error = %e, "failed to get all helicone api keys created after");
        })?;

        Ok(res)
    }

    pub async fn get_all_provider_keys(
        &self,
    ) -> Result<FxHashMap<OrgId, ProviderKeyMap>, InitError> {
        let res = sqlx::query_as::<_, DbProviderKey>(
            "SELECT decrypted_provider_keys.provider_name, \
             decrypted_provider_keys.decrypted_provider_key, \
             decrypted_provider_keys.org_id, decrypted_provider_keys.config \
             FROM decrypted_provider_keys WHERE soft_delete = false AND \
             provider_key IS NOT NULL",
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
            let Ok(inference_provider) =
                InferenceProvider::from_helicone_provider_name(
                    &key.provider_name,
                )
            else {
                continue;
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
        let res = sqlx::query_as::<_, DbProviderKey>(
            "SELECT decrypted_provider_keys.provider_name, \
             decrypted_provider_keys.decrypted_provider_key, \
             decrypted_provider_keys.org_id, decrypted_provider_keys.config \
             FROM decrypted_provider_keys WHERE org_id = $1 AND soft_delete = \
             false AND provider_key IS NOT NULL",
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
                        warn!(error = %e, provider_name = %key.provider_name, "Failed to parse inference provider, skipping");
                        continue;
                    }
                };
            provider_keys.insert(inference_provider, provider_key);
        }
        Ok(ProviderKeyMap::from_db(provider_keys))
    }
}
