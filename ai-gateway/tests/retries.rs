use std::collections::HashMap;

use ai_gateway::{
    config::{
        Config,
        balance::BalanceConfig,
        helicone::HeliconeFeatures,
        retry::RetryConfig,
        router::{RouterConfig, RouterConfigs},
    },
    tests::{TestDefault, harness::Harness, mock::MockArgs},
    types::router::RouterId,
};
use compact_str::CompactString;
use http::{Method, Request, StatusCode};
use http_body_util::BodyExt;
use serde_json::json;
use tower::Service;

#[tokio::test]
#[serial_test::serial(default_mock)]
async fn unified_api() {
    let mut config = Config::test_default();
    // Disable auth for this test since we're testing basic passthrough
    // functionality
    config.helicone.features = HeliconeFeatures::All;
    config.global.retries = Some(RetryConfig::test_default());

    let mock_args = MockArgs::builder()
        .stubs(HashMap::from([
            ("internal_error:openai:chat_completion", 3.into()),
            ("success:minio:upload_request", 1.into()),
            ("success:jawn:log_request", 1.into()),
            ("success:jawn:sign_s3_url", 1.into()),
        ]))
        .build();

    let mut harness = Harness::builder()
        .with_config(config)
        .with_mock_args(mock_args)
        .with_mock_auth()
        .build()
        .await;

    let request_body = axum_core::body::Body::from(
        serde_json::to_vec(&json!({
            "model": "openai/gpt-4o-mini",
            "messages": [
                {
                    "role": "user",
                    "content": "Hello, world!"
                }
            ]
        }))
        .unwrap(),
    );

    let request = Request::builder()
        .method(Method::POST)
        // Route to the fake endpoint through the default router
        .uri("http://router.helicone.com/ai/chat/completions")
        .header("content-type", "application/json")
        .header("authorization", "Bearer sk-helicone-test-key")
        .body(request_body)
        .unwrap();

    let response = harness.call(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let _response_body = response.into_body().collect().await.unwrap();

    // sleep so that the background task for logging can complete
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
}

#[tokio::test]
#[serial_test::serial(default_mock)]
async fn router() {
    let mut config = Config::test_default();
    // Disable auth for this test since we're testing basic passthrough
    // functionality
    config.helicone.features = HeliconeFeatures::All;
    let router_configs = RouterConfigs::new(HashMap::from([(
        RouterId::Named(CompactString::new("my-router")),
        RouterConfig {
            load_balance: BalanceConfig::openai_chat(),
            retries: Some(RetryConfig::test_default()),
            ..Default::default()
        },
    )]));
    config.routers = router_configs;

    let mock_args = MockArgs::builder()
        .stubs(HashMap::from([
            ("internal_error:openai:chat_completion", 3.into()),
            ("success:minio:upload_request", 1.into()),
            ("success:jawn:log_request", 1.into()),
            ("success:jawn:sign_s3_url", 1.into()),
        ]))
        .build();

    let mut harness = Harness::builder()
        .with_config(config)
        .with_mock_args(mock_args)
        .with_mock_auth()
        .build()
        .await;

    let request_body = axum_core::body::Body::from(
        serde_json::to_vec(&json!({
            "model": "openai/gpt-4o-mini",
            "messages": [
                {
                    "role": "user",
                    "content": "Hello, world!"
                }
            ]
        }))
        .unwrap(),
    );

    let request = Request::builder()
        .method(Method::POST)
        // Route to the fake endpoint through the default router
        .uri("http://router.helicone.com/router/my-router/chat/completions")
        .header("content-type", "application/json")
        .header("authorization", "Bearer sk-helicone-test-key")
        .body(request_body)
        .unwrap();

    let response = harness.call(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::INTERNAL_SERVER_ERROR);
    let _response_body = response.into_body().collect().await.unwrap();

    // sleep so that the background task for logging can complete
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
}
