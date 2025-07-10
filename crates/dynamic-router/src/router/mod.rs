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
    fmt,
    hash::Hash,
    marker::PhantomData,
    pin::Pin,
    task::{Context, Poll},
};

use futures::{
    future::{self, FutureExt, TryFutureExt},
    ready,
};
use tower::{
    Service,
    discover::{Change, Discover},
    ready_cache::{ReadyCache, error::Failed},
};
use tracing::{debug, trace};

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("Not found: {0}")]
    NotFound(String),
    #[error("Inner service error: {0}")]
    InnerService(tower::BoxError),
    #[error("Discover error: {0}")]
    Discover(tower::BoxError),
}

/// Efficiently distributes requests across an arbitrary number of services.
///
/// See the [module-level documentation](..) for details.
///
/// Note that [`WeightedBalance`] requires that the [`Discover`] you use is
/// [`Unpin`] in order to implement [`Service`]. This is because it needs to be
/// accessed from [`Service::poll_ready`], which takes `&mut self`. You can
/// achieve this easily by wrapping your [`Discover`] in [`Box::pin`] before you
/// construct the [`WeightedBalance`] instance. For more details, see [#319].
///
/// [`Box::pin`]: std::boxed::Box::pin()
/// [#319]: https://github.com/tower-rs/tower/issues/319
pub struct DynamicRouter<D, ReqBody>
where
    D: Discover,
    D::Key: Hash + Send + Sync,
{
    discover: D,

    services: ReadyCache<D::Key, D::Service, http::Request<ReqBody>>,

    _req: PhantomData<ReqBody>,
}

impl<D: Discover, ReqBody> fmt::Debug for DynamicRouter<D, ReqBody>
where
    D: fmt::Debug,
    D::Key: Hash + fmt::Debug + Send + Sync,
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
    D::Key: Hash + Send + Sync,
    D::Service: Service<http::Request<ReqBody>>,
    <D::Service as Service<http::Request<ReqBody>>>::Error:
        Into<tower::BoxError>,
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
    D::Key: Hash + Clone + Send + Sync,
    D::Error: Into<tower::BoxError>,
    D::Service: Service<http::Request<ReqBody>>,
    <D::Service as Service<http::Request<ReqBody>>>::Error:
        Into<tower::BoxError>,
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

    // fn ready_index(&mut self) -> Result<Option<usize>, Error> {
    //     match self.services.ready_len() {
    //         0 => Ok(None),
    //         _ => {
    //             let request = self.services.get_ready(key);
    //         } // 1 => Ok(Some(0)),
    //           // len => {
    //           //     // let sample_fn = |idx| {
    //           //     //     let (key, _service) = self
    //           //     //         .services
    //           //     //         .get_ready_index(idx)
    //           //     //         .expect("invalid index");

    //           //     //     key.weight()
    //           //     // };
    //           //     // // NOTE: This is O(n) over number of services, but it
    // can           //     // // be made to O(1) using precomputed
    // probability tables as           //     // // described here: https://www.keithschwarz.com/darts-dice-coins/
    //           //     // let sample = rand::seq::index::sample_weighted(
    //           //     //     &mut self.rng,
    //           //     //     len,
    //           //     //     sample_fn,
    //           //     //     1,
    //           //     // )?;
    //           //     // let chosen = sample.index(0);
    //           //     // trace!(chosen = chosen, "p2c");

    //           //     // TODO: Implement dynamic router

    //           //     Ok(None)
    //           // }
    //     }
    // }
}

impl<D, ReqBody> Service<http::Request<ReqBody>> for DynamicRouter<D, ReqBody>
where
    D: Discover + Unpin,
    D::Key: Hash + Clone + Send + Sync + 'static,
    D::Error: Into<tower::BoxError>,
    D::Service: Service<http::Request<ReqBody>>,
    <D::Service as Service<http::Request<ReqBody>>>::Future: Send + 'static,
    <D::Service as Service<http::Request<ReqBody>>>::Error:
        Into<tower::BoxError> + Send + 'static,
    <<D as tower::discover::Discover>::Service as Service<
        http::Request<ReqBody>,
    >>::Response: Send + 'static,
{
    type Response = <D::Service as Service<http::Request<ReqBody>>>::Response;
    type Error = Error;
    type Future = futures::future::BoxFuture<
        'static,
        Result<Self::Response, Self::Error>,
    >;

    fn poll_ready(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        tracing::trace!("DynamicRouter::poll_ready");

        // `ready_index` may have already been set by a prior invocation. These
        // updates cannot disturb the order of existing ready services.
        let _ = self.update_pending_from_discover(cx)?;
        self.promote_pending_to_ready(cx);

        // TODO: REMOVE FOLLOWING LINES OF COMMENTS BEFORE MERGING
        // const MAX_RETRIES: usize = 10;
        // let mut retries = 0;
        // while retries < MAX_RETRIES {
        //     let mut all_ready = true;
        //     for (_, svc) in self.services.iter_ready_mut() {
        //         // ignor
        //         match svc.poll_ready(cx) {
        //             Poll::Ready(Ok(())) => {
        //                 continue;
        //             }
        //             Poll::Pending => {
        //                 all_ready = false;
        //             }
        //             Poll::Ready(Err(_e)) => {
        //                 all_ready = false;
        //             }
        //         }
        //         // match self.services.check_ready(cx, router.0) {
        //         //     Ok(true) => {
        //         //         continue;
        //         //     }
        //         //     Ok(false) => {
        //         //         all_ready = false;
        //         //     }
        //         //     Err(Failed(_, error)) => {
        //         //         all_ready = false;
        //         //     }
        //         // }
        //     }

        //     if all_ready {
        //         return Poll::Ready(Ok(()));
        //     }

        //     retries += 1;
        // }

        // Poll::Pending
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, request: http::Request<ReqBody>) -> Self::Future {
        tracing::trace!("DynamicRouter::call");
        let key = request.extensions().get::<D::Key>().unwrap().clone();
        self.services
            .call_ready(&key, request)
            .map_err(|e| Error::InnerService(e.into()))
            .boxed()

        // let (_, _, service) = match self.services.get_ready_mut(&key) {
        //     Some(result) => result,
        //     None => {
        //         return futures::future::ready(Err(Error::NotFound(
        //             request.uri().path().to_string(),
        //         )))
        //         .boxed();
        //     }
        // };

        // service
        //     .call(request)
        //     .map_err(|e| Error::InnerService(e.into()))
        //     .boxed()
    }
}
