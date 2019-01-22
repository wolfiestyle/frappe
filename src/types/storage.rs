//! Storage cell used by Signal.

use crate::sync::Mutex;

/// Storage cell for shared signal values.
pub struct Storage<T> {
    val: Mutex<Option<T>>,
}

const ERR_EMPTY: &str = "storage empty";

impl<T> Storage<T> {
    /// Creates a storage with an initial value.
    pub fn new(val: T) -> Self {
        Storage {
            val: Mutex::new(Some(val)),
        }
    }

    /// Gets the value by cloning.
    pub fn get(&self) -> T
    where
        T: Clone,
    {
        self.val.lock().clone().expect(ERR_EMPTY)
    }

    /// Sets the value.
    pub fn set(&self, val: T) {
        *self.val.lock() = Some(val);
    }

    /// Maps the stored value in place.
    pub fn replace<F>(&self, f: F)
    where
        F: FnOnce(T) -> T,
    {
        let mut st = self.val.lock();
        let old = st.take().expect(ERR_EMPTY);
        *st = Some(f(old));
    }

    /// Same as `replace` but it also returns the new value.
    pub fn replace_fetch<F>(&self, f: F) -> T
    where
        F: FnOnce(T) -> T,
        T: Clone,
    {
        let mut st = self.val.lock();
        let old = st.take().expect(ERR_EMPTY);
        let new = f(old);
        *st = Some(new.clone());
        new
    }

    /// A `replace` version with cloning.
    pub fn replace_clone<F>(&self, f: F)
    where
        F: FnOnce(T) -> T,
        T: Clone,
    {
        let mut st = self.val.lock();
        let old = st.clone().expect(ERR_EMPTY);
        *st = Some(f(old));
    }
}

impl<T> Default for Storage<T> {
    /// Creates an empty storage.
    fn default() -> Self {
        Storage {
            val: Default::default(),
        }
    }
}
