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
    fmt::{self, Display},
    hash::Hash,
    marker::PhantomData,
    pin::Pin,
    task::{Context, Poll},
};

use futures::ready;
use pin_project::pin_project;
use tower::{
    Service,
    discover::{Change, Discover},
    ready_cache::ReadyCache,
};
use tracing::{debug, trace};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Service Key extension not found")]
    ExtensionNotFound,
    #[error("Discover error: {0}")]
    Discover(tower::BoxError),
    #[error("Router not found: {0}")]
    RouterNotFound(String),
}

pub struct DynamicRouter<D, ReqBody>
where
    D: Discover,
    D::Key: Hash + Send + Sync + Display,
{
    discover: D,

    services: ReadyCache<D::Key, D::Service, http::Request<ReqBody>>,

    _req: PhantomData<ReqBody>,
}

impl<D: Discover, ReqBody> fmt::Debug for DynamicRouter<D, ReqBody>
where
    D: fmt::Debug,
    D::Key: Hash + fmt::Debug + Send + Sync + Display,
    D::Service: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("DynamicRouter")
            .field("discover", &self.discover)
            .field("services", &self.services)
            .finish_non_exhaustive()
    }
}

impl<D, ReqBody> DynamicRouter<D, ReqBody>
where
    D: Discover,
    D::Key: Hash + Send + Sync + Display,
    D::Service: Service<http::Request<ReqBody>, Error = Infallible>,
{
    pub fn new(discover: D) -> Self {
        tracing::trace!("DynamicRouter::new");
        Self {
            discover,
            services: ReadyCache::default(),

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

impl<D, ReqBody> DynamicRouter<D, ReqBody>
where
    D: Discover + Unpin,
    D::Key: Hash + Clone + Send + Sync + Display,
    D::Error: Into<tower::BoxError>,
    D::Service: Service<http::Request<ReqBody>, Error = Infallible>,
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
}

impl<D, ReqBody> Service<http::Request<ReqBody>> for DynamicRouter<D, ReqBody>
where
    D: Discover + Unpin,
    D::Key: Hash + Clone + Send + Sync + Display + 'static,
    D::Error: Into<tower::BoxError>,
    D::Service: Service<http::Request<ReqBody>, Error = Infallible>,
    <D::Service as Service<http::Request<ReqBody>>>::Future: Send + 'static,
    <<D as tower::discover::Discover>::Service as Service<
        http::Request<ReqBody>,
    >>::Response: Send + 'static,
{
    type Response = <D::Service as Service<http::Request<ReqBody>>>::Response;
    type Error = Error;
    type Future = ResponseFuture<D, ReqBody>;

    fn poll_ready(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        let _ = self.update_pending_from_discover(cx)?;
        self.promote_pending_to_ready(cx);
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: http::Request<ReqBody>) -> Self::Future {
        let Some(key) = request.extensions().get::<D::Key>().cloned() else {
            return ResponseFuture::Ready {
                error: Some(Error::ExtensionNotFound),
            };
        };

        if let Some((_, _, _)) = self.services.get_ready(&key) {
            let future = self.services.call_ready(&key, request);
            ResponseFuture::Inner { future }
        } else {
            ResponseFuture::Ready {
                error: Some(Error::RouterNotFound(key.to_string())),
            }
        }
    }
}

#[pin_project(project = ResponseFutureProj)]
pub enum ResponseFuture<D, ReqBody>
where
    D: Discover + Unpin,
    D::Key: Hash + Clone + Send + Sync + Display + 'static,
    D::Error: Into<tower::BoxError>,
    D::Service: Service<http::Request<ReqBody>, Error = Infallible>,
    <D::Service as Service<http::Request<ReqBody>>>::Future: Send + 'static,
    <<D as tower::discover::Discover>::Service as Service<
        http::Request<ReqBody>,
    >>::Response: Send + 'static,
{
    Ready {
        error: Option<Error>,
    },
    Inner {
        #[pin]
        future: <D::Service as Service<http::Request<ReqBody>>>::Future,
    },
}

impl<D, ReqBody> Future for ResponseFuture<D, ReqBody>
where
    D: Discover + Unpin,
    D::Key: Hash + Clone + Send + Sync + Display + 'static,
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
            ResponseFutureProj::Ready { error } => Poll::Ready(Err(error
                .take()
                .expect("future polled after completion"))),
            ResponseFutureProj::Inner { future } => {
                match ready!(future.poll(cx)) {
                    Ok(res) => Poll::Ready(Ok(res)),
                    // never happens due to `Infallible` bound
                    Err(e) => match e {},
                }
            }
        }
    }
}
