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
            default = "default_db_poll_interval"
        )]
        #[serde(rename = "db-poll-interval")]
        db_poll_interval: Duration,
    },
    #[default]
    #[serde(untagged)]
    Sidecar,
}

impl DeploymentTarget {
    pub fn is_cloud(&self) -> bool {
        matches!(self, DeploymentTarget::Cloud { .. })
    }

    pub fn is_sidecar(&self) -> bool {
        matches!(self, DeploymentTarget::Sidecar)
    }
}

fn default_db_poll_interval() -> Duration {
    Duration::from_secs(30)
}
