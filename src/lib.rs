//! Functional Reactive Programming library for Rust
//!
//! Frappe is a concurrent Event-Driven FRP library. It aims to provide a simple, efficient and
//! Rust-idiomatic way to write interactive applications in a declarative way.
//!
//! See each module documentation for more details.
#![warn(missing_docs)]
#![cfg_attr(
    feature = "nightly",
    feature(arbitrary_self_types, futures_api, async_await, await_macro)
)]

#[macro_use]
mod helpers;
mod lift;
pub mod signal;
pub mod stream;
mod sync;
pub mod types;

#[cfg(feature = "nightly")]
pub mod futures;

pub use crate::signal::Signal;
pub use crate::stream::{Sink, Stream};
