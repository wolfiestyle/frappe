use std::rc::Rc;
use std::cell::Cell;
use std::ptr;
use std::sync::{mpsc, Arc, RwLock};
use std::any::Any;
use helpers::{rc_and_weak, with_weak};
use types::{Callbacks, SumType2};
use signal::Signal;
use maybe_owned::MaybeOwned;

#[cfg(feature="either")]
use either::Either;

/// A source of events that feeds the streams connected to it.
#[derive(Debug)]
pub struct Sink<T>
{
    cbs: Rc<Callbacks<T>>,
}

impl<T> Sink<T>
{
    /// Creates a new sink.
    pub fn new() -> Self
    {
        Sink{ cbs: Default::default() }
    }

    /// Creates a stream that receives the events sent to this sink.
    pub fn stream(&self) -> Stream<T>
    {
        Stream{ cbs: self.cbs.clone(), source: None }
    }

    /// Sends a value into the sink.
    #[inline]
    pub fn send(&self, val: T)
    {
        self.cbs.call(val)
    }

    /// Sends values from an Iterator into the sink.
    #[inline]
    pub fn feed<I>(&self, iter: I)
        where I: IntoIterator<Item=T>
    {
        for val in iter
        {
            self.cbs.call(val)
        }
    }
}

impl<T> Default for Sink<T>
{
    fn default() -> Self
    {
        Sink::new()
    }
}

impl<T> Clone for Sink<T>
{
    fn clone(&self) -> Self
    {
        Sink{ cbs: self.cbs.clone() }
    }
}

/// A stream of discrete events sent over time.
///
/// All the streams returned by the methods below contain an internal reference to it's parent,
/// so dropping intermediate streams won't break the chain.
#[derive(Debug)]
pub struct Stream<T>
{
    cbs: Rc<Callbacks<T>>,
    source: Option<Rc<Any>>,  // strong reference to a parent Stream
}

impl<T: 'static> Stream<T>
{
    /// Creates a stream that never fires.
    pub fn never() -> Self
    {
        Stream{ cbs: Default::default(), source: None }
    }

    /// Maps this stream into another stream using the provided function.
    #[inline]
    pub fn map<F, R>(&self, f: F) -> Stream<R>
        where F: Fn(MaybeOwned<T>) -> R + 'static,
        R: 'static
    {
        self.filter_map(move |arg| Some(f(arg)))
    }

    /// Creates a new stream that only contains the values where the predicate is `true`.
    pub fn filter<F>(&self, pred: F) -> Self
        where F: Fn(&T) -> bool + 'static
    {
        let (new_cbs, weak) = rc_and_weak(Callbacks::new());
        self.cbs.push(move |arg| {
            with_weak(&weak, |cb| if pred(&arg) { cb.call_dyn(arg) })
        });
        Stream{ cbs: new_cbs, source: Some(Rc::new(self.clone())) }
    }

    /// Filter and map a stream simultaneously.
    pub fn filter_map<F, R>(&self, f: F) -> Stream<R>
        where F: Fn(MaybeOwned<T>) -> Option<R> + 'static,
        R: 'static
    {
        let (new_cbs, weak) = rc_and_weak(Callbacks::new());
        self.cbs.push(move |arg| {
            with_weak(&weak, |cb| if let Some(val) = f(arg) { cb.call(val) })
        });
        Stream{ cbs: new_cbs, source: Some(Rc::new(self.clone())) }
    }

    /// Creates a new stream that fires with the events from both streams.
    pub fn merge(&self, other: &Stream<T>) -> Self
    {
        let (new_cbs, weak1) = rc_and_weak(Callbacks::new());
        let weak2 = weak1.clone();
        self.cbs.push(move |arg| {
            with_weak(&weak1, |cb| cb.call_dyn(arg))
        });
        other.cbs.push(move |arg| {
            with_weak(&weak2, |cb| cb.call_dyn(arg))
        });
        Stream{ cbs: new_cbs, source: Some(Rc::new((self.clone(), other.clone()))) }
    }

    /// Merges two streams of different types using the provided function.
    #[cfg(feature="either")]
    pub fn merge_with<U, F, R>(&self, other: &Stream<U>, f: F) -> Stream<R>
        where F: Fn(Either<MaybeOwned<T>, MaybeOwned<U>>) -> R + 'static,
        U: 'static, R: 'static
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

    /// Accumulates the values sent over this stream.
    ///
    /// The fold operation is done by locking the accumulator, consuming it's value, and then
    /// putting back the transformed value. This avoids cloning, but it will cause readers on
    /// another thread to block if the closure takes too long to execute.
    /// If you don't want to block, use `Stream::fold_clone` instead.
    pub fn fold<A, F>(&self, initial: A, f: F) -> Signal<A>
        where F: Fn(A, MaybeOwned<T>) -> A + 'static,
        A: 'static
    {
        let storage = Arc::new(RwLock::new(initial));
        let weak = Arc::downgrade(&storage);
        self.cbs.push(move |arg| {
            weak.upgrade()
                .map(|st| unsafe {
                    let acc = &mut *st.write().unwrap();
                    let old = ptr::read(acc);
                    let new = f(old, arg);
                    ptr::write(acc, new);
                })
                .is_some()
        });

        Signal::Shared(storage, Some(Rc::new(self.clone())))
    }

