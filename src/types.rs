//! Miscellaneous types used by the library.

use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, AtomicBool, Ordering};
use std::{fmt, ops};
use parking_lot::RwLock;

pub use maybe_owned::MaybeOwned;
#[cfg(feature="either")]
pub use either::Either;

/// Function that becomes uncallable after it returns false.
///
/// Callbacks use a MaybeOwned<T> argument so we can choose at runtime if we will send a ref or an owned value.
struct FnCell<T>
{
    f: Box<dyn Fn(MaybeOwned<'_, T>) -> bool + Send + Sync>,
    alive: AtomicBool,
}

impl<T> FnCell<T>
{
    /// Creates a new `FnCell` from the supplied closure.
    fn new<F>(f: F) -> Self
        where F: Fn(MaybeOwned<'_, T>) -> bool + Send + Sync + 'static
    {
        FnCell{ f: Box::new(f), alive: AtomicBool::new(true) }
    }

    /// Calls the stored function and updates it's callable status.
    fn call(&self, arg: MaybeOwned<'_, T>) -> bool
    {
        let is_alive = self.alive.load(Ordering::Acquire) && (self.f)(arg);
        if !is_alive { self.alive.store(false, Ordering::Release); }
        is_alive
    }

    /// Checks if this function can still be called.
    fn is_alive(&self) -> bool
    {
        self.alive.load(Ordering::Relaxed)
    }
}

impl<T> fmt::Debug for FnCell<T>
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result
    {
        write!(f, "FnCell {{ f: Fn@{:p}, alive: {:?} }}", self.f, self.alive)
    }
}

/// A collection of callbacks.
#[derive(Debug)]
pub(crate) struct Callbacks<T>
{
    fs: RwLock<Vec<FnCell<T>>>,
}

impl<T> Callbacks<T>
{
    /// Creates an empty callback list.
    pub fn new() -> Self
    {
        Callbacks{ fs: Default::default() }
    }

