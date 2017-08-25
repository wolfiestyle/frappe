use std::cell::{Cell, RefCell};
use std::fmt;
use maybe_owned::MaybeOwned;

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
pub struct Callbacks<T>
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

    fn call_ref(&self, arg: &T)
    {
        let n_dead = self.fs.borrow().iter().fold(0, |a, f| {
            if f.call(MaybeOwned::Borrowed(arg)) { a } else { a + 1 }
        });

        if n_dead > 0 { self.cleanup(n_dead); }
    }

    // we use this to passthrough an unprocessed value
    pub fn call_cow(&self, arg: MaybeOwned<T>)
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
    type Type1;
    type Type2;

    fn from_type1(val: Self::Type1) -> Self;
    fn from_type2(val: Self::Type2) -> Self;

    fn is_type1(&self) -> bool;
    fn is_type2(&self) -> bool;

    fn into_type1(self) -> Option<Self::Type1>;
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

    fn from_type1(val: Self::Type1) -> Self { ::either::Either::Left(val) }
    fn from_type2(val: Self::Type2) -> Self { ::either::Either::Right(val) }

    fn is_type1(&self) -> bool { self.is_left() }
    fn is_type2(&self) -> bool { self.is_right() }

    fn into_type1(self) -> Option<Self::Type1> { self.left() }
    fn into_type2(self) -> Option<Self::Type2> { self.right() }
}
