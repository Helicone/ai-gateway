use chrono::{DateTime, Utc};

use super::types::{ControlPlaneError, MessageTypeRX, Update};
use crate::{app_state::AppState, control_plane::types::ControlPlaneState};
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

    pub fn update(&mut self, m: MessageTypeRX, app_state: &AppState) {
        self.history.push(m.clone());
        if self.history.len() > MAX_HISTORY_SIZE {
            self.history.remove(0);
        }

        match m {
            MessageTypeRX::Update(Update::Keys { data }) => {
                if let Some(state) = self.state.as_mut() {
                    let old_len =
                        i64::try_from(state.keys.len()).unwrap_or(i64::MAX);
                    app_state
                        .0
                        .metrics
                        .routers
                        .helicone_api_keys
                        .add(-old_len, &[]);
                    let new_len = i64::try_from(data.len()).unwrap_or(i64::MAX);
                    app_state
                        .0
                        .metrics
                        .routers
                        .helicone_api_keys
                        .add(new_len, &[]);
                    state.keys = data;
                }
            }
            MessageTypeRX::Update(Update::Config { data }) => {
                let state = &self.state;
                let old_len = if let Some(state) = state {
                    i64::try_from(state.keys.len()).unwrap_or(i64::MAX)
                } else {
                    0
                };
                app_state
                    .0
                    .metrics
                    .routers
                    .helicone_api_keys
                    .add(-old_len, &[]);
                let new_len =
                    i64::try_from(data.keys.len()).unwrap_or(i64::MAX);
                app_state
                    .0
                    .metrics
                    .routers
                    .helicone_api_keys
                    .add(new_len, &[]);
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
