use parking_lot::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};

/// Storage cell for shared signal values.
pub struct Storage<T> {
    val: Mutex<Option<T>>,
    serial: AtomicUsize,
}

const ERR_EMPTY: &str = "storage empty";

impl<T> Storage<T> {
    /// Creates a storage with an initial value.
    pub fn new(val: T) -> Self {
        Storage {
            val: Mutex::new(Some(val)),
            serial: AtomicUsize::new(1),
        }
    }

    /// Gets the value by cloning.
    pub fn get(&self) -> T
    where
        T: Clone,
    {
        self.val.lock().clone().expect(ERR_EMPTY)
    }

    /// Sets value and increments the serial.
    pub fn set(&self, val: T) {
        *self.val.lock() = Some(val);
        self.inc_serial();
    }

    /// Replaces the stored value and returns the previous one.
    pub fn replace(&self, val: T) -> T {
        let old = self.val.lock().replace(val);
        self.inc_serial();
        old.expect(ERR_EMPTY)
    }

    /// Passes the stored value through a function.
    pub fn replace_with<F>(&self, f: F)
    where
        F: FnOnce(T) -> T,
    {
        let mut st = self.val.lock();
        let old = st.take().expect(ERR_EMPTY);
        *st = Some(f(old));
        drop(st);
        self.inc_serial();
    }

    /// Increments the serial of this storage.
    fn inc_serial(&self) {
        self.serial.fetch_add(1, Ordering::Release);
    }
}

impl<T> Default for Storage<T> {
    /// Creates an empty storage.
    fn default() -> Self {
        Storage {
            val: Default::default(),
            serial: Default::default(),
        }
    }
}
