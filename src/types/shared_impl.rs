use crate::types::Storage;
use parking_lot::Mutex;
use std::ops;
use std::sync::{mpsc, Arc};

/// Defines a signal that contains shared storage.
pub trait SharedSignal<T> {
    /// Obtains the internal storage.
    fn get_storage(&self) -> &Storage<T>;
    /// Samples the signal.
    fn sample(&self) -> &Storage<T>;
}

/// Common template for shared signal implementations.
pub struct SharedImpl<T, S, F> {
    pub storage: Storage<T>,
    pub source: S,
    pub f: F,
}

impl<T, S, F> SharedImpl<T, S, F> {
    pub fn wrap(self) -> Arc<Self> {
        Arc::new(self)
    }
}

impl<T, S, F> ops::Deref for SharedImpl<T, S, F> {
    type Target = Storage<T>;

    fn deref(&self) -> &Self::Target {
        &self.storage
    }
}

// A signal that contains only storage.
impl<T, S> SharedImpl<T, S, ()> {
    pub fn new(initial: T, source: S) -> Self {
        SharedImpl {
            storage: Storage::new(initial),
            source,
            f: (),
        }
    }
}

impl<T, S> SharedSignal<T> for SharedImpl<T, S, ()> {
    fn get_storage(&self) -> &Storage<T> {
        &self.storage
    }

    fn sample(&self) -> &Storage<T> {
        &self.storage
    }
}

// A signal that maps a parent shared signal.
impl<T, P, F> SharedSignal<T> for SharedImpl<T, Arc<dyn SharedSignal<P> + Send + Sync>, F>
where
    F: Fn(P) -> T + 'static,
    P: Clone,
{
    fn get_storage(&self) -> &Storage<T> {
        &self.storage
    }

    fn sample(&self) -> &Storage<T> {
        let res = (self.f)(self.source.sample().get());
        self.storage.set(res);
        &self.storage
    }
}

// A signal that folds a channel.
impl<T, S, F> SharedSignal<T> for SharedImpl<T, Mutex<mpsc::Receiver<S>>, F>
where
    F: Fn(T, S) -> T + 'static,
{
    fn get_storage(&self) -> &Storage<T> {
        &self.storage
    }

    fn sample(&self) -> &Storage<T> {
        let source = self.source.lock();
        if let Ok(first) = source.try_recv() {
            self.storage.replace_with(|old| {
                let acc = (self.f)(old, first);
                source.try_iter().fold(acc, &self.f)
            });
        }
        &self.storage
    }
}
