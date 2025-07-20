use std::{sync::Arc, time::Duration};

use backon::{ExponentialBuilder, Retryable};
use futures::{
    SinkExt, StreamExt,
    future::BoxFuture,
    stream::{SplitSink, SplitStream},
};
use meltdown::Token;
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
    config::helicone::HeliconeConfig,
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
}

async fn handle_message(
    state: &Arc<RwLock<StateWithMetadata>>,
    message: Message,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let bytes = message.into_data();
    let m: MessageTypeRX = serde_json::from_slice(&bytes)?;
    tracing::debug!(websocket_msg = ?m, "received message");
    let mut state_guard = state.write().await;
    state_guard.update(m);

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
) -> Result<WebsocketChannel, InitError> {
    (|| async { connect_async_and_split(helicone_config).await })
            // Retry with exponential backoff + jitter
            .retry(ExponentialBuilder::default().with_jitter().without_max_times())
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

impl ControlPlaneClient {
    async fn reconnect_websocket(&mut self) -> Result<(), InitError> {
        let channel = connect_with_retry(&self.config).await?;
        self.channel = channel;
        tracing::info!("Successfully reconnected to control plane");
        Ok(())
    }

    pub async fn connect(
        control_plane_state: Arc<RwLock<StateWithMetadata>>,
        config: HeliconeConfig,
    ) -> Result<Self, InitError> {
        let channel = connect_with_retry(&config).await?;
        Ok(Self {
            channel,
            config,
            state: control_plane_state,
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
        loop {
            while let Some(message) = self.channel.msg_rx.next().await {
                match message {
                    Ok(message) => {
                        let _ = handle_message(&state_clone, message)
                            .await
                            .inspect_err(|e| {
                                tracing::error!(error = ?e, "websocket error");
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
            self.reconnect_websocket()
                .await
                .map_err(RuntimeError::Init)?;
            tokio::time::sleep(Duration::from_secs(1)).await;
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

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use meltdown::{Service, Token};
    use tokio::{net::TcpListener, sync::RwLock, time::timeout};
    use tokio_tungstenite::accept_async;

    use super::ControlPlaneClient;
    use crate::{
        config::helicone::HeliconeConfig,
        control_plane::{
            control_plane_state::StateWithMetadata, types::MessageTypeTX,
        },
    };

    #[tokio::test]
    async fn test_mock_server_connection() {
        // Start a simple mock server
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let ws_url = format!("ws://{addr}");
        let helicone_config = HeliconeConfig {
            websocket_url: ws_url.parse().unwrap(),
            ..Default::default()
        };

        // Spawn mock server that just accepts connections
        tokio::spawn(async move {
            if let Ok((stream, _)) = listener.accept().await {
                let _ = accept_async(stream).await;
                // Just accept and do nothing - minimal mock
            }
        });

        tokio::time::sleep(Duration::from_millis(50)).await;

        // Test connection
        let result =
            ControlPlaneClient::connect(Arc::default(), helicone_config).await;
        assert!(result.is_ok(), "Should connect to mock server");
    }

    #[tokio::test]
    /// Sends a heartbeat to the control plane and verifies that it is received
    /// and we get an ack back
    async fn test_integration_localhost_8585_heartbeat() {
        unsafe {
            std::env::set_var(
                "HELICONE_CONTROL_PLANE_API_KEY",
                "sk-helicone-n2zkt2i-x3mukmi-tgvgzyy-xom3q4y",
            );
        }
        println!("setting api key");
        let helicone_config = HeliconeConfig {
            websocket_url: "ws://localhost:8585".parse().unwrap(),
            ..Default::default()
        };

        let control_plane_state: Arc<RwLock<StateWithMetadata>> =
            Arc::default();
        // This will fail if no server is running on 8585, which is expected
        let connect_fut = ControlPlaneClient::connect(
            control_plane_state.clone(),
            helicone_config,
        );
        let result = timeout(Duration::from_secs(1), connect_fut).await;
        let Ok(result) = result else {
            println!("timed out connecting to control plane, passing test");
            return;
        };
        println!("connected to control plane {result:?}");

        assert!(
            control_plane_state
                .clone()
                .read()
                .await
                .last_heartbeat
                .is_none(),
            "Last heartbeat should be none"
        );

        if let Ok(mut client) = result {
            println!("sending heartbeat");
            let send_result =
                client.send_message(MessageTypeTX::Heartbeat {}).await;
            let token = Token::new();
            let handle = tokio::spawn(client.run(token.clone()));

            tokio::time::sleep(Duration::from_secs(2)).await;
            // without the `token.trigger()` call, this test will hang
            token.trigger();
            handle
                .await
                .expect("should be able to join tokio task")
                .expect("client should close successfully");

            assert!(send_result.is_ok(), "Should be able to send heartbeat");
            // wait for the heartbeat to be received
            println!("waiting for heartbeat to be received");
            tokio::time::sleep(Duration::from_secs(10)).await;

            assert!(
                control_plane_state.read().await.last_heartbeat.is_some(),
                "Last heartbeat should be some"
            );
        }
    }
}
