//! FRP library for Rust

#[cfg(feature="either")]
pub extern crate either;

use std::rc::Rc;
use std::cell::RefCell;
use std::borrow::Cow;
use std::ptr;
use std::sync::mpsc;

mod types;
use types::{Callbacks, Untyped};
pub use types::SumType2;

mod helpers;
use helpers::{rc_and_weak, with_weak};

#[cfg(feature="either")]
use either::Either;

/// A source of values that feeds the streams connected to it
#[derive(Clone)]
pub struct Sink<T: Clone>
{
    cbs: Rc<Callbacks<T>>,
}

impl<T: Clone> Sink<T>
{
    /// Creates a new sink
    pub fn new() -> Self
    {
        Sink{ cbs: Rc::new(Callbacks::new()) }
    }

    /// Creates a Stream that receives the objects sent through this sink
    pub fn stream(&self) -> Stream<T>
    {
        Stream{ cbs: self.cbs.clone(), source: None }
    }

    /// Send a value into the sink
    pub fn send(&self, val: T)
    {
        self.cbs.call(val)
    }

    /// Sends values from an Iterator into the sink
    pub fn feed<I>(&self, iter: I)
        where I: IntoIterator<Item=T>
    {
        for val in iter
        {
            self.cbs.call(val)
        }
    }
}

/// A stream of discrete events sent over time
#[derive(Clone)]
pub struct Stream<T: Clone>
{
    cbs: Rc<Callbacks<T>>,
    source: Option<Rc<Untyped>>,  // strong reference to a parent Stream
}

impl<T: Clone + 'static> Stream<T>
{
    /// Maps this stream into another stream using the provided function
    pub fn map<F, R>(&self, f: F) -> Stream<R>
        where F: Fn(Cow<T>) -> R + 'static,
        R: Clone + 'static
    {
        self.filter_map(move |arg| Some(f(arg)))
    }

    /// Creates a new Stream that only contains the values where the predicate is true
    pub fn filter<F>(&self, pred: F) -> Self
        where F: Fn(&T) -> bool + 'static
    {
        let (new_cbs, weak) = rc_and_weak(Callbacks::new());
        self.cbs.push(move |arg| {
            with_weak(&weak, |cb| if pred(&arg) { cb.call_cow(arg) })
        });
        Stream{ cbs: new_cbs, source: Some(Rc::new(self.clone())) }
    }

    /// Filter and map a stream simultaneously
    pub fn filter_map<F, R>(&self, f: F) -> Stream<R>
        where F: Fn(Cow<T>) -> Option<R> + 'static,
        R: Clone + 'static
    {
        let (new_cbs, weak) = rc_and_weak(Callbacks::new());
        self.cbs.push(move |arg| {
            with_weak(&weak, |cb| if let Some(val) = f(arg) { cb.call(val) })
        });
        Stream{ cbs: new_cbs, source: Some(Rc::new(self.clone())) }
    }

    /// Creates a new stream that fires with the events from both streams
    pub fn merge(&self, other: &Stream<T>) -> Self
    {
        let (new_cbs, weak1) = rc_and_weak(Callbacks::new());
        let weak2 = weak1.clone();
        self.cbs.push(move |arg| {
            with_weak(&weak1, |cb| cb.call_cow(arg))
        });
        other.cbs.push(move |arg| {
            with_weak(&weak2, |cb| cb.call_cow(arg))
        });
        Stream{ cbs: new_cbs, source: Some(Rc::new((self.clone(), other.clone()))) }
    }

    /// Merges two streams of different types using the provided function
    #[cfg(feature="either")]
    pub fn merge_with<U, F, R>(&self, other: &Stream<U>, f: F) -> Stream<R>
        where F: Fn(Either<Cow<T>, Cow<U>>) -> R + 'static,
        U: Clone + 'static, R: Clone + 'static
    {
        let (new_cbs, weak1) = rc_and_weak(Callbacks::new());
        let weak2 = weak1.clone();
        let f1 = Rc::new(f);
        let f2 = f1.clone();
        self.cbs.push(move |arg| {
            with_weak(&weak1, |cb| cb.call(f1(Either::Left(arg))))
        });
        other.cbs.push(move |arg| {
            with_weak(&weak2, |cb| cb.call(f2(Either::Right(arg))))
        });
        Stream{ cbs: new_cbs, source: Some(Rc::new((self.clone(), other.clone()))) }
    }

