//! Module that contains the selected version of Mutex/RwLock.

#[cfg(feature = "parking_lot")]
pub use parking_lot::{Mutex, RwLock};

#[cfg(not(feature = "parking_lot"))]
pub use self::wrapper::{Mutex, RwLock};

#[cfg(not(feature = "parking_lot"))]
#[allow(dead_code)]
mod wrapper {
    use std::sync::{MutexGuard, RwLockReadGuard, RwLockWriteGuard};

    #[derive(Debug, Default)]
    pub struct Mutex<T>(std::sync::Mutex<T>);

    impl<T> Mutex<T> {
        #[inline]
        pub fn new(val: T) -> Self {
            Mutex(std::sync::Mutex::new(val))
        }

        #[inline]
        pub fn lock(&self) -> MutexGuard<T> {
            self.0.lock().unwrap()
        }

        #[inline]
        pub fn try_lock(&self) -> Option<MutexGuard<T>> {
            self.0.try_lock().ok()
        }
    }

    #[derive(Debug, Default)]
    pub struct RwLock<T>(std::sync::RwLock<T>);

    impl<T> RwLock<T> {
        #[inline]
        pub fn new(val: T) -> Self {
            RwLock(std::sync::RwLock::new(val))
        }

        #[inline]
        pub fn read(&self) -> RwLockReadGuard<T> {
            self.0.read().unwrap()
        }

        #[inline]
        pub fn write(&self) -> RwLockWriteGuard<T> {
            self.0.write().unwrap()
        }

        #[inline]
        pub fn try_read(&self) -> Option<RwLockReadGuard<T>> {
            self.0.try_read().ok()
        }

        #[inline]
        pub fn try_write(&self) -> Option<RwLockWriteGuard<T>> {
            self.0.try_write().ok()
        }
    }
}
