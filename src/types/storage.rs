use parking_lot::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

/// Storage cell for shared signal values.
pub struct Storage<T> {
    val: Mutex<Option<T>>,
    serial: AtomicUsize,
    root_ser: Arc<AtomicUsize>,
}

const ERR_EMPTY: &str = "storage empty";

impl<T> Storage<T> {
    /// Creates a storage with a new root serial.
    pub fn new(val: T) -> Self {
        Storage {
            val: Mutex::new(Some(val)),
            serial: AtomicUsize::new(1),
            root_ser: Arc::new(AtomicUsize::new(1)),
        }
    }

    /// Creates a storage with an inherited root serial.
    pub fn inherit<P>(parent: &Storage<P>) -> Self {
        Storage {
            val: Default::default(),
            serial: Default::default(),
            root_ser: parent.root_ser.clone(),
        }
    }

    /// Gets the value by cloning.
    pub fn get(&self) -> T
    where
        T: Clone,
    {
        self.val.lock().clone().expect(ERR_EMPTY)
    }

    /// Sets value and increments the root serial.
    ///
    /// This is called by source signals.
    pub fn set(&self, val: T) {
        let mut st = self.val.lock();
        *st = Some(val);
        self.inc_root();
    }

    /// Sets value and increments the local serial.
    ///
    /// This is called by mapped (child) signals.
    pub fn set_local(&self, val: T) {
        let mut st = self.val.lock();
        *st = Some(val);
        self.inc_local();
    }

    /// Replaces the stored value.
    pub fn replace(&self, val: T) -> T {
        let mut st = self.val.lock();
        let old = st.take().expect(ERR_EMPTY);
        *st = Some(val);
        self.inc_root();
        old
    }

    /// Passes the stored value through a function.
    pub fn replace_with<F>(&self, f: F)
    where
        F: FnOnce(T) -> T,
    {
        let mut st = self.val.lock();
        let old = st.take().expect(ERR_EMPTY);
        *st = Some(f(old));
        self.inc_root();
    }

    /// Checks if a parent storage has changed, so this needs update.
    pub fn must_update(&self) -> bool {
        self.root_ser.load(Ordering::Relaxed) > self.serial.load(Ordering::Relaxed)
    }

    /// Increments the serial of a source signal, so child must update.
    pub fn inc_root(&self) {
        self.root_ser.fetch_add(1, Ordering::Relaxed);
    }

    /// Marks this storage as updated.
    pub fn inc_local(&self) {
        self.serial
            .store(self.root_ser.load(Ordering::Relaxed), Ordering::Relaxed);
    }
}

impl<T> Default for Storage<T> {
    fn default() -> Self {
        Storage {
            val: Default::default(),
            serial: Default::default(),
            root_ser: Arc::new(AtomicUsize::new(1)),
        }
    }
}
