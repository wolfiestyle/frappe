use std::rc::Rc;
use std::sync::mpsc;
use std::fmt;
use types::{MaybeOwned, Storage};
use stream::Stream;

use self::SigValue::*;

/// Represents a discrete value that changes over time.
///
/// Signals are usually constructed by stream operations and can be read using the `sample` or
/// `sample_with` methods.
#[derive(Debug)]
pub struct Signal<T>(SigValue<T>);

enum SigValue<T>
{
    /// A signal with constant value.
    ///
    /// We store the value in a Rc to provide Clone support without a `T: Clone` bound.
    Constant(Rc<T>),
    /// A signal that generates it's values from a function.
    ///
    /// This is produced by `Signal::from_fn`
    Dynamic(Rc<Fn() -> T>),
    /// A signal that contains shared data.
    ///
    /// This is produced by stream methods that create a signal.
    /// It contains an updater function and a value storage.
    Shared(Rc<Fn()>, Rc<Storage<T>>),
    /// A signal that contains a signal, and allows sampling the inner signal directly.
    ///
    /// This is produced by `Signal::switch`
    Nested(Rc<Fn() -> Signal<T>>),
}

impl<T> Signal<T>
{
    /// Creates a signal with constant value.
    ///
    /// The value is assumed to be constant, so changing it while it's stored on the
    /// signal is a logic error and will cause unexpected results.
    pub fn constant<V: Into<Rc<T>>>(val: V) -> Self
    {
        Signal(Constant(val.into()))
    }

    /// Creates a signal that samples it's values from an external source.
    ///
    /// The closure is meant to sample a continuous value from the real world,
    /// so the signal value is assumed to be always changing.
    pub fn from_fn<F>(f: F) -> Self
        where F: Fn() -> T + 'static
    {
        Signal(Dynamic(Rc::new(f)))
    }

    /// Creates a new shared signal.
    fn shared<F>(storage: Rc<Storage<T>>, updater: F) -> Self
        where F: Fn() + 'static
    {
        Signal(Shared(Rc::new(updater), storage))
    }

    /// Checks if the signal has changed since the last time it was sampled.
    pub fn has_changed(&self) -> bool
    {
        match self.0 {
            Constant(_) => false,
            Dynamic(_) | Nested(_) => true,
            Shared(_, ref st) => st.must_update(),
        }
    }

    /// Sample by reference.
    ///
    /// This is meant to be the most efficient way when cloning is undesirable,
    /// but it requires a callback to prevent outliving internal borrows.
    pub fn sample_with<F, R>(&self, cb: F) -> R
        where F: FnOnce(MaybeOwned<T>) -> R
    {
        match self.0
        {
            Constant(ref val) => cb(MaybeOwned::Borrowed(val)),
            Dynamic(ref f) => cb(MaybeOwned::Owned(f())),
            Shared(ref f, ref st) => { f(); st.borrow_with(cb) },
            Nested(ref f) => f().sample_with(cb),
        }
    }
}

impl<T: Clone> Signal<T>
{
    /// Sample by value.
    ///
    /// This will clone the content of the signal.
    pub fn sample(&self) -> T
    {
        match self.0
        {
            Constant(ref val) => (**val).clone(),
            Dynamic(ref f) => f(),
            Shared(ref f, ref st) => { f(); st.get() },
            Nested(ref f) => f().sample(),
        }
    }
}

impl<T: 'static> Signal<T>
{
    /// Maps a signal with the provided function.
    ///
    /// The closure is called only when the parent signal changes.
    pub fn map<F, R>(&self, f: F) -> Signal<R>
        where F: Fn(MaybeOwned<T>) -> R + 'static,
        R: 'static
    {
        match self.0 {
            // constant signal: apply f once to produce another constant signal
            Constant(ref val) => {
                Signal::constant(f(MaybeOwned::Borrowed(val)))
            }
            // shared signal: apply f only when the parent signal has changed
            Shared(ref upd_, ref st) => {
                let updater = upd_.clone();
                let parent_st = st.clone();
                // the storage inherits the root serial so we can track changes at the source
                let storage = Rc::new(Storage::empty(st.root_ser.clone()));
                Signal::shared(storage.clone(), move || {
                    if storage.must_update()
                    {
                        // sample and map the parent signal
                        updater();
                        storage.set_local(parent_st.borrow_with(&f))
                    }
                })
            }
            // dynamic/nested signal: apply f unconditionally
            Dynamic(_) | Nested(_) => {
                let this = self.clone();
                Signal::from_fn(move || this.sample_with(&f))
            }
        }
    }

    /// Samples the value of this signal every time the trigger stream fires.
    pub fn snapshot<S, F, R>(&self, trigger: &Stream<S>, f: F) -> Stream<R>
        where F: Fn(MaybeOwned<T>, MaybeOwned<S>) -> R + 'static,
        S: 'static, R: 'static
    {
        let this = self.clone();
        trigger.map(move |b| this.sample_with(|a| f(a, b)))
    }

    /// Creates a signal from a shared value.
    pub(crate) fn from_storage<A: 'static>(storage: Rc<Storage<T>>, keepalive: A) -> Self
    {
        Signal::shared(storage, move || {
            // the closure owns and does type erasure on the value
            let _keepalive = &keepalive;
        })
    }

