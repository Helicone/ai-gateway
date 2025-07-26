use serde::{Deserialize, Serialize};
use url::Url;

use crate::types::secret::Secret;

/// The request url format of a S3 bucket.
#[derive(
    Default, Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash,
)]
#[serde(rename_all = "kebab-case")]
pub enum UrlStyle {
    /// Requests will use "path-style" url: i.e:
    /// `https://s3.<region>.amazonaws.com/<bucket>/<key>`.
    ///
    /// This style should be considered deprecated and is **NOT RECOMMENDED**.
    /// Check [Amazon S3 Path Deprecation Plan](https://aws.amazon.com/blogs/aws/amazon-s3-path-deprecation-plan-the-rest-of-the-story/)
    /// for more informations.
    #[default]
    Path,
    /// Requests will use "virtual-hosted-style" urls, i.e:
    /// `https://<bucket>.s3.<region>.amazonaws.com/<key>`.
    VirtualHost,
}

impl From<UrlStyle> for rusty_s3::UrlStyle {
    fn from(value: UrlStyle) -> Self {
        match value {
            UrlStyle::Path => rusty_s3::UrlStyle::Path,
            UrlStyle::VirtualHost => rusty_s3::UrlStyle::VirtualHost,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq, Hash)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Config {
    #[serde(default)]
    pub url_style: UrlStyle,
    #[serde(default = "default_bucket_name")]
    pub bucket_name: String,
    #[serde(default = "default_host")]
    pub host: Url,
    #[serde(default = "default_region")]
    pub region: String,
    /// set via env vars: `AI_GATEWAY__MINIO__ACCESS_KEY`
    ///
    /// Only required if running with `DeploymentTarget::Cloud`.
    #[serde(default = "default_access_key")]
    pub access_key: Secret<String>,
    /// set via env vars: `AI_GATEWAY__MINIO__SECRET_KEY`
    ///
    /// Only required if running with `DeploymentTarget::Cloud`.
    #[serde(default = "default_secret_key")]
    pub secret_key: Secret<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            url_style: UrlStyle::default(),
            bucket_name: default_bucket_name(),
            host: default_host(),
            region: default_region(),
            access_key: default_access_key(),
            secret_key: default_secret_key(),
        }
    }
}

fn default_bucket_name() -> String {
    "request-response-storage".to_string()
}

fn default_host() -> Url {
    Url::parse("http://localhost:9000").unwrap()
}

fn default_region() -> String {
    "us-east-1".to_string()
}

fn default_access_key() -> Secret<String> {
    Secret::from("minioadmin".to_string())
}

fn default_secret_key() -> Secret<String> {
    Secret::from("minioadmin".to_string())
}

#[cfg(feature = "testing")]
impl crate::tests::TestDefault for Config {
    fn test_default() -> Self {
        Self {
            host: Url::parse("http://localhost:9190").unwrap(),
            ..Self::default()
        }
    }
}
