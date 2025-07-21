use std::{str::FromStr, time::Duration};

use ai_gateway::types::{model_id::ModelId, provider::InferenceProvider};
use clap::{Parser, Subcommand};
use futures::StreamExt;
use rand::Rng;
use tokio::{
    signal,
    time::{Instant, sleep},
};

#[derive(Parser, Debug)]
#[command(
    author,
    version,
    about,
    long_about = None,
    subcommand_required = false,
    arg_required_else_help = false,
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Send a single test request (default when no subcommand is provided)
    #[command(name = "test-request")]
    TestRequest {
        /// Stream the response
        #[arg(short, long, default_value_t = false)]
        stream: bool,

        /// Include the Authorization header
        #[arg(short = 'a', long = "auth", default_value_t = false)]
        auth: bool,

        /// Request type
        #[arg(short = 'r', long = "request-type", default_value_t = RequestType::LoadBalanced)]
        request_type: RequestType,

        #[arg(long = "router-id", default_value = "my-router")]
        router_id: Option<String>,

        /// Model identifier to use
        #[arg(
            short = 'm',
            long = "model",
            default_value = "openai/gpt-4o-mini"
        )]
        model: String,
    },

    /// Continuously send requests until interrupted (load testing)
    #[command(name = "load-test")]
    LoadTest {
        /// Stream the responses
        #[arg(short, long, default_value_t = false)]
        stream: bool,

        /// Include the Authorization header
        #[arg(short = 'a', long = "auth", default_value_t = false)]
        auth: bool,

        /// Request type
        #[arg(short = 'r', long = "request-type", default_value_t = RequestType::LoadBalanced)]
        request_type: RequestType,

        /// Model identifier to use
        #[arg(
            short = 'm',
            long = "model",
            default_value = "openai/gpt-4o-mini"
        )]
        model: String,

        #[arg(long = "router-id", default_value = "my-router")]
        router_id: Option<String>,
    },
}

#[derive(
    Default, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, clap::ValueEnum,
)]
#[clap(rename_all = "kebab-case")]
enum RequestType {
    Direct,
    UnifiedApi,
    #[default]
    LoadBalanced,
}

impl std::fmt::Display for RequestType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RequestType::Direct => write!(f, "direct"),
            RequestType::UnifiedApi => write!(f, "unified-api"),
            RequestType::LoadBalanced => write!(f, "load-balanced"),
        }
    }
}

impl Default for Commands {
    fn default() -> Self {
        Self::TestRequest {
            request_type: RequestType::LoadBalanced,
            stream: false,
            auth: false,
            model: "openai/gpt-4o-mini".to_string(),
            router_id: Some("my-router".to_string()),
        }
    }
}

