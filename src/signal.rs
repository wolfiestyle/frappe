use std::rc::Rc;
use std::cell::RefCell;
use std::fmt;
use stream::Stream;
use maybe_owned::MaybeOwned;

/// Represents a continuous value that changes over time.
///
/// Signals are usually constructed by stream operations and can be read using the `sample` or
/// `sample_with` methods.
///
/// This type is meant to be used as a black box, but you can fiddle with the enum variants if
/// you know what you're doing.
pub enum Signal<T>
{
    /// A signal with constant value.
    Constant(Rc<T>),
    /// A signal that generates it's values from a function.
    ///
    /// This is produced by `Signal::map`
    Dynamic(Rc<Fn() -> T>),
    /// A signal that contains shared data.
    ///
    /// This is produced by stream methods that create a signal.
    Shared(Rc<Fn() -> Rc<RefCell<T>>>),
    /// A signal that contains a signal, and allows sampling the inner signal directly.
    ///
    /// This is produced by `Signal::switch`
    Nested(Rc<Fn() -> Signal<T>>),
}

impl<T> Signal<T>
{
    pub fn constant<V: Into<Rc<T>>>(val: V) -> Self
    {
        Signal::Constant(val.into())
    }

    /// Creates a signal that samples it's values from the supplied function.
    pub fn from_fn<F>(f: F) -> Self
        where F: Fn() -> T + 'static
    {
        Signal::Dynamic(Rc::new(f))
    }

    /// Sample by reference.
    ///
    /// This is meant to be the most efficient way when cloning is undesirable,
    /// but it requires a callback to prevent outliving internal `RwLock` borrows.
    pub fn sample_with<F, R>(&self, cb: F) -> R
        where F: FnOnce(MaybeOwned<T>) -> R
    {
        match *self
        {
            Signal::Constant(ref val) => cb(MaybeOwned::Borrowed(val)),
            Signal::Dynamic(ref f) => cb(MaybeOwned::Owned(f())),
            Signal::Shared(ref f) => {
                let st = f();
                let v = st.borrow();
                cb(MaybeOwned::Borrowed(&v))
            },
            Signal::Nested(ref f) => f().sample_with(cb),
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
        match *self
        {
            Signal::Constant(ref val) => (**val).clone(),
            Signal::Dynamic(ref f) => f(),
            Signal::Shared(ref f) => {
                let st = f();
                let v = st.borrow();
                v.clone()
            },
            Signal::Nested(ref f) => f().sample(),
        }
    }
}

impl<T: 'static> Signal<T>
{
    /// Maps a signal with the provided function.
    pub fn map<F, R>(&self, f: F) -> Signal<R>
        where F: Fn(MaybeOwned<T>) -> R + 'static,
        R: 'static
    {
        let this = self.clone();
        Signal::from_fn(move || this.sample_with(&f))
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
    pub(crate) fn from_storage<A: 'static>(storage: Rc<RefCell<T>>, keepalive: A) -> Self
    {
        Signal::Shared(Rc::new(move || {
            let _keepalive = &keepalive;
            storage.clone()
        }))
    }
}

impl<T: 'static> Signal<Signal<T>>
{
    /// Creates a new signal that samples the inner value of a nested signal.
    pub fn switch(&self) -> Signal<T>
    {
        let this = self.clone();
        Signal::Nested(Rc::new(move || this.sample()))
    }
}

impl<T> From<T> for Signal<T>
{
    fn from(val: T) -> Self
    {
        Signal::constant(val)
    }
}

impl<T> From<Rc<T>> for Signal<T>
{
    fn from(val: Rc<T>) -> Self
    {
        Signal::Constant(val)
    }
}

// the derive impl adds a `T: Clone` we don't want
impl<T> Clone for Signal<T>
{
    fn clone(&self) -> Self
    {
        match *self
        {
            Signal::Constant(ref val) => Signal::Constant(val.clone()),
            Signal::Dynamic(ref rf) => Signal::Dynamic(rf.clone()),
            Signal::Shared(ref rf) => Signal::Shared(rf.clone()),
            Signal::Nested(ref rf) => Signal::Nested(rf.clone()),
        }
    }
}

impl<T: fmt::Debug> fmt::Debug for Signal<T>
{
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result
    {
        match *self
        {
            Signal::Constant(ref val) => write!(f, "Signal::Constant({:?})", val),
            Signal::Dynamic(ref rf) => write!(f, "Signal::Dynamic(Fn@{:p})", rf),
            Signal::Shared(ref rf) => write!(f, "Signal::Shared(Fn@{:p})", rf),
            Signal::Nested(ref rf) => write!(f, "Signal::Nested(Fn@{:p})", rf),
        }
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
        assert_eq!(signal.sample(), 42);
        signal.sample_with(|val| assert_eq!(*val, 42));

        let t = Instant::now();
        let signal = Signal::from_fn(move || t);
        assert_eq!(signal.sample(), t);
        signal.sample_with(|val| assert_eq!(*val, t));

        let n = Rc::new(Cell::new(1));
        let cloned = n.clone();
        let signal = Signal::from_fn(move || cloned.get());
        let double = signal.map(|a| *a * 2);
        assert_eq!(signal.sample(), 1);
        assert_eq!(double.sample(), 2);
        n.set(13);
        assert_eq!(signal.sample(), 13);
        assert_eq!(double.sample(), 26);
    }
}
