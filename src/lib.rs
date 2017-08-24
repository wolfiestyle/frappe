//! Functional Reactive Programming library for Rust

pub extern crate maybe_owned;

#[cfg(feature="either")]
pub extern crate either;

mod helpers;
mod types;
mod stream;
mod signal;

#[macro_use]
mod macros;

pub use stream::{Sink, Stream};
pub use signal::Signal;
pub use types::SumType2;

pub use maybe_owned::MaybeOwned;

#[cfg(test)]
mod tests;
