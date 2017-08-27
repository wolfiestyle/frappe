//! Simple benchmarks, from https://github.com/aepsil0n/carboxyl
#![feature(test)]

extern crate test;
extern crate frappe;

use test::Bencher;
use frappe::Sink;

#[bench]
fn bench_chain(b: &mut Bencher) {
    let sink: Sink<i32> = Sink::new();
    let _ = sink.stream()
        .map(|x| *x + 4)
        .filter(|&x| x < 4)
        .merge(&sink.stream().map(|x| *x * 5))
        .hold(15);
    b.iter(|| sink.send(-5));
}
