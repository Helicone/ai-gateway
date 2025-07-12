use std::time::Duration;

use ai_gateway::control_plane::types::{MessageTypeRX, Update};
use axum::{
    Json,
    extract::{
        State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::StatusCode,
    response::IntoResponse,
};
use futures::{SinkExt, StreamExt};

use crate::AppState;

pub(crate) async fn log_request(
    State(state): State<AppState>,
) -> impl IntoResponse {
    if state.jawn_latency > 0 {
        tokio::time::sleep(Duration::from_millis(state.jawn_latency.into()))
            .await;
    }
    StatusCode::OK
}

pub(crate) async fn sign_s3_url(
    State(state): State<AppState>,
) -> impl IntoResponse {
    if state.jawn_latency > 0 {
        crate::routes::sleep(state.jawn_latency).await;
    }
    let minio_base_url = format!("http://{}:{}", state.address, state.port);
    let presigned_url = format!(
        "{minio_base_url}/request-response-storage/organizations/\
         c3bc2b69-c55c-4dfc-8a29-47db1245ee7c/requests/\
         a41cbcd7-5e9e-4104-b29b-2ef4473d71a7/raw_request_response_body"
    );
    let response = serde_json::json!({
        "data": {
            "url": presigned_url
        }
    });
    Json(response)
}

pub(crate) async fn websocket_handler(
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(|socket| websocket(socket))
}

// This function deals with a single websocket connection, i.e., a single
// connected client / user, for which we will spawn two independent tasks (for
// receiving / sending chat messages).
async fn websocket(stream: WebSocket) {
    // By splitting, we can send and receive at the same time.
    let (mut sender, mut receiver) = stream.split();

    let mock_auth = mock_auth();
    let update_msg = MessageTypeRX::Update(Update::Config { data: mock_auth });
    let body = serde_json::to_vec(&update_msg).unwrap();
    let message = Message::Binary(body.into());
    sender.send(message).await.unwrap();

    // Loop until a text message is found.
    while let Some(Ok(message)) = receiver.next().await {
        match message {
            Message::Text(utf8_bytes) => {
                tracing::info!("Received text message: {}", utf8_bytes);
            }
            Message::Binary(bytes) => {
                tracing::info!("Received binary message: {:?}", bytes);
            }
            Message::Ping(bytes) => {
                tracing::info!("Received ping message: {:?}", bytes);
            }
            Message::Pong(bytes) => {
                tracing::info!("Received pong message: {:?}", bytes);
            }
            Message::Close(close_frame) => {
                tracing::info!("Received close message: {:?}", close_frame);
            }
        }
    }
}

fn mock_auth() -> ai_gateway::control_plane::types::Config {
    let test_key = "sk-helicone-test-key";
    let key_hash = ai_gateway::control_plane::types::hash_key(test_key);
    let organization_id = "c3bc2b69-c55c-4dfc-8a29-47db1245ee7c".to_string();
    let user_id = "a41cbcd7-5e9e-4104-b29b-2ef4473d71a7".to_string();
    ai_gateway::control_plane::types::Config {
        auth: ai_gateway::control_plane::types::AuthData {
            user_id: user_id.clone(),
            organization_id,
        },
        keys: vec![ai_gateway::control_plane::types::Key {
            key_hash: key_hash,
            owner_id: user_id,
            organization_id,
        }],
        router_id: "my-router".to_string(),
        router_config: "{}".to_string(),
    }
}
