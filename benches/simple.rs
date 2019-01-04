//! Simple benchmarks, from https://github.com/aepsil0n/carboxyl

use bencher::{benchmark_group, benchmark_main, black_box, Bencher};
use frappe::{Signal, Sink};

fn make_chain() -> (Sink<i32>, Signal<i32>) {
    let sink = Sink::new();
    let sig = sink
        .stream()
        .map(|x| *x + 4)
        .filter(|&x| x < 4)
        .merge(&sink.stream().map(|x| *x * 5))
        .hold(15)
        .map(|x| x + 1);
    (sink, sig)
}

fn send(b: &mut Bencher) {
    let (sink, sig) = make_chain();
    b.iter(|| sink.send(-5));
    black_box(sig.sample());
}

fn sample(b: &mut Bencher) {
    let (sink, sig) = make_chain();
    sink.send(-5);
    b.iter(|| sig.sample());
}

fn send_and_sample(b: &mut Bencher) {
    let (sink, sig) = make_chain();
    b.iter(|| {
        sink.send(-5);
        sig.sample()
    });
}

benchmark_group!(simple, send, sample, send_and_sample);
benchmark_main!(simple);
