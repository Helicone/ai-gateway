pub mod anthropic;
mod bedrock;
pub mod model;
pub mod ollama;
pub mod openai;
pub mod openai_compatible;
pub mod registry;
pub mod service;

use async_openai::error::WrappedError;
use base64::Engine;
use bytes::Bytes;
use http::{StatusCode, response::Parts};
use serde::{Serialize, de::DeserializeOwned};

pub use self::service::*;
use crate::{
    endpoints::{AiRequest, Endpoint},
    error::{
        api::ApiError, internal::InternalError,
        invalid_req::InvalidRequestError, mapper::MapperError,
    },
    types::extensions::MapperContext,
};

pub(crate) const DEFAULT_MAX_TOKENS: u32 = 2000;

/// `TryFrom` but allows us to implement it for foreign types, so we can
/// maintain boundaries between our business logic and the provider types.
pub trait TryConvert<Source, Target>: Sized {
    type Error;

    fn try_convert(
        &self,
        value: Source,
    ) -> std::result::Result<Target, Self::Error>;
}

pub trait TryConvertError<Source, Target>: Sized {
    type Error;

    fn try_convert_error(
        &self,
        resp_parts: &Parts,
        value: Source,
    ) -> std::result::Result<Target, Self::Error>;
}

pub trait TryConvertStreamData<Source, Target>: Sized {
    type Error;

    /// Returns `None` if the chunk in `value` cannot be converted to an
    /// equivalent chunk in `Target`.
    fn try_convert_chunk(
        &self,
        value: Source,
    ) -> std::result::Result<Option<Target>, Self::Error>;
}
pub trait EndpointConverter {
    /// Convert a request body to a target request body with raw bytes.
    ///
    /// `MapperContext` is used to determine if the request is a stream
    /// since within the converter we have deserialized the request
    /// bytes to a concrete type.
    fn convert_req_body(
        &self,
        req_body_bytes: Bytes,
    ) -> Result<(Bytes, MapperContext), ApiError>;
    /// Convert a response body to a target response body with raw bytes.
    ///
    /// Returns `None` if there is no applicable mapping for a given chunk
    /// when converting stream response bodies.
    fn convert_resp_body(
        &self,
        resp_parts: Parts,
        resp_body_bytes: Bytes,
        is_stream: bool,
    ) -> Result<Option<Bytes>, ApiError>;
}

pub struct TypedEndpointConverter<S, T, C>
where
    S: Endpoint,
    T: Endpoint,
    C: TryConvert<S::RequestBody, T::RequestBody>
        + TryConvert<T::ResponseBody, S::ResponseBody>,
{
    converter: C,
    _phantom: std::marker::PhantomData<(S, T)>,
}

impl<S, T, C> TypedEndpointConverter<S, T, C>
where
    S: Endpoint,
    T: Endpoint,
    C: TryConvert<S::RequestBody, T::RequestBody>,
    C: TryConvert<T::ResponseBody, S::ResponseBody>,
{
    pub fn new(converter: C) -> Self {
        Self {
            converter,
            _phantom: std::marker::PhantomData,
        }
    }
}

