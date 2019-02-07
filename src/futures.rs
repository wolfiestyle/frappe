//! Futures integration.

use crate::stream::Stream;
use crate::sync::Mutex;
use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{LocalWaker, Poll, Waker};

/// The storage of a stream future.
#[derive(Debug)]
struct StreamFutureStorage<T> {
    value: Option<T>,
    waker: Option<Waker>,
}

impl<T> StreamFutureStorage<T> {
    fn new() -> Arc<Mutex<Self>> {
        Arc::new(Mutex::new(StreamFutureStorage {
            value: None,
            waker: None,
        }))
    }
}

/// A future that waits for a stream value.
///
/// This is created by `Stream::next`.
#[derive(Debug)]
pub struct StreamFuture<T> {
    storage: Arc<Mutex<StreamFutureStorage<T>>>,
    stream: Stream<T>,
}

impl<T> StreamFuture<T> {
    /// Creates a future that returns the next value sent to this stream.
    pub(crate) fn new(stream: Stream<T>) -> Self
    where
        T: Clone + Send + 'static,
    {
        let storage = StreamFutureStorage::new();
        let st = storage.clone();
        stream.observe(move |val| {
            let mut storage = st.lock();
            storage.value = Some(val.into_owned());
            if let Some(waker) = &storage.waker {
                waker.wake();
            }
            false
        });
        StreamFuture { storage, stream }
    }

    /// Obtains the source stream.
    pub fn get_source(&self) -> &Stream<T> {
        &self.stream
    }
}

impl<T> Future for StreamFuture<T> {
    type Output = T;

    fn poll(self: Pin<&mut Self>, lw: &LocalWaker) -> Poll<Self::Output> {
        let mut storage = self.storage.lock();
        if let Some(value) = storage.value.take() {
            Poll::Ready(value)
        } else {
            storage.waker = Some(lw.clone().into_waker());
            Poll::Pending
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::stream::Sink;
    use futures::executor::block_on;

    #[test]
    fn basic() {
        let sink = Sink::new();
        let future = StreamFuture::new(sink.stream());

        sink.send(42);
        sink.send(13);
        assert_eq!(block_on(future), 42);
    }
}
