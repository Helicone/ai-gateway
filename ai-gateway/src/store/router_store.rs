use sqlx::PgPool;
use tracing::error;
use uuid::Uuid;

use crate::error::init::InitError;

#[derive(Debug)]
pub struct RouterStore {
    pub pool: PgPool,
}

#[derive(Debug, sqlx::FromRow)]
pub struct DBRouterConfig {
    pub router_id: Uuid,
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
            "SELECT DISTINCT ON (router_id) router_id, config FROM \
             router_config_versions ORDER BY router_id, created_at DESC",
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
