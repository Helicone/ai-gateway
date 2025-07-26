use std::time::Duration;

use serde::{Deserialize, Serialize};
use strum::IntoStaticStr;

#[derive(
    Debug,
    Default,
    Clone,
    PartialEq,
    Eq,
    Deserialize,
    Serialize,
    IntoStaticStr,
    Hash,
)]
#[serde(deny_unknown_fields, tag = "type", rename_all = "kebab-case")]
pub enum DeploymentTarget {
    Cloud {
        #[serde(
            with = "humantime_serde",
            default = "default_db_poll_interval",
            rename = "db-poll-interval"
        )]
        db_poll_interval: Duration,
        #[serde(
            with = "humantime_serde",
            default = "default_listener_reconnect_interval",
            rename = "listener-reconnect-interval"
        )]
        listener_reconnect_interval: Duration,
    },
    #[default]
    #[serde(untagged)]
    Sidecar,
}

impl DeploymentTarget {
    #[must_use]
    pub fn is_cloud(&self) -> bool {
        matches!(self, DeploymentTarget::Cloud { .. })
    }

    #[must_use]
    pub fn is_sidecar(&self) -> bool {
        matches!(self, DeploymentTarget::Sidecar)
    }
}

fn default_db_poll_interval() -> Duration {
    Duration::from_secs(30)
}

fn default_listener_reconnect_interval() -> Duration {
    // 5 minutes
    Duration::from_secs(300)
}
