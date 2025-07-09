use sqlx::{PgPool, postgres::PgPoolOptions};
use tracing::error;
use uuid::Uuid;

use crate::{config::database::DatabaseConfig, error::init::InitError};

#[derive(Debug)]
pub struct Database {
    pub pool: PgPool,
}

#[derive(Debug, sqlx::FromRow)]
pub struct DBRouterConfig {
    pub router_id: Uuid,
    pub config: serde_json::Value,
}

impl Database {
    pub async fn new(config: DatabaseConfig) -> Result<Self, InitError> {
        let pool = PgPoolOptions::new()
            .max_connections(config.max_connections)
            .min_connections(config.min_connections)
            .acquire_timeout(config.acquire_timeout)
            .idle_timeout(config.idle_timeout)
            .max_lifetime(config.max_lifetime)
            .connect(&config.url)
            .await
            .map_err(|e| {
                error!(error = %e, "failed to create database pool");
                InitError::DatabaseConnection(e)
            })?;
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
