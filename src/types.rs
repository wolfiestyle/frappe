//! Miscellaneous types used by the library.

#[cfg(feature = "either")]
pub use either::Either;
pub use maybe_owned::MaybeOwned;

mod callbacks;
pub(crate) use crate::types::callbacks::Callbacks;

mod storage;
pub(crate) use crate::types::storage::Storage;

mod shared_impl;
pub(crate) use crate::types::shared_impl::{SharedImpl, SharedSignal};

/// Generic sum type of two elements.
///
/// It's used to provide generics over the `Option`/`Result`/`Either` types
pub trait SumType2 {
    /// Type of the first variant.
    type Type1;
    /// Type of the second variant.
    type Type2;

    /// Creates a value using the first variant.
    fn from_type1(val: Self::Type1) -> Self;
    /// Creates a value using the second variant.
    fn from_type2(val: Self::Type2) -> Self;

    /// Checks if the value is of the first variant.
    fn is_type1(&self) -> bool;
    /// Checks if the value is of the second variant.
    fn is_type2(&self) -> bool;

    /// Attempts to extract the value contained on the first variant.
    fn into_type1(self) -> Option<Self::Type1>;
    /// Attempts to extract the value contained on the second variant.
    fn into_type2(self) -> Option<Self::Type2>;
}

impl<T> SumType2 for Option<T> {
    type Type1 = T;
    type Type2 = ();

    fn from_type1(val: Self::Type1) -> Self {
        Some(val)
    }
    fn from_type2(_: Self::Type2) -> Self {
        None
    }

    fn is_type1(&self) -> bool {
        self.is_some()
    }
    fn is_type2(&self) -> bool {
        self.is_none()
    }

    fn into_type1(self) -> Option<Self::Type1> {
        self
    }
    fn into_type2(self) -> Option<Self::Type2> {
        self.ok_or(()).err()
    }
}

impl<T, E> SumType2 for Result<T, E> {
    type Type1 = T;
    type Type2 = E;

    fn from_type1(val: Self::Type1) -> Self {
        Ok(val)
    }
    fn from_type2(val: Self::Type2) -> Self {
        Err(val)
    }

    fn is_type1(&self) -> bool {
        self.is_ok()
    }
    fn is_type2(&self) -> bool {
        self.is_err()
    }

    fn into_type1(self) -> Option<Self::Type1> {
        self.ok()
    }
    fn into_type2(self) -> Option<Self::Type2> {
        self.err()
    }
}

#[cfg(feature = "either")]
impl<L, R> SumType2 for ::either::Either<L, R> {
    type Type1 = L;
    type Type2 = R;

    fn from_type1(val: Self::Type1) -> Self {
        Either::Left(val)
    }
    fn from_type2(val: Self::Type2) -> Self {
        Either::Right(val)
    }

    fn is_type1(&self) -> bool {
        self.is_left()
    }
    fn is_type2(&self) -> bool {
        self.is_right()
    }

    fn into_type1(self) -> Option<Self::Type1> {
        self.left()
    }
    fn into_type2(self) -> Option<Self::Type2> {
        self.right()
    }
}

/// Determines if the `Stream::observe` callback should be dropped or not.
pub trait ObserveResult {
    /// If it returns `true` the callback is kept, otherwise it's dropped.
    fn is_callback_alive(self) -> bool;
}

impl ObserveResult for () {
    /// No return value: never dropped.
    fn is_callback_alive(self) -> bool {
        true
    }
}

impl ObserveResult for bool {
    /// `bool` return value: dropped when it's `false`.
    fn is_callback_alive(self) -> bool {
        self
    }
}

impl<T> ObserveResult for Option<T> {
    /// `Option` return value: dropped when it's `None`.
    fn is_callback_alive(self) -> bool {
        self.is_some()
    }
}

impl<T, E> ObserveResult for Result<T, E> {
    /// `Result` return value: dropped when it's `Err`.
    fn is_callback_alive(self) -> bool {
        self.is_ok()
    }
}
