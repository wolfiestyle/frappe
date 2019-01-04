//! Miscellaneous types used by the library.

use std::ops;

pub use maybe_owned::MaybeOwned;
#[cfg(feature="either")]
pub use either::Either;

mod callbacks;
pub(crate) use crate::types::callbacks::Callbacks;

mod storage;
pub(crate) use crate::types::storage::Storage;

/// Generic sum type of two elements.
///
/// It's used to provide generics over the `Option`/`Result`/`Either` types
pub trait SumType2
{
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

    fn from_type1(val: Self::Type1) -> Self { Either::Left(val) }
    fn from_type2(val: Self::Type2) -> Self { Either::Right(val) }

    fn is_type1(&self) -> bool { self.is_left() }
    fn is_type2(&self) -> bool { self.is_right() }

    fn into_type1(self) -> Option<Self::Type1> { self.left() }
    fn into_type2(self) -> Option<Self::Type2> { self.right() }
}

/// Defines a signal that contains shared storage.
pub(crate) trait SharedSignal<T>
{
    /// Updates the signal.
    fn update(&self);
    /// Obtains the internal storage.
    fn get_storage(&self) -> &Storage<T>;
    /// Samples the signal.
    fn sample(&self) -> &Storage<T>;
}

/// Common template for shared signal implementations.
pub(crate) struct SharedImpl<T, S, F>
{
    pub storage: Storage<T>,
    pub source: S,
    pub f: F,
}

impl<T, S, F> ops::Deref for SharedImpl<T, S, F>
{
    type Target = Storage<T>;

    fn deref(&self) -> &Self::Target
    {
        &self.storage
    }
}
