use std::{
    collections::HashMap,
    convert::Infallible,
    hash::{Hash, Hasher},
    str::FromStr,
    sync::Arc,
    task::{Context, Poll},
};

use bytes::Bytes;
use chrono::{DateTime, Utc};
use futures::{StreamExt, future::BoxFuture, stream::FuturesUnordered};
use http::{HeaderMap, HeaderName, HeaderValue, StatusCode, request::Parts};
use http_body_util::BodyExt;
use http_cache::{CacheManager, HttpResponse};
use http_cache_semantics::{
    BeforeRequest, CacheOptions, CachePolicy, ResponseLike,
};
use opentelemetry::KeyValue;
use rustc_hash::FxHasher;
use tracing::Instrument;
use url::Url;

use crate::{
    app_state::AppState,
    cache::InternalCacheManager,
    config::{
        cache::{CacheConfig, DEFAULT_BUCKETS, MAX_BUCKET_SIZE},
        router::RouterConfig,
    },
    error::{
        api::ApiError, init::InitError, internal::InternalError,
        invalid_req::InvalidRequestError,
    },
    logger::service::LoggerService,
    types::{
        body::BodyReader,
        extensions::{AuthContext, MapperContext},
        model_id::ModelId,
        provider::InferenceProvider,
        request::Request,
        response::Response,
    },
};

const CACHE_HIT_HEADER: HeaderName = HeaderName::from_static("helicone-cache");
const CACHE_BUCKET_IDX: HeaderName =
    HeaderName::from_static("helicone-cache-bucket-idx");
const CACHE_HIT_HEADER_VALUE: HeaderValue = HeaderValue::from_static("HIT");
const CACHE_MISS_HEADER_VALUE: HeaderValue = HeaderValue::from_static("MISS");

#[derive(Debug)]
struct CacheContext {
    // `Some` only if explicitly set in headers, `None` if not set
    enabled: Option<bool>,
    /// Cache-control header: <https://developer.mozilla.org/en-US/docs/Web/HTTP/Reference/Headers/Cache-Control>
    directive: Option<String>,
    buckets: Option<u8>,
    seed: Option<String>,
    options: Option<CacheOptions>,
}