    /// Stores the last value sent to a channel.
    ///
    /// When sampled, the resulting signal consumes all the current values on the channel
    /// (using non-blocking operations) and returns the last value seen.
    pub fn from_channel(initial: T, rx: mpsc::Receiver<T>) -> Self
    {
        let storage = Rc::new(Storage::new(initial));
        Signal::shared(storage.clone(), move || {
            let last = rx.try_iter().last();
            if let Some(val) = last { storage.set(val); }
        })
    }

    /// Creates a signal that folds the values from a channel.
    ///
    /// When sampled, the resulting signal consumes all the current values on the channel
    /// (using non-blocking operations) and folds them using the current signal value as the
    /// initial accumulator state.
    pub fn fold_channel<V, F>(initial: T, rx: mpsc::Receiver<V>, f: F) -> Self
        where F: Fn(T, V) -> T + 'static,
        V: 'static
    {
        let storage = Rc::new(Storage::new(initial));
        Signal::shared(storage.clone(), move || {
            if let Ok(first) = rx.try_recv()
            {
                let acc = f(storage.take(), first);
                let current = rx.try_iter().fold(acc, &f);
                storage.set(current);
            }
        })
    }
}

impl<T: 'static> Signal<Signal<T>>
{
    /// Creates a new signal that samples the inner value of a nested signal.
    pub fn switch(&self) -> Signal<T>
    {
        match self.0
        {
            // constant signal: just extract the inner signal
            Constant(ref sig) => (**sig).clone(),
            // dynamic signal: re-label as nested
            Dynamic(ref f) => Signal(Nested(f.clone())),
            // shared signal: sample to extract the inner signal
            Shared(ref upd, ref st) => {
                let updater = upd.clone();
                let storage = st.clone();
                Signal(Nested(Rc::new(move || { updater(); storage.get() })))
            }
            // nested signal: remove one layer
            Nested(ref f_) => {
                let f = f_.clone();
                Signal(Nested(Rc::new(move || f().sample())))
            }
        }
    }
}

impl<T: Default> Default for Signal<T>
{
    /// Creates a constant signal with T's default value.
    #[inline]
    fn default() -> Self
    {
        Signal::constant(T::default())
    }
}

impl<T> From<T> for Signal<T>
{
    /// Creates a constant signal from T.
    #[inline]
    fn from(val: T) -> Self
    {
        Signal::constant(val)
    }
}

impl<T> From<Rc<T>> for Signal<T>
{
    /// Creates a constant signal from T (avoids re-wrapping the Rc).
    #[inline]
    fn from(val: Rc<T>) -> Self
    {
        Signal(Constant(val))
    }
}

// the derive impl adds a `T: Clone` we don't want
impl<T> Clone for Signal<T>
{
    /// Creates a copy of this signal that references the same value.
    fn clone(&self) -> Self
    {
        Signal(match self.0
        {
            Constant(ref val) => Constant(val.clone()),
            Dynamic(ref rf) => Dynamic(rf.clone()),
            Shared(ref rf, ref st) => Shared(rf.clone(), st.clone()),
            Nested(ref rf) => Nested(rf.clone()),
        })
    }
}

impl<T: fmt::Debug> fmt::Debug for SigValue<T>
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result
    {
        match *self
        {
            Constant(ref val) => write!(f, "Constant({:?})", val),
            Dynamic(ref rf) => write!(f, "Dynamic(Fn@{:p})", rf),
            Shared(ref rf, ref st) => write!(f, "Shared(Fn@{:p}, {:?})", rf, st),
            Nested(ref rf) => write!(f, "Nested(Fn@{:p})", rf),
        }
    }
}

impl<T: fmt::Display> fmt::Display for Signal<T>
{
    /// Samples the signal and formats the value.
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result
    {
        self.sample_with(|val| fmt::Display::fmt(&*val, f))
    }
}


#[cfg(test)]
mod tests
{
    use super::*;
    use std::rc::Rc;
    use std::cell::Cell;
    use std::time::Instant;

    #[test]
    fn signal_basic()
    {
        let signal = Signal::constant(42);
        let double = signal.map(|a| *a * 2);
        let plusone = double.map(|a| *a + 1);
        assert_eq!(signal.sample(), 42);
        assert_eq!(double.sample(), 84);
        assert_eq!(plusone.sample(), 85);
        signal.sample_with(|val| assert_eq!(*val, 42));
    }

    #[test]
    fn signal_shared()
    {
        let st = Rc::new(Storage::new(1));
        let signal = Signal::from_storage(st.clone(), ());
        let double = signal.map(|a| *a * 2);

        assert_eq!(signal.sample(), 1);
        assert_eq!(double.sample(), 2);
        st.set(42);
        assert_eq!(signal.sample(), 42);
        assert_eq!(double.sample(), 84);
    }

    #[test]
    fn signal_dynamic()
    {
        let t = Instant::now();
        let signal = Signal::from_fn(move || t);
        assert_eq!(signal.sample(), t);
        signal.sample_with(|val| assert_eq!(*val, t));

        let n = Rc::new(Cell::new(1));
        let cloned = n.clone();
        let signal = Signal::from_fn(move || cloned.get());
        let double = signal.map(|a| *a * 2);
        let plusone = double.map(|a| *a + 1);
        assert_eq!(signal.sample(), 1);
        assert_eq!(double.sample(), 2);
        assert_eq!(plusone.sample(), 3);
        n.set(13);
        assert_eq!(signal.sample(), 13);
        assert_eq!(double.sample(), 26);
        assert_eq!(plusone.sample(), 27);
    }
}
