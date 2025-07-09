use std::{path::PathBuf, time::Duration};

use ai_gateway::{
    app::App,
    config::{Config, DeploymentTarget},
    control_plane::websocket::ControlPlaneClient,
    discover::monitor::{
        health::provider::HealthMonitor, rate_limit::RateLimitMonitor,
    },
    error::{init::InitError, runtime::RuntimeError},
    metrics::system::SystemMetrics,
    middleware::rate_limit,
    store::db_listener::DatabaseListener,
    utils::meltdown::TaggedService,
};
use clap::Parser;
use meltdown::Meltdown;
use opentelemetry_sdk::{
    logs::SdkLoggerProvider, metrics::SdkMeterProvider,
    trace::SdkTracerProvider,
};
use tracing::{debug, info};

#[global_allocator]
static GLOBAL: jemallocator::Jemalloc = jemallocator::Jemalloc;

#[derive(Debug, Parser)]
#[command(version)]
pub struct Args {
    /// Path to the default config file.
    /// Configs in this file can be overridden by environment variables.
    #[arg(short, long)]
    config: Option<PathBuf>,

    /// Enable verbose logging
    #[arg(short, long)]
    verbose: bool,
}

#[tokio::main]
async fn main() -> Result<(), RuntimeError> {
    let config = load_and_validate_config()?;
    let (logger_provider, tracer_provider, metrics_provider) =
        init_telemetry(&config)?;

    run_app(config).await?;

    shutdown_telemetry(logger_provider, &tracer_provider, metrics_provider);

    println!("shut down");

    Ok(())
}

fn load_and_validate_config() -> Result<Config, RuntimeError> {
    dotenvy::dotenv().ok();
    let args = Args::parse();
    let mut config = match Config::try_read(args.config) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("failed to read config: {error}");
            std::process::exit(1);
        }
    };

    // Override telemetry level if verbose flag is provided
    if args.verbose {
        config.telemetry.level = "info,ai_gateway=trace".to_string();
    }

    config.validate().inspect_err(|e| {
        tracing::error!(error = %e, "configuration validation failed");
    })?;

    Ok(config)
}

fn init_telemetry(
    config: &Config,
) -> Result<
    (
        Option<SdkLoggerProvider>,
        SdkTracerProvider,
        Option<SdkMeterProvider>,
    ),
    InitError,
> {
    let (logger_provider, tracer_provider, metrics_provider) =
        telemetry::init_telemetry(&config.telemetry)?;

    debug!("telemetry initialized");
    let pretty_config = serde_yml::to_string(&config)
        .expect("config should always be serializable");
    tracing::debug!(config = pretty_config, "Creating app with config");

    #[cfg(debug_assertions)]
    tracing::warn!("running in debug mode");

    Ok((logger_provider, tracer_provider, metrics_provider))
}

async fn run_app(config: Config) -> Result<(), RuntimeError> {
    // 5 mins
    const CLEANUP_INTERVAL: Duration = Duration::from_secs(60 * 5);
    let mut shutting_down = false;
    let helicone_config = config.helicone.clone();
    let app = App::new(config).await?;
    let config = app.state.config();
    let health_monitor = HealthMonitor::new(app.state.clone());
    let rate_limit_monitor = RateLimitMonitor::new(app.state.clone());
    let control_plane_state = app.state.0.control_plane_state.clone();

    let rate_limiting_cleanup_service =
        config.global.rate_limit.as_ref().map(|_| {
            rate_limit::cleanup::GarbageCollector::new(
                app.state.clone(),
                CLEANUP_INTERVAL,
            )
        });

    let mut tasks = vec![
        "shutdown-signals",
        "gateway",
        "provider-health-monitor",
        "provider-rate-limit-monitor",
        "system-metrics",
    ];
    let mut meltdown = Meltdown::new().register(TaggedService::new(
        "shutdown-signals",
        ai_gateway::utils::meltdown::wait_for_shutdown_signals,
    ));

    if app.state.0.config.helicone.is_auth_enabled() {
        meltdown = meltdown.register(TaggedService::new(
            "control-plane-client",
            ControlPlaneClient::connect(control_plane_state, helicone_config)
                .await?,
        ));
        tasks.push("control-plane-client");
    }

    if app.state.0.config.deployment_target == DeploymentTarget::Cloud
        && let Some(pg_pool) = &app.state.0.pg_pool
    {
        meltdown = meltdown.register(TaggedService::new(
            "database-listener",
            DatabaseListener::new(pg_pool.clone(), app.state.clone())?,
        ));
        tasks.push("database-listener");
    }

    meltdown = meltdown
        .register(TaggedService::new("gateway", app))
        .register(TaggedService::new(
            "provider-health-monitor",
            health_monitor,
        ))
        .register(TaggedService::new(
            "provider-rate-limit-monitor",
            rate_limit_monitor,
        ))
        .register(TaggedService::new("system-metrics", SystemMetrics));

    if let Some(rate_limiting_cleanup_service) = rate_limiting_cleanup_service {
        meltdown = meltdown.register(TaggedService::new(
            "rate-limiting-cleanup",
            rate_limiting_cleanup_service,
        ));
        tasks.push("rate-limiting-cleanup");
    }

    info!(tasks = ?tasks, "starting services");

    while let Some((service, result)) = meltdown.next().await {
        match result {
            Ok(()) => info!(%service, "service stopped successfully"),
            Err(error) => tracing::error!(%service, %error, "service crashed"),
        }

        if !shutting_down {
            info!("propagating shutdown signal...");
            meltdown.trigger();
            shutting_down = true;
        }
    }
    Ok(())
}

fn shutdown_telemetry(
    logger_provider: Option<SdkLoggerProvider>,
    tracer_provider: &SdkTracerProvider,
    metrics_provider: Option<SdkMeterProvider>,
) {
    if let Some(logger_provider) = logger_provider {
        if let Err(e) = logger_provider.shutdown() {
            println!("error shutting down logger provider: {e}");
        }
    }
    if let Err(e) = tracer_provider.shutdown() {
        println!("error shutting down tracer provider: {e}");
    }
    if let Some(metrics_provider) = metrics_provider {
        if let Err(e) = metrics_provider.shutdown() {
            println!("error shutting down metrics provider: {e}");
        }
    }
}
