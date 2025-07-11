pub mod dispatcher;
pub mod model;
pub mod monitor;
pub mod provider;
pub mod router;

use std::{
    collections::HashMap,
    pin::Pin,
    task::{Context, Poll},
};

use futures::Stream;
use pin_project_lite::pin_project;
use tower::discover::Change;

pin_project! {
    /// Static service discovery based on a predetermined map of services.
    ///
    /// [`ServiceMap`] is created with an initial map of services. The discovery
    /// process will yield this map once and do nothing after.
    #[derive(Debug)]
    pub(crate) struct ServiceMap<K, V> {
        inner: std::collections::hash_map::IntoIter<K, V>,
    }
}

impl<K, V> ServiceMap<K, V>
where
    K: std::hash::Hash + Eq,
{
    pub fn new<Request>(services: HashMap<K, V>) -> ServiceMap<K, V>
    where
        V: tower::Service<Request>,
    {
        ServiceMap {
            inner: services.into_iter(),
        }
    }
}

impl<K, V> Stream for ServiceMap<K, V>
where
    K: std::hash::Hash + Eq + Clone,
{
    type Item = Change<K, V>;

    fn poll_next(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        match self.project().inner.next() {
            Some((key, service)) => {
                Poll::Ready(Some(Change::Insert(key, service)))
            }
            None => Poll::Ready(None),
        }
    }
}
