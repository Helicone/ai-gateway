use std::collections::HashMap;

use ai_gateway::{
    config::{Config, helicone::HeliconeFeatures},
    tests::{TestDefault, harness::Harness, mock::MockArgs},
};
use http::{Method, Request, StatusCode};
use serde_json::json;
use tower::Service;

/// Sending a request to https://localhost/nebius should
/// result in the proxied request targeting the Nebius mock server
#[tokio::test]
#[serial_test::serial(nebius_mock)]
async fn nebius_direct_proxy() {
    let mut config = Config::test_default();
    // Disable auth for this test since we're testing basic provider
    // functionality
    config.helicone.features = HeliconeFeatures::None;
    let mock_args = MockArgs::builder()
        .stubs(HashMap::from([
            ("success:nebius:fake_endpoint", 1.into()),
            ("success:minio:upload_request", 0.into()),
            ("success:jawn:log_request", 0.into()),
        ]))
        .build();
    let mut harness = Harness::builder()
        .with_config(config)
        .with_mock_args(mock_args)
        .build()
        .await;
    let request_body = axum_core::body::Body::from(
        serde_json::to_vec(&json!({
            "test": "data"
        }))
        .unwrap(),
    );

    let request = Request::builder()
        .method(Method::POST)
        .uri("http://router.helicone.com/nebius/v1/fake_endpoint")
        .header("Content-Type", "application/json")
        .header("Authorization", "Bearer test-token")
        .body(request_body)
        .unwrap();

    let response = harness.call(request).await.unwrap();
    assert_eq!(response.status(), StatusCode::OK);
}
