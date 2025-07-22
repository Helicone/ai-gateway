use std::{sync::Arc, time::Duration};

use backon::{BackoffBuilder, ConstantBuilder, ExponentialBuilder, Retryable};
use futures::{
    SinkExt, StreamExt,
    future::BoxFuture,
    stream::{SplitSink, SplitStream},
};
use meltdown::Token;
use rust_decimal::prelude::ToPrimitive;
use tokio::{net::TcpStream, sync::RwLock};
use tokio_tungstenite::{
    MaybeTlsStream, WebSocketStream, connect_async,
    tungstenite::{
        self, Message, client::IntoClientRequest, handshake::client::Request,
    },
};
use tracing::{debug, error};

use super::{
    control_plane_state::StateWithMetadata,
    types::{MessageTypeRX, MessageTypeTX},
};
use crate::{
    app_state::AppState,
    config::{
        control_plane::ControlPlaneConfig, helicone::HeliconeConfig,
        retry::RetryConfig,
    },
    error::{init::InitError, runtime::RuntimeError},
};
type TlsWebSocketStream = WebSocketStream<MaybeTlsStream<TcpStream>>;

#[derive(Debug)]
pub struct WebsocketChannel {
    msg_tx: SplitSink<TlsWebSocketStream, Message>,
    msg_rx: SplitStream<TlsWebSocketStream>,
}

#[derive(Debug)]
pub struct ControlPlaneClient {
    pub state: Arc<RwLock<StateWithMetadata>>,
    channel: WebsocketChannel,
    /// Config about Control plane, such as the websocket url,
    /// reconnect interval/backoff policy, heartbeat interval, etc.
    config: HeliconeConfig,
    retry_config: RetryConfig,
    app_state: AppState,
}

async fn handle_message(
    app_state: &AppState,
    state: &Arc<RwLock<StateWithMetadata>>,
    message: Message,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let bytes = message.into_data();
    let m: MessageTypeRX = serde_json::from_slice(&bytes)?;
    tracing::debug!("received websocket message");
    let mut state_guard = state.write().await;
    state_guard.update(m, app_state);

    Ok(())
}

impl IntoClientRequest for &HeliconeConfig {
    fn into_client_request(
        self,
    ) -> Result<Request, tokio_tungstenite::tungstenite::Error> {
        let host = self.websocket_url.authority();
        Request::builder()
            .uri(self.websocket_url.as_str())
            .header("Host", host)
            .header(
                "Authorization",
                format!("Bearer {}", self.api_key.expose()),
            )
            .header("Connection", "Upgrade")
            .header("Upgrade", "websocket")
            .header("Sec-WebSocket-Version", "13")
            .header(
                "Sec-WebSocket-Key",
                tokio_tungstenite::tungstenite::handshake::client::generate_key(
                ),
            )
            .body(())
            .map_err(|_| {
                tokio_tungstenite::tungstenite::Error::Url(
                    tungstenite::error::UrlError::UnsupportedUrlScheme,
                )
            })
    }
}

async fn connect_async_and_split(
    helicone_config: &HeliconeConfig,
) -> Result<WebsocketChannel, InitError> {
    let (tx, rx) = connect_async(helicone_config)
        .await
        .map_err(|e| InitError::WebsocketConnection(Box::new(e)))?
        .0
        .split();

    Ok(WebsocketChannel {
        msg_tx: tx,
        msg_rx: rx,
    })
}

