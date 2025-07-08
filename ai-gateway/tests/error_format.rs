use std::collections::{HashMap, HashSet};

use ai_gateway::{
    config::{Config, helicone::HeliconeFeatures},
    tests::{TestDefault, harness::Harness, mock::MockArgs},
};
use http::{Method, Request, StatusCode};
use http_body_util::BodyExt;
use tower::Service;

#[tokio::test]
#[serial_test::serial]
async fn unauthorized() {
    let mut config = Config::test_default();
    config.helicone.features = HashSet::from_iter([HeliconeFeatures::Auth]);

    let mock_args = MockArgs::builder()
        .stubs(HashMap::from([
            ("success:openai:chat_completion", 0.into()),
            ("success:anthropic:messages", 0.into()),
            ("success:minio:upload_request", 0.into()),
            ("success:jawn:log_request", 0.into()),
        ]))
        .build();
    let mut harness = Harness::builder()
        .with_config(config)
        .with_mock_args(mock_args)
        .build()
        .await;

    let request = Request::builder()
        .method(Method::POST)
        .uri("http://router.helicone.com/ai/chat/completions")
        .body(axum_core::body::Body::empty())
        .unwrap();

    let response = harness.call(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    let response_body = response.into_body().collect().await.unwrap();
    let response_body = serde_json::from_slice::<
        async_openai::error::WrappedError,
    >(&response_body.to_bytes());
    assert!(
        response_body.is_ok(),
        "should be able to deserialize error json into openai error format"
    );
    let response_body = response_body.unwrap();
    assert_eq!(
        response_body.error.r#type,
        Some("invalid_request_error".to_string())
    );
    assert_eq!(
        response_body.error.code,
        Some("invalid_api_key".to_string())
    );
}

#[tokio::test]
#[serial_test::serial]
async fn invalid_request_body() {
    let mut config = Config::test_default();
    config.helicone.features = HashSet::from_iter([HeliconeFeatures::None]);

    let mock_args = MockArgs::builder()
        .stubs(HashMap::from([
            ("success:openai:chat_completion", 0.into()),
            ("success:anthropic:messages", 0.into()),
            ("success:minio:upload_request", 0.into()),
            ("success:jawn:log_request", 0.into()),
        ]))
        .build();
    let mut harness = Harness::builder()
        .with_config(config)
        .with_mock_args(mock_args)
        .build()
        .await;

    let request = Request::builder()
        .method(Method::POST)
        .uri("http://router.helicone.com/ai/chat/completions")
        .body(axum_core::body::Body::empty())
        .unwrap();

    let response = harness.call(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    let response_body = response.into_body().collect().await.unwrap();
    let response_body = serde_json::from_slice::<
        async_openai::error::WrappedError,
    >(&response_body.to_bytes());
    assert!(
        response_body.is_ok(),
        "should be able to deserialize error json into openai error format"
    );
    let response_body = response_body.unwrap();
    assert_eq!(
        response_body.error.r#type,
        Some("invalid_request_error".to_string())
    );
    assert_eq!(response_body.error.code, None);
}
