use serde::{Deserialize, Serialize};

pub(crate) const MAX_BUCKET_SIZE: u8 = 10;
pub(crate) const DEFAULT_BUCKETS: u8 = 1;

#[derive(
    Debug, Default, Clone, Deserialize, Serialize, Eq, PartialEq, Hash,
)]
#[serde(default, rename_all = "kebab-case")]
pub struct CacheConfig {
    /// Cache-control header: <https://developer.mozilla.org/en-US/docs/Web/HTTP/Reference/Headers/Cache-Control>
    #[serde(skip_serializing_if = "Option::is_none")]
    pub directive: Option<String>,
    #[serde(default = "default_buckets")]
    pub buckets: u8,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seed: Option<String>,
}

#[cfg(feature = "testing")]
impl crate::tests::TestDefault for CacheConfig {
    fn test_default() -> Self {
        Self {
            directive: None,
            buckets: DEFAULT_BUCKETS,
            seed: None,
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Eq, PartialEq, Hash)]
#[serde(rename_all = "kebab-case", tag = "type")]
pub enum CacheStore {
    Redis {
        #[serde(rename = "host-url", default = "default_host_url")]
        host_url: String,
    },
    InMemory {
        // apparently container-level `rename_all` for enums doesn't
        // apply to the fields of the enum, so we need to rename the field
        // manually
        #[serde(rename = "max-size", default = "default_max_size")]
        max_size: usize,
    },
}

impl Default for CacheStore {
    fn default() -> Self {
        Self::InMemory {
            max_size: default_max_size(),
        }
    }
}

fn default_max_size() -> usize {
    // 256MB
    1024 * 1024 * 256
}

fn default_buckets() -> u8 {
    1
}

fn default_host_url() -> String {
    "redis://localhost:6379".to_string()
}
