mod make;
use std::{
    convert::Infallible,
    fmt,
    hash::Hash,
    marker::PhantomData,
    pin::Pin,
    task::{Context, Poll},
};

use futures::ready;
use pin_project::pin_project;
use rustc_hash::FxHashMap as HashMap;
use tower::{
    Service,
    discover::{Change, Discover},
    load::Load,
    ready_cache::ReadyCache,
};
use tracing::{debug, trace};

pub use self::make::MakeRouter;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Service Key extension not found")]
    ExtensionNotFound,
    #[error("No services available for {0}")]
    NoServicesAvailable(String),
    #[error("Discover error: {0}")]
    Discover(tower::BoxError),
}

type ServiceCache<D, ReqBody> = ReadyCache<
    <D as Discover>::Key,
    <D as Discover>::Service,
    http::Request<ReqBody>,
>;

pub struct LatencyRouter<M, D, ReqBody>
where
    D: Discover,
    D::Key: Hash,
    M: Hash,
{
    discover: D,

    services: HashMap<M, ServiceCache<D, ReqBody>>,

    _req: PhantomData<ReqBody>,
}

impl<M, D, ReqBody> fmt::Debug for LatencyRouter<M, D, ReqBody>
where
    D: Discover + fmt::Debug,
    D::Key: Hash + fmt::Debug,
    D::Service: fmt::Debug,
    M: Hash + fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LatencyRouter")
            .field("discover", &self.discover)
            .field("services", &self.services)
            .finish_non_exhaustive()
    }
}

impl<M, D, ReqBody> LatencyRouter<M, D, ReqBody>
where
    D: Discover,
    M: Hash,
    D::Key: Hash,
    D::Service: Service<http::Request<ReqBody>, Error = Infallible>,
{
    pub fn new(discover: D) -> Self {
        Self {
            discover,
            services: HashMap::default(),
            _req: PhantomData,
        }
    }

    /// Returns the number of endpoints currently tracked by the balancer.
    pub fn len(&self) -> usize {
        self.services.values().map(ReadyCache::len).sum()
    }

    /// Returns whether or not the balancer is empty.
    pub fn is_empty(&self) -> bool {
        self.services.values().all(ReadyCache::is_empty)
    }
}

