pub mod direct;
pub mod meta;
pub mod router_details;
pub mod service;
pub mod strategy;
pub mod unified_api;

pub(in crate::router) const FORCED_ROUTING_HEADER: http::HeaderName =
    http::HeaderName::from_static("helicone-forced-routing");