async fn connect_with_retry(
    helicone_config: &HeliconeConfig,
    retry_config: &RetryConfig,
) -> Result<WebsocketChannel, InitError> {
    match retry_config {
        RetryConfig::Exponential {
            min_delay,
            max_delay,
            max_retries,
            factor,
        } => {
            let retry_strategy = ExponentialBuilder::default()
                .with_max_delay(*max_delay)
                .with_min_delay(*min_delay)
                .with_max_times(usize::from(*max_retries))
                .with_factor(
                    factor
                        .to_f32()
                        .unwrap_or(crate::config::retry::DEFAULT_RETRY_FACTOR),
                )
                .with_jitter()
                .build();
            (|| async { connect_async_and_split(helicone_config).await })
            .retry(retry_strategy)
            .sleep(tokio::time::sleep)
            .when(|e: &InitError| {
                matches!(e, InitError::WebsocketConnection(_))
            })
            .notify(|err: &InitError, dur: Duration| {
                if let InitError::WebsocketConnection(_) = err {
                    tracing::warn!(
                        error = %err,
                        "Failed to connect to control plane, retrying in {} seconds...",
                        dur.as_secs()
                    );
                }
            }).await
        }
        RetryConfig::Constant { delay, max_retries } => {
            let retry_strategy = ConstantBuilder::default()
                .with_max_times(usize::from(*max_retries))
                .with_delay(*delay)
                .build();
            (|| async { connect_async_and_split(helicone_config).await })
            .retry(retry_strategy)
            .sleep(tokio::time::sleep)
            .when(|e: &InitError| {
                matches!(e, InitError::WebsocketConnection(_))
            })
            .notify(|err: &InitError, dur: Duration| {
                if let InitError::WebsocketConnection(_) = err {
                    tracing::warn!(
                        error = %err,
                        "Failed to connect to control plane, retrying in {} seconds...",
                        dur.as_secs()
                    );
                }
            }).await
        }
    }
}

impl ControlPlaneClient {
    async fn reconnect_websocket(&mut self) -> Result<(), InitError> {
        let channel =
            connect_with_retry(&self.config, &self.retry_config).await?;
        self.channel = channel;
        tracing::info!("Successfully reconnected to control plane");
        Ok(())
    }

    pub async fn connect(
        control_plane_state: Arc<RwLock<StateWithMetadata>>,
        config: HeliconeConfig,
        control_plane_config: ControlPlaneConfig,
        app_state: AppState,
    ) -> Result<Self, InitError> {
        let channel =
            connect_with_retry(&config, &control_plane_config.retry).await?;
        Ok(Self {
            channel,
            config,
            state: control_plane_state,
            retry_config: control_plane_config.retry,
            app_state,
        })
    }

    pub async fn send_message(
        &mut self,
        m: MessageTypeTX,
    ) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let bytes = serde_json::to_vec(&m)?;
        let message = Message::Binary(bytes.into());

        match self.channel.msg_tx.send(message).await {
            Ok(()) => (),
            Err(tungstenite::Error::AlreadyClosed) => {
                tracing::error!("websocket connection closed, reconnecting...");
                self.reconnect_websocket().await?;
            }
            Err(e) => {
                tracing::error!(error = %e, "websocket error");
            }
        }

        Ok(())
    }

    async fn run_control_plane_forever(mut self) -> Result<(), RuntimeError> {
        let state_clone = Arc::clone(&self.state);
        let mut backoff = self.retry_config.as_iterator();
        loop {
            while let Some(message) = self.channel.msg_rx.next().await {
                match message {
                    Ok(message) => {
                        let _ = handle_message(&self.app_state, &state_clone, message)
                            .await
                            .inspect_err(|e| {
                                tracing::error!(error = ?e, "error handling websocket message");
                            });
                    }
                    Err(tungstenite::Error::AlreadyClosed) => {
                        tracing::error!(
                            "websocket connection closed, reconnecting..."
                        );
                        self.reconnect_websocket()
                            .await
                            .map_err(RuntimeError::Init)?;
                    }
                    Err(e) => {
                        tracing::error!(error = ?e, "websocket error");
                    }
                }
            }

            // if the connection is closed, we need to reconnect
            let sleep_duration =
                backoff.next().unwrap_or(Duration::from_secs(20));
            tracing::info!(
                "control plane client reconnecting in {} seconds",
                sleep_duration.as_secs()
            );
            tokio::time::sleep(sleep_duration).await;
            self.reconnect_websocket()
                .await
                .map_err(RuntimeError::Init)?;
        }
    }
}

impl meltdown::Service for ControlPlaneClient {
    type Future = BoxFuture<'static, Result<(), RuntimeError>>;

    fn run(self, mut token: Token) -> Self::Future {
        Box::pin(async move {
            tokio::select! {
                result = self.run_control_plane_forever() => {
                    if let Err(e) = result {
                        error!(name = "control-plane-client-task", error = ?e, "Monitor encountered error, shutting down");
                    } else {
                        debug!(name = "control-plane-client-task", "Monitor shut down successfully");
                    }
                    token.trigger();
                }
                () = &mut token => {
                    debug!(name = "control-plane-client-task", "task shut down successfully");
                }
            }
            Ok(())
        })
    }
}
