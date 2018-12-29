//! Simple benchmarks, from https://github.com/aepsil0n/carboxyl

use bencher::{Bencher, benchmark_group, benchmark_main};
use frappe::Sink;

fn bench_chain(b: &mut Bencher) {
    let sink: Sink<i32> = Sink::new();
    let _ = sink.stream()
        .map(|x| *x + 4)
        .filter(|&x| x < 4)
        .merge(&sink.stream().map(|x| *x * 5))
        .hold(15);
    b.iter(|| sink.send(-5));
}


benchmark_group!(simple, bench_chain);
benchmark_main!(simple);