    /// Read the values without modifying them
    pub fn inspect<F>(self, f: F) -> Self
        where F: Fn(Cow<T>) + 'static
    {
        self.cbs.push(move |arg| { f(arg); true });
        self
    }

    /// Creates a channel and sends the stream events through it
    pub fn channel(&self) -> mpsc::Receiver<T>
    {
        let (tx, rx) = mpsc::channel();
        self.cbs.push(move |arg| {
            tx.send(arg.into_owned()).is_ok()
        });
        rx
    }

    /// Creates a Signal that holds the last value sent to this Stream
    pub fn hold(&self, initial: T) -> Signal<T>
    {
        self.hold_if(initial, |_| true)
    }

    /// Holds the last value in this stream where the predicate is true
    pub fn hold_if<F>(&self, initial: T, pred: F) -> Signal<T>
        where F: Fn(&T) -> bool + 'static
    {
        let (storage, weak) = rc_and_weak(RefCell::new(initial));
        self.cbs.push(move |arg| {
            with_weak(&weak, |st| if pred(&arg) { *st.borrow_mut() = arg.into_owned() })
        });

        Signal::Shared(storage, Rc::new(self.clone()))
    }

    /// Accumulates the values sent over this stream
    pub fn fold<A, F>(&self, initial: A, f: F) -> Signal<A>
        where F: Fn(A, Cow<T>) -> A + 'static,
        A: Clone + 'static
    {
        let (storage, weak) = rc_and_weak(RefCell::new(initial));
        self.cbs.push(move |arg| {
            with_weak(&weak, |st| unsafe {
                let acc = &mut *st.borrow_mut();
                let old = ptr::read(acc);
                let new = f(old, arg);
                ptr::write(acc, new);
            })
        });

        Signal::Shared(storage, Rc::new(self.clone()))
    }
}

impl<T: Clone + 'static> Stream<Option<T>>
{
    /// Filters a stream of `Option`, returning the unwrapped `Some` values
    pub fn filter_some(&self) -> Stream<T>
    {
        self.filter_first()
    }
}

impl<T: SumType2 + Clone + 'static> Stream<T>
    where T::Type1: Clone + 'static, T::Type2: Clone + 'static
{
    /// Creates a stream with only the first element of a sum type
    pub fn filter_first(&self) -> Stream<T::Type1>
    {
        self.filter_map(|res| if res.is_type1() { res.into_owned().into_type1() } else { None })
    }

    /// Creates a stream with only the second element of a sum type
    pub fn filter_second(&self) -> Stream<T::Type2>
    {
        self.filter_map(|res| if res.is_type2() { res.into_owned().into_type2() } else { None })
    }

    /// Splits a two element sum type stream into two streams with the unwrapped values
    pub fn split(&self) -> (Stream<T::Type1>, Stream<T::Type2>)
    {
        let (cbs_1, weak_1) = rc_and_weak(Callbacks::new());
        let (cbs_2, weak_2) = rc_and_weak(Callbacks::new());
        self.cbs.push(move |result| {
            match (result.is_type1(), weak_1.upgrade(), weak_2.upgrade()) {
                (true, Some(cb), _) => { cb.call(result.into_owned().into_type1().unwrap()); true },
                (false, _, Some(cb)) => { cb.call(result.into_owned().into_type2().unwrap()); true },
                (_, None, None) => return false,  // both output steams dropped, drop this callback
                _ => true,  // sent to a dropped stream, but the other is still alive. keep this callback
            }
        });
        let source_rc = Rc::new(self.clone());
        let stream_1 = Stream{ cbs: cbs_1, source: Some(source_rc.clone()) };
        let stream_2 = Stream{ cbs: cbs_2, source: Some(source_rc) };
        (stream_1, stream_2)
    }
}

