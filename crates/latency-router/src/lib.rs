//! The purpose of this crate is to create a router that can route
//! request to the service with the lowest latency, without doing
//! any load balancing/distribution of requests, and instead simply
//! always picking the service with the lowest latency.
pub mod router;
