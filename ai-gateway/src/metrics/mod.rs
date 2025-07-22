pub mod attribute_extractor;
pub mod request_count;
pub mod rolling_counter;
pub mod system;
pub mod tfft;

use opentelemetry::metrics::{Counter, Gauge, Histogram, Meter, UpDownCounter};

pub use self::rolling_counter::RollingCounter;

/// The top level struct that contains all metrics
/// which are exported to OpenTelemetry.
#[derive(Debug, Clone)]
pub struct Metrics {
    pub error_count: Counter<u64>,
    pub provider_health: Gauge<u64>,
    pub auth_attempts: Counter<u64>,
    pub auth_rejections: Counter<u64>,
    pub request_count: Counter<u64>,
    pub response_count: Counter<u64>,
    pub tfft_duration: Histogram<f64>,
    pub cache: CacheMetrics,
    pub routers: RouterMetrics,
}

impl Metrics {
    #[must_use]
    pub fn new(meter: &Meter) -> Self {
        let error_count = meter
            .u64_counter("error_count")
            .with_description("Number of error occurences")
            .build();
        let provider_health = meter
            .u64_gauge("provider_health")
            .with_description("Upstream provider health")
            .build();
        let auth_attempts = meter
            .u64_counter("auth_attempts")
            .with_description("Number of authentication attempts")
            .build();
        let auth_rejections = meter
            .u64_counter("auth_rejections")
            .with_description("Number of unauthenticated requests")
            .build();
        let request_count = meter
            .u64_counter("request_count")
            .with_description("Total request count")
            .build();
        let response_count = meter
            .u64_counter("response_count")
            .with_description("Number of successful responses")
            .build();
        let tfft_duration = meter
            .f64_histogram("tfft_duration")
            .with_unit("ms")
            .with_description("Time to first token duration")
            .build();
        let cache = CacheMetrics::new(meter);
        let routers = RouterMetrics::new(meter);
        Self {
            error_count,
            provider_health,
            auth_attempts,
            auth_rejections,
            request_count,
            response_count,
            tfft_duration,
            cache,
            routers,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CacheMetrics {
    pub hits: Counter<u64>,
    pub misses: Counter<u64>,
    pub evictions: Counter<u64>,
}

impl CacheMetrics {
    #[must_use]
    pub fn new(meter: &Meter) -> Self {
        let hits = meter
            .u64_counter("cache_hits")
            .with_description("Number of cache hits")
            .build();
        let misses = meter
            .u64_counter("cache_misses")
            .with_description("Number of cache misses")
            .build();
        let evictions = meter
            .u64_counter("cache_evictions")
            .with_description("Number of cache evictions")
            .build();
        Self {
            hits,
            misses,
            evictions,
        }
    }
}

#[derive(Debug, Clone)]
pub struct RouterMetrics {
    /// labels:
    /// - `router_id`
    /// - `org_id`
    pub routers: UpDownCounter<i64>,
    /// labels:
    /// - `router_id`
    /// - `endpoint_type`
    pub router_strategies: UpDownCounter<i64>,
    /// labels:
    /// - `router_id`
    pub model_mappings: UpDownCounter<i64>,
    /// labels:
    /// - `router_id`
    pub cache_enabled: UpDownCounter<i64>,
    /// labels:
    /// - `router_id`
    pub retries_enabled: UpDownCounter<i64>,
    /// labels:
    /// - `router_id`
    pub rate_limit_enabled: UpDownCounter<i64>,
    pub provider_api_keys: UpDownCounter<i64>,
    pub helicone_api_keys: UpDownCounter<i64>,
}

impl RouterMetrics {
    #[must_use]
    pub fn new(meter: &Meter) -> Self {
        let routers = meter
            .i64_up_down_counter("routers")
            .with_description("Number of routers")
            .build();
        let router_strategies = meter
            .i64_up_down_counter("router_strategies")
            .with_description("Number of router strategies")
            .build();
        let model_mappings = meter
            .i64_up_down_counter("model_mappings")
            .with_description("Number of model mappings")
            .build();
        let cache_enabled = meter
            .i64_up_down_counter("cache_enabled")
            .with_description("Number of routers with cache enabled")
            .build();
        let retries_enabled = meter
            .i64_up_down_counter("retries_enabled")
            .with_description("Number of routers with retries enabled")
            .build();
        let rate_limit_enabled = meter
            .i64_up_down_counter("rate_limit_enabled")
            .with_description("Number of routers with rate limit enabled")
            .build();
        let provider_api_keys = meter
            .i64_up_down_counter("provider_api_keys")
            .with_description("Number of provider API keys")
            .build();
        let helicone_api_keys = meter
            .i64_up_down_counter("helicone_api_keys")
            .with_description("Number of helicone API keys")
            .build();
        Self {
            routers,
            router_strategies,
            model_mappings,
            cache_enabled,
            retries_enabled,
            rate_limit_enabled,
            provider_api_keys,
            helicone_api_keys,
        }
    }
}
