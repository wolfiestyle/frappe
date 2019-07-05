//! The Stream type.
//!
//! Streams provide a composable way to handle events that's focused on data instead of callbacks.
//! You can think of it as a data processing pipeline. Streams do all their work on the sender side,
//! so they're "eager".
//!
//! A stream chain begins with a `Sink` that receives the input values and can send those values to
//! multiple streams. Operations applied to a `Stream` are applied to all the values that pass
//! through it. The result of a stream chain be viewed with the `Stream::observe` method or stored
//! on a `Signal`.
//! All the objects that result from stream operations contain an internal reference to it's parent,
//! so dropping intermediate temporary streams (like the ones created from chaining methods) won't
//! break the chain.
//!
//! This implementation of Stream distributes the data as `MaybeOwned<T>` values to avoid
//! unnecessary cloning, so the first observers will receive a `MaybeOwned::Borrowed` value, and the
//! last one will receive a`MaybeOwned::Owned`. This also allows sending values as a reference with
//! an arbitrary lifetime, not just `&'static` refs.
//!
//! # Example
//! ```
//! use frappe::Sink;
//!
//! let sink1 = Sink::new();
//! let sink2 = Sink::new();
//! let stream = sink1.stream().map(|x| *x + 1)
//!             .merge(&sink2.stream().map(|x| *x * 2));
//! let signal = stream.hold(0);
//!
//! sink1.send(10);
//! assert_eq!(signal.sample(), 11);
//!
//! sink2.send(10);
//! assert_eq!(signal.sample(), 20);
//! ```

use crate::futures::StreamFuture;
use crate::helpers::arc_and_weak;
use crate::signal::Signal;
use crate::sync::Mutex;
use crate::types::{Callbacks, MaybeOwned, ObserveResult, Storage, SumType2};
use std::any::Any;
use std::collections::VecDeque;
use std::ops::{Bound, RangeBounds};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

#[cfg(feature = "either")]
use crate::types::Either;

/// A source of events that feeds the streams connected to it.
#[derive(Debug)]
pub struct Sink<T> {
    cbs: Arc<Callbacks<T>>,
}

impl<T> Sink<T> {
    /// Creates a new sink.
    pub fn new() -> Self {
        Sink {
            cbs: Default::default(),
        }
    }

    /// Creates a stream that receives the events sent to this sink.
    pub fn stream(&self) -> Stream<T> {
        Stream::new(self.cbs.clone(), Source::None)
    }

    /// Sends a value into the sink.
    ///
    /// The value will be distributed `N-1` times as reference and then one time by value,
    /// where `N` is the amount of streams connected to this sink.
    #[inline]
    pub fn send<'a>(&self, val: impl Into<MaybeOwned<'a, T>>)
    where
        T: 'a,
    {
        self.cbs.call(val)
    }

    /// Sends multiple values into the sink.
    #[inline]
    pub fn feed<'a, I, U>(&self, iter: I)
    where
        I: IntoIterator<Item = U>,
        U: Into<MaybeOwned<'a, T>>,
        T: 'a,
    {
        for val in iter {
            self.send(val)
        }
    }

    /// Sends a value using multiple threads.
    ///
    /// This method sends a value to each of the Sink's connected streams simultaneously by spawning
    /// a thread for each one, then it waits for all threads to finish. The value is sent by
    /// reference, so no cloning is done.
    #[cfg(feature = "crossbeam-utils")]
    #[inline]
    pub fn send_parallel(&self, val: &T)
    where
        T: Sync,
    {
        self.cbs.call_parallel(val)
    }
}

impl<T> Default for Sink<T> {
    /// Creates a new sink.
    #[inline]
    fn default() -> Self {
        Sink::new()
    }
}

impl<T> Clone for Sink<T> {
    /// Creates a copy of this sink that references the same event source.
    fn clone(&self) -> Self {
        Sink {
            cbs: self.cbs.clone(),
        }
    }
}

/// The source object of a Stream.
///
/// This is used to create a strong reference to a parent stream.
#[derive(Debug, Clone)]
enum Source {
    /// No source.
    None,
    /// The source is a type-erased object. Usually a stream of a different type.
    Erased(Arc<dyn Any + Send + Sync>),
}

