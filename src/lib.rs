//! Functional Reactive Programming library for Rust
//!
//! Frappe is a concurrent Event-Driven FRP library. It aims to provide a simple, efficient and
//! Rust-idiomatic way to write interactive applications in a declarative way.
//!
//! See each module documentation for more details.
#![warn(missing_docs)]
#![cfg_attr(feature = "nightly", feature(async_await))]

#[macro_use]
mod helpers;
pub mod futures;
mod lift;
pub mod signal;
pub mod stream;
mod sync;
pub mod types;

pub use crate::signal::Signal;
pub use crate::stream::{Sink, Stream};
