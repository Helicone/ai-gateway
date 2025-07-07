use std::time::Duration;

use reqwest::Client;
use rusty_s3::{
    Bucket, Credentials,
    actions::{GetObject, PutObject},
};

use crate::{config::minio::Config, error::init::InitError};

const DEFAULT_MINIO_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Debug)]
pub struct Minio {
    pub bucket: Bucket,
    pub client: Client,
    pub credentials: Credentials,
}

impl Minio {
    pub fn new(config: Config) -> Result<Self, InitError> {
        let bucket = Bucket::new(
            config.host,
            config.url_style.into(),
            config.bucket_name,
            config.region,
        )?;
        let client = Client::builder()
            .connect_timeout(DEFAULT_MINIO_TIMEOUT)
            .tcp_nodelay(true)
            .build()
            .map_err(InitError::CreateReqwestClient)?;
        let credentials = Credentials::new(
            config.access_key.expose(),
            config.secret_key.expose(),
        );
        Ok(Self {
            bucket,
            client,
            credentials,
        })
    }

    #[must_use]
    pub fn put_object<'obj, 'client>(
        &'client self,
        object: &'obj str,
    ) -> PutObject<'obj>
    where
        'client: 'obj,
    {
        PutObject::new(&self.bucket, Some(&self.credentials), object)
    }

    #[must_use]
    pub fn get_object<'obj, 'client>(
        &'client self,
        object: &'obj str,
    ) -> GetObject<'obj>
    where
        'client: 'obj,
    {
        GetObject::new(&self.bucket, Some(&self.credentials), object)
    }
}
