use displaydoc::Display;
use thiserror::Error;

use crate::types::{provider::InferenceProvider, router::RouterId};

#[derive(Debug, Error, Display)]
pub enum ProviderError {
    /// Provider not set in global providers config: {0}
    ProviderNotConfigured(InferenceProvider),
    /// Provider keys not found for router: {0}
    ProviderKeysNotFound(RouterId),
    /// Invalid provider name: {0}
    InvalidProviderName(String),
}
