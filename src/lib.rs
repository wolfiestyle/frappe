use std::rc::Rc;
use std::cell::RefCell;
use std::borrow::Cow;
use std::ptr;

mod types;
use types::{Callbacks, Untyped};

mod helpers;
use helpers::{rc_and_weak, with_weak};

/// A source of values that feeds the streams connected to it
#[derive(Clone)]
pub struct Sink<T: Clone>
{
    cbs: Rc<RefCell<Callbacks<T>>>,
}

impl<T: Clone> Sink<T>
{
    /// Creates a new sink
    pub fn new() -> Self
    {
        Sink{ cbs: Rc::new(RefCell::new(Callbacks::new())) }
    }

    /// Creates a Stream that receives the objects sent through this sink
    pub fn stream(&self) -> Stream<T>
    {
        Stream{ cbs: self.cbs.clone(), source: None }
    }

    /// Send a value into the sink
    pub fn send(&self, val: T)
    {
        self.cbs.borrow_mut().call(val)
    }

    /// Sends values from an Iterator into the sink
    pub fn feed<I>(&self, iter: I)
        where I: IntoIterator<Item=T>
    {
        let mut cbs = self.cbs.borrow_mut();
        for val in iter
        {
            cbs.call(val)
        }
    }
}

/// A stream of discrete events sent over time
#[derive(Clone)]
pub struct Stream<T: Clone>
{
    cbs: Rc<RefCell<Callbacks<T>>>,
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
        self.cbs.borrow_mut().push(move |arg| {
            with_weak(&weak, |cb| if pred(&arg) { cb.borrow_mut().call_cow(arg) })
        });
        Stream{ cbs: new_cbs, source: Some(Rc::new(self.clone())) }
    }

    /// Filter and map a stream simultaneously
    pub fn filter_map<F, R>(&self, f: F) -> Stream<R>
        where F: Fn(Cow<T>) -> Option<R> + 'static,
        R: Clone + 'static
    {
        let (new_cbs, weak) = rc_and_weak(Callbacks::new());
        self.cbs.borrow_mut().push(move |arg| {
            with_weak(&weak, |cb| if let Some(val) = f(arg) { cb.borrow_mut().call(val) })
        });
        Stream{ cbs: new_cbs, source: Some(Rc::new(self.clone())) }
    }

    /// Creates a new stream that fires with the events from both streams
    pub fn merge(&self, other: &Stream<T>) -> Self
    {
        let (new_cbs, weak1) = rc_and_weak(Callbacks::new());
        let weak2 = weak1.clone();
        self.cbs.borrow_mut().push(move |arg| {
            with_weak(&weak1, |cb| cb.borrow_mut().call_cow(arg))
        });
        other.cbs.borrow_mut().push(move |arg| {
            with_weak(&weak2, |cb| cb.borrow_mut().call_cow(arg))
        });
        Stream{ cbs: new_cbs, source: Some(Rc::new((self.clone(), other.clone()))) }
    }

    /// Read the values without modifying them
    pub fn inspect<F>(self, f: F) -> Self
        where F: Fn(Cow<T>) + 'static
    {
        self.cbs.borrow_mut().push(move |arg| { f(arg); true });
        self
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
        let (storage, weak) = rc_and_weak(initial);
        self.cbs.borrow_mut().push(move |arg| {
            with_weak(&weak, |st| if pred(&arg) { *st.borrow_mut() = arg.into_owned() })
        });
        Signal{
            val: SigVal::Shared(storage),
            source: Some(Rc::new(self.clone()))
        }
    }

    /// Accumulates the values sent over this stream
    pub fn fold<A, F>(&self, initial: A, f: F) -> Signal<A>
        where F: Fn(A, Cow<T>) -> A + 'static,
        A: Clone + 'static
    {
        let (storage, weak) = rc_and_weak(initial);
        self.cbs.borrow_mut().push(move |arg| {
            with_weak(&weak, |st| unsafe {
                let acc = &mut *st.borrow_mut();
                let old = ptr::read(acc);
                let new = f(old, arg);
                ptr::write(acc, new);
            })
        });
        Signal{
            val: SigVal::Shared(storage),
            source: Some(Rc::new(self.clone()))
        }
    }
}

impl<T: Clone + 'static> Stream<Option<T>>
{
    /// Filters a stream of `Option`s, returning the unwrapped `Some` values
    pub fn filter_some(&self) -> Stream<T>
    {
        self.filter_map(|opt| opt.into_owned())
    }
}

