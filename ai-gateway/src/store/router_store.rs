use std::collections::HashMap;

use sqlx::PgPool;
use tracing::error;
use uuid::Uuid;

use crate::{
    control_plane::types::Key, error::init::InitError, types::router::RouterId,
};

#[derive(Debug)]
pub struct RouterStore {
    pub pool: PgPool,
}

#[derive(Debug, sqlx::FromRow)]
pub struct DBRouterConfig {
    pub router_hash: String,
    pub config: serde_json::Value,
}

#[derive(Debug, sqlx::FromRow)]
pub struct DBRouterKeys {
    pub hash: String,
    pub api_key_hash: String,
    pub user_id: Uuid,
}

#[derive(Debug, sqlx::FromRow)]
pub struct DBUnifiedApiKeys {
    pub api_key_hash: String,
    pub user_id: Uuid,
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
             config FROM router_config_versions INNER JOIN routers on \
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

    pub async fn get_all_router_keys(
        &self,
    ) -> Result<HashMap<RouterId, Vec<Key>>, InitError> {
        let res = sqlx::query_as::<_, DBRouterKeys>(
            "SELECT routers.hash, helicone_api_keys.api_key_hash, \
             helicone_api_keys.user_id FROM router_keys INNER JOIN \
             helicone_api_keys ON router_keys.api_key_id = \
             helicone_api_keys.id INNER JOIN routers ON router_keys.router_id \
             = routers.id WHERE routers.hash IS NOT NULL",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| {
            error!(error = %e, "failed to get all router keys");
            InitError::DatabaseConnection(e)
        })?;
        let mut map = HashMap::new();
        tracing::info!("keys length: {:?}", res.len());
        for r in res {
            tracing::info!("router_hash: {:?}", r.hash);
            map.entry(RouterId::Named(r.hash.into()))
                .or_insert(vec![])
                .push(Key {
                    key_hash: r.api_key_hash,
                    owner_id: r.user_id.to_string(),
                });
        }

        // get /ai keys - router_keys.router_id is NULL
        let res = sqlx::query_as::<_, DBUnifiedApiKeys>(
            "SELECT helicone_api_keys.api_key_hash, \
             helicone_api_keys.user_id FROM router_keys INNER JOIN \
             helicone_api_keys ON router_keys.api_key_id = \
             helicone_api_keys.id WHERE router_keys.router_id IS NULL",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| {
            error!(error = %e, "failed to get all router keys");
            InitError::DatabaseConnection(e)
        })?;

        for r in res {
            map.entry(RouterId::Named("hcone_rsv_ai".into()))
                .or_insert(vec![])
                .push(Key {
                    key_hash: r.api_key_hash,
                    owner_id: r.user_id.to_string(),
                });
        }

        tracing::info!("map length: {:?}", map.len());
        Ok(map)
    }
}
