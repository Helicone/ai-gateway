use derive_more::{AsRef, From};
use serde::{Deserialize, Serialize};
use ts_rs::TS;
use uuid::Uuid;

use crate::error::auth::AuthError;

#[derive(
    TS,
    Debug,
    AsRef,
    Copy,
    Clone,
    Eq,
    PartialEq,
    Hash,
    Serialize,
    Deserialize,
    From,
)]
#[ts(export)]
pub struct UserId(Uuid);

impl UserId {
    #[must_use]
    pub fn new(id: Uuid) -> Self {
        Self(id)
    }
}

impl std::fmt::Display for UserId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl TryFrom<&str> for UserId {
    type Error = AuthError;
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        Ok(UserId::new(
            Uuid::parse_str(value)
                .map_err(|_| AuthError::InvalidCredentials)?,
        ))
    }
}
