use sqlx::PgPool;
use tracing::error;

use crate::error::init::InitError;

#[derive(Debug)]
pub struct RouterStore {
    pub pool: PgPool,
}

#[derive(Debug, sqlx::FromRow)]
pub struct DBRouterConfig {
    pub router_hash: String,
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
}
