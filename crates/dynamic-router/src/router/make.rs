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
use std::{
    convert::Infallible,
    fmt,
    future::Future,
    hash::Hash,
    marker::PhantomData,
    pin::Pin,
    task::{Context, Poll},
};

use futures::ready;
use http::Request;
use pin_project_lite::pin_project;
use tower::{Service, discover::Discover};

use super::DynamicRouter;

/// Constructs load balancers over dynamic service sets produced by a wrapped
/// "inner" service.
///
/// This is effectively an implementation of [`MakeService`] except that it
/// forwards the service descriptors (`Target`) to an inner service (`S`), and
/// expects that service to produce a service set in the form of a [`Discover`].
/// It then wraps the service set in a [`Balance`] before returning it as the
/// "made" service.
///
/// See the [module-level documentation](crate::balance) for details on load
/// balancing.
///
/// [`MakeService`]: crate::MakeService
/// [`Discover`]: crate::discover::Discover
/// [`Balance`]: crate::balance::p2c::Balance
pub struct MakeRouter<S, ReqBody> {
    inner: S,
    _marker: PhantomData<fn(ReqBody)>,
}

pin_project! {
    pub struct MakeFuture<F, ReqBody> {
        #[pin]
        inner: F,
        _marker: PhantomData<fn(ReqBody)>,
    }
}

impl<S, ReqBody> MakeRouter<S, ReqBody> {
    /// Build routers using operating system entropy.
    pub const fn new(make_discover: S) -> Self {
        Self {
            inner: make_discover,
            _marker: PhantomData,
        }
    }
}

impl<S, ReqBody> Clone for MakeRouter<S, ReqBody>
where
    S: Clone,
{
    fn clone(&self) -> Self {
        Self {
            inner: self.inner.clone(),
            _marker: PhantomData,
        }
    }
}

impl<S, Target, ReqBody> Service<Target> for MakeRouter<S, ReqBody>
where
    S: Service<Target>,
    S::Response: Discover,
    <S::Response as Discover>::Key: Hash + Send + Sync,
    <S::Response as Discover>::Service:
        Service<Request<ReqBody>, Error = Infallible>,
{
    type Response = DynamicRouter<S::Response, ReqBody>;
    type Error = S::Error;
    type Future = MakeFuture<S::Future, ReqBody>;

    fn poll_ready(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        self.inner.poll_ready(cx)
    }

    fn call(&mut self, target: Target) -> Self::Future {
        MakeFuture {
            inner: self.inner.call(target),
            _marker: PhantomData,
        }
    }
}

impl<S, ReqBody> fmt::Debug for MakeRouter<S, ReqBody>
where
    S: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let Self { inner, _marker } = self;
        f.debug_struct("MakeRouter").field("inner", inner).finish()
    }
}

impl<F, T, E, ReqBody> Future for MakeFuture<F, ReqBody>
where
    F: Future<Output = Result<T, E>>,
    T: Discover,
    <T as Discover>::Key: Hash + Send + Sync,
    <T as Discover>::Service: Service<Request<ReqBody>, Error = Infallible>,
{
    type Output = Result<DynamicRouter<T, ReqBody>, E>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let inner = ready!(this.inner.poll(cx))?;
        let svc = DynamicRouter::new(inner);
        Poll::Ready(Ok(svc))
    }
}

impl<F, ReqBody> fmt::Debug for MakeFuture<F, ReqBody>
where
    F: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let Self { inner, _marker } = self;
        f.debug_struct("MakeFuture").field("inner", inner).finish()
    }
}
