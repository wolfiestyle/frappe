use std::sync::{mpsc, Arc, Mutex};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::any::Any;
use std::iter;
use crate::types::{Callbacks, SumType2, MaybeOwned, SharedImpl};
use crate::helpers::arc_and_weak;
use crate::signal::Signal;

/// A source of events that feeds the streams connected to it.
#[derive(Debug)]
pub struct Sink<T>
{
    cbs: Arc<Callbacks<T>>,
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
        Stream::new(self.cbs.clone(), ())
    }

    /// Sends a value into the sink.
    ///
    /// The value will be distributed `N-1` times as reference and then one time by value,
    /// where `N` is the amount of streams connected to this sink.
    #[inline]
    pub fn send(&self, val: T)
    {
        self.cbs.call(val)
    }

    /// Sends a value by reference.
    #[inline]
    pub fn send_ref(&self, val: &T)
    {
        self.cbs.call_ref(val)
    }

    /// Sends multiple values into the sink.
    #[inline]
    pub fn feed<I>(&self, iter: I)
        where I: IntoIterator<Item=T>
    {
        for val in iter
        {
            self.cbs.call(val)
        }
    }

    /// Sends multiple values by reference.
    #[inline]
    pub fn feed_ref<'a, I>(&self, iter: I)
        where T: 'a, I: IntoIterator<Item=&'a T>
    {
        for val in iter
        {
            self.cbs.call_ref(val)
        }
    }
}

impl<T> Default for Sink<T>
{
    /// Creates a new sink.
    #[inline]
    fn default() -> Self
    {
        Sink::new()
    }
}

impl<T> Clone for Sink<T>
{
    /// Creates a copy of this sink that references the same event source.
    fn clone(&self) -> Self
    {
        Sink{ cbs: self.cbs.clone() }
    }
}

/// A stream of discrete events sent over time.
///
/// All the objects that result from stream operations contain an internal reference to it's parent,
/// so dropping intermediate temporary streams (like the ones created from chaining methods) won't break the chain.
#[derive(Debug)]
pub struct Stream<T>
{
    cbs: Arc<Callbacks<T>>,
    source: Arc<dyn Any + Send + Sync>,  // strong reference to a parent Stream
}

impl<T> Stream<T>
{
    fn new<S: Send + Sync + 'static>(cbs: Arc<Callbacks<T>>, source: S) -> Self
    {
        Stream{ cbs, source: Arc::new(source) }
    }

    /// Creates a stream that never fires.
    pub fn never() -> Self
    {
        Stream::new(Default::default(), ())
    }
}

