//! Signals are values that discretely change over time.

use crate::stream::Stream;
use crate::types::{MaybeOwned, SharedImpl, SharedSignal, Storage};
use parking_lot::Mutex;
use std::fmt;
use std::sync::{mpsc, Arc};

use self::SigValue::*;

/// Represents a discrete value that changes over time.
///
/// Signals are usually constructed by stream operations and can be read using the `sample` or
/// `take` methods. They update lazily when someone reads them.
#[derive(Clone, Debug)]
pub struct Signal<T>(SigValue<T>);

/// The content source of a signal.
#[derive(Clone)]
enum SigValue<T> {
    /// A signal with constant value.
    Constant(T),
    /// A signal that generates it's values from a function.
    ///
    /// This is produced by `Signal::from_fn`
    Dynamic(Arc<dyn Fn() -> T + Send + Sync>),
    /// A signal that contains shared data.
    ///
    /// This is mainly produced by stream methods or mapping other signals.
    Shared(Arc<dyn SharedSignal<T> + Send + Sync>),
    /// A signal that contains a signal, and allows sampling the inner signal directly.
    ///
    /// This is produced by `Signal::switch`
    Nested(Arc<dyn Fn() -> Signal<T> + Send + Sync>),
}

impl<T> Signal<T> {
    /// Creates a signal with constant value.
    ///
    /// The value is assumed to be constant, so changing it while it's stored on the
    /// signal is a logic error and will cause unexpected results.
    pub fn constant(val: T) -> Self {
        Signal(Constant(val))
    }

    /// Creates a signal that samples it's values from an external source.
    ///
    /// The closure is meant to sample a continuous value from the real world,
    /// so the signal value is assumed to be always changing.
    pub fn from_fn<F>(f: F) -> Self
    where
        F: Fn() -> T + Send + Sync + 'static,
    {
        Signal(Dynamic(Arc::new(f)))
    }

    /// Creates a new shared signal.
    pub(crate) fn shared<S>(storage: S) -> Self
    where
        S: SharedSignal<T> + Send + Sync + 'static,
    {
        Signal(Shared(Arc::new(storage)))
    }

    /// Checks if the signal has changed since the last time it was sampled.
    pub fn has_changed(&self) -> bool {
        match self.0 {
            Constant(_) => false,
            Dynamic(_) | Nested(_) => true,
            Shared(ref s) => {
                s.update();
                s.get_storage().must_update()
            }
        }
    }

    /// Moves the value out of the signal.
    ///
    /// To avoid leaving the internal storage empty (and causing panics), it must be filled with
    /// a default value. This marks the signal chain as changed, so most of time no one will
    /// notice anything.
    pub fn take(self) -> T
    where
        T: Default,
    {
        match self.0 {
            Constant(val) => val,
            Dynamic(f) => f(),
            Shared(s) => {
                s.update();
                s.sample().replace(T::default())
            }
            Nested(f) => f().take(),
        }
    }

    /// Samples the value of the signal.
    ///
    /// This will clone the value stored in the signal.
    pub fn sample(&self) -> T
    where
        T: Clone,
    {
        match self.0 {
            Constant(ref val) => T::clone(val),
            Dynamic(ref f) => f(),
            Shared(ref s) => {
                s.update();
                s.sample().get()
            }
            Nested(ref f) => f().sample(),
        }
    }

    /// Maps a signal with the provided function.
    ///
    /// The closure is called only when the parent signal changes.
    pub fn map<F, R>(&self, f: F) -> Signal<R>
    where
        F: Fn(T) -> R + Send + Sync + 'static,
        T: Clone + 'static,
        R: Send + 'static,
    {
        match self.0 {
            // constant signal: apply f once to produce another constant signal
            Constant(ref val) => Signal::constant(f(val.clone())),
            // dynamic signal: sample and apply f
            Dynamic(ref sf) => {
                let sf_ = sf.clone();
                Signal::from_fn(move || f(sf_()))
            }
            // shared signal: apply f only when the parent signal has changed
            Shared(ref sig) => Signal::shared(SharedImpl {
                storage: Storage::inherit(sig.get_storage()),
                source: sig.clone(),
                f,
            }),
            // nested signal: extract signal, sample and apply f
            Nested(ref sf) => {
                let sf_ = sf.clone();
                Signal::from_fn(move || f(sf_().sample()))
            }
        }
    }
}