    /// Adds a new closure to the callback list.
    pub fn push<F>(&self, cb: F)
        where F: Fn(MaybeOwned<'_, T>) -> bool + Send + Sync + 'static
    {
        self.fs.write().push(FnCell::new(cb))
    }

    /// Sends an owned value.
    ///
    /// This sends a ref to the first N-1 callbacks, and the owned value to the last.
    pub fn call(&self, arg: T)
    {
        let fs = self.fs.read();
        let n = fs.len();

        let mut i = 0;
        let mut all_alive = true;
        for _ in 1..n
        {
            all_alive &= fs[i].call(MaybeOwned::Borrowed(&arg));
            i += 1;
        }
        if n > 0 { all_alive &= fs[i].call(MaybeOwned::Owned(arg)) }
        drop(fs);

        if !all_alive { self.cleanup(); }
    }

    /// Sends a value by reference.
    pub fn call_ref(&self, arg: &T)
    {
        let all_alive = self.fs.read().iter()
            .map(|f| f.call(MaybeOwned::Borrowed(arg)))
            .fold(true, |a, alive| a & alive);

        if !all_alive { self.cleanup(); }
    }

    /// Sends a MaybeOwned value.
    ///
    /// We use this to passthrough an unprocessed value.
    pub fn call_dyn(&self, arg: MaybeOwned<'_, T>)
    {
        match arg
        {
            MaybeOwned::Borrowed(r) => self.call_ref(r),
            MaybeOwned::Owned(v) => self.call(v),
        }
    }

    /// Removes the dead callbacks.
    fn cleanup(&self)
    {
        if let Some(mut fs) = self.fs.try_write()
        {
            let mut i = 0;
            while i < fs.len()
            {
                if fs[i].is_alive()
                {
                    i += 1;
                }
                else
                {
                    fs.swap_remove(i);
                }
            }
        }
    }
}

impl<T> Default for Callbacks<T>
{
    fn default() -> Self
    {
        Callbacks::new()
    }
}

/// Generic sum type of two elements.
///
/// It's used to provide generics over the `Option`/`Result`/`Either` types
pub trait SumType2
{
    /// Type of the first variant.
    type Type1;
    /// Type of the second variant.
    type Type2;

    /// Creates a value using the first variant.
    fn from_type1(val: Self::Type1) -> Self;
    /// Creates a value using the second variant.
    fn from_type2(val: Self::Type2) -> Self;

    /// Checks if the value is of the first variant.
    fn is_type1(&self) -> bool;
    /// Checks if the value is of the second variant.
    fn is_type2(&self) -> bool;

    /// Attempts to extract the value contained on the first variant.
    fn into_type1(self) -> Option<Self::Type1>;
    /// Attempts to extract the value contained on the second variant.
    fn into_type2(self) -> Option<Self::Type2>;
}

impl<T> SumType2 for Option<T>
{
    type Type1 = T;
    type Type2 = ();

    fn from_type1(val: Self::Type1) -> Self { Some(val) }
    fn from_type2(_: Self::Type2) -> Self { None }

    fn is_type1(&self) -> bool { self.is_some() }
    fn is_type2(&self) -> bool { self.is_none() }

    fn into_type1(self) -> Option<Self::Type1> { self }
    fn into_type2(self) -> Option<Self::Type2> { self.ok_or(()).err() }
}

impl<T, E> SumType2 for Result<T, E>
{
    type Type1 = T;
    type Type2 = E;

    fn from_type1(val: Self::Type1) -> Self { Ok(val) }
    fn from_type2(val: Self::Type2) -> Self { Err(val) }

    fn is_type1(&self) -> bool { self.is_ok() }
    fn is_type2(&self) -> bool { self.is_err() }

    fn into_type1(self) -> Option<Self::Type1> { self.ok() }
    fn into_type2(self) -> Option<Self::Type2> { self.err() }
}

#[cfg(feature="either")]
impl<L, R> SumType2 for ::either::Either<L, R>
{
    type Type1 = L;
    type Type2 = R;

    fn from_type1(val: Self::Type1) -> Self { Either::Left(val) }
    fn from_type2(val: Self::Type2) -> Self { Either::Right(val) }

    fn is_type1(&self) -> bool { self.is_left() }
    fn is_type2(&self) -> bool { self.is_right() }

    fn into_type1(self) -> Option<Self::Type1> { self.left() }
    fn into_type2(self) -> Option<Self::Type2> { self.right() }
}

/// Storage cell for shared signal values.
pub(crate) struct Storage<T>
{
    val: RwLock<Option<T>>,
    serial: AtomicUsize,
    root_ser: Arc<AtomicUsize>,
}

const ERR_EMPTY: &str = "storage empty";

impl<T> Storage<T>
{
    /// Creates a storage with a new root serial.
    pub fn new(val: T) -> Self
    {
        Storage{
            val: RwLock::new(Some(val)),
            serial: AtomicUsize::new(1),
            root_ser: Arc::new(AtomicUsize::new(1)),
        }
    }

    /// Creates a storage with an inherited root serial.
    pub fn inherit<P>(parent: &Storage<P>) -> Self
    {
        Storage{
            val: RwLock::new(None),
            serial: Default::default(),
            root_ser: parent.root_ser.clone(),
        }
    }

    /// Gets the value by cloning.
    pub fn get(&self) -> T
        where T: Clone
    {
        self.val.read().clone().expect(ERR_EMPTY)
    }

    /// Sets value and increments the root serial.
    ///
    /// This is called by source signals.
    pub fn set(&self, val: T)
    {
        let mut st = self.val.write();
        *st = Some(val);
        self.inc_root();
    }

    /// Sets value and increments the local serial.
    ///
    /// This is called by mapped (child) signals.
    pub fn set_local(&self, val: T)
    {
        let mut st = self.val.write();
        *st = Some(val);
        self.inc_local();
    }

    /// Replaces the stored value.
    pub fn replace(&self, val: T) -> T
    {
        let mut st = self.val.write();
        let old = st.take().expect(ERR_EMPTY);
        *st = Some(val);
        self.inc_root();
        old
    }

    /// Passes the stored value through a function.
    pub fn replace_with<F>(&self, f: F)
        where F: FnOnce(T) -> T
    {
        let mut st = self.val.write();
        let old = st.take().expect(ERR_EMPTY);
        *st = Some(f(old));
        self.inc_root();
    }

    /// Checks if a parent storage has changed, so this needs update.
    pub fn must_update(&self) -> bool
    {
        self.root_ser.load(Ordering::Relaxed) > self.serial.load(Ordering::Relaxed)
    }

    /// Increments the serial of a source signal, so child must update.
    pub fn inc_root(&self)
    {
        self.root_ser.fetch_add(1, Ordering::Relaxed);
    }

    /// Marks this storage as updated.
    pub fn inc_local(&self)
    {
        self.serial.store(self.root_ser.load(Ordering::Relaxed), Ordering::Relaxed);
    }
}

impl<T> Default for Storage<T>
{
    fn default() -> Self
    {
        Storage{
            val: Default::default(),
            serial: Default::default(),
            root_ser: Arc::new(AtomicUsize::new(1)),
        }
    }
}

/// Defines a signal that contains shared storage.
pub(crate) trait SharedSignal<T>
{
    /// Updates the signal.
    fn update(&self);
    /// Checks if the value has changed.
    fn has_changed(&self) -> bool;
    /// Obtains the internal storage.
    fn get_storage(&self) -> &Storage<T>;
    /// Samples the signal.
    fn sample(&self) -> &Storage<T>;
}

/// Common template for shared signal implementations.
pub(crate) struct SharedImpl<T, S, F>
{
    pub storage: Storage<T>,
    pub source: S,
    pub f: F,
}

impl<T, S, F> ops::Deref for SharedImpl<T, S, F>
{
    type Target = Storage<T>;

    fn deref(&self) -> &Self::Target
    {
        &self.storage
    }
}
