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

/// A signal that contains only storage.
pub struct SharedStorage<T, S> {
    storage: Storage<T>,
    _source: S,
}

impl<T, S> SharedStorage<T, S> {
    pub fn new(initial: T, source: S) -> Self {
        SharedStorage {
            storage: Storage::new(initial),
            _source: source,
        }
    }
}

impl<T, S> SharedSignal<T> for SharedStorage<T, S> {
    fn get_storage(&self) -> &Storage<T> {
        &self.storage
    }

    fn sample(&self) -> &Storage<T> {
        &self.storage
    }
}

impl<T, S> ops::Deref for SharedStorage<T, S> {
    type Target = Storage<T>;

    fn deref(&self) -> &Self::Target {
        &self.storage
    }
}

/// A signal that maps a parent shared signal.
pub struct SharedMap<T, S, F> {
    storage: Storage<T>,
    source: Arc<dyn SharedSignal<S> + Send + Sync>,
    f: F,
}

impl<T, S, F> SharedMap<T, S, F> {
    pub fn new(source: Arc<dyn SharedSignal<S> + Send + Sync>, f: F) -> Arc<Self> {
        Arc::new(SharedMap {
            storage: Default::default(),
            source,
            f,
        })
    }
}

impl<T, S, F> SharedSignal<T> for SharedMap<T, S, F>
where
    F: Fn(S) -> T,
    S: Clone,
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

/// A signal that folds a parent signal.
pub struct SharedFold<T, S, F> {
    storage: Storage<T>,
    source: S,
    f: F,
}

impl<T, S, F> SharedFold<T, S, F> {
    pub fn new(initial: T, f: F, source: S) -> Arc<Self> {
        Arc::new(SharedFold {
            storage: Storage::new(initial),
            source,
            f,
        })
    }
}

impl<T, S, V, F> SharedSignal<T> for SharedFold<T, S, F>
where
    F: Fn(T, V) -> T,
    S: Fn() -> V,
{
    fn get_storage(&self) -> &Storage<T> {
        &self.storage
    }

    fn sample(&self) -> &Storage<T> {
        let val = (self.source)();
        self.storage.replace_with(|acc| (self.f)(acc, val));
        &self.storage
    }
}

/// A signal that folds a channel.
pub struct SharedChannel<T, S, F> {
    storage: Storage<T>,
    source: Mutex<mpsc::Receiver<S>>,
    f: F,
}

impl<T, S, F> SharedChannel<T, S, F> {
    pub fn new(initial: T, rx: mpsc::Receiver<S>, f: F) -> Arc<Self> {
        Arc::new(SharedChannel {
            storage: Storage::new(initial),
            source: Mutex::new(rx),
            f,
        })
    }
}

impl<T, S, F> SharedSignal<T> for SharedChannel<T, S, F>
where
    F: Fn(T, S) -> T,
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