impl<M, D, ReqBody> LatencyRouter<M, D, ReqBody>
where
    D: Discover + Unpin,
    M: Hash + Clone + Eq + std::fmt::Debug + From<D::Key>,
    D::Key: Hash + Clone + std::fmt::Debug,
    D::Error: Into<tower::BoxError>,
    D::Service: Service<http::Request<ReqBody>, Error = Infallible> + Load,
    <D::Service as Load>::Metric: std::fmt::Debug,
{
    /// Polls `discover` for updates, adding new items to `not_ready`.
    ///
    /// Removals may alter the order of either `ready` or `not_ready`.
    fn update_pending_from_discover(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<(), Error>>> {
        debug!("updating from discover");
        loop {
            match ready!(Pin::new(&mut self.discover).poll_discover(cx))
                .transpose()
                .map_err(|e| Error::Discover(e.into()))?
            {
                None => return Poll::Ready(None),
                Some(Change::Remove(key)) => {
                    trace!("remove");
                    if let Some(cache) =
                        self.services.get_mut(&key.clone().into())
                    {
                        cache.evict(&key);
                    }
                }
                Some(Change::Insert(key, svc)) => {
                    let svc_key = key.clone().into();
                    trace!(svc_key = ?svc_key, key = ?key, "insert");
                    // If this service already existed in the set, it will be
                    // replaced as the new one becomes ready.
                    let cache = self.services.entry(svc_key).or_default();
                    cache.push(key, svc);
                }
            }
        }
    }

    fn promote_pending_to_ready(&mut self, cx: &mut Context<'_>) {
        for cache in self.services.values_mut() {
            loop {
                match cache.poll_pending(cx) {
                    Poll::Ready(Ok(())) => {
                        // There are no remaining pending services.
                        debug_assert_eq!(cache.pending_len(), 0);
                        break;
                    }
                    Poll::Pending => {
                        // None of the pending services are ready.
                        debug_assert!(cache.pending_len() > 0);
                        break;
                    }
                    Poll::Ready(Err(error)) => {
                        // An individual service was lost; continue processing
                        // pending services.
                        debug!(%error, "dropping failed endpoint");
                    }
                }
            }
            trace!(
                ready = %cache.ready_len(),
                pending = %cache.pending_len(),
                "poll_unready"
            );
        }
    }

    fn ready_index(&mut self, model: &M) -> Result<usize, Error> {
        let Some(cache) = self.services.get_mut(model) else {
            return Err(Error::NoServicesAvailable(format!("{model:?}")));
        };
        match cache.ready_len() {
            0 => Err(Error::NoServicesAvailable(format!("{model:?}"))),
            _ => {
                // O(n) based on the number of services
                cache
                    .iter_ready()
                    .enumerate()
                    .min_by(|(_idx_a, (svc_a_key, svc_a)), (_idx_b, (svc_b_key, svc_b))| {
                        let a_load = svc_a.load();
                        let b_load = svc_b.load();
                        tracing::trace!(svc_a_key = ?svc_a_key, svc_b_key = ?svc_b_key, a_load = ?a_load, b_load = ?b_load, "comparing services");
                        a_load
                            .partial_cmp(&b_load)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    })
                    .map(|(index, _)| index)
                    .ok_or(Error::NoServicesAvailable(format!("{model:?}")))
            }
        }
    }
}

impl<M, D, ReqBody> Service<http::Request<ReqBody>>
    for LatencyRouter<M, D, ReqBody>
where
    M: Hash
        + Clone
        + Eq
        + Send
        + Sync
        + 'static
        + std::fmt::Debug
        + From<D::Key>,
    D: Discover + Unpin,
    D::Key: Hash + Clone + std::fmt::Debug,
    D::Error: Into<tower::BoxError>,
    D::Service: Service<http::Request<ReqBody>, Error = Infallible> + Load,
    <D::Service as Load>::Metric: std::fmt::Debug,
    <D::Service as Service<http::Request<ReqBody>>>::Future: Send + 'static,
    <<D as tower::discover::Discover>::Service as Service<
        http::Request<ReqBody>,
    >>::Response: Send + 'static,
{
    type Response = <D::Service as Service<http::Request<ReqBody>>>::Response;
    type Error = Error;
    type Future = ResponseFuture<M, D, ReqBody>;

    fn poll_ready(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        let _ = self.update_pending_from_discover(cx)?;
        self.promote_pending_to_ready(cx);
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: http::Request<ReqBody>) -> Self::Future {
        let Some(model) = request.extensions().get::<M>() else {
            return ResponseFuture::Ready {
                error: Some(Error::ExtensionNotFound),
                _phantom: PhantomData,
            };
        };

        // Find the service with the least load
        let Ok(ready_index) = self.ready_index(model) else {
            return ResponseFuture::Ready {
                error: Some(Error::NoServicesAvailable(format!("{model:?}"))),
                _phantom: PhantomData,
            };
        };

        let Some(cache) = self.services.get_mut(model) else {
            return ResponseFuture::Ready {
                error: Some(Error::NoServicesAvailable(format!("{model:?}"))),
                _phantom: PhantomData,
            };
        };

        let future = cache.call_ready_index(ready_index, request);
        ResponseFuture::Inner {
            future,
            _phantom: PhantomData,
        }
    }
}

#[pin_project(project = ResponseFutureProj)]
pub enum ResponseFuture<M, D, ReqBody>
where
    M: Hash + Clone + Eq + Send + Sync + 'static,
    D: Discover + Unpin,
    D::Key: Hash + Clone,
    D::Error: Into<tower::BoxError>,
    D::Service: Service<http::Request<ReqBody>, Error = Infallible>,
    <D::Service as Service<http::Request<ReqBody>>>::Future: Send + 'static,
    <<D as tower::discover::Discover>::Service as Service<
        http::Request<ReqBody>,
    >>::Response: Send + 'static,
{
    Ready {
        error: Option<Error>,
        _phantom: PhantomData<M>,
    },
    Inner {
        #[pin]
        future: <D::Service as Service<http::Request<ReqBody>>>::Future,
        _phantom: PhantomData<M>,
    },
}

impl<M, D, ReqBody> Future for ResponseFuture<M, D, ReqBody>
where
    M: Hash + Clone + Eq + Send + Sync + 'static,
    D: Discover + Unpin,
    D::Key: Hash + Clone,
    D::Error: Into<tower::BoxError>,
    D::Service: Service<http::Request<ReqBody>, Error = Infallible>,
    <D::Service as Service<http::Request<ReqBody>>>::Future: Send + 'static,
    <<D as tower::discover::Discover>::Service as Service<
        http::Request<ReqBody>,
    >>::Response: Send + 'static,
{
    type Output = Result<
        <D::Service as Service<http::Request<ReqBody>>>::Response,
        Error,
    >;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.project() {
            ResponseFutureProj::Ready { error, .. } => Poll::Ready(Err(error
                .take()
                .expect("future polled after completion"))),
            ResponseFutureProj::Inner { future, .. } => {
                match ready!(future.poll(cx)) {
                    Ok(res) => Poll::Ready(Ok(res)),
                    // never happens due to `Infallible` bound
                    Err(e) => match e {},
                }
            }
        }
    }
}
