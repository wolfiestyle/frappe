use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::fmt;
use helpers::retain_swap;

// amount of dropped callbacks allowed to stay before initiating a cleanup
// (arbitrary value, hasn't been performance tuned)
const GC_THRESHOLD: usize = 5;

// function that becomes uncallable after it returns false.
// callbacks use a Cow<T> argument so we can choose at runtime if we will send a ref or an owned value
struct FnCell<T: Clone>
{
    f: Box<Fn(Cow<T>) -> bool>,
    alive: Cell<bool>,
}

impl<T: Clone> FnCell<T>
{
    fn new<F>(f: F) -> Self
        where F: Fn(Cow<T>) -> bool + 'static
    {
        FnCell{ f: Box::new(f), alive: Cell::new(true) }
    }

    fn call(&self, arg: Cow<T>) -> bool
    {
        let is_alive = self.alive.get();
        if is_alive && !(self.f)(arg)
        {
            self.alive.set(false);
            return false
        }
        is_alive
    }

    fn is_alive(&self) -> bool
    {
        self.alive.get()
    }
}

impl<T: Clone> fmt::Debug for FnCell<T>
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result
    {
        write!(f, "FnCell{{ alive: {} }}", self.alive.get())
    }
}

// a collection of callbacks
#[derive(Debug)]
pub struct Callbacks<T: Clone>
{
    fs: RefCell<Vec<FnCell<T>>>,
}

impl<T: Clone> Callbacks<T>
{
    pub fn new() -> Self
    {
        Callbacks{ fs: RefCell::new(Vec::new()) }
    }

    pub fn push<F>(&self, cb: F)
        where F: Fn(Cow<T>) -> bool + 'static
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
            if !fs[i].call(Cow::Borrowed(&arg)) { n_dead += 1 }
            i += 1;
        }
        if n > 0
        {
            if !fs[i].call(Cow::Owned(arg)) { n_dead += 1 }
        }
        drop(fs);

        if n_dead > GC_THRESHOLD { self.cleanup(); }
    }

    fn call_ref(&self, arg: &T)
    {
        let n_dead = self.fs.borrow().iter().fold(0, |a, f| {
            if f.call(Cow::Borrowed(arg)) { a } else { a + 1 }
        });

        if n_dead > GC_THRESHOLD { self.cleanup(); }
    }

    // we use this to passthrough an unprocessed value
    pub fn call_cow(&self, arg: Cow<T>)
    {
        match arg
        {
            Cow::Borrowed(r) => self.call_ref(r),
            Cow::Owned(v) => self.call(v),
        }
    }

    // removes the dead callbacks
    fn cleanup(&self)
    {
        if let Ok(mut fs) = self.fs.try_borrow_mut()
        {
            retain_swap(&mut fs, FnCell::is_alive);
        }
    }
}

/// Describes an asynchronous operation.
///
/// Frappe doesn't implement concurrency directly. This serves as an interface to
/// an external async IO implementation, thread pool or similar so you can run slow
/// parts of the stream chain without blocking the main thread.
///
/// Implementations should run the task asynchronously, possibly on another thread,
/// and deliver the result to the main thread via some kind of event loop.
pub trait AsyncTask
{
    /// Value from the source stream
    type Input;
    /// Result that gets sent to the output stream
    type Output;
    /// Handle to the main loop, thread pool or whatever resource is needed.
    type Handler;

    /// Runs the operation with the supplied input.
    ///
    /// The result has to be returned by calling the supplied `ret` closure.
    /// The closure will return `true` if the value was successfully delivered,
    /// or `false` if the output stream is gone.
    ///
    /// This closure must be called in the same thread where the destination stream lives.
    /// Also it can be called multiple times to deliver more than one result. This can
    /// be useful to report the progress of the operation.
    fn run<F>(&self, handler: &Self::Handler, input: Self::Input, ret: F)
        where F: Fn(Self::Output) -> bool + 'static;
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