impl<T: 'static> Stream<T>
{
    /// Maps this stream into another stream using the provided function.
    #[inline]
    pub fn map<F, R>(&self, f: F) -> Stream<R>
        where F: Fn(MaybeOwned<'_, T>) -> R + Send + Sync + 'static,
        R: 'static
    {
        self.filter_map(move |arg| Some(f(arg)))
    }

    /// Creates a new stream that only contains the values where the predicate is `true`.
    pub fn filter<F>(&self, pred: F) -> Self
        where F: Fn(&T) -> bool + Send + Sync + 'static
    {
        let (new_cbs, weak) = arc_and_weak(Callbacks::new());
        self.cbs.push(move |arg| {
            with_weak!(weak, |cb| if pred(&arg) { cb.call_dyn(arg) })
        });
        Stream::new(new_cbs, self.clone())
    }

    /// Filter and map a stream simultaneously.
    pub fn filter_map<F, R>(&self, f: F) -> Stream<R>
        where F: Fn(MaybeOwned<'_, T>) -> Option<R> + Send + Sync + 'static,
        R: 'static
    {
        let (new_cbs, weak) = arc_and_weak(Callbacks::new());
        self.cbs.push(move |arg| {
            with_weak!(weak, |cb| if let Some(val) = f(arg) { cb.call(val) })
        });
        Stream::new(new_cbs, self.clone())
    }

    /// Creates a new stream that fires with the events from both streams.
    pub fn merge(&self, other: &Stream<T>) -> Self
    {
        let (new_cbs, weak1) = arc_and_weak(Callbacks::new());
        let weak2 = weak1.clone();
        self.cbs.push(move |arg| {
            with_weak!(weak1, |cb| cb.call_dyn(arg))
        });
        other.cbs.push(move |arg| {
            with_weak!(weak2, |cb| cb.call_dyn(arg))
        });
        Stream::new(new_cbs, (self.clone(), other.clone()))
    }

    /// Merges two streams of different types using two functions that return the same type.
    pub fn merge_with<U, F1, F2, R>(&self, other: &Stream<U>, f1: F1, f2: F2) -> Stream<R>
        where F1: Fn(MaybeOwned<'_, T>) -> R + Send + Sync + 'static,
        F2: Fn(MaybeOwned<'_, U>) -> R + Send + Sync + 'static,
        U: 'static, R: 'static
    {
        let (new_cbs, weak1) = arc_and_weak(Callbacks::new());
        let weak2 = weak1.clone();
        self.cbs.push(move |arg| {
            with_weak!(weak1, |cb| cb.call(f1(arg)))
        });
        other.cbs.push(move |arg| {
            with_weak!(weak2, |cb| cb.call(f2(arg)))
        });
        Stream::new(new_cbs, (self.clone(), other.clone()))
    }

    /// Accumulates the values sent over this stream.
    ///
    /// The fold operation is done by taking the accumulator, consuming it's value, and then
    /// putting back the transformed value. This avoids cloning, but if the closure panics it will
    /// leave the storage empty, and then any sampling attempt on this object will panic until
    /// someone puts back a value on it.
    /// If this is undesirable, use `Stream::fold_clone` instead.
    pub fn fold<A, F>(&self, initial: A, f: F) -> Signal<A>
        where F: Fn(A, MaybeOwned<'_, T>) -> A + Send + Sync + 'static,
        A: Send + Sync + 'static
    {
        let (storage, weak) = arc_and_weak(SharedImpl::new(initial, self.clone()));
        self.cbs.push(move |arg| {
            with_weak!(weak, |st| {
                st.replace_with(|old| f(old, arg));
            })
        });

        Signal::from_storage(storage)
    }

    /// Folds the stream by cloning the accumulator.
    ///
    /// This will clone the accumulator on every value processed, but if the closure panics, the
    /// storage will remain unchanged and later attempts at sampling will succeed like nothing
    /// happened.
    pub fn fold_clone<A, F>(&self, initial: A, f: F) -> Signal<A>
        where F: Fn(A, MaybeOwned<'_, T>) -> A + Send + Sync + 'static,
        A: Clone + Send + Sync + 'static
    {
        let (storage, weak) = arc_and_weak(SharedImpl::new(initial, self.clone()));
        self.cbs.push(move |arg| {
            with_weak!(weak, |st| {
                let old = st.get();
                let new = f(old, arg);
                st.set(new);
            })
        });

        Signal::from_storage(storage)
    }

    /// Maps each stream event to `0..N` output values.
    ///
    /// The closure must return it's value by sending it through the provided sink.
    /// Multiple values (or none) can be sent to the output stream this way.
    ///
    /// This primitive is useful to construct asynchronous operations, since you can
    /// store the sink for later usage.
    pub fn map_n<F, R>(&self, f: F) -> Stream<R>
        where F: Fn(MaybeOwned<'_, T>, Sink<R>) + Send + Sync + 'static,
        R: 'static
    {
        let (new_cbs, weak) = arc_and_weak(Callbacks::new());
        self.cbs.push(move |arg| {
            with_weak!(weak, |cb| f(arg, Sink{ cbs: cb }))
        });
        Stream::new(new_cbs, self.clone())
    }

    /// Reads the values without modifying them.
    ///
    /// This is meant to be used as a debugging tool and not to cause side effects.
    pub fn inspect<F>(self, f: F) -> Self
        where F: Fn(MaybeOwned<'_, T>) + Send + Sync + 'static
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
        where T: Send + Sync
    {
        self.hold_if(initial, |_| true)
    }

    /// Holds the last value in this stream where the predicate is `true`.
    pub fn hold_if<F>(&self, initial: T, pred: F) -> Signal<T>
        where F: Fn(&T) -> bool + Send + Sync + 'static,
        T: Send + Sync
    {
        let (storage, weak) = arc_and_weak(SharedImpl::new(initial, self.clone()));
        self.cbs.push(move |arg| {
            with_weak!(weak, |st| if pred(&arg) { st.set(arg.into_owned()); })
        });

        Signal::from_storage(storage)
    }

    /// Creates a collection from the values of this stream.
    #[inline]
    pub fn collect<C>(&self) -> Signal<C>
        where C: Default + Extend<T> + Send + Sync + 'static
    {
        self.fold(C::default(), |mut a, v| {
            a.extend(iter::once(v.into_owned()));
            a
        })
    }

    /// Creates a channel and sends the stream events through it.
    ///
    /// This doesn't create a strong reference to the parent stream, so the channel sender will be dropped
    /// when the stream is deleted.
    pub fn as_channel(&self) -> mpsc::Receiver<T>
        where T: Send
    {
        let (tx, rx) = mpsc::channel();
        //FIXME: it should use one Sender instance per thread but idk how to do it
        let tx_ = Mutex::new(tx);
        self.cbs.push(move |arg| {
            tx_.lock().unwrap().send(arg.into_owned()).is_ok()
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
        let (cbs_1, weak_1) = arc_and_weak(Callbacks::new());
        let (cbs_2, weak_2) = arc_and_weak(Callbacks::new());
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
        let source_rc = Arc::new(self.clone());
        let stream_1 = Stream{ cbs: cbs_1, source: source_rc.clone() };
        let stream_2 = Stream{ cbs: cbs_2, source: source_rc };
        (stream_1, stream_2)
    }
}

impl<T: 'static> Stream<Stream<T>>
{
    /// Listens to the events from the last stream sent to a nested stream
    pub fn switch(&self) -> Stream<T>
    {
        let (new_cbs, weak) = arc_and_weak(Callbacks::new());
        let id = Arc::new(AtomicUsize::new(0));  // id of each stream sent
        self.cbs.push(move |stream| {
            if weak.upgrade().is_none() { return false }
            let cbs_w = weak.clone();
            let cur_id = id.clone();
            // increment the id so it will only send to the last stream
            let my_id = id.fetch_add(1, Ordering::SeqCst) + 1;
            // redirect the inner stream to the output stream
            stream.cbs.push(move |arg| {
                if my_id != cur_id.load(Ordering::SeqCst) { return false }
                with_weak!(cbs_w, |cb| cb.call_dyn(arg))
            });
            true
        });
        Stream::new(new_cbs, self.clone())
    }
}

impl<T> Clone for Stream<T>
{
    /// Creates a copy of this stream that references the same event chain.
    fn clone(&self) -> Self
    {
        Stream{ cbs: self.cbs.clone(), source: self.source.clone() }
    }
}

impl<T> Default for Stream<T>
{
    /// Creates a stream that never fires.
    #[inline]
    fn default() -> Self
    {
        Stream::never()
    }
}


#[cfg(test)]
mod tests
{
    use super::*;

    #[test]
    fn stream_basic()
    {
        let sink = Sink::new();
        let stream = sink.stream();
        let rx = stream.as_channel();

        sink.send(42);
        sink.send(33);
        sink.send_ref(&12);
        sink.feed(0..5);
        sink.feed_ref(&[11, 22, 33]);

        let result: Vec<_> = rx.try_iter().collect();
        assert_eq!(result, [42, 33, 12, 0, 1, 2, 3, 4, 11, 22, 33]);
    }

    #[test]
    fn stream_switch()
    {
        let stream_sink = Sink::new();
        let sink1 = Sink::new();
        let sink2 = Sink::new();

        let switched = stream_sink.stream().switch();
        let events = switched.as_channel();

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
}