    /// Folds the stream without locking the accumulator.
    ///
    /// This will clone the accumulator on every value processed, but it won't block other threads
    /// reading it if the closure takes too long to execute.
    pub fn fold_clone<A, F>(&self, initial: A, f: F) -> Signal<A>
        where F: Fn(A, MaybeOwned<T>) -> A + 'static,
        A: Clone + 'static
    {
        let storage = Arc::new(RwLock::new(initial));
        let weak = Arc::downgrade(&storage);
        self.cbs.push(move |arg| {
            weak.upgrade()
                .map(|st| {
                    let old = st.read().unwrap().clone();
                    let new = f(old, arg);
                    *st.write().unwrap() = new;
                })
                .is_some()
        });

        Signal::Shared(storage, Some(Rc::new(self.clone())))
    }

    /// Maps each stream event to `0..N` output values.
    ///
    /// The closure must return it's value by sending it through the provided sink.
    /// Multiple values (or none) can be sent to the output stream this way.
    ///
    /// This primitive is useful to construct asynchronous operations, since you can
    /// store the sink for later usage.
    pub fn map_n<F, R>(&self, f: F) -> Stream<R>
        where F: Fn(MaybeOwned<T>, Sink<R>) + 'static,
        R: 'static
    {
        let (new_cbs, weak) = rc_and_weak(Callbacks::new());
        self.cbs.push(move |arg| {
            with_weak(&weak, |cb| f(arg, Sink{ cbs: cb }))
        });
        Stream{ cbs: new_cbs, source: Some(Rc::new(self.clone())) }
    }

    /// Reads the values without modifying them.
    ///
    /// This is meant to be used as a debugging tool and not to cause side effects.
    pub fn inspect<F>(self, f: F) -> Self
        where F: Fn(MaybeOwned<T>) + 'static
    {
        self.cbs.push(move |arg| { f(arg); true });
        self
    }
}

impl<T: Clone + 'static> Stream<T>
{
    /// Creates a Signal that holds the last value sent to this stream.
    #[inline]
    pub fn hold(&self, initial: T) -> Signal<T>
    {
        self.hold_if(initial, |_| true)
    }

    /// Holds the last value in this stream where the predicate is `true`.
    pub fn hold_if<F>(&self, initial: T, pred: F) -> Signal<T>
        where F: Fn(&T) -> bool + 'static
    {
        let storage = Arc::new(RwLock::new(initial));
        let weak = Arc::downgrade(&storage);
        self.cbs.push(move |arg| {
            weak.upgrade()
                .map(|st| if pred(&arg) { *st.write().unwrap() = arg.into_owned() })
                .is_some()
        });

        Signal::Shared(storage, Some(Rc::new(self.clone())))
    }

    /// Creates a channel and sends the stream events through it.
    pub fn channel(&self) -> mpsc::Receiver<T>
    {
        let (tx, rx) = mpsc::channel();
        self.cbs.push(move |arg| {
            tx.send(arg.into_owned()).is_ok()
        });
        rx
    }
}

impl<T: Clone + 'static> Stream<Option<T>>
{
    /// Filters a stream of `Option`, returning the unwrapped `Some` values
    #[inline]
    pub fn filter_some(&self) -> Stream<T>
    {
        self.filter_first()
    }
}

impl<T: SumType2 + Clone + 'static> Stream<T>
    where T::Type1: 'static, T::Type2: 'static
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
            if result.is_type1()
            {
                if let Some(cb) = weak_1.upgrade()
                {
                    cb.call(result.into_owned().into_type1().unwrap());
                    true
                }
                else
                {
                    // drop callback if both output streams dropped
                    weak_2.upgrade().is_some()
                }
            }
            else // if result.is_type2()
            {
                if let Some(cb) = weak_2.upgrade()
                {
                    cb.call(result.into_owned().into_type2().unwrap());
                    true
                }
                else
                {
                    weak_1.upgrade().is_some()
                }
            }
        });
        let source_rc = Rc::new(self.clone());
        let stream_1 = Stream{ cbs: cbs_1, source: Some(source_rc.clone()) };
        let stream_2 = Stream{ cbs: cbs_2, source: Some(source_rc) };
        (stream_1, stream_2)
    }
}

impl<T: 'static> Stream<Stream<T>>
{
    /// Listens to the events from the last stream sent to a nested stream
    pub fn switch(&self) -> Stream<T>
    {
        let (new_cbs, weak) = rc_and_weak(Callbacks::new());
        let id = Rc::new(Cell::new(0u64));  // id of each stream sent
        self.cbs.push(move |stream| {
            if weak.upgrade().is_none() { return false }
            let cbs_w = weak.clone();
            let cur_id = id.clone();
            // increment the id so it will only send to the last stream
            let my_id = id.get() + 1;
            id.set(my_id);
            // redirect the inner stream to the output stream
            stream.cbs.push(move |arg| {
                if my_id != cur_id.get() { return false }
                with_weak(&cbs_w, |cb| cb.call_dyn(arg))
            });
            true
        });
        Stream{ cbs: new_cbs, source: Some(Rc::new(self.clone())) }
    }
}

impl<T> Clone for Stream<T>
{
    fn clone(&self) -> Self
    {
        Stream{ cbs: self.cbs.clone(), source: self.source.clone() }
    }
}
