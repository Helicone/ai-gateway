pub mod anthropic_client;
mod bedrock_client;
pub mod client;
mod extensions;
pub mod ollama_client;
pub mod openai_compatible_client;
pub mod service;

use std::pin::Pin;

use bytes::Bytes;
use futures::Stream;

pub use self::service::{Dispatcher, DispatcherService};
use crate::error::api::ApiError;

pub(crate) type BoxTryStream<I> =
    Pin<Box<dyn Stream<Item = Result<I, ApiError>> + Send>>;
pub(crate) type SSEStream = BoxTryStream<Bytes>;
