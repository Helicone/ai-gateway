use chrono::{DateTime, Utc};

use super::types::{ControlPlaneError, MessageTypeRX, Update};
use crate::control_plane::types::ControlPlaneState;
const MAX_HISTORY_SIZE: usize = 100;

#[derive(Debug, Default)]
pub struct StateWithMetadata {
    pub last_heartbeat: Option<DateTime<Utc>>,
    pub state: Option<ControlPlaneState>,
    // used mainly for debugging and testing, can remove later
    pub history: Vec<MessageTypeRX>,
}

impl StateWithMetadata {
    #[must_use]
    pub fn new() -> Self {
        Self {
            last_heartbeat: None,
            state: None,
            history: Vec::new(),
        }
    }

    pub fn update(&mut self, m: MessageTypeRX) {
        self.history.push(m.clone());
        if self.history.len() > MAX_HISTORY_SIZE {
            self.history.remove(0);
        }

        match m {
            MessageTypeRX::Update(Update::Keys { data }) => {
                if let Some(state) = self.state.as_mut() {
                    state.keys = data;
                }
            }
            MessageTypeRX::Update(Update::Config { data }) => {
                self.state.replace(data);
            }
            MessageTypeRX::Error(ControlPlaneError::Unauthorized {
                message,
            }) => {
                tracing::error!(
                    message = %message,
                    "Received unauthorized error from control plane",
                );
            }
        }
    }
}