impl<S, T, C> EndpointConverter for TypedEndpointConverter<S, T, C>
where
    S: Endpoint,
    S::RequestBody: DeserializeOwned + AiRequest,
    S::ResponseBody: Serialize,
    S::StreamResponseBody: Serialize,
    S::ErrorResponseBody: Serialize,
    T: Endpoint,
    T::RequestBody: Serialize + AiRequest,
    T::ResponseBody: DeserializeOwned,
    T::StreamResponseBody: DeserializeOwned,
    T::ErrorResponseBody: DeserializeOwned,
    C: TryConvert<S::RequestBody, T::RequestBody>,
    C: TryConvert<T::ResponseBody, S::ResponseBody>,
    C: TryConvertStreamData<T::StreamResponseBody, S::StreamResponseBody>,
    C: TryConvertError<T::ErrorResponseBody, S::ErrorResponseBody>,
    <C as TryConvert<S::RequestBody, T::RequestBody>>::Error: Into<MapperError>,
    <C as TryConvert<T::ResponseBody, S::ResponseBody>>::Error: Into<MapperError>,
    <C as TryConvertStreamData<T::StreamResponseBody, S::StreamResponseBody>>::Error:
        Into<MapperError>,
    <C as TryConvertError<T::ErrorResponseBody, S::ErrorResponseBody>>::Error: Into<MapperError>,
{
    fn convert_req_body(
        &self,
        bytes: Bytes,
    ) -> Result<(Bytes, MapperContext), ApiError> {
        let source_request: S::RequestBody = serde_json::from_slice(&bytes)
            .map_err(InvalidRequestError::InvalidRequestBody)?;
        let is_stream = source_request.is_stream();
        let target_request: T::RequestBody = self
            .converter
            .try_convert(source_request)
            .map_err(|e| InternalError::MapperError(e.into()))?;
        let model = target_request.model().map_err(InternalError::MapperError).inspect_err(|e| {
            tracing::error!(?e, "failed to get model from request");
        })?;

        let mapper_ctx = MapperContext { is_stream, model: Some(model) };
        let target_bytes =
            Bytes::from(serde_json::to_vec(&target_request).map_err(|e| {
                InternalError::Serialize {
                    ty: std::any::type_name::<T::RequestBody>(),
                    error: e,
                }
            })?);

        Ok((target_bytes, mapper_ctx))
    }

    fn convert_resp_body(
        &self,
        resp_parts: Parts,
        bytes: Bytes,
        is_stream: bool,
    ) -> Result<Option<Bytes>, ApiError> {
        if is_stream {
            let source_response: T::StreamResponseBody =
                serde_json::from_slice(&bytes)
                    .map_err(|e| InternalError::Deserialize {
                        ty: std::any::type_name::<T::StreamResponseBody>(),
                        error: e,
                    })?;
            let target_response: Option<S::StreamResponseBody> = self
                .converter
                .try_convert_chunk(source_response)
                .map_err(|e| InternalError::MapperError(e.into()))?;

            if let Some(target_response) = target_response {
                let target_bytes =
                serde_json::to_vec(&target_response).map_err(|e| {
                    InternalError::Serialize {
                        ty: std::any::type_name::<T::ResponseBody>(),
                        error: e,
                    }
                })?;

                Ok(Some(Bytes::from(target_bytes)))
            } else {
                Ok(None)
            }
        } else if resp_parts.status.is_client_error() || resp_parts.status.is_server_error() {
            let source_error: T::ErrorResponseBody = serde_json::from_slice(&bytes)
                .map_err(|e| InternalError::Deserialize {
                    ty: std::any::type_name::<T::ErrorResponseBody>(),
                    error: e,
                })?;
            let target_response: S::ErrorResponseBody = self
                .converter
                .try_convert_error(&resp_parts, source_error)
                .map_err(|e| InternalError::MapperError(e.into()))?;

            let target_bytes =
            serde_json::to_vec(&target_response).map_err(|e| {
                InternalError::Serialize {
                    ty: std::any::type_name::<T::ResponseBody>(),
                    error: e,
                }
            })?;

            Ok(Some(Bytes::from(target_bytes)))
        } else {
            let source_response: T::ResponseBody =
            serde_json::from_slice(&bytes)
                .map_err(|e| InternalError::Deserialize {
                    ty: std::any::type_name::<T::ResponseBody>(),
                    error: e,
                })?;
            let target_response: S::ResponseBody = self
            .converter
            .try_convert(source_response)
            .map_err(|e| InternalError::MapperError(e.into()))?;

            let target_bytes =
            serde_json::to_vec(&target_response).map_err(|e| {
                InternalError::Serialize {
                    ty: std::any::type_name::<T::ResponseBody>(),
                    error: e,
                }
            })?;

            Ok(Some(Bytes::from(target_bytes)))
        }
    }
}

pub(crate) fn openai_error_from_status(
    status_code: StatusCode,
    message: Option<String>,
) -> WrappedError {
    let kind = self::openai::get_error_type(status_code);
    let code = self::openai::get_error_code(status_code);
    let message = message.unwrap_or_else(|| kind.clone());

    async_openai::error::WrappedError {
        error: async_openai::error::ApiError {
            message,
            code,
            param: None,
            r#type: Some(kind),
        },
    }
}

pub(super) fn mime_from_data_uri(uri: &str) -> Option<infer::Type> {
    // Split on the first comma.  If no comma => not a data-URI.
    let (_first, b64) = uri.split_once(',')?;

    // Only decode the first portion of base64 data for efficiency.
    // Base64 has 4:3 ratio, so 64 chars -> ~48 bytes, which is plenty for MIME
    // detection.
    let b64_prefix = &b64[..b64.len().min(64)];

    // Decode only the prefix into a fixed buffer.
    let mut header = [0u8; 48];
    let n = base64::engine::general_purpose::STANDARD
        .decode_slice(b64_prefix.as_bytes(), &mut header)
        .ok()?;

    infer::get(&header[..n])
}
