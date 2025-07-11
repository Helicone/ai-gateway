use sqlx::{PgPool, postgres::PgPoolOptions};

use crate::{config::database::DatabaseConfig, error::init::InitError};

pub mod db_listener;
pub mod minio;
pub mod router;

pub async fn connect(config: &DatabaseConfig) -> Result<PgPool, InitError> {
    let pool = PgPoolOptions::new()
        .max_connections(config.max_connections)
        .min_connections(config.min_connections)
        .acquire_timeout(config.acquire_timeout)
        .idle_timeout(config.idle_timeout)
        .max_lifetime(config.max_lifetime)
        .connect(&config.url)
        .await
        .map_err(|e| {
            tracing::error!(error = %e, "failed to create database pool");
            InitError::DatabaseConnection(e)
        })?;

    Ok(pool)
}
