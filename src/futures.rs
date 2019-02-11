//! Futures integration.

use crate::stream::Stream;
use crate::sync::Mutex;
use std::future::Future;
use std::mem;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{LocalWaker, Poll, Waker};

/// The state a stream future.
#[derive(Debug)]
enum FutureValue<T> {
    Pending,
    Ready(T),
    Finished,
}

/// The storage of a stream future.
#[derive(Debug)]
struct StreamFutureStorage<T> {
    value: FutureValue<T>,
    waker: Option<Waker>,
}

impl<T> StreamFutureStorage<T> {
    fn new() -> Arc<Mutex<Self>> {
        Arc::new(Mutex::new(StreamFutureStorage {
            value: FutureValue::Pending,
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
        let weak = Arc::downgrade(&storage);
        stream.observe(move |val| {
            if let Some(st) = weak.upgrade() {
                let mut storage = st.lock();
                storage.value = FutureValue::Ready(val.into_owned());
                if let Some(waker) = &storage.waker {
                    waker.wake();
                }
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
        match mem::replace(&mut storage.value, FutureValue::Pending) {
            FutureValue::Ready(value) => {
                storage.value = FutureValue::Finished;
                Poll::Ready(value)
            }
            FutureValue::Pending => {
                storage.waker = Some(lw.clone().into_waker());
                Poll::Pending
            }
            FutureValue::Finished => {
                storage.value = FutureValue::Finished;
                panic!("future polled again after completion");
            }
        }
    }
}

impl<T> Unpin for StreamFuture<T> {}

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

    #[test]
    #[should_panic]
    fn invalid_poll() {
        let sink = Sink::new();
        let mut future = StreamFuture::new(sink.stream());

        sink.send(42);
        let _a = block_on(&mut future);
        let _b = block_on(&mut future);
    }
}
