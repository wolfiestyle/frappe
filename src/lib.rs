//! Functional Reactive Programming library for Rust
#![warn(missing_docs)]

#[macro_use] mod helpers;
pub mod types;
mod stream;
mod signal;
pub mod lift;

pub use crate::stream::{Sink, Stream};
pub use crate::signal::Signal;
