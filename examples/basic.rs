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
    // `take` moves the value out of the signal
    println!("half_even: {:?}", half_even.take());
}
