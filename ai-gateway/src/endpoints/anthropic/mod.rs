pub mod messages;

use super::{Endpoint, EndpointType};
pub use crate::endpoints::anthropic::messages::Messages;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, strum::EnumIter)]
pub enum Anthropic {
    Messages(Messages),
}

impl Anthropic {
    #[must_use]
    pub fn path(&self) -> &str {
        match self {
            Self::Messages(_) => Messages::PATH,
        }
    }

    #[must_use]
    pub fn messages() -> Self {
        Self::Messages(Messages)
    }

    #[must_use]
    pub fn endpoint_type(&self) -> EndpointType {
        match self {
            Self::Messages(_) => EndpointType::Chat,
        }
    }
}