impl CacheContext {
    /// Merge two cache configs. `Other` takes precedence over `Self`.
    #[must_use]
    pub fn merge(&self, other: &Self) -> Self {
        let enabled = if let Some(other_explicitly_set) = other.enabled {
            // if other is set, just use that value (so req headers can disable
            // whether caching is enabled or not per request)
            other_explicitly_set
        } else {
            // if other is not set, use self's value (so router/global config
            // can enable caching if explicitly enabled)
            self.enabled.unwrap_or(false)
        };
        Self {
            enabled: Some(enabled),
            directive: other
                .directive
                .clone()
                .or_else(|| self.directive.clone()),
            buckets: other.buckets.or(self.buckets),
            seed: other.seed.clone().or_else(|| self.seed.clone()),
            options: other.options.or(self.options),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CacheLayer {
    app_state: AppState,
    backend: InternalCacheManager,
    context: Arc<CacheContext>,
}

impl CacheLayer {
    fn new(
        app_state: AppState,
        config: CacheConfig,
    ) -> Result<Self, InitError> {
        let backend = app_state
            .0
            .cache_manager
            .clone()
            .ok_or(InitError::CacheNotConfigured)?;
        let context = CacheContext {
            enabled: Some(true),
            directive: config.directive,
            buckets: Some(config.buckets),
            seed: config.seed,
            options: Some(CacheOptions {
                shared: false,
                ..Default::default()
            }),
        };
        Ok(Self {
            app_state,
            backend,
            context: Arc::new(context),
        })
    }

    pub fn for_router(
        app_state: AppState,
        router_config: &RouterConfig,
    ) -> Option<Self> {
        if let Some(config) = router_config.cache.as_ref() {
            Self::new(app_state, config.clone()).ok()
        } else {
            None
        }
    }

    pub fn global(app_state: &AppState) -> Option<Self> {
        let cloned_app_state = app_state.clone();
        if let Some(config) = &app_state.config().global.cache {
            Self::new(cloned_app_state, config.clone()).ok()
        } else {
            None
        }
    }
}

impl<S> tower::Layer<S> for CacheLayer {
    type Service = CacheService<S>;

    fn layer(&self, inner: S) -> Self::Service {
        CacheService {
            inner,
            app_state: self.app_state.clone(),
            backend: self.backend.clone(),
            context: Arc::clone(&self.context),
        }
    }
}

#[derive(Debug, Clone)]
pub struct CacheService<S> {
    inner: S,
    app_state: AppState,
    backend: InternalCacheManager,
    context: Arc<CacheContext>,
}

impl<S> tower::Service<Request> for CacheService<S>
where
    S: tower::Service<Request, Response = Response, Error = Infallible>
        + Send
        + Clone
        + 'static,
    S::Future: Send + 'static,
{
    type Response = Response;
    type Error = ApiError;
    type Future = BoxFuture<'static, Result<Self::Response, Self::Error>>;

    fn poll_ready(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        self.inner
            .poll_ready(cx)
            .map_err(|_| ApiError::Internal(InternalError::Internal))
    }

    #[tracing::instrument(name = "cache", skip_all)]
    fn call(&mut self, req: Request) -> Self::Future {
        tracing::trace!("cache middleware");
        // see: https://docs.rs/tower/latest/tower/trait.Service.html#be-careful-when-cloning-inner-services
        let mut this = self.clone();
        std::mem::swap(self, &mut this);
        Box::pin(async move {
            let merged_ctx = this.context.merge(&get_cache_ctx(&req)?);
            let backend = this.backend.clone();
            make_request(
                &mut this.inner,
                &this.app_state,
                req,
                &backend,
                merged_ctx,
            )
            .await
        })
    }
}

#[allow(clippy::too_many_lines)]
async fn check_cache(
    app_state: AppState,
    cache: &InternalCacheManager,
    key: &str,
    req: Request,
    bucket: u8,
    now: std::time::SystemTime,
) -> Result<CacheCheckResult, ApiError> {
    let Some((http_resp, policy)) =
        cache.get(key).await.map_err(InternalError::CacheError)?
    else {
        return Ok(CacheCheckResult::Miss);
    };

    match policy.before_request(&req, now) {
        BeforeRequest::Fresh(parts) => {
            let additional_headers = vec![
                (CACHE_HIT_HEADER, CACHE_HIT_HEADER_VALUE),
                (CACHE_BUCKET_IDX, bucket_header_value(bucket)),
            ];
            let response =
                build_response(http_resp, parts.status, additional_headers)?;

            let start_instant = req
                .extensions()
                .get::<tokio::time::Instant>()
                .copied()
                .ok_or(InternalError::ExtensionNotFound("Instant"))?;
            let start_time =
                req.extensions()
                    .get::<DateTime<Utc>>()
                    .copied()
                    .ok_or(InternalError::ExtensionNotFound("DateTime<Utc>"))?;

            let target_url = get_url(&req)?;
            let req_headers = req.headers().clone();

            let (req_parts, req_body) = req.into_parts();
            let req_body_bytes = req_body
                .collect()
                .await
                .map_err(InternalError::CollectBodyError)?
                .to_bytes();
            let (resp_parts, resp_body) = response.into_parts();
            let stream = futures::TryStreamExt::map_err(
                resp_body.into_data_stream(),
                InternalError::CollectBodyError,
            );
            let (user_resp_body, body_reader, tfft_rx) =
                BodyReader::wrap_stream(stream, false);
            let response = Response::from_parts(resp_parts, user_resp_body);

            if app_state.config().helicone.observability {
                if !app_state.config().helicone.authentication {
                    tracing::warn!(
                        "Authentication is disabled, skipping response logging"
                    );
                    return Ok(CacheCheckResult::Fresh(response));
                }
                let auth_ctx =
                    req_parts.extensions.get::<AuthContext>().cloned().ok_or(
                        InternalError::ExtensionNotFound("AuthContext"),
                    )?;

                let app_state_cloned = app_state.clone();
                tokio::spawn(
            async move {
                        // TODO(eng-2160): make cache service agnostic to which endpoint is used
                        let deserialized_body = serde_json::from_slice::<async_openai::types::CreateChatCompletionRequest>(&req_body_bytes).map_err(|e| {
                            InternalError::Deserialize {
                                ty: "async_openai::types::CreateChatCompletionRequest",
                                error: e,
                            }
                        });
                        let Ok(deserialized_body) = deserialized_body else {
                            tracing::error!("Could not deserialize request body");
                            return;
                        };
                        let Ok(model) = ModelId::from_str(&deserialized_body.model) else {
                            tracing::error!("Could not parse model id from request body");
                            return;
                        };
                        let provider = model.inference_provider().unwrap_or_else(|| {
                            // this should never happen in practice, but we need to handle it, so we
                            // default to OpenAI
                            tracing::error!("Could not parse inference provider from request body");
                            InferenceProvider::OpenAI
                        });
                        let is_stream = deserialized_body.stream.is_some_and(|stream| stream);
                        let mapper_ctx = MapperContext {
                            is_stream,
                            model: Some(model),
                        };

                        let response_logger = LoggerService::builder()
                            .app_state(app_state.clone())
                            .auth_ctx(auth_ctx)
                            .start_time(start_time)
                            .start_instant(start_instant)
                            .target_url(target_url)
                            .request_headers(req_headers)
                            .request_body(req_body_bytes)
                            .response_status(parts.status)
                            .response_body(body_reader)
                            .provider(provider)
                            .tfft_rx(tfft_rx)
                            .mapper_ctx(mapper_ctx)
                            .build();
                        if let Err(e) = response_logger.log().await {
                            let error_str = e.as_ref().to_string();
                            app_state_cloned
                                .0
                                .metrics
                                .error_count
                                .add(1, &[KeyValue::new("type", error_str)]);
                        }
                    }
                    .instrument(tracing::Span::current()),
                );
            }

            Ok(CacheCheckResult::Fresh(response))
        }
        BeforeRequest::Stale { request, matches } if matches => {
            Ok(CacheCheckResult::Stale(request))
        }
        BeforeRequest::Stale { .. } => Ok(CacheCheckResult::Miss),
    }
}

enum CacheCheckResult {
    Fresh(Response),
    Stale(Parts),
    Miss,
}

fn bucket_header_value(bucket: u8) -> HeaderValue {
    HeaderValue::from_str(&bucket.to_string())
        .unwrap_or_else(|_| HeaderValue::from_static("0"))
}

async fn handle_response_for_cache_miss(
    cache: &InternalCacheManager,
    ctx: &CacheContext,
    key: String,
    req: Request,
    resp: Response,
    bucket: u8,
    now: std::time::SystemTime,
) -> Result<Response, ApiError> {
    let cacheable_resp =
        CacheableResponse::new(ctx, resp.headers(), resp.status());
    let cache_options = ctx.options.unwrap_or_default();
    let policy =
        CachePolicy::new_options(&req, &cacheable_resp, now, cache_options);

    if !policy.is_storable() || !resp.status().is_success() {
        tracing::trace!(
            status = ?resp.status(),
            is_storable = policy.is_storable(),
            "got response that is not storable"
        );
        return Ok(resp);
    }
    tracing::trace!("caching storable response");
    let url = get_url(&req)?;
    let (parts, body) = resp.into_parts();
    let body_bytes = body
        .collect()
        .await
        .map_err(InternalError::CollectBodyError)?
        .to_bytes();

    let http_resp = HttpResponse {
        body: body_bytes.clone().into(),
        headers: header_map_to_hash_map(parts.headers),
        status: parts.status.as_u16(),
        url,
        version: get_version(parts.version),
    };

    let cached = cache
        .put(key, http_resp, policy)
        .await
        .map_err(InternalError::CacheError)?;

    build_response(
        cached,
        parts.status,
        vec![
            (CACHE_HIT_HEADER, CACHE_MISS_HEADER_VALUE),
            (CACHE_BUCKET_IDX, bucket_header_value(bucket)),
        ],
    )
    .map_err(Into::into)
}

#[allow(clippy::too_many_lines)]
async fn make_request<S>(
    inner: &mut S,
    app_state: &AppState,
    mut req: Request,
    cache: &InternalCacheManager,
    ctx: CacheContext,
) -> Result<Response, ApiError>
where
    S: tower::Service<Request, Response = Response, Error = Infallible>
        + Send
        + 'static,
{
    // just call inner service if caching is disabled
    if ctx.enabled.is_none_or(|enabled| !enabled) {
        return inner.call(req).await.map_err(|e| {
            tracing::error!(error = %e, "encountered infallible error");
            ApiError::Internal(InternalError::Internal)
        });
    }

    if let Some(directive) = &ctx.directive {
        if req.headers().get(http::header::CACHE_CONTROL).is_none() {
            req.headers_mut().insert(
                http::header::CACHE_CONTROL,
                HeaderValue::from_str(directive)
                    .map_err(InternalError::InvalidHeader)?,
            );
        }
    }

    let (parts, body) = req.into_parts();
    let body_bytes = body
        .collect()
        .await
        .map_err(InternalError::CollectBodyError)?
        .to_bytes();
    let buckets = ctx.buckets.unwrap_or(DEFAULT_BUCKETS);
    let now = std::time::SystemTime::now();

    // Try each bucket in parallel
    let mut futures = FuturesUnordered::new();
    let hasher = get_hasher(&parts, &body_bytes, ctx.seed.as_deref());
    // fairly sample different buckets
    let mut bucket_indices: Vec<u8> = (0..buckets).collect();
    {
        use rand::seq::SliceRandom;
        let mut rng = rand::rng();
        bucket_indices.shuffle(&mut rng);
    }

    for bucket in bucket_indices {
        let mut cloned_hasher = hasher.clone();
        bucket.hash(&mut cloned_hasher);
        let key = cloned_hasher.finish().to_string();
        let req = Request::from_parts(parts.clone(), body_bytes.clone().into());
        futures.push(async move {
            check_cache(app_state.clone(), cache, &key, req, bucket, now)
                .await
                .map(|result| (bucket, key, result))
        });
    }

    let mut stale_hits = Vec::new();
    let mut empty_buckets = Vec::new();

    while let Some(result) = futures.next().await {
        match result {
            Ok((bucket, _key, CacheCheckResult::Fresh(mut resp))) => {
                record_cache_hit(app_state, bucket, &parts.uri);
                resp.headers_mut().extend([
                    (CACHE_HIT_HEADER, CACHE_HIT_HEADER_VALUE),
                    (CACHE_BUCKET_IDX, bucket_header_value(bucket)),
                ]);
                return Ok(resp);
            }
            Ok((bucket, key, CacheCheckResult::Stale(stale_parts))) => {
                stale_hits.push((bucket, key, stale_parts));
            }
            Ok((bucket, _, CacheCheckResult::Miss)) => {
                empty_buckets.push(bucket);
            }
            Err(e) => {
                tracing::warn!(error = %e, "Cache check error");
            }
        }
    }

    // Try stale hits
    if let Some((bucket, key, stale_parts)) = stale_hits.into_iter().next() {
        let req =
            Request::from_parts(stale_parts.clone(), body_bytes.clone().into());
        let resp = inner.call(req).await.map_err(|e| {
            tracing::error!(error = %e, "encountered infallible error");
            ApiError::Internal(InternalError::Internal)
        })?;
        let req_for_cache =
            Request::from_parts(stale_parts, body_bytes.clone().into());
        return handle_response_for_cache_miss(
            cache,
            &ctx,
            key,
            req_for_cache,
            resp,
            bucket,
            now,
        )
        .await;
    }

    // Complete miss - pick a bucket and make the request
    let bucket = empty_buckets
        .first()
        .copied()
        .unwrap_or_else(|| rand::random::<u8>() % buckets);
    let mut cloned_hasher = hasher.clone();
    bucket.hash(&mut cloned_hasher);
    let key = cloned_hasher.finish().to_string();
    record_cache_miss(app_state, &parts.uri, bucket);

    let req = Request::from_parts(parts.clone(), body_bytes.clone().into());
    let resp = inner.call(req).await.map_err(|e| {
        tracing::error!(error = %e, "encountered infallible error");
        ApiError::Internal(InternalError::Internal)
    })?;

    let req_for_cache = Request::from_parts(parts, body_bytes.into());
    handle_response_for_cache_miss(
        cache,
        &ctx,
        key,
        req_for_cache,
        resp,
        bucket,
        now,
    )
    .await
}

fn get_hasher(parts: &Parts, body: &Bytes, seed: Option<&str>) -> FxHasher {
    let mut hasher = FxHasher::default();
    if let Some(s) = seed {
        s.hash(&mut hasher);
    }
    if let Some(pq) = parts.uri.path_and_query() {
        pq.hash(&mut hasher);
    }
    body.hash(&mut hasher);
    hasher
}

fn record_cache_hit(app_state: &AppState, bucket: u8, uri: &http::Uri) {
    let attributes = &[
        KeyValue::new("bucket", bucket.to_string()),
        KeyValue::new("path", uri.path().to_string()),
    ];
    tracing::trace!(bucket = bucket, path = uri.path(), "cache hit");
    app_state.0.metrics.cache.hits.add(1, attributes);
}

fn record_cache_miss(app_state: &AppState, uri: &http::Uri, bucket: u8) {
    let attributes = &[
        KeyValue::new("bucket", bucket.to_string()),
        KeyValue::new("path", uri.path().to_string()),
    ];
    tracing::trace!(bucket = bucket, path = uri.path(), "cache miss");
    app_state.0.metrics.cache.misses.add(1, attributes);
}

fn get_cache_ctx(req: &Request) -> Result<CacheContext, InvalidRequestError> {
    let headers = req.headers();
    let enabled = headers
        .get("helicone-cache-enabled")
        .and_then(|v| v.to_str().map_or(None, |v| v.parse::<bool>().ok()));
    let buckets = headers
        .get("helicone-cache-bucket-max-size")
        .and_then(|v| v.to_str().map_or(None, |v| v.parse::<u8>().ok()));
    if buckets.is_some_and(|b| b > MAX_BUCKET_SIZE) {
        return Err(InvalidRequestError::InvalidCacheConfig);
    }
    let seed = headers
        .get("helicone-cache-seed")
        .and_then(|v| v.to_str().ok().map(String::from));
    let directive = headers
        .get(http::header::CACHE_CONTROL)
        .and_then(|v| v.to_str().ok().map(String::from));
    Ok(CacheContext {
        enabled,
        directive,
        buckets,
        seed,
        options: None,
    })
}

fn get_version(version: http::Version) -> http_cache::HttpVersion {
    match version {
        http::Version::HTTP_09 => http_cache::HttpVersion::Http09,
        http::Version::HTTP_10 => http_cache::HttpVersion::Http10,
        http::Version::HTTP_2 => http_cache::HttpVersion::H2,
        http::Version::HTTP_3 => http_cache::HttpVersion::H3,
        _ => http_cache::HttpVersion::Http11,
    }
}

fn header_map_to_hash_map(headers: HeaderMap) -> HashMap<String, String> {
    headers
        .into_iter()
        .filter_map(|(name, value)| {
            Some((name?.to_string(), value.to_str().ok()?.to_string()))
        })
        .collect()
}

fn get_url(req: &Request) -> Result<Url, InvalidRequestError> {
    let host = req.uri().host().unwrap_or_else(|| {
        tracing::warn!("no host in request uri");
        "localhost"
    });
    let scheme = req.uri().scheme().unwrap_or_else(|| {
        tracing::warn!("no scheme in request uri");
        &http::uri::Scheme::HTTP
    });

    let full_url = format!("{}://{}{}", scheme, host, req.uri());

    let url = Url::parse(&full_url)
        .map_err(|e| InvalidRequestError::InvalidUrl(e.to_string()))?;
    Ok(url)
}

fn build_response(
    cached: HttpResponse,
    status: StatusCode,
    extra_headers: impl IntoIterator<Item = (HeaderName, HeaderValue)>,
) -> Result<Response, InternalError> {
    let mut builder = http::Response::builder().status(status);
    for (k, v) in cached.headers {
        builder = builder.header(k, v);
    }
    let mut response = builder
        .body(cached.body.into())
        .map_err(|_| InternalError::Internal)?;

    response.headers_mut().extend(extra_headers);
    Ok(response)
}

struct CacheableResponse {
    resp_headers: HeaderMap,
    status: StatusCode,
}

impl CacheableResponse {
    fn new(ctx: &CacheContext, resp: &HeaderMap, status: StatusCode) -> Self {
        let mut resp_headers = resp.clone();
        resp_headers.remove(http::header::SET_COOKIE);
        if let Some(directive) = ctx.directive.as_ref() {
            if let Some(value) =
                cache_control::CacheControl::from_value(directive)
            {
                tracing::trace!("parsed cache control value");
                if let Some(max_age) = value.max_age {
                    HeaderValue::from_str(&format!(
                        "max-age={}",
                        max_age.as_secs()
                    ))
                    .inspect_err(|_e| {
                        tracing::error!(
                            "failed to set max-age response header"
                        );
                    })
                    .map(|header_value| {
                        resp_headers
                            .append(http::header::CACHE_CONTROL, header_value);
                    })
                    .ok();
                }
                if value.must_revalidate {
                    let header_value =
                        HeaderValue::from_static("must-revalidate");
                    resp_headers
                        .append(http::header::CACHE_CONTROL, header_value);
                }
                if value.proxy_revalidate {
                    let header_value =
                        HeaderValue::from_static("proxy-revalidate");
                    resp_headers
                        .append(http::header::CACHE_CONTROL, header_value);
                }
                if value.no_store {
                    let header_value = HeaderValue::from_static("no-store");
                    resp_headers
                        .append(http::header::CACHE_CONTROL, header_value);
                }
                if value.no_transform {
                    let header_value = HeaderValue::from_static("no-transform");
                    resp_headers
                        .append(http::header::CACHE_CONTROL, header_value);
                }
                match value.cachability {
                    Some(cache_control::Cachability::Private) => {
                        let header_value = HeaderValue::from_static("private");
                        resp_headers
                            .append(http::header::CACHE_CONTROL, header_value);
                    }
                    Some(cache_control::Cachability::Public) => {
                        let header_value = HeaderValue::from_static("public");
                        resp_headers
                            .append(http::header::CACHE_CONTROL, header_value);
                    }
                    Some(cache_control::Cachability::NoCache) => {
                        let header_value = HeaderValue::from_static("no-cache");
                        resp_headers
                            .append(http::header::CACHE_CONTROL, header_value);
                    }
                    _ => {}
                }
            }
        }
        Self {
            resp_headers,
            status,
        }
    }
}

impl ResponseLike for CacheableResponse {
    fn status(&self) -> StatusCode {
        self.status
    }

    fn headers(&self) -> &HeaderMap {
        &self.resp_headers
    }
}
