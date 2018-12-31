//! Miscellaneous types used by the library.

use std::rc::Rc;
use std::cell::{Cell, RefCell};
use std::{fmt, ops};

pub use maybe_owned::MaybeOwned;
#[cfg(feature="either")]
pub use either::Either;

/// Function that becomes uncallable after it returns false.
///
/// Callbacks use a MaybeOwned<T> argument so we can choose at runtime if we will send a ref or an owned value.
struct FnCell<T>
{
    f: Box<dyn Fn(MaybeOwned<'_, T>) -> bool>,
    alive: Cell<bool>,
}

impl<T> FnCell<T>
{
    fn new<F>(f: F) -> Self
        where F: Fn(MaybeOwned<'_, T>) -> bool + 'static
    {
        FnCell{ f: Box::new(f), alive: Cell::new(true) }
    }

    fn call(&self, arg: MaybeOwned<'_, T>) -> bool
    {
        let is_alive = self.alive.get() && (self.f)(arg);
        self.alive.set(is_alive);
        is_alive
    }

    fn is_alive(&self) -> bool
    {
        self.alive.get()
    }
}

impl<T> fmt::Debug for FnCell<T>
{
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result
    {
        write!(f, "FnCell {{ f: Fn@{:p}, alive: {} }}", self.f, self.alive.get())
    }
}

/// A collection of callbacks.
#[derive(Debug)]
pub(crate) struct Callbacks<T>
{
    fs: RefCell<Vec<FnCell<T>>>,
}

impl<T> Callbacks<T>
{
    pub fn new() -> Self
    {
        Callbacks{ fs: Default::default() }
    }

    pub fn push<F>(&self, cb: F)
        where F: Fn(MaybeOwned<'_, T>) -> bool + 'static
    {
        self.fs.borrow_mut().push(FnCell::new(cb))
    }

    /// Sends an owned value.
    ///
    /// This sends a ref to the first N-1 callbacks, and the owned value to the last.
    pub fn call(&self, arg: T)
    {
        let fs = self.fs.borrow();
        let n = fs.len();

        let mut i = 0;
        let mut n_dead = 0;
        for _ in 1..n
        {
            if !fs[i].call(MaybeOwned::Borrowed(&arg)) { n_dead += 1 }
            i += 1;
        }
        if n > 0 && !fs[i].call(MaybeOwned::Owned(arg)) { n_dead += 1 }
        drop(fs);

        if n_dead > 0 { self.cleanup(n_dead); }
    }

    /// Sends a value by reference.
    pub fn call_ref(&self, arg: &T)
    {
        let n_dead = self.fs.borrow().iter()
            .map(|f| f.call(MaybeOwned::Borrowed(arg)))
            .fold(0, |a, alive| if alive { a } else { a + 1 });

        if n_dead > 0 { self.cleanup(n_dead); }
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
    fn cleanup(&self, n_dead: usize)
    {
        if let Ok(mut fs) = self.fs.try_borrow_mut()
        {
            let mut i = 0;
            let mut removed = 0;
            while removed < n_dead && i < fs.len()
            {
                if fs[i].is_alive()
                {
                    i += 1;
                }
                else
                {
                    fs.swap_remove(i);
                    removed += 1;
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
    val: Cell<Option<T>>,
    serial: Cell<SerialId>,
    pub root_ser: Rc<Cell<SerialId>>,
}

const ERR_EMPTY: &str = "storage empty";

impl<T> Storage<T>
{
    /// Creates a storage with a new root serial.
    pub fn new(val: T) -> Self
    {
        Storage{
            val: Cell::new(Some(val)),
            serial: Cell::new(SERIAL_ONE),
            root_ser: Rc::new(Cell::new(SERIAL_ONE)),
        }
    }

    /// Creates a storage with an inherited root serial.
    pub fn inherit<P>(parent: &Storage<P>) -> Self
    {
        Storage{
            val: Cell::new(None),
            serial: Default::default(),
            root_ser: parent.root_ser.clone(),
        }
    }

    /// Gets the value by cloning.
    pub fn get(&self) -> T
        where T: Clone
    {
        let val = self.val.replace(None);
        let cloned = val.clone();
        self.val.set(val);
        cloned.expect(ERR_EMPTY)
    }

    /// Sets value and increments the root serial.
    ///
    /// This is called by source signals.
    pub fn set(&self, val: T)
    {
        self.val.set(Some(val));
        self.inc_root();
    }

    /// Sets value and increments the local serial.
    ///
    /// This is called by mapped (child) signals.
    pub fn set_local(&self, val: T)
    {
        self.val.set(Some(val));
        self.inc_local();
    }

    /// Moves the value out and sets the storage empty.
    ///
    /// This needs to be paired with a .set() afterwards.
    pub fn take(&self) -> T
    {
        self.val.replace(None).expect(ERR_EMPTY)
    }

    /// Checks if a parent storage has changed, so this needs update.
    pub fn must_update(&self) -> bool
    {
        self.root_ser.get() > self.serial.get()
    }

    /// Increments the serial of a source signal, so child must update.
    pub fn inc_root(&self)
    {
        self.root_ser.set(self.root_ser.get().inc());
    }

    /// Marks this storage as updated.
    pub fn inc_local(&self)
    {
        self.serial.set(self.root_ser.get());
    }
}

impl<T> Default for Storage<T>
{
    fn default() -> Self
    {
        Storage{
            val: Cell::new(None),
            serial: Default::default(),
            root_ser: Rc::new(Cell::new(SERIAL_ONE)),
        }
    }
}

/// A counter on how many times a signal value has been modified.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
pub(crate) struct SerialId(u64);

/// The serial id of a constant value.
const SERIAL_ONE: SerialId = SerialId(1);

impl SerialId
{
    fn inc(self) -> Self
    {
        SerialId(self.0 + 1)
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
