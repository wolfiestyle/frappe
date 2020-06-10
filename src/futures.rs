//! Futures integration.

use crate::stream::Stream;
use crate::sync::Mutex;
use std::future::Future;
use std::mem;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll, Waker};

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

impl<T> Default for StreamFutureStorage<T> {
    fn default() -> Self {
        StreamFutureStorage {
            value: FutureValue::Pending,
            waker: None,
        }
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

impl<T: Clone + Send + 'static> StreamFuture<T> {
    /// Creates a future that returns the next value sent to this stream.
    pub(crate) fn new(stream: Stream<T>) -> Self {
        let this = StreamFuture {
            storage: Default::default(),
            stream,
        };
        this.register_callback();
        this
    }

    /// Registers the stream observer that will update this future.
    fn register_callback(&self) {
        let weak = Arc::downgrade(&self.storage);
        self.stream.observe(move |val| {
            if let Some(st) = weak.upgrade() {
                let mut storage = st.lock();
                storage.value = FutureValue::Ready(val.into_owned());
                if let Some(waker) = storage.waker.take() {
                    waker.wake();
                }
            }
            false
        });
    }

    /// Obtains the source stream.
    #[inline]
    pub fn get_source(&self) -> &Stream<T> {
        &self.stream
    }

    /// Reuses a finished future so it can wait for another value.
    ///
    /// Normally calling `poll` on a future after it returned `Poll::Ready` will panic.
    /// This method will restart this future so it can be polled again for another value.
    /// This allows awaiting for multiple values without having to create and allocate multiple
    /// future objects.
    ///
    /// Calling this on a pending (or ready but unread) future will have no effect.
    pub fn reload(&self) {
        let mut storage = self.storage.lock();
        if let FutureValue::Finished = storage.value {
            *storage = Default::default();
            self.register_callback();
        }
    }
}

impl<T> Future for StreamFuture<T> {
    type Output = T;

    fn poll(self: Pin<&mut Self>, ctx: &mut Context) -> Poll<Self::Output> {
        let mut storage = self.storage.lock();
        match mem::replace(&mut storage.value, FutureValue::Pending) {
            FutureValue::Ready(value) => {
                storage.value = FutureValue::Finished;
                Poll::Ready(value)
            }
            FutureValue::Pending => {
                storage.waker = Some(ctx.waker().clone());
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

    #[test]
    fn reload() {
        let sink = Sink::new();
        let mut future = StreamFuture::new(sink.stream());

        sink.send(42);
        assert_eq!(block_on(&mut future), 42);

        future.reload();
        sink.send(13);
        assert_eq!(block_on(&mut future), 13);
    }
}
