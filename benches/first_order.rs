//! FRP benchmarks from https://github.com/aepsil0n/carboxyl

use bencher::{benchmark_group, benchmark_main, black_box, Bencher};
use frappe::Sink;
use rand::prelude::*;

/// First-order benchmark.
///
/// Generate `n_sinks` `Stream<String>`. Create a network that prints the output
/// of each stream. At each network step push a string (the step number
/// formatted as a string) into 10 randomly-selected nodes.
///
/// Benchmark the time required for `n_steps` steps.
fn first_order(n_sinks: usize, n_steps: usize, b: &mut Bencher) {
    // Setup network
    let sinks: Vec<Sink<String>> = (0..n_sinks).map(|_| Sink::new()).collect();
    let _printers: Vec<_> = sinks
        .iter()
        .map(|sink| {
            sink.stream().map(|s| {
                black_box(format!("{}", s));
            })
        })
        .collect();

    // Feed events
    let mut rng = rand::thread_rng();
    b.iter(|| {
        for k in 0..n_steps {
            let s = format!("{}", k);
            for sink in sinks.choose_multiple(&mut rng, 10) {
                sink.send(s.clone());
            }
        }
    });
}

fn first_order_100(b: &mut Bencher) {
    first_order(1_000, 100, b);
}

fn first_order_1k(b: &mut Bencher) {
    first_order(1_000, 1_000, b);
}

fn first_order_10k(b: &mut Bencher) {
    first_order(1_000, 10_000, b);
}

/// A small reference benchmark to do the same amount of actual work without FRP
fn first_order_1k_ref(b: &mut Bencher) {
    let mut rng = rand::thread_rng();
    let dist = rand::distributions::Uniform::new_inclusive(0, 1000);
    b.iter(|| {
        for i in 0..1_000 {
            for _k in rng.sample_iter(&dist).take(10) {
                black_box(format!("{}", i));
            }
        }
    });
}

benchmark_group!(
    first_order_,
    first_order_100,
    first_order_1k,
    first_order_10k,
    first_order_1k_ref
);
benchmark_main!(first_order_);