impl<T: Clone + 'static> Stream<Stream<T>>
{
    /// Listens to the events from the last stream sent to a nested stream
    pub fn switch(&self) -> Stream<T>
    {
        let (new_cbs, weak) = rc_and_weak(Callbacks::new());
        let storage = RefCell::new(Rc::new(()));
        self.cbs.push(move |stream| {
            // check if the ouput stream is still alive
            if weak.upgrade().is_none() { return false }
            let cbs_w = weak.clone();
            // this represents the connection between this stream and the output
            let lifetime = Rc::new(());
            let lifetime_w = Rc::downgrade(&lifetime);
            // drop the previous Rc so it unlinks from the output stream
            *storage.borrow_mut() = lifetime;
            // redirect the inner stream to the output stream
            stream.cbs.push(move |arg| {
                lifetime_w.upgrade() // check if we're still linked to this stream
                    .and_then(|_| cbs_w.upgrade()) // check if output stream still alive
                    .map(|cb| cb.call_cow(arg))
                    .is_some()
            });
            true
        });
        Stream{ cbs: new_cbs, source: Some(Rc::new(self.clone())) }
    }
}

/// Represents a continuous value that changes over time
#[derive(Clone)]
pub enum Signal<T>
{
    Constant(Rc<T>),
    Shared(Rc<RefCell<T>>, Rc<Untyped>),
    Dynamic(Rc<Fn() -> T>),
    Nested(Rc<Fn() -> Signal<T>>),
}

impl<T: Clone> Signal<T>
{
    /// Creates a signal with a constant value
    pub fn constant<U>(val: U) -> Self
        where U: Into<Rc<T>>
    {
        Signal::Constant(val.into())
    }

    /// Creates a signal that samples it's values from a function
    pub fn from_fn<F>(f: F) -> Self
        where F: Fn() -> T + 'static
    {
        Signal::Dynamic(Rc::new(f))
    }

    /// Sample by value.
    /// This clones the content of the signal
    pub fn sample(&self) -> T
    {
        match *self
        {
            Signal::Constant(ref v) => (**v).clone(),
            Signal::Shared(ref s, _) => s.borrow().clone(),
            Signal::Dynamic(ref f) => f(),
            Signal::Nested(ref sig) => sig().sample(),
        }
    }

    /// Sample by reference.
    ///
    /// This is meant to be the most efficient way when cloning is undesirable,
    /// but it requires a callback to prevent outliving the internal `RefCell` borrow
    pub fn sample_with<F, R>(&self, cb: F) -> R
        where F: FnOnce(Cow<T>) -> R
    {
        match *self
        {
            Signal::Constant(ref v) => cb(Cow::Borrowed(v)),
            Signal::Shared(ref s, _) => cb(Cow::Borrowed(&s.borrow())),
            Signal::Dynamic(ref f) => cb(Cow::Owned(f())),
            Signal::Nested(ref sig) => sig().sample_with(cb),
        }
    }

    /// Maps a signal with the provided function
    pub fn map<F, R>(&self, f: F) -> Signal<R>
        where F: Fn(Cow<T>) -> R + 'static,
        R: Clone, T: 'static
    {
        let this = self.clone();
        Signal::from_fn(move || this.sample_with(|val| f(val)))
    }

    /// Takes a snapshot of this signal every time the trigger stream fires
    pub fn snapshot<S, F, R>(&self, trigger: &Stream<S>, f: F) -> Stream<R>
        where F: Fn(Cow<T>, Cow<S>) -> R + 'static,
        S: Clone + 'static, R: Clone + 'static, T: 'static
    {
        let this = self.clone();
        trigger.map(move |b| this.sample_with(|a| f(a, b)))
    }
}

impl <T: Clone + 'static> Signal<Signal<T>>
{
    /// Creates a new signal that samples the inner value of a nested signal
    pub fn switch(&self) -> Signal<T>
    {
        let this = self.clone();
        Signal::Nested(Rc::new(move || this.sample()))
    }
}

impl<T: Clone, U: Into<Rc<T>>> From<U> for Signal<T>
{
    fn from(val: U) -> Self
    {
        Signal::constant(val)
    }
}

#[cfg(test)]
mod tests;
