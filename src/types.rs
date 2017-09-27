//! Miscellaneous types used by the library.

use std::cell::{Cell, RefCell};
use std::fmt;

pub use maybe_owned::MaybeOwned;
#[cfg(feature="either")]
pub use either::Either;

// function that becomes uncallable after it returns false.
// callbacks use a MaybeOwned<T> argument so we can choose at runtime if we will send a ref or an owned value
struct FnCell<T>
{
    f: Box<Fn(MaybeOwned<T>) -> bool>,
    alive: Cell<bool>,
}

impl<T> FnCell<T>
{
    fn new<F>(f: F) -> Self
        where F: Fn(MaybeOwned<T>) -> bool + 'static
    {
        FnCell{ f: Box::new(f), alive: Cell::new(true) }
    }

    fn call(&self, arg: MaybeOwned<T>) -> bool
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
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result
    {
        write!(f, "FnCell {{ f: Fn@{:p}, alive: {} }}", self.f, self.alive.get())
    }
}

// a collection of callbacks
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
        where F: Fn(MaybeOwned<T>) -> bool + 'static
    {
        self.fs.borrow_mut().push(FnCell::new(cb))
    }

    // sends a ref to the first N-1 callbacks, and the owned value to the last
    // this way we prevent tons of cloning
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

    pub fn call_ref(&self, arg: &T)
    {
        let n_dead = self.fs.borrow().iter()
            .map(|f| f.call(MaybeOwned::Borrowed(arg)))
            .fold(0, |a, alive| if alive { a } else { a + 1 });

        if n_dead > 0 { self.cleanup(n_dead); }
    }

    // we use this to passthrough an unprocessed value
    pub fn call_dyn(&self, arg: MaybeOwned<T>)
    {
        match arg
        {
            MaybeOwned::Borrowed(r) => self.call_ref(r),
            MaybeOwned::Owned(v) => self.call(v),
        }
    }

    // removes the dead callbacks
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
#[derive(Debug)]
pub(crate) struct Storage<T>
{
    val: RefCell<Option<T>>,
    serial: Cell<SerialId>,
}

const ERR_EMPTY: &'static str = "storage empty";

impl<T> Storage<T>
{
    pub fn new(val: T) -> Self
    {
        Storage{
            val: RefCell::new(Some(val)),
            serial: Cell::new(SerialId::once()),
        }
    }

    pub fn empty() -> Self
    {
        Storage{
            val: RefCell::new(None),
            serial: Default::default(),
        }
    }

    /// Gets the value by cloning.
    pub fn get(&self) -> T
        where T: Clone
    {
        self.val.borrow().clone().expect(ERR_EMPTY)
    }

    pub fn set(&self, val: T)
    {
        *self.val.borrow_mut() = Some(val);
        SerialId::inc_cell(&self.serial);
    }

    pub fn set_with_serial(&self, val: T, serial: SerialId)
    {
        *self.val.borrow_mut() = Some(val);
        self.serial.set(serial);
    }

    pub fn take(&self) -> T
    {
        self.val.borrow_mut().take().expect(ERR_EMPTY)
    }

    /// Gets the value by borrowing it to a closure.
    pub fn borrow<R, F>(&self, f: F) -> R
        where F: FnOnce(&T) -> R
    {
        f(self.val.borrow().as_ref().expect(ERR_EMPTY))
    }

    pub fn get_serial(&self) -> SerialId
    {
        self.serial.get()
    }
}

/// A counter on how many times a signal value has been modified.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct SerialId(u64);

impl SerialId
{
    /// The serial id of a constant value.
    #[inline]
    pub fn once() -> Self
    {
        SerialId(1)
    }

    pub(crate) fn inc(self) -> Self
    {
        SerialId(self.0 + 1)
    }

    pub(crate) fn inc_cell(cell: &Cell<Self>) -> SerialId
    {
        let ser = cell.get().inc();
        cell.set(ser);
        ser
    }

    /// Gets the count stored on this object.
    #[inline]
    pub fn get(&self) -> u64
    {
        self.0
    }
}
