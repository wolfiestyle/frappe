# frappe - FRP library for Rust

[![Build Status](https://travis-ci.org/darkstalker/frappe.svg?branch=master)](https://travis-ci.org/darkstalker/frappe) [![crates.io](https://meritbadge.herokuapp.com/frappe)](https://crates.io/crates/frappe) [![Documentation](https://docs.rs/frappe/badge.svg)](https://docs.rs/frappe)

Functional Reactive Programming library inspired by [Carboxyl](https://github.com/aepsil0n/carboxyl).
It's designed to efficiently pass objects around by avoiding cloning as much as possible.
Threading is now supported (experimental), so streams and signals can be freely moved and used
between threads.

Work in progress, so the API can change at any time.

## Usage

```Rust
use frappe::Sink;

fn main() {
    // values are sent from a sink..
    let sink = Sink::new();
    // ..into a stream chain
    let stream = sink.stream().inspect(|a| println!("--sent: {}", a));

    // `hold` creates a Signal that stores the last value sent to the stream
    let last = stream.hold(0);

    // stream callbacks receive a MaybeOwned<T> argument, so we need to deref the value
    let sum = stream.fold(0, |acc, n| acc + *n);

    let half_even = stream
        // the methods filter, map, fold are analogous to Iterator operations
        .filter(|n| n % 2 == 0)
        .map(|n| *n / 2)
        .fold(Vec::new(), |mut vec, n| {
            vec.push(*n);
            vec
        }); // note: .collect::<Vec<_>>() does the same

    // we can send individual values
    sink.send(6);
    sink.send(42);
    sink.send(-1);
    // or multiple ones at once
    sink.feed(10..15);

    // `sample` gets a copy of the value stored in the signal
    println!("last: {}", last.sample());
    // printing a signal samples it
    println!("sum: {}", sum);
    println!("half_even: {:?}", half_even);
}
```
