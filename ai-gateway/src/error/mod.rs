pub mod api;
pub mod auth;
pub mod init;
pub mod internal;
pub mod invalid_req;
pub mod logger;
pub mod mapper;
pub mod prompts;
pub mod provider;
pub mod runtime;
pub mod stream;
pub mod prompts;

pub trait ErrorMetric {
    /// Convert an error type into a low-cardinality string
    /// that can be used in metrics.
    fn error_metric(&self) -> String;
}