impl Source {
    fn stream<T: 'static>(s: &Stream<T>) -> Self {
        Source::Erased(Arc::new(s.clone()))
    }

    fn stream2<A: 'static, B: 'static>(s1: &Stream<A>, s2: &Stream<B>) -> Self {
        Source::Erased(Arc::new((s1.clone(), s2.clone())))
    }
}

/// A stream of discrete events sent over time.
#[derive(Debug)]
pub struct Stream<T> {
    cbs: Arc<Callbacks<T>>,
    source: Source,
}

impl<T> Stream<T> {
    /// Creates a stream from it's components.
    fn new(cbs: Arc<Callbacks<T>>, source: Source) -> Self {
        Stream { cbs, source }
    }

    /// Creates a stream that never fires.
    pub fn never() -> Self {
        Stream::new(Default::default(), Source::None)
    }

    /// Reads the values from the stream.
    ///
    /// This method registers a callback that will be called every time a stream event is received.
    /// It is meant to be used as a debugging tool or as a way to interface with imperative code.
    ///
    /// The closure will be dropped when it returns a false-y value (see `ObserveResult`) or when
    /// the source stream is dropped, so you should avoid calling `Stream::observe` as the last
    /// step of a stream chain.
    pub fn observe<F, R>(&self, f: F)
    where
        F: Fn(MaybeOwned<'_, T>) -> R + Send + Sync + 'static,
        R: ObserveResult,
    {
        self.cbs.push(move |arg| f(arg).is_callback_alive());
    }

    /// Observes the stream while keeping a reference to it.
    ///
    /// This is the same as `Stream::observe`, but it keeps a strong reference to it's source stream,
    /// so it's safe to call it as the last step of a stream chain. The closure lifetime only depends
    /// on it's return value.
    ///
    /// # Warning
    /// This creates a cyclic `Arc` reference that can only be broken by the closure signaling it's
    /// deletion (via `ObserveResult`), so if the closure never unregisters itself it will leak memory.
    pub fn observe_strong<F, R>(&self, f: F)
    where
        F: Fn(MaybeOwned<'_, T>) -> R + Send + Sync + 'static,
        T: 'static,
        R: ObserveResult,
    {
        let this = self.clone();
        self.cbs.push(move |arg| {
            let _keepalive = &this;
            f(arg).is_callback_alive()
        });
    }

    /// Chainable version of `Stream::observe`.
    #[inline]
    pub fn inspect<F, R>(self, f: F) -> Self
    where
        F: Fn(MaybeOwned<'_, T>) -> R + Send + Sync + 'static,
        R: ObserveResult,
    {
        self.observe(f);
        self
    }
}

impl<T: 'static> Stream<T> {
    /// Maps this stream into another stream using the provided function.
    ///
    /// The closure will be called every time a stream event is received.
    #[inline]
    pub fn map<F, R>(&self, f: F) -> Stream<R>
    where
        F: Fn(MaybeOwned<'_, T>) -> R + Send + Sync + 'static,
        R: 'static,
    {
        self.filter_map(move |arg| Some(f(arg)))
    }

    /// Creates a new stream that only contains the values where the predicate is `true`.
    pub fn filter<F>(&self, pred: F) -> Self
    where
        F: Fn(&T) -> bool + Send + Sync + 'static,
    {
        let (new_cbs, weak) = arc_and_weak(Callbacks::new());
        self.cbs.push(move |arg| {
            with_weak!(weak, |cb| if pred(&arg) {
                cb.call(arg)
            })
        });
        Stream::new(new_cbs, Source::stream(self))
    }

    /// Does filter and map on a stream simultaneously.
    ///
    /// The output stream will only contain the unwrapped `Some` values returned by the closure.
    pub fn filter_map<F, R>(&self, f: F) -> Stream<R>
    where
        F: Fn(MaybeOwned<'_, T>) -> Option<R> + Send + Sync + 'static,
        R: 'static,
    {
        let (new_cbs, weak) = arc_and_weak(Callbacks::new());
        self.cbs.push(move |arg| {
            with_weak!(weak, |cb| if let Some(val) = f(arg) {
                cb.call(val)
            })
        });
        Stream::new(new_cbs, Source::stream(self))
    }

    /// Creates a new stream that fires with the events from both streams.
    pub fn merge(&self, other: &Stream<T>) -> Self {
        let (new_cbs, weak1) = arc_and_weak(Callbacks::new());
        let weak2 = weak1.clone();
        self.cbs
            .push(move |arg| with_weak!(weak1, |cb| cb.call(arg)));
        other
            .cbs
            .push(move |arg| with_weak!(weak2, |cb| cb.call(arg)));
        Stream::new(new_cbs, Source::stream2(self, other))
    }

    /// Merges two streams of different types using two functions.
    ///
    /// The first function will be called when receiving events on `self`, and the second one
    /// when receiving events from `other`. Their combined values will be used to form a
    /// stream of a single type.
    pub fn merge_with<U, F1, F2, R>(&self, other: &Stream<U>, f1: F1, f2: F2) -> Stream<R>
    where
        F1: Fn(MaybeOwned<'_, T>) -> R + Send + Sync + 'static,
        F2: Fn(MaybeOwned<'_, U>) -> R + Send + Sync + 'static,
        U: 'static,
        R: 'static,
    {
        let (new_cbs, weak1) = arc_and_weak(Callbacks::new());
        let weak2 = weak1.clone();
        self.cbs
            .push(move |arg| with_weak!(weak1, |cb| cb.call(f1(arg))));
        other
            .cbs
            .push(move |arg| with_weak!(weak2, |cb| cb.call(f2(arg))));
        Stream::new(new_cbs, Source::stream2(self, other))
    }

    /// Merges two streams of different types using a single function that takes an `Either` argument.
    ///
    /// Events from `self` will produce an `Either::Left`, and events from `other` will produce
    /// an `Either::Right`.
    #[cfg(feature = "either")]
    #[inline]
    pub fn merge_with_either<U, F, R>(&self, other: &Stream<U>, f: F) -> Stream<R>
    where
        F: Fn(Either<MaybeOwned<'_, T>, MaybeOwned<'_, U>>) -> R + Clone + Send + Sync + 'static,
        U: 'static,
        R: 'static,
    {
        let f_ = f.clone();
        self.merge_with(
            other,
            move |a| f(Either::Left(a)),
            move |b| f_(Either::Right(b)),
        )
    }

    /// Accumulates the values sent over this stream.
    ///
    /// The fold operation is done by taking the accumulator, consuming it's value, and then
    /// putting back the transformed value. This avoids cloning, but if the closure panics it will
    /// leave the storage empty, and then any sampling attempt on this object will panic until
    /// someone puts back a value on it.
    /// If this is undesirable, use `Stream::fold_clone` instead.
    pub fn fold<A, F>(&self, initial: A, f: F) -> Signal<A>
    where
        F: Fn(A, MaybeOwned<'_, T>) -> A + Send + Sync + 'static,
        A: Clone + Send + Sync + 'static,
    {
        let (storage, weak) = arc_and_weak(Storage::new(initial));
        self.cbs.push(move |arg| {
            with_weak!(weak, |st| {
                st.replace(|old| f(old, arg));
            })
        });
        Signal::from_storage(storage, self.clone())
    }

    /// Folds the stream by cloning the accumulator.
    ///
    /// This does the same as `Stream::fold` but it will clone the accumulator on every value
    /// processed. If the closure panics, the storage will remain unchanged and later attempts at
    /// sampling will succeed like nothing happened.
    pub fn fold_clone<A, F>(&self, initial: A, f: F) -> Signal<A>
    where
        F: Fn(A, MaybeOwned<'_, T>) -> A + Send + Sync + 'static,
        A: Clone + Send + Sync + 'static,
    {
        let (storage, weak) = arc_and_weak(Storage::new(initial));
        self.cbs.push(move |arg| {
            with_weak!(weak, |st| {
                st.replace_clone(|old| f(old, arg));
            })
        });
        Signal::from_storage(storage, self.clone())
    }

    /// Maps each stream event to `0..N` output values.
    ///
    /// On every stream event received the closure must return its value by sending it through the
    /// provided Sender. Multiple values (or none) can be sent to the output stream this way.
    ///
    /// This primitive is useful to construct asynchronous operations, since you can store the
    /// Sender and then use it when the data is ready.
    pub fn map_n<F, R>(&self, f: F) -> Stream<R>
    where
        F: Fn(MaybeOwned<'_, T>, Sender<R>) + Send + Sync + 'static,
        R: 'static,
    {
        let (new_cbs, weak) = arc_and_weak(Callbacks::new());
        self.cbs
            .push(move |arg| with_weak!(weak, |cb| f(arg, Sender::new(cb))));
        Stream::new(new_cbs, Source::stream(self))
    }

    /// Folds the stream and returns the accumulator values as a stream.
    ///
    /// This is the equivalent of doing `stream.fold(initial, f).snapshot(&stream, |a, _| a)`,
    /// but more efficient.
    pub fn scan<A, F>(&self, initial: A, f: F) -> Stream<A>
    where
        F: Fn(A, MaybeOwned<'_, T>) -> A + Send + Sync + 'static,
        A: Clone + Send + Sync + 'static,
    {
        let (new_cbs, weak) = arc_and_weak(Callbacks::new());
        let storage = Storage::new(initial);
        self.cbs.push(move |arg| {
            with_weak!(weak, |cb| {
                let new = storage.replace_fetch(|old| f(old, arg));
                cb.call(new)
            })
        });
        Stream::new(new_cbs, Source::stream(self))
    }

    /// Folds the stream and returns `0..N` output values.
    ///
    /// The closure must process the input state `A`, send a value to the output stream using the
    /// provided Sender and then return a new state. Multiple values (or none) can be sent to the
    /// output stream this way.
    pub fn scan_n<A, F, R>(&self, initial: A, f: F) -> Stream<R>
    where
        F: Fn(A, MaybeOwned<'_, T>, Sender<R>) -> A + Send + Sync + 'static,
        A: Send + Sync + 'static,
        R: 'static,
    {
        let (new_cbs, weak) = arc_and_weak(Callbacks::new());
        let storage = Storage::new(initial);
        self.cbs.push(move |arg| {
            with_weak!(weak, |cb| storage.replace(|old| f(
                old,
                arg,
                Sender::new(cb)
            )))
        });
        Stream::new(new_cbs, Source::stream(self))
    }

    /// Creates a collection from the values sent to this stream.
    #[inline]
    pub fn collect<C>(&self) -> Signal<C>
    where
        C: Default + Extend<T> + Clone + Send + Sync + 'static,
        T: Clone,
    {
        self.fold(C::default(), |mut a, v| {
            a.extend(Some(v.into_owned()));
            a
        })
    }

    /// Returns a stream that contains only the Nth value from the input stream.
    pub fn element_at(&self, index: usize) -> Self {
        let (new_cbs, weak) = arc_and_weak(Callbacks::new());
        let pos = AtomicUsize::new(0);
        self.cbs.push(move |arg| {
            weak.upgrade()
                .map(|cb| {
                    let cur_pos = pos.fetch_add(1, Ordering::Relaxed);
                    if cur_pos == index {
                        cb.call(arg);
                    }
                    cur_pos < index // drop the callback after we're done
                })
                .unwrap_or(false)
        });
        Stream::new(new_cbs, Source::stream(self))
    }

    /// Returns a stream that contains the values with index in the specified range.
    pub fn elements_between<B>(&self, range: B) -> Self
    where
        B: RangeBounds<usize> + Send + Sync + 'static,
    {
        let (new_cbs, weak) = arc_and_weak(Callbacks::new());
        let pos = AtomicUsize::new(0);
        self.cbs.push(move |arg| {
            weak.upgrade()
                .map(|cb| {
                    let cur_pos = pos.fetch_add(1, Ordering::Relaxed);
                    let after_start = match range.start_bound() {
                        Bound::Included(s) => cur_pos >= *s,
                        Bound::Excluded(s) => cur_pos > *s,
                        Bound::Unbounded => true,
                    };
                    let before_end = match range.end_bound() {
                        Bound::Included(e) => cur_pos <= *e,
                        Bound::Excluded(e) => cur_pos < *e,
                        Bound::Unbounded => true,
                    };
                    if after_start && before_end {
                        cb.call(arg)
                    }
                    before_end // drop the callback after we're past the end
                })
                .unwrap_or(false)
        });
        Stream::new(new_cbs, Source::stream(self))
    }
}

impl<T: Clone + Send + 'static> Stream<T> {
    /// Creates a Signal that holds the last value sent to this stream.
    #[inline]
    pub fn hold(&self, initial: T) -> Signal<T>
    where
        T: Sync,
    {
        self.hold_if(initial, |_| true)
    }

    /// Holds the last value in this stream where the predicate is `true`.
    pub fn hold_if<F>(&self, initial: T, pred: F) -> Signal<T>
    where
        F: Fn(&T) -> bool + Send + Sync + 'static,
        T: Sync,
    {
        let (storage, weak) = arc_and_weak(Storage::new(initial));
        self.cbs.push(move |arg| {
            with_weak!(weak, |st| if pred(&arg) {
                st.set(arg.into_owned());
            })
        });
        Signal::from_storage(storage, self.clone())
    }

    /// Collects all pairs of values from two streams.
    ///
    /// This creates a Stream of tuples containing each of `self`'s values and `other`'s values in
    /// chronological order. An unique value from both streams is required to send a result to the
    /// output stream.
    #[inline]
    pub fn zip<U>(&self, other: &Stream<U>) -> Stream<(T, U)>
    where
        U: Clone + Send + 'static,
    {
        self.zip_with(other, |a, b| (a, b))
    }

    /// Zips two streams using a custom function.
    pub fn zip_with<U, F, R>(&self, other: &Stream<U>, f: F) -> Stream<R>
    where
        F: Fn(T, U) -> R + Clone + Send + Sync + 'static,
        U: Clone + Send + 'static,
        R: 'static,
    {
        let (new_cbs, weak1) = arc_and_weak(Callbacks::new());
        let weak2 = weak1.clone();

        let left = Arc::new(Mutex::new(VecDeque::new()));
        let right = Arc::new(Mutex::new(VecDeque::new()));
        let left1 = left.clone();
        let right1 = right.clone();
        let f_ = f.clone();

        self.cbs.push(move |arg| {
            with_weak!(weak1, |cb| if let Some(val) = right1.lock().pop_front() {
                cb.call(f(arg.into_owned(), val));
            } else {
                left.lock().push_back(arg.into_owned());
            })
        });

        other.cbs.push(move |arg| {
            with_weak!(weak2, |cb| if let Some(val) = left1.lock().pop_front() {
                cb.call(f_(val, arg.into_owned()));
            } else {
                right.lock().push_back(arg.into_owned());
            })
        });

        Stream::new(new_cbs, Source::stream2(self, other))
    }

    /// Collects pairs of values from two streams using their last value seen.
    ///
    /// This creates a Stream that sends the last value of `self` and `other` when either of those
    /// receives a value. The stream values before calling this function aren't known, so to send
    /// the first output value it's required that both input streams send their initial value.
    #[inline]
    pub fn combine<U>(&self, other: &Stream<U>) -> Stream<(T, U)>
    where
        U: Clone + Send + 'static,
    {
        self.combine_with(other, |a, b| (a, b))
    }

    /// Combines two streams using a custom function.
    pub fn combine_with<U, F, R>(&self, other: &Stream<U>, f: F) -> Stream<R>
    where
        F: Fn(T, U) -> R + Clone + Send + Sync + 'static,
        U: Clone + Send + 'static,
        R: 'static,
    {
        let (new_cbs, weak1) = arc_and_weak(Callbacks::new());
        let weak2 = weak1.clone();

        let left = Arc::new(Mutex::new(None));
        let right = Arc::new(Mutex::new(None));
        let left1 = left.clone();
        let right1 = right.clone();
        let f_ = f.clone();

        self.cbs.push(move |arg| {
            with_weak!(weak1, |cb| {
                let arg = arg.into_owned();
                *left.lock() = Some(arg.clone());
                if let Some(val) = right1.lock().as_ref() {
                    cb.call(f(arg, U::clone(val)));
                }
            })
        });

        other.cbs.push(move |arg| {
            with_weak!(weak2, |cb| {
                let arg = arg.into_owned();
                *right.lock() = Some(arg.clone());
                if let Some(val) = left1.lock().as_ref() {
                    cb.call(f_(T::clone(val), arg));
                }
            })
        });

        Stream::new(new_cbs, Source::stream2(self, other))
    }

    /// Creates a future that returns the next value sent to this stream.
    #[inline]
    pub fn next(&self) -> StreamFuture<T> {
        StreamFuture::new(self.clone())
    }
}

impl<T: Clone + 'static> Stream<Option<T>> {
    /// Filters a stream of `Option`, returning only the unwrapped `Some` values.
    #[inline]
    pub fn filter_some(&self) -> Stream<T> {
        self.filter_first()
    }
}

impl<T: Clone + 'static, E: Clone + 'static> Stream<Result<T, E>> {
    /// Filters a stream of `Result`, returning only the unwrapped `Ok` values.
    #[inline]
    pub fn filter_ok(&self) -> Stream<T> {
        self.filter_first()
    }

    /// Filters a stream of `Result`, returning only the unwrapped `Err` values.
    #[inline]
    pub fn filter_err(&self) -> Stream<E> {
        self.filter_second()
    }
}

impl<T: SumType2 + Clone + 'static> Stream<T>
where
    T::Type1: 'static,
    T::Type2: 'static,
{
    /// Creates a stream with only the first element of a sum type.
    pub fn filter_first(&self) -> Stream<T::Type1> {
        self.filter_map(|res| {
            if res.is_type1() {
                res.into_owned().into_type1()
            } else {
                None
            }
        })
    }

    /// Creates a stream with only the second element of a sum type.
    pub fn filter_second(&self) -> Stream<T::Type2> {
        self.filter_map(|res| {
            if res.is_type2() {
                res.into_owned().into_type2()
            } else {
                None
            }
        })
    }

    /// Splits a two element sum type stream into two streams with the unwrapped values.
    pub fn split(&self) -> (Stream<T::Type1>, Stream<T::Type2>) {
        let (cbs_1, weak_1) = arc_and_weak(Callbacks::new());
        let (cbs_2, weak_2) = arc_and_weak(Callbacks::new());
        self.cbs.push(move |result| {
            if result.is_type1() {
                if let Some(cb) = weak_1.upgrade() {
                    cb.call(result.into_owned().into_type1().unwrap());
                    true
                } else {
                    // drop callback if both output streams dropped
                    weak_2.upgrade().is_some()
                }
            } else
            // if result.is_type2()
            {
                if let Some(cb) = weak_2.upgrade() {
                    cb.call(result.into_owned().into_type2().unwrap());
                    true
                } else {
                    weak_1.upgrade().is_some()
                }
            }
        });
        let source = Source::stream(self);
        let stream_1 = Stream::new(cbs_1, source.clone());
        let stream_2 = Stream::new(cbs_2, source);
        (stream_1, stream_2)
    }
}

impl<T: 'static> Stream<Stream<T>> {
    /// Listens to the events from the last stream sent to a nested stream.
    pub fn switch(&self) -> Stream<T> {
        let (new_cbs, weak) = arc_and_weak(Callbacks::new());
        let id = Arc::new(AtomicUsize::new(0)); // id of each stream sent
        self.cbs.push(move |stream| {
            if weak.upgrade().is_none() {
                return false;
            }
            let cbs_w = weak.clone();
            let cur_id = id.clone();
            // increment the id so it will only send to the last stream
            let my_id = id.fetch_add(1, Ordering::Relaxed) + 1;
            // redirect the inner stream to the output stream
            stream.cbs.push(move |arg| {
                if my_id != cur_id.load(Ordering::Relaxed) {
                    return false;
                }
                with_weak!(cbs_w, |cb| cb.call(arg))
            });
            true
        });
        Stream::new(new_cbs, Source::stream(self))
    }
}

impl<T> Clone for Stream<T> {
    /// Creates a copy of this stream that references the same event chain.
    fn clone(&self) -> Self {
        Stream {
            cbs: self.cbs.clone(),
            source: self.source.clone(),
        }
    }
}

impl<T> Default for Stream<T> {
    /// Creates a stream that never fires.
    #[inline]
    fn default() -> Self {
        Stream::never()
    }
}

/// Sends values into a stream.
///
/// This is a restricted version of `Sink` used by `Stream::map_n` and `Stream::scan_n`.
#[derive(Debug)]
pub struct Sender<T>(Sink<T>);

impl<T> Sender<T> {
    /// Constructs a new Sender from a list of callbacks.
    fn new(cbs: Arc<Callbacks<T>>) -> Self {
        Sender(Sink { cbs })
    }

    /// Sends a value.
    #[inline]
    pub fn send(&self, val: T) {
        self.0.send(val)
    }

    /// Sends multiple values.
    #[inline]
    pub fn feed(&self, iter: impl IntoIterator<Item = T>) {
        self.0.feed(iter)
    }
}

impl<T> Clone for Sender<T> {
    /// Creates a copy of this sender that references the same event source.
    fn clone(&self) -> Self {
        Sender(self.0.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    impl<T: Clone + Send + 'static> Stream<T> {
        /// Creates a sync channel and sends the stream events through it.
        fn as_sync_channel(&self, bound: usize) -> mpsc::Receiver<T> {
            let (tx, rx) = mpsc::sync_channel(bound);
            self.observe(move |arg| tx.send(arg.into_owned()));
            rx
        }
    }

    #[test]
    fn stream_basic() {
        let sink = Sink::new();
        let stream = sink.stream();
        let rx = stream.as_sync_channel(20);

        sink.send(42);
        sink.send(33);
        sink.send(12);
        sink.feed(0..5);
        sink.feed(vec![11, 22, 33]);

        let result: Vec<_> = rx.try_iter().collect();
        assert_eq!(result, [42, 33, 12, 0, 1, 2, 3, 4, 11, 22, 33]);
    }

    #[test]
    fn stream_send_ref() {
        #[derive(Debug, Clone, PartialEq, Eq)]
        struct Test(i32);

        let sink: Sink<Test> = Sink::new();
        let stream = sink.stream();
        let rx = stream.as_sync_channel(10);

        {
            let a = Test(42);
            let b = [Test(33), Test(-1)];
            sink.send(&a);
            sink.feed(&b);
        }

        assert_eq!(rx.try_recv(), Ok(Test(42)));
        assert_eq!(rx.try_recv(), Ok(Test(33)));
        assert_eq!(rx.try_recv(), Ok(Test(-1)));
    }

    #[test]
    fn stream_switch() {
        let stream_sink = Sink::new();
        let sink1 = Sink::new();
        let sink2 = Sink::new();

        let switched = stream_sink.stream().switch();
        let events = switched.as_sync_channel(10);

        sink1.send(1);
        sink2.send(2);

        stream_sink.send(sink2.stream());
        sink1.send(3);
        sink2.send(4);
        assert_eq!(events.try_recv(), Ok(4));

        stream_sink.send(sink1.stream());
        sink1.send(5);
        sink2.send(6);
        assert_eq!(events.try_recv(), Ok(5));
    }

    #[test]
    fn stream_default() {
        let sink: Sink<i32> = Default::default();
        let stream1 = sink.stream();
        let stream2: Stream<i32> = Default::default();
        let merged = stream1.merge(&stream2);
        let rx = merged.as_sync_channel(10);

        sink.send(42);
        sink.send(13);

        assert_eq!(rx.try_recv(), Ok(42));
        assert_eq!(rx.try_recv(), Ok(13));
    }

    #[test]
    fn stream_scan() {
        let sink = Sink::new();
        let stream = sink.stream().scan(0, |a, n| a + *n);
        let rx = stream.as_sync_channel(10);

        sink.send(1);
        assert_eq!(rx.try_recv(), Ok(1));
        sink.send(2);
        sink.send(10);
        assert_eq!(rx.try_recv(), Ok(3));
        assert_eq!(rx.try_recv(), Ok(13));
    }

    #[test]
    fn stream_scan_n() {
        let sink = Sink::new();
        let stream = sink.stream().scan_n(std::i32::MIN, |a, n, sender| {
            let n = *n;
            if n > a {
                sender.send(n);
                n
            } else {
                a
            }
        });
        let rx = stream.as_sync_channel(10);

        sink.feed(&[1, 2, -1, 10, 5, 7, 42]);

        let result: Vec<_> = rx.try_iter().collect();
        assert_eq!(result, [1, 2, 10, 42]);
    }

    #[test]
    fn stream_observe_strong() {
        let sink = Sink::new();
        let (tx, rx) = mpsc::sync_channel(10);
        let (arc, weak) = arc_and_weak(Arc::new(()));
        sink.stream().map(|x| *x * 2).observe_strong(move |x| {
            let _a = &arc;
            tx.send(*x)
        });

        sink.send(6);
        assert_eq!(rx.try_recv(), Ok(12));
        assert!(weak.upgrade().is_some());

        drop(rx);
        sink.send(10);
        assert_eq!(weak.upgrade(), None);
        sink.send(42);
        assert_eq!(sink.cbs.len(), 0);
    }

    #[cfg(feature = "crossbeam-utils")]
    #[test]
    fn stream_send_parallel() {
        use std::thread;
        use std::time::{Duration, Instant};

        let sink = Sink::new();
        let s1 = sink.stream().map(|x| {
            thread::sleep(Duration::from_millis(50));
            *x + 1
        });
        let s2 = sink.stream().map(|x| {
            thread::sleep(Duration::from_millis(50));
            *x * 2
        });
        let result = s1.merge(&s2).fold(0, |a, n| a + *n);

        let t = Instant::now();
        sink.send_parallel(&10);
        assert!(t.elapsed() < Duration::from_millis(100));
        assert_eq!(result.sample(), 31);
        sink.send_parallel(&1);
        sink.send_parallel(&13);
        assert_eq!(result.sample(), 75);
    }

    #[test]
    fn stream_zip() {
        use std::sync::mpsc::TryRecvError::Empty;

        let sink1: Sink<i32> = Sink::new();
        let sink2: Sink<&str> = Sink::new();
        let zipped = sink1.stream().zip(&sink2.stream());
        let rx = zipped.as_sync_channel(10);

        sink1.send(1);
        assert_eq!(rx.try_recv(), Err(Empty));

        sink2.send("foo");
        assert_eq!(rx.try_recv(), Ok((1, "foo")));

        sink2.send("bar");
        assert_eq!(rx.try_recv(), Err(Empty));

        sink2.send("asd");
        sink1.send(2);
        assert_eq!(rx.try_recv(), Ok((2, "bar")));
    }

    #[test]
    fn stream_combine() {
        use std::sync::mpsc::TryRecvError::Empty;

        let sink1: Sink<i32> = Sink::new();
        let sink2: Sink<&str> = Sink::new();
        let combined = sink1.stream().combine(&sink2.stream());
        let rx = combined.as_sync_channel(10);

        sink1.send(1);
        assert_eq!(rx.try_recv(), Err(Empty));

        sink2.send("foo");
        assert_eq!(rx.try_recv(), Ok((1, "foo")));

        sink1.send(2);
        assert_eq!(rx.try_recv(), Ok((2, "foo")));

        sink1.send(3);
        assert_eq!(rx.try_recv(), Ok((3, "foo")));
    }

    #[test]
    fn stream_element_at() {
        use std::sync::mpsc::TryRecvError::Empty;

        let sink: Sink<i32> = Sink::new();
        let stream1 = sink.stream().element_at(0);
        let stream2 = sink.stream().element_at(2);
        let stream3 = sink.stream().element_at(13);
        let rx1 = stream1.as_sync_channel(10);
        let rx2 = stream2.as_sync_channel(10);
        let rx3 = stream3.as_sync_channel(10);

        sink.feed(&[1, 12, 42, 7, 13]);

        assert_eq!(rx1.try_recv(), Ok(1));
        assert_eq!(rx1.try_recv(), Err(Empty));
        assert_eq!(rx2.try_recv(), Ok(42));
        assert_eq!(rx2.try_recv(), Err(Empty));
        assert_eq!(rx3.try_recv(), Err(Empty));
    }

    #[test]
    fn stream_elements_between() {
        let sink: Sink<i32> = Sink::new();
        let stream1 = sink.stream().elements_between(..3);
        let stream2 = sink.stream().elements_between(2..=4);
        let stream3 = sink.stream().elements_between(3..);
        let rx1 = stream1.as_sync_channel(10);
        let rx2 = stream2.as_sync_channel(10);
        let rx3 = stream3.as_sync_channel(10);

        sink.feed(&[1, 12, 42, 7, 13, -6, 22]);

        let result1: Vec<_> = rx1.try_iter().collect();
        let result2: Vec<_> = rx2.try_iter().collect();
        let result3: Vec<_> = rx3.try_iter().collect();
        assert_eq!(result1, [1, 12, 42]);
        assert_eq!(result2, [42, 7, 13]);
        assert_eq!(result3, [7, 13, -6, 22]);
    }

    #[cfg(feature = "nightly")]
    #[test]
    fn stream_await() {
        use futures::executor::LocalPool;
        use futures::task::SpawnExt;
        use std::thread;
        use std::time::Duration;

        let sink = Sink::new();
        let future = sink.stream().map(|a| *a * 2).next();
        let mut pool = LocalPool::new();

        pool.spawner()
            .spawn(async {
                let res = future.await;
                assert_eq!(res, 42);
            })
            .unwrap();

        thread::spawn(move || {
            thread::sleep(Duration::from_millis(100));
            sink.send(21);
        });

        pool.run();
    }
}
