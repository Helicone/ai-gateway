use std::{future::Future, path::PathBuf, pin::Pin};

use ai_gateway::{
    app::App,
    config::{Config, DeploymentTarget},
    control_plane::websocket::ControlPlaneClient,
    db_listener::DatabaseListener,
    discover::monitor::{
        health::provider::HealthMonitor, rate_limit::RateLimitMonitor,
    },
    error::{init::InitError, runtime::RuntimeError},
    metrics::system::SystemMetrics,
    middleware::rate_limit,
    utils::meltdown::TaggedService,
};
use clap::Parser;
use meltdown::{Meltdown, Service, Token};
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

struct FnService<F>(F);

impl<F, Fut> Service for FnService<F>
where
    F: FnOnce(Token) -> Fut + Send + 'static,
    Fut: Future<Output = Result<(), RuntimeError>> + Send + 'static,
{
    type Future =
        Pin<Box<dyn Future<Output = Result<(), RuntimeError>> + Send>>;

    fn run(self, token: Token) -> Self::Future {
        Box::pin((self.0)(token))
    }
}

enum AllServices {
    App(App),
    HealthMonitor(HealthMonitor),
    RateLimitMonitor(RateLimitMonitor),
    RateLimitCleanup(rate_limit::cleanup::GarbageCollector),
    SystemMetrics(SystemMetrics),
    Shutdown(
        FnService<
            fn(
                Token,
            ) -> Pin<
                Box<dyn Future<Output = Result<(), RuntimeError>> + Send>,
            >,
        >,
    ),
    ControlPlane(ControlPlaneClient),
    DbListener(DatabaseListener),
}

impl Service for AllServices {
    type Future =
        Pin<Box<dyn Future<Output = Result<(), RuntimeError>> + Send>>;

    fn run(self, token: Token) -> Self::Future {
        match self {
            Self::App(s) => Box::pin(s.run(token)),
            Self::HealthMonitor(s) => Box::pin(s.run(token)),
            Self::RateLimitMonitor(s) => Box::pin(s.run(token)),
            Self::RateLimitCleanup(s) => Box::pin(s.run(token)),
            Self::SystemMetrics(s) => Box::pin(s.run(token)),
            Self::Shutdown(s) => s.run(token),
            Self::ControlPlane(s) => Box::pin(s.run(token)),
            Self::DbListener(s) => Box::pin(s.run(token)),
        }
    }
}

async fn run_app(config: Config) -> Result<(), RuntimeError> {
    let app = App::new(config).await?;
    let config = app.state.config();
    let health_monitor = HealthMonitor::new(app.state.clone());
    let rate_limit_monitor = RateLimitMonitor::new(app.state.clone());
    let rate_limiting_cleanup_service =
        config.global.rate_limit.as_ref().map(|rl| {
            rate_limit::cleanup::GarbageCollector::new(
                app.state.clone(),
                rl.cleanup_interval(),
            )
        });

    fn wait_for_shutdown_signals_service(
        token: Token,
    ) -> Pin<Box<dyn Future<Output = Result<(), RuntimeError>> + Send>> {
        Box::pin(ai_gateway::utils::meltdown::wait_for_shutdown_signals(
            token,
        ))
    }

    let shutdown_service =
        FnService(wait_for_shutdown_signals_service as fn(_) -> _);

    let mut meltdown = Meltdown::new()
        .register(TaggedService::new(
            "shutdown-signals",
            AllServices::Shutdown(shutdown_service),
        ))
        .register(TaggedService::new("gateway", AllServices::App(app.clone())))
        .register(TaggedService::new(
            "provider-health-monitor",
            AllServices::HealthMonitor(health_monitor),
        ))
        .register(TaggedService::new(
            "provider-rate-limit-monitor",
            AllServices::RateLimitMonitor(rate_limit_monitor),
        ))
        .register(TaggedService::new(
            "system-metrics",
            AllServices::SystemMetrics(SystemMetrics),
        ));

    let mut tasks = vec![
        "shutdown-signals",
        "gateway",
        "provider-health-monitor",
        "provider-rate-limit-monitor",
        "system-metrics",
    ];

    if app.state.0.config.helicone.is_auth_enabled() {
        let control_plane_state = app.state.0.control_plane_state.clone();
        let helicone_config = app.state.0.config.helicone.clone();
        meltdown = meltdown.register(TaggedService::new(
            "control-plane-client",
            AllServices::ControlPlane(
                ControlPlaneClient::connect(
                    control_plane_state,
                    helicone_config,
                )
                .await?,
            ),
        ));
        tasks.push("control-plane-client");
    }

    if app.state.0.config.deployment_target == DeploymentTarget::Cloud {
        meltdown = meltdown.register(TaggedService::new(
            "database-listener",
            AllServices::DbListener(
                DatabaseListener::new(app.state.0.config.database.clone())
                    .await?,
            ),
        ));
        tasks.push("database-listener");
    }

    if let Some(rate_limiting_cleanup_service) = rate_limiting_cleanup_service {
        meltdown = meltdown.register(TaggedService::new(
            "rate-limiting-cleanup",
            AllServices::RateLimitCleanup(rate_limiting_cleanup_service),
        ));
        tasks.push("rate-limiting-cleanup");
    }

    info!(tasks = ?tasks, "starting services");

    let mut shutting_down = false;
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
