use std::{collections::HashMap, str::FromStr};

use ai_gateway::{
    config::{
        Config,
        balance::{
            BalanceConfig, BalanceConfigInner, WeightedModel, WeightedProvider,
        },
        helicone::HeliconeFeatures,
        router::{RouterConfig, RouterConfigs},
    },
    endpoints::EndpointType,
    tests::{TestDefault, harness::Harness, mock::MockArgs},
    types::{model_id::ModelId, provider::InferenceProvider, router::RouterId},
};
use compact_str::CompactString;
use http::{Method, Request, StatusCode};
use http_body_util::BodyExt;
use nonempty_collections::nes;
use rust_decimal::Decimal;
use serde_json::json;
use tower::Service;

#[tokio::test]
#[serial_test::serial]
async fn weighted_balancer_anthropic_preferred() {
    let mut config = Config::test_default();
    // Disable auth for this test since we're not testing authentication
    config.helicone.features = HeliconeFeatures::None;
    let balance_config = BalanceConfig::from(HashMap::from([(
        EndpointType::Chat,
        BalanceConfigInner::ProviderWeighted {
            providers: nes![
                WeightedProvider {
                    provider: InferenceProvider::OpenAI,
                    weight: Decimal::try_from(0.25).unwrap(),
                },
                WeightedProvider {
                    provider: InferenceProvider::Anthropic,
                    weight: Decimal::try_from(0.75).unwrap(),
                },
            ],
        },
    )]));
    config.routers = RouterConfigs::new(HashMap::from([(
        RouterId::Named(CompactString::new("my-router")),
        RouterConfig {
            load_balance: balance_config,
            ..Default::default()
        },
    )]));
    // Determine dynamic expected ranges based on 100 total requests and a ±15%
    // tolerance
    let num_requests = 100;
    let tolerance = num_requests as f64 * 0.15;
    let expected_openai_midpt = num_requests as f64 * 0.25;
    let expected_anthropic_midpt = num_requests as f64 * 0.75;
    let openai_range = (expected_openai_midpt - tolerance).floor() as u64
        ..(expected_openai_midpt + tolerance).ceil() as u64;
    let anthropic_range = (expected_anthropic_midpt - tolerance).floor() as u64
        ..(expected_anthropic_midpt + tolerance).ceil() as u64;
    let mock_args = MockArgs::builder()
        .stubs(HashMap::from([
            (
                "success:openai:chat_completion",
                openai_range.clone().into(),
            ),
            ("success:anthropic:messages", anthropic_range.clone().into()),
            // When auth is disabled, logging services should not be called
            ("success:minio:upload_request", 0.into()),
            ("success:jawn:log_request", 0.into()),
        ]))
        .build();
    let mut harness = Harness::builder()
        .with_config(config)
        .with_mock_args(mock_args)
        .build()
        .await;

    let body_bytes = serde_json::to_vec(&json!({
        "model": "openai/gpt-4o-mini",
        "messages": [
            {
                "role": "user",
                "content": "Hello, world!"
            }
        ]
    }))
    .unwrap();

    for _ in 0..num_requests {
        let request_body = axum_core::body::Body::from(body_bytes.clone());
        let request = Request::builder()
            .method(Method::POST)
            // default router
            .uri("http://router.helicone.com/router/my-router/chat/completions")
            .body(request_body)
            .unwrap();
        let response = harness.call(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        // we need to collect the body here in order to poll the underlying body
        // so that the async logging task can complete
        let _response_body = response.into_body().collect().await.unwrap();
    }

    // sleep so that the background task for logging can complete
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
}

#[tokio::test]
#[serial_test::serial]
async fn weighted_balancer_openai_preferred() {
    let mut config = Config::test_default();
    // Disable auth for this test since we're not testing authentication
    config.helicone.features = HeliconeFeatures::None;
    let balance_config = BalanceConfig::from(HashMap::from([(
        EndpointType::Chat,
        BalanceConfigInner::ProviderWeighted {
            providers: nes![
                WeightedProvider {
                    provider: InferenceProvider::OpenAI,
                    weight: Decimal::try_from(0.75).unwrap(),
                },
                WeightedProvider {
                    provider: InferenceProvider::Anthropic,
                    weight: Decimal::try_from(0.25).unwrap(),
                },
            ],
        },
    )]));
    config.routers = RouterConfigs::new(HashMap::from([(
        RouterId::Named(CompactString::new("my-router")),
        RouterConfig {
            load_balance: balance_config,
            ..Default::default()
        },
    )]));
    // Determine dynamic expected ranges based on 100 total requests and a ±15%
    // tolerance
    let num_requests = 100;
    let tolerance = num_requests as f64 * 0.15;
    let expected_openai_midpt = num_requests as f64 * 0.75;
    let expected_anthropic_midpt = num_requests as f64 * 0.25;
    let openai_range = (expected_openai_midpt - tolerance).floor() as u64
        ..(expected_openai_midpt + tolerance).ceil() as u64;
    let anthropic_range = (expected_anthropic_midpt - tolerance).floor() as u64
        ..(expected_anthropic_midpt + tolerance).ceil() as u64;
    let mock_args = MockArgs::builder()
        .stubs(HashMap::from([
            (
                "success:openai:chat_completion",
                openai_range.clone().into(),
            ),
            ("success:anthropic:messages", anthropic_range.clone().into()),
            // When auth is disabled, logging services should not be called
            ("success:minio:upload_request", 0.into()),
            ("success:jawn:log_request", 0.into()),
        ]))
        .build();
    let mut harness = Harness::builder()
        .with_config(config)
        .with_mock_args(mock_args)
        .build()
        .await;

    let body_bytes = serde_json::to_vec(&json!({
        "model": "openai/gpt-4o-mini",
        "messages": [
            {
                "role": "user",
                "content": "Hello, world!"
            }
        ]
    }))
    .unwrap();

    for _ in 0..num_requests {
        let request_body = axum_core::body::Body::from(body_bytes.clone());
        let request = Request::builder()
            .method(Method::POST)
            // default router
            .uri("http://router.helicone.com/router/my-router/chat/completions")
            .body(request_body)
            .unwrap();
        let response = harness.call(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        // we need to collect the body here in order to poll the underlying body
        // so that the async logging task can complete
        let _response_body = response.into_body().collect().await.unwrap();
    }

    // sleep so that the background task for logging can complete
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
}

#[tokio::test]
#[serial_test::serial]
async fn weighted_balancer_anthropic_heavily_preferred() {
    let mut config = Config::test_default();
    // Disable auth for this test since we're not testing authentication
    config.helicone.features = HeliconeFeatures::None;
    let balance_config = BalanceConfig::from(HashMap::from([(
        EndpointType::Chat,
        BalanceConfigInner::ProviderWeighted {
            providers: nes![
                WeightedProvider {
                    provider: InferenceProvider::OpenAI,
                    weight: Decimal::try_from(0.05).unwrap(),
                },
                WeightedProvider {
                    provider: InferenceProvider::Anthropic,
                    weight: Decimal::try_from(0.95).unwrap(),
                },
            ],
        },
    )]));
    config.routers = RouterConfigs::new(HashMap::from([(
        RouterId::Named(CompactString::new("my-router")),
        RouterConfig {
            load_balance: balance_config,
            ..Default::default()
        },
    )]));
    // Determine dynamic expected ranges based on 100 total requests and a ±15%
    // tolerance
    let num_requests = 100;
    let tolerance = num_requests as f64 * 0.20;
    let expected_openai_midpt = num_requests as f64 * 0.05;
    let expected_anthropic_midpt = num_requests as f64 * 0.95;
    let openai_range_lower =
        (expected_openai_midpt - tolerance).max(0.0).floor() as u64;
    let openai_range_upper = (expected_openai_midpt + tolerance).ceil() as u64;
    let openai_range = openai_range_lower..openai_range_upper;
    let anthropic_range_lower =
        (expected_anthropic_midpt - tolerance).floor() as u64;
    let anthropic_range_upper = ((expected_anthropic_midpt + tolerance).ceil()
        as u64)
        .min(num_requests as u64);
    let anthropic_range = anthropic_range_lower..anthropic_range_upper;
    let mock_args = MockArgs::builder()
        .stubs(HashMap::from([
            (
                "success:openai:chat_completion",
                openai_range.clone().into(),
            ),
            ("success:anthropic:messages", anthropic_range.clone().into()),
            // When auth is disabled, logging services should not be called
            ("success:minio:upload_request", 0.into()),
            ("success:jawn:log_request", 0.into()),
        ]))
        .build();
    let mut harness = Harness::builder()
        .with_config(config)
        .with_mock_args(mock_args)
        .build()
        .await;

    let body_bytes = serde_json::to_vec(&json!({
        "model": "openai/gpt-4o-mini",
        "messages": [
            {
                "role": "user",
                "content": "Hello, world!"
            }
        ]
    }))
    .unwrap();

    for _ in 0..num_requests {
        let request_body = axum_core::body::Body::from(body_bytes.clone());
        let request = Request::builder()
            .method(Method::POST)
            // default router
            .uri("http://router.helicone.com/router/my-router/chat/completions")
            .body(request_body)
            .unwrap();
        let response = harness.call(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        // we need to collect the body here in order to poll the underlying body
        // so that the async logging task can complete
        let _response_body = response.into_body().collect().await.unwrap();
    }

    // sleep so that the background task for logging can complete
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
}

#[tokio::test]
#[serial_test::serial]
async fn weighted_balancer_equal_four_providers() {
    let mut config = Config::test_default();
    // Disable auth for this test since we're not testing authentication
    config.helicone.features = HeliconeFeatures::None;
    let balance_config = BalanceConfig::from(HashMap::from([(
        EndpointType::Chat,
        BalanceConfigInner::ProviderWeighted {
            providers: nes![
                WeightedProvider {
                    provider: InferenceProvider::OpenAI,
                    weight: Decimal::try_from(0.25).unwrap(),
                },
                WeightedProvider {
                    provider: InferenceProvider::Anthropic,
                    weight: Decimal::try_from(0.25).unwrap(),
                },
                WeightedProvider {
                    provider: InferenceProvider::GoogleGemini,
                    weight: Decimal::try_from(0.25).unwrap(),
                },
                WeightedProvider {
                    provider: InferenceProvider::Ollama,
                    weight: Decimal::try_from(0.25).unwrap(),
                },
            ],
        },
    )]));
    config.routers = RouterConfigs::new(HashMap::from([(
        RouterId::Named(CompactString::new("my-router")),
        RouterConfig {
            load_balance: balance_config,
            ..Default::default()
        },
    )]));
    let num_requests = 100;
    let expected_midpt = num_requests as f64 * 0.25;
    let range = num_requests as f64 * 0.15;
    let lower = (expected_midpt - range).floor() as u64;
    let upper = (expected_midpt + range).floor() as u64;
    let expected_range = lower..upper;
    let mock_args = MockArgs::builder()
        .stubs(HashMap::from([
            (
                "success:openai:chat_completion",
                expected_range.clone().into(),
            ),
            ("success:anthropic:messages", expected_range.clone().into()),
            (
                "success:gemini:generate_content",
                expected_range.clone().into(),
            ),
            (
                "success:ollama:chat_completions",
                expected_range.clone().into(),
            ),
            // When auth is disabled, logging services should not be called
            ("success:minio:upload_request", 0.into()),
            ("success:jawn:log_request", 0.into()),
        ]))
        .build();
    let mut harness = Harness::builder()
        .with_config(config)
        .with_mock_args(mock_args)
        .build()
        .await;
    let body_bytes = serde_json::to_vec(&json!({
        "model": "openai/gpt-4o-mini",
        "messages": [
            {
                "role": "user",
                "content": "Hello, world!"
            }
        ]
    }))
    .unwrap();

    for _ in 0..num_requests {
        let request_body = axum_core::body::Body::from(body_bytes.clone());
        let request = Request::builder()
            .method(Method::POST)
            // default router
            .uri("http://router.helicone.com/router/my-router/chat/completions")
            .body(request_body)
            .unwrap();
        let response = harness.call(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        // we need to collect the body here in order to poll the underlying body
        // so that the async logging task can complete
        let _response_body = response.into_body().collect().await.unwrap();
    }

    // sleep so that the background task for logging can complete
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
}

#[tokio::test]
#[serial_test::serial]
async fn weighted_balancer_bedrock() {
    let mut config = Config::test_default();
    // Disable auth for this test since we're not testing authentication
    config.helicone.features = HeliconeFeatures::None;
    let balance_config = BalanceConfig::from(HashMap::from([(
        EndpointType::Chat,
        BalanceConfigInner::ProviderWeighted {
            providers: nes![
                WeightedProvider {
                    provider: InferenceProvider::OpenAI,
                    weight: Decimal::try_from(0.25).unwrap(),
                },
                WeightedProvider {
                    provider: InferenceProvider::Anthropic,
                    weight: Decimal::try_from(0.25).unwrap(),
                },
                WeightedProvider {
                    provider: InferenceProvider::Ollama,
                    weight: Decimal::try_from(0.25).unwrap(),
                },
                WeightedProvider {
                    provider: InferenceProvider::Bedrock,
                    weight: Decimal::try_from(0.25).unwrap(),
                },
            ],
        },
    )]));
    config.routers = RouterConfigs::new(HashMap::from([(
        RouterId::Named(CompactString::new("my-router")),
        RouterConfig {
            load_balance: balance_config,
            ..Default::default()
        },
    )]));
    // Determine dynamic expected ranges based on 100 total requests and a ±15%
    // tolerance
    let num_requests = 100;
    let expected_midpt = num_requests as f64 * 0.25;
    let tolerance = num_requests as f64 * 0.15;
    let lower = (expected_midpt - tolerance).floor() as u64;
    let upper = (expected_midpt + tolerance).ceil() as u64;
    let expected_range = lower..upper;
    let mock_args = MockArgs::builder()
        .stubs(HashMap::from([
            (
                "success:openai:chat_completion",
                expected_range.clone().into(),
            ),
            ("success:anthropic:messages", expected_range.clone().into()),
            ("success:bedrock:converse", expected_range.clone().into()),
            (
                "success:ollama:chat_completions",
                expected_range.clone().into(),
            ),
            // When auth is disabled, logging services should not be called
            ("success:minio:upload_request", 0.into()),
            ("success:jawn:log_request", 0.into()),
        ]))
        .build();
    let mut harness = Harness::builder()
        .with_config(config)
        .with_mock_args(mock_args)
        .build()
        .await;

    let body_bytes = serde_json::to_vec(&json!({
        "model": "openai/gpt-4o-mini",
        "messages": [
            {
                "role": "user",
                "content": "Hello, world!"
            }
        ]
    }))
    .unwrap();

    for _ in 0..num_requests {
        let request_body = axum_core::body::Body::from(body_bytes.clone());
        let request = Request::builder()
            .method(Method::POST)
            // default router
            .uri("http://router.helicone.com/router/my-router/chat/completions")
            .body(request_body)
            .unwrap();
        let response = harness.call(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        // we need to collect the body here in order to poll the underlying body
        // so that the async logging task can complete
        let _response_body = response.into_body().collect().await.unwrap();
    }

    // sleep so that the background task for logging can complete
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
}

#[tokio::test]
#[serial_test::serial]
async fn model_weighted() {
    let mut config = Config::test_default();
    // Disable auth for this test since we're not testing authentication
    config.helicone.features = HeliconeFeatures::None;
    let balance_config = BalanceConfig::from(HashMap::from([(
        EndpointType::Chat,
        BalanceConfigInner::ModelWeighted {
            models: nes![
                WeightedModel {
                    model: ModelId::from_str("openai/gpt-4o-mini").unwrap(),
                    weight: Decimal::try_from(0.25).unwrap(),
                },
                WeightedModel {
                    model: ModelId::from_str(
                        "anthropic/claude-3-haiku-20240307"
                    )
                    .unwrap(),
                    weight: Decimal::try_from(0.75).unwrap(),
                },
            ],
        },
    )]));
    config.routers = RouterConfigs::new(HashMap::from([(
        RouterId::Named(CompactString::new("my-router")),
        RouterConfig {
            load_balance: balance_config,
            ..Default::default()
        },
    )]));
    // Determine dynamic expected ranges based on 100 total requests and a ±15%
    // tolerance
    let num_requests = 100;
    let tolerance = num_requests as f64 * 0.15;
    let expected_openai_midpt = num_requests as f64 * 0.25;
    let expected_anthropic_midpt = num_requests as f64 * 0.75;
    let openai_range = (expected_openai_midpt - tolerance).floor() as u64
        ..(expected_openai_midpt + tolerance).ceil() as u64;
    let anthropic_range = (expected_anthropic_midpt - tolerance).floor() as u64
        ..(expected_anthropic_midpt + tolerance).ceil() as u64;
    let mock_args = MockArgs::builder()
        .stubs(HashMap::from([
            (
                "success:openai:chat_completion",
                openai_range.clone().into(),
            ),
            ("success:anthropic:messages", anthropic_range.clone().into()),
            // When auth is disabled, logging services should not be called
            ("success:minio:upload_request", 0.into()),
            ("success:jawn:log_request", 0.into()),
        ]))
        .build();
    let mut harness = Harness::builder()
        .with_config(config)
        .with_mock_args(mock_args)
        .build()
        .await;

    // Send all requests with a model name that will be distributed
    // based on the weighted configuration
    let body_bytes = serde_json::to_vec(&json!({
        "model": "openai/gpt-4o-mini",
        "messages": [
            {
                "role": "user",
                "content": "Hello, world!"
            }
        ]
    }))
    .unwrap();

    for _ in 0..num_requests {
        let request_body = axum_core::body::Body::from(body_bytes.clone());
        let request = Request::builder()
            .method(Method::POST)
            .uri("http://router.helicone.com/router/my-router/chat/completions")
            .body(request_body)
            .unwrap();
        let response = harness.call(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        // we need to collect the body here in order to poll the underlying body
        // so that the async logging task can complete
        let _response_body = response.into_body().collect().await.unwrap();
    }

    // sleep so that the background task for logging can complete
    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
}