impl<T, E> Stream<Result<T, E>>
    where T: Clone + 'static, E: Clone + 'static
{
    /// Splits a Result into two streams with their unwrapped Ok and Err values
    pub fn split_result(&self) -> (Stream<T>, Stream<E>)
    {
        let (cbs_ok, weak_ok) = rc_and_weak(Callbacks::new());
        let (cbs_err, weak_err) = rc_and_weak(Callbacks::new());
        self.cbs.borrow_mut().push(move |result| {
            match (result.is_ok(), weak_ok.upgrade(), weak_err.upgrade()) {
                (true, Some(cb), _) => { cb.borrow_mut().call(result.into_owned().ok().unwrap()); true },
                (false, _, Some(cb)) => { cb.borrow_mut().call(result.into_owned().err().unwrap()); true },
                (_, None, None) => return false,  // both output steams dropped, drop this callback
                _ => true,  // sent to a dropped stream, but the other is still alive. keep this callback
            }
        });
        let stream_ok = Stream{ cbs: cbs_ok, source: Some(Rc::new(self.clone())) };
        let stream_err = Stream{ cbs: cbs_err, source: Some(Rc::new(self.clone())) };
        (stream_ok, stream_err)
    }
}

impl<T: Clone + 'static> Stream<Stream<T>>
{
    /// Listens to the events from the last stream sent to a nested stream
    pub fn switch(&self) -> Stream<T>
    {
        let (new_cbs, weak) = rc_and_weak(Callbacks::new());
        let mut storage = Rc::new(());
        self.cbs.borrow_mut().push(move |stream| {
            // check if the ouput stream is still alive
            if weak.upgrade().is_none() { return false }
            let cbs_w = weak.clone();
            // this represents the connection between this stream and the output
            let lifetime = Rc::new(());
            let lifetime_w = Rc::downgrade(&lifetime);
            // drop the previous Rc so it unlinks from the output stream
            storage = lifetime;
            // redirect the inner stream to the output stream
            stream.cbs.borrow_mut().push(move |arg| {
                lifetime_w.upgrade() // check if we're still linked to this stream
                    .and_then(|_| cbs_w.upgrade()) // check if output stream still alive
                    .map(|cb| cb.borrow_mut().call_cow(arg))
                    .is_some()
            });
            true
        });
        Stream{ cbs: new_cbs, source: Some(Rc::new(self.clone())) }
    }
}

#[derive(Clone)]
enum SigVal<T>
{
    Constant(Rc<T>),
    Shared(Rc<RefCell<T>>),
    Dynamic(Rc<Fn() -> T>),
    Nested(Rc<Fn() -> Signal<T>>),
}

impl<T> SigVal<T>
{
    fn from_fn<F>(f: F) -> Self
        where F: Fn() -> T + 'static
    {
        SigVal::Dynamic(Rc::new(f))
    }
}

// Represents a continuous value that changes over time
#[derive(Clone)]
pub struct Signal<T>
{
    val: SigVal<T>,
    source: Option<Rc<Untyped>>,
}

impl<T: Clone> Signal<T>
{
    /// Creates a signal with a constant value
    pub fn constant<U>(val: U) -> Self
        where U: Into<Rc<T>>
    {
        Signal{ val: SigVal::Constant(val.into()), source: None }
    }

    /// Creates a signal that samples it's values from a function
    pub fn from_fn<F>(f: F) -> Self
        where F: Fn() -> T + 'static
    {
        Signal{ val: SigVal::from_fn(f), source: None }
    }

    /// Sample by value.
    /// This clones the content of the signal
    pub fn sample(&self) -> T
    {
        match self.val
        {
            SigVal::Constant(ref v) => (**v).clone(),
            SigVal::Shared(ref s) => s.borrow().clone(),
            SigVal::Dynamic(ref f) => f(),
            SigVal::Nested(ref sig) => sig().sample(),
        }
    }

    /// Sample by reference.
    /// This is meant to be the most efficient way when cloning is undesirable,
    /// but it requires a callback to prevent outliving the RefCell borrow
    pub fn sample_with<F, R>(&self, cb: F) -> R
        where F: FnOnce(Cow<T>) -> R
    {
        match self.val
        {
            SigVal::Constant(ref v) => cb(Cow::Borrowed(v)),
            SigVal::Shared(ref s) => cb(Cow::Borrowed(&s.borrow())),
            SigVal::Dynamic(ref f) => cb(Cow::Owned(f())),
            SigVal::Nested(ref sig) => sig().sample_with(cb),
        }
    }

    /// Maps a signal with the provided function
    pub fn map<F, R>(&self, f: F) -> Signal<R>
        where F: Fn(Cow<T>) -> R + 'static,
        R: Clone, T: 'static
    {
        let this = self.clone();
        Signal{
            val: SigVal::from_fn(move || this.sample_with(|val| f(val))),
            source: None
        }
    }

    /// Takes a snapshot of the Signal every time the trigger signal fires
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
        Signal{
            val: SigVal::Nested(Rc::new(move || this.sample())),
            source: None
        }
    }
}

impl<T: Clone, U: Into<Rc<T>>> From<U> for Signal<T>
{
    fn from(val: U) -> Self
    {
        Signal::constant(val)
    }
}
