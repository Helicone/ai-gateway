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
use pin_project::pin_project;
use tower::{Service, discover::Discover};

use super::LatencyRouter;

pub struct MakeRouter<S, ReqBody, P> {
    inner: S,
    _marker: PhantomData<fn(ReqBody, P)>,
}

#[pin_project]
pub struct MakeFuture<F, ReqBody, P> {
    #[pin]
    inner: F,
    _marker: PhantomData<fn(ReqBody, P)>,
}

impl<S, ReqBody, P> MakeRouter<S, ReqBody, P> {
    /// Build routers using operating system entropy.
    pub const fn new(make_discover: S) -> Self {
        Self {
            inner: make_discover,
            _marker: PhantomData,
        }
    }
}

impl<S, ReqBody, P> Clone for MakeRouter<S, ReqBody, P>
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

impl<S, Target, ReqBody, P> Service<Target> for MakeRouter<S, ReqBody, P>
where
    S: Service<Target>,
    S::Response: Discover,
    <S::Response as Discover>::Key: Hash + Send + Sync,
    <S::Response as Discover>::Service:
        Service<http::Request<ReqBody>, Error = Infallible>,
    P: Hash,
{
    type Response = LatencyRouter<P, S::Response, ReqBody>;
    type Error = S::Error;
    type Future = MakeFuture<S::Future, ReqBody, P>;

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

impl<S, ReqBody, P> fmt::Debug for MakeRouter<S, ReqBody, P>
where
    S: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let Self { inner, _marker } = self;
        f.debug_struct("MakeRouter").field("inner", inner).finish()
    }
}

impl<F, T, E, ReqBody, P> Future for MakeFuture<F, ReqBody, P>
where
    F: Future<Output = Result<T, E>>,
    T: Discover,
    <T as Discover>::Key: Hash + Send + Sync,
    <T as Discover>::Service:
        Service<http::Request<ReqBody>, Error = Infallible>,
    P: Hash,
{
    type Output = Result<LatencyRouter<P, T, ReqBody>, E>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let this = self.project();
        let inner = ready!(this.inner.poll(cx))?;
        let svc = LatencyRouter::new(inner);
        Poll::Ready(Ok(svc))
    }
}

impl<F, Req, P> fmt::Debug for MakeFuture<F, Req, P>
where
    F: fmt::Debug,
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let Self { inner, _marker } = self;
        f.debug_struct("MakeFuture").field("inner", inner).finish()
    }
}
