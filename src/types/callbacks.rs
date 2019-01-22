//! Callback container for Stream.

use crate::sync::RwLock;
use maybe_owned::MaybeOwned;
use std::fmt;
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(feature = "crossbeam-utils")]
use crossbeam_utils::thread;

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

    /// Sends a value using multiple threads.
    #[cfg(feature = "crossbeam-utils")]
    pub fn call_parallel(&self, arg: &T)
    where
        T: Sync,
    {
        let fs = self.fs.read();
        let n = fs.len();
        // nothing to do
        if n == 0 {
            return;
        }
        // only 1 callback, just run it on this thread
        if n == 1 {
            if !fs[0].call(MaybeOwned::Borrowed(arg)) {
                self.cleanup();
            }
            return;
        }
        // 2+ callbacks, we need more threads
        let all_alive = AtomicBool::new(true);
        thread::scope(|scope| {
            let all_alive = &all_alive;

            let mut i = 0;
            // spawn N-1 threads
            for _ in 1..n {
                let f = &fs[i];
                scope.spawn(move |_| {
                    if !f.call(MaybeOwned::Borrowed(arg)) {
                        all_alive.store(false, Ordering::Relaxed);
                    }
                });
                i += 1;
            }
            // run the last callback on current thread
            let f = &fs[i];
            if !f.call(MaybeOwned::Borrowed(arg)) {
                all_alive.store(false, Ordering::Relaxed);
            }
        })
        .unwrap();
        // after all threads finished, continue with the cleanup
        drop(fs);
        if all_alive.load(Ordering::Relaxed) {
            self.cleanup();
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
