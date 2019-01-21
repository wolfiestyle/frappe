//! Callback container for Stream.

use crate::sync::RwLock;
use maybe_owned::MaybeOwned;
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};

/// Function that becomes uncallable after it returns false.
///
/// Callbacks use a `MaybeOwned<T>` argument so we can choose at runtime if we will send a ref or an owned value.
struct FnCell<T> {
    f: Box<dyn Fn(MaybeOwned<'_, T>) -> bool + Send + Sync>,
    alive: AtomicBool,
}

impl<T> FnCell<T> {
    /// Creates a new `FnCell` from the supplied closure.
    fn new<F>(f: F) -> Self
    where
        F: Fn(MaybeOwned<'_, T>) -> bool + Send + Sync + 'static,
    {
        FnCell {
            f: Box::new(f),
            alive: AtomicBool::new(true),
        }
    }

    /// Calls the stored function and updates it's callable status.
    fn call(&self, arg: MaybeOwned<'_, T>) -> bool {
        if self.alive.load(Ordering::Relaxed) {
            let is_alive = (self.f)(arg);
            if !is_alive {
                self.alive.store(false, Ordering::Relaxed);
            }
            is_alive
        } else {
            false
        }
    }

    /// Checks if this function can still be called.
    fn is_alive(&self) -> bool {
        self.alive.load(Ordering::Relaxed)
    }
}

impl<T> fmt::Debug for FnCell<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "FnCell {{ f: Fn@{:p}, alive: {:?} }}",
            self.f, self.alive
        )
    }
}

/// A collection of callbacks.
#[derive(Debug)]
pub struct Callbacks<T> {
    fs: RwLock<Vec<FnCell<T>>>,
}

impl<T> Callbacks<T> {
    /// Creates an empty callback list.
    pub fn new() -> Self {
        Callbacks {
            fs: Default::default(),
        }
    }

    /// Adds a new closure to the callback list.
    pub fn push<F>(&self, cb: F)
    where
        F: Fn(MaybeOwned<'_, T>) -> bool + Send + Sync + 'static,
    {
        self.fs.write().push(FnCell::new(cb))
    }

    /// Sends an owned value.
    ///
    /// This sends a ref to the first N-1 callbacks, and the owned value to the last.
    pub fn call_owned(&self, arg: T) {
        let fs = self.fs.read();
        let n = fs.len();

        let mut i = 0;
        let mut all_alive = true;
        for _ in 1..n {
            all_alive &= fs[i].call(MaybeOwned::Borrowed(&arg));
            i += 1;
        }
        if n > 0 {
            all_alive &= fs[i].call(MaybeOwned::Owned(arg));
        }
        drop(fs);

        if !all_alive {
            self.cleanup();
        }
    }

    /// Sends a value by reference.
    pub fn call_ref(&self, arg: &T) {
        let all_alive = self
            .fs
            .read()
            .iter()
            .map(|f| f.call(MaybeOwned::Borrowed(arg)))
            .fold(true, |a, alive| a & alive);

        if !all_alive {
            self.cleanup();
        }
    }

    /// Sends a value.
    #[inline]
    pub fn call<'a>(&self, arg: impl Into<MaybeOwned<'a, T>>)
    where
        T: 'a,
    {
        match arg.into() {
            MaybeOwned::Owned(v) => self.call_owned(v),
            MaybeOwned::Borrowed(r) => self.call_ref(r),
        }
    }

    /// Removes the dead callbacks.
    fn cleanup(&self) {
        if let Some(mut fs) = self.fs.try_write() {
            fs.retain(FnCell::is_alive);
        }
    }

    #[cfg(test)]
    pub(crate) fn len(&self) -> usize {
        self.fs.read().len()
    }
}

impl<T> Default for Callbacks<T> {
    fn default() -> Self {
        Callbacks::new()
    }
}
