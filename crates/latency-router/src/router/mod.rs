//! Copyright (c) 2019 Tower Contributors
//!
//! Permission is hereby granted, free of charge, to any
//! person obtaining a copy of this software and associated
//! documentation files (the "Software"), to deal in the
//! Software without restriction, including without
//! limitation the rights to use, copy, modify, merge,
//! publish, distribute, sublicense, and/or sell copies of
//! the Software, and to permit persons to whom the Software
//! is furnished to do so, subject to the following
//! conditions:
//!
//! The above copyright notice and this permission notice
//! shall be included in all copies or substantial portions
//! of the Software.
//!
//! THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF
//! ANY KIND, EXPRESS OR IMPLIED, INCLUDING BUT NOT LIMITED
//! TO THE WARRANTIES OF MERCHANTABILITY, FITNESS FOR A
//! PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT
//! SHALL THE AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY
//! CLAIM, DAMAGES OR OTHER LIABILITY, WHETHER IN AN ACTION
//! OF CONTRACT, TORT OR OTHERWISE, ARISING FROM, OUT OF OR
//! IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER
//! DEALINGS IN THE SOFTWARE.
pub mod make;

use std::{
    convert::Infallible,
    fmt,
    hash::Hash,
    marker::PhantomData,
    pin::Pin,
    task::{Context, Poll},
};

use futures::ready;
use pin_project_lite::pin_project;
use tower::{
    Service,
    discover::{Change, Discover},
    load::Load,
    ready_cache::{ReadyCache, error::Failed},
};
use tracing::{debug, trace};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Service Key extension not found")]
    ExtensionNotFound,
    #[error("Discover error: {0}")]
    Discover(tower::BoxError),
}

pub struct LatencyRouter<D, Req>
where
    D: Discover,
    D::Key: Hash,
{
    discover: D,

    services: ReadyCache<D::Key, D::Service, Req>,

    ready_index: Option<usize>,

    _req: PhantomData<Req>,
}

impl<D: Discover, Req> fmt::Debug for LatencyRouter<D, Req>
where
    D: fmt::Debug,
    D::Key: Hash + fmt::Debug,
    D::Service: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LatencyRouter")
            .field("discover", &self.discover)
            .field("services", &self.services)
            .finish_non_exhaustive()
    }
}

impl<D, Req> LatencyRouter<D, Req>
where
    D: Discover,
    D::Key: Hash,
    D::Service: Service<Req, Error = Infallible>,
{
    pub fn new(discover: D) -> Self {
        tracing::trace!("LatencyRouter::new");
        Self {
            discover,
            services: ReadyCache::default(),
            ready_index: None,
            _req: PhantomData,
        }
    }

    /// Returns the number of endpoints currently tracked by the balancer.
    pub fn len(&self) -> usize {
        self.services.len()
    }

    /// Returns whether or not the balancer is empty.
    pub fn is_empty(&self) -> bool {
        self.services.is_empty()
    }
}

impl<D, Req> LatencyRouter<D, Req>
where
    D: Discover + Unpin,
    D::Key: Hash + Clone,
    D::Error: Into<tower::BoxError>,
    D::Service: Service<Req, Error = Infallible> + Load,
    <D::Service as Load>::Metric: std::fmt::Debug + Ord,
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
                    self.services.evict(&key);
                }
                Some(Change::Insert(key, svc)) => {
                    trace!("insert");
                    // If this service already existed in the set, it will be
                    // replaced as the new one becomes ready.
                    self.services.push(key, svc);
                }
            }
        }
    }

    fn promote_pending_to_ready(&mut self, cx: &mut Context<'_>) {
        loop {
            match self.services.poll_pending(cx) {
                Poll::Ready(Ok(())) => {
                    // There are no remaining pending services.
                    debug_assert_eq!(self.services.pending_len(), 0);
                    break;
                }
                Poll::Pending => {
                    // None of the pending services are ready.
                    debug_assert!(self.services.pending_len() > 0);
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
            ready = %self.services.ready_len(),
            pending = %self.services.pending_len(),
            "poll_unready"
        );
    }

    fn ready_index(&mut self) -> Option<usize> {
        match self.services.ready_len() {
            0 => None,
            1 => Some(0),
            _ => {
                // O(n) based on the number of services
                let min_loaded_index = self
                    .services
                    .iter_ready()
                    .enumerate()
                    .min_by_key(|(_index, (_, svc))| svc.load())
                    .map(|(index, _)| index);
                min_loaded_index
            }
        }
    }
}

impl<D, Req> Service<Req> for LatencyRouter<D, Req>
where
    D: Discover + Unpin,
    D::Key: Hash + Clone,
    D::Error: Into<tower::BoxError>,
    D::Service: Service<Req, Error = Infallible> + Load,
    <D::Service as Load>::Metric: std::fmt::Debug + Ord,
    <D::Service as Service<Req>>::Future: Send + 'static,
    <<D as tower::discover::Discover>::Service as Service<Req>>::Response:
        Send + 'static,
{
    type Response = <D::Service as Service<Req>>::Response;
    type Error = Error;
    type Future = ResponseFuture<
        <D::Service as Service<Req>>::Future,
        <D::Service as Service<Req>>::Response,
    >;

    fn poll_ready(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        let _ = self.update_pending_from_discover(cx)?;
        self.promote_pending_to_ready(cx);
        loop {
            // If a service has already been selected, ensure that it is ready.
            // This ensures that the underlying service is ready immediately
            // before a request is dispatched to it (i.e. in the same task
            // invocation). If, e.g., a failure detector has changed the state
            // of the service, it may be evicted from the ready set so that
            // another service can be selected.
            if let Some(index) = self.ready_index.take() {
                match self.services.check_ready_index(cx, index) {
                    Ok(true) => {
                        // The service remains ready.
                        self.ready_index = Some(index);
                        return Poll::Ready(Ok(()));
                    }
                    Ok(false) => {
                        // The service is no longer ready. Try to find a new
                        // one.
                        trace!("ready service became unavailable");
                    }
                    Err(Failed(_, error)) => {
                        // The ready endpoint failed, so log the error and try
                        // to find a new one.
                        debug!(%error, "endpoint failed");
                    }
                }
            }

            // Select a new service by comparing two at random and using the
            // lesser-loaded service.
            self.ready_index = self.ready_index();
            if self.ready_index.is_none() {
                debug_assert_eq!(self.services.ready_len(), 0);
                // We have previously registered interest in updates from
                // discover and pending services.
                return Poll::Pending;
            }
        }
    }

    fn call(&mut self, request: Req) -> Self::Future {
        let index = self.ready_index.take().expect("called before ready");
        let future = self.services.call_ready_index(index, request);
        ResponseFuture { future }
    }
}

pin_project! {
    pub struct ResponseFuture<F, Resp>
    where
        F: Future<Output = Result<Resp, Infallible>>,
    {
        #[pin]
        future: F,
    }
}

impl<F, Resp> Future for ResponseFuture<F, Resp>
where
    F: Future<Output = Result<Resp, Infallible>>,
{
    type Output = Result<Resp, Error>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        match ready!(this.future.poll(cx)) {
            Ok(resp) => Poll::Ready(Ok(resp)),
            Err(e) => match e {},
        }
    }
}