impl<T: Send + 'static> Signal<T> {
    /// Samples the value of this signal every time the trigger stream fires.
    pub fn snapshot<S, F, R>(&self, trigger: &Stream<S>, f: F) -> Stream<R>
    where
        F: Fn(T, MaybeOwned<'_, S>) -> R + Send + Sync + 'static,
        T: Clone,
        S: 'static,
        R: 'static,
    {
        let this = Mutex::new(self.clone()); // need Sync for T
        trigger.map(move |b| f(this.lock().sample(), b))
    }

    /// Creates a signal from a shared value.
    pub(crate) fn from_storage<P: Send + Sync + 'static>(
        storage: Arc<SharedImpl<T, P, ()>>,
    ) -> Self {
        Signal(Shared(storage))
    }

    /// Stores the last value sent to a channel.
    ///
    /// When sampled, the resulting signal consumes all the current values on the channel
    /// (using non-blocking operations) and returns the last value seen.
    #[inline]
    pub fn from_channel(initial: T, rx: mpsc::Receiver<T>) -> Self {
        Self::fold_channel(initial, rx, |_, v| v)
    }

    /// Creates a signal that folds the values from a channel.
    ///
    /// When sampled, the resulting signal consumes all the current values on the channel
    /// (using non-blocking operations) and folds them using the current signal value as the
    /// initial accumulator state.
    pub fn fold_channel<V, F>(initial: T, rx: mpsc::Receiver<V>, f: F) -> Self
    where
        F: Fn(T, V) -> T + Send + Sync + 'static,
        V: Send + 'static,
    {
        Signal::shared(SharedImpl {
            storage: Storage::new(initial),
            source: Mutex::new(rx),
            f,
        })
    }
}

impl<T: Clone + 'static> Signal<Signal<T>> {
    /// Creates a new signal that samples the inner value of a nested signal.
    pub fn switch(&self) -> Signal<T> {
        match self.0 {
            // constant signal: just extract the inner signal
            Constant(ref sig) => Signal::clone(sig),
            // dynamic signal: re-label as nested
            Dynamic(ref f) => Signal(Nested(f.clone())),
            // shared signal: sample to extract the inner signal
            Shared(ref sig_) => {
                let sig = sig_.clone();
                Signal(Nested(Arc::new(move || {
                    sig.update();
                    sig.sample().get()
                })))
            }
            // nested signal: remove one layer
            Nested(ref f_) => {
                let f = f_.clone();
                Signal(Nested(Arc::new(move || f().sample())))
            }
        }
    }
}

impl<T: Default> Default for Signal<T> {
    /// Creates a constant signal with T's default value.
    #[inline]
    fn default() -> Self {
        Signal::constant(T::default())
    }
}

impl<T> From<T> for Signal<T> {
    /// Creates a constant signal from T.
    #[inline]
    fn from(val: T) -> Self {
        Signal::constant(val)
    }
}

impl<T: fmt::Debug> fmt::Debug for SigValue<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Constant(ref val) => write!(f, "Constant({:?})", val),
            Dynamic(ref rf) => write!(f, "Dynamic(Fn@{:p})", rf),
            Shared(ref rs) => write!(f, "Shared(SharedSignal@{:p})", rs),
            Nested(ref rf) => write!(f, "Nested(Fn@{:p})", rf),
        }
    }
}

impl<T: fmt::Display + Clone> fmt::Display for Signal<T> {
    /// Samples the signal and formats the value.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(&self.sample(), f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::RwLock;
    use std::time::Instant;

    #[test]
    fn signal_basic() {
        let signal = Signal::constant(42);
        let double = signal.map(|a| a * 2);
        let plusone = double.map(|a| a + 1);
        assert_eq!(signal.sample(), 42);
        assert_eq!(double.sample(), 84);
        assert_eq!(plusone.sample(), 85);
    }

    #[test]
    fn signal_shared() {
        let st = Arc::new(SharedImpl::new(1, ()));
        let signal = Signal::from_storage(st.clone());
        let double = signal.map(|a| a * 2);

        assert_eq!(signal.sample(), 1);
        assert_eq!(double.sample(), 2);
        st.set(42);
        assert_eq!(signal.sample(), 42);
        assert_eq!(double.sample(), 84);
    }

    #[test]
    fn signal_dynamic() {
        let t = Instant::now();
        let signal = Signal::from_fn(move || t);
        assert_eq!(signal.sample(), t);

        let n = Arc::new(RwLock::new(1));
        let cloned = n.clone();
        let signal = Signal::from_fn(move || *cloned.read().unwrap());
        let double = signal.map(|a| a * 2);
        let plusone = double.map(|a| a + 1);
        assert_eq!(signal.sample(), 1);
        assert_eq!(double.sample(), 2);
        assert_eq!(plusone.sample(), 3);
        *n.write().unwrap() = 13;
        assert_eq!(signal.sample(), 13);
        assert_eq!(double.sample(), 26);
        assert_eq!(plusone.sample(), 27);
    }

    #[test]
    fn signal_take() {
        let signal = Signal::constant(42);
        assert_eq!(signal.take(), 42);

        let signal = Signal::from_fn(move || 33);
        assert_eq!(signal.clone().take(), 33);
        assert_eq!(signal.take(), 33);

        let st = Arc::new(SharedImpl::new(123, ()));
        let signal = Signal::from_storage(st.clone());
        assert_eq!(signal.clone().take(), 123);
        assert_eq!(signal.clone().take(), 0);
        st.set(13);
        assert_eq!(signal.take(), 13);
    }
}