async fn test(
    print_response: bool,
    is_stream: bool,
    send_auth: bool,
    request_type: RequestType,
    model: &str,
    router_id: Option<String>,
) {
    let openai_request_body = serde_json::json!({
        "model": model,
        "messages": [
            {
                "role": "system",
                "content": "You are a helpful assistant that can answer
    questions and help with tasks."
            },
            {
                "role": "user",
                "content": "Hello, world!"
            }
        ],
        "max_tokens": 400,
        "stream": is_stream
    });

    let openai_request: async_openai::types::CreateChatCompletionRequest =
        serde_json::from_value(openai_request_body).unwrap();

    let bytes = serde_json::to_vec(&openai_request).unwrap();
    let model_id = ModelId::from_str(model).expect("Invalid model");
    let url = match request_type {
        RequestType::Direct => {
            let provider = match model_id {
                ModelId::ModelIdWithVersion { provider, .. } => provider,
                ModelId::Bedrock(_) => InferenceProvider::Bedrock,
                ModelId::Ollama(_) => InferenceProvider::Ollama,
                ModelId::Unknown(_) => InferenceProvider::OpenAI,
            };
            format!("http://localhost:8080/{}/v1/chat/completions", provider)
        }
        RequestType::UnifiedApi => {
            "http://localhost:8080/ai/chat/completions".to_string()
        }
        RequestType::LoadBalanced => {
            let router_id = router_id.unwrap_or("my-router".to_string());
            format!(
                "http://localhost:8080/router/{}/chat/completions",
                router_id
            )
        }
    };

    let mut request_builder = reqwest::Client::new()
        .post(url)
        .header("Content-Type", "application/json");

    if send_auth {
        if let Ok(helicone_api_key) =
            std::env::var("HELICONE_CONTROL_PLANE_API_KEY")
        {
            request_builder =
                request_builder.header("authorization", helicone_api_key);
        } else {
            eprintln!(
                "Warning: HELICONE_CONTROL_PLANE_API_KEY not set, skipping \
                 Authorization header."
            );
        }
    }

    let response = request_builder.body(bytes).send().await.unwrap();
    println!("Status: {}", response.status());
    let trace_id = response
        .headers()
        .get("x-request-id")
        .unwrap()
        .to_str()
        .unwrap();
    println!("Trace ID: {}", trace_id);
    if print_response {
        if is_stream {
            let mut body_stream = response.bytes_stream();
            while let Some(Ok(chunk)) = body_stream.next().await {
                let chunk_str = String::from_utf8_lossy(&chunk);

                // Handle SSE format: look for "data: " prefix
                for line in chunk_str.lines() {
                    if line.starts_with("data: ") {
                        let json_str = &line[6..]; // Skip "data: "

                        // Skip "[DONE]" messages
                        if json_str.trim() == "[DONE]" {
                            continue;
                        }

                        // Try to parse the JSON
                        if let Ok(json) =
                            serde_json::from_str::<serde_json::Value>(json_str)
                        {
                            let pretty_json =
                                serde_json::to_string_pretty(&json).unwrap();
                            println!("Chunk: {}", pretty_json);
                        } else {
                            println!("Failed to parse JSON: {}", json_str);
                        }
                    }
                }
            }
        } else {
            let response_bytes =
                response.json::<serde_json::Value>().await.unwrap();
            println!("Response: {}", response_bytes);
        }
    }
}

async fn run_forever_loop(
    is_stream: bool,
    send_auth: bool,
    request_type: RequestType,
    model: String,
    router_id: Option<String>,
) {
    let mut rng = rand::rng();
    let mut request_count = 0u64;
    let start_time = Instant::now();

    loop {
        tokio::select! {
            _ = signal::ctrl_c() => {
                let elapsed = start_time.elapsed();
                let rps = request_count as f64 / elapsed.as_secs_f64();
                println!("\nShutdown signal received!");
                println!("Total requests: {}", request_count);
                println!("Total time: {:.2}s", elapsed.as_secs_f64());
                println!("Average RPS: {:.2}", rps);
                break;
            }
            _ = async {
                test(false, is_stream, send_auth, request_type, &model, router_id.clone()).await;
                request_count += 1;

                if request_count % 100 == 0 {
                    let elapsed = start_time.elapsed();
                    let current_rps = request_count as f64 / elapsed.as_secs_f64();
                    println!("Requests sent: {}, Current RPS: {:.2}", request_count, current_rps);
                }

                let delay_ms = rng.random_range(0..=2);
                sleep(Duration::from_millis(delay_ms)).await;
            } => {}
        }
    }
}

#[tokio::main]
async fn main() {
    let cli = Cli::parse();
    dotenvy::dotenv().ok();

    match cli.command.unwrap_or_default() {
        Commands::TestRequest {
            request_type,
            stream,
            auth,
            model,
            router_id,
        } => {
            println!("Starting single test...");
            test(true, stream, auth, request_type, &model, router_id).await;
            println!("Test completed successfully!");
        }
        Commands::LoadTest {
            request_type,
            stream,
            auth,
            model,
            router_id,
        } => {
            println!("Starting load test - press Ctrl+C to stop...");
            run_forever_loop(stream, auth, request_type, model, router_id)
                .await;
        }
    }
}
