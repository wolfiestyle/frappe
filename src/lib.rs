//! Functional Reactive Programming library for Rust
#![warn(missing_docs)]

pub extern crate maybe_owned;
#[cfg(feature="either")]
pub extern crate either;

mod helpers;
pub mod types;
mod stream;
mod signal;
pub mod lift;

pub use stream::{Sink, Stream};
pub use signal::Signal;
