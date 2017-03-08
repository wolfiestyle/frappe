extern crate frappe;
use frappe::Sink;

//TODO: tests
fn main()
{
    let sink: Sink<i32> = Sink::new();
    let stream = sink.stream();
    let signal = stream
        .fold(Vec::new(), |mut a, n| { a.push(*n); a })
        .snapshot(&stream, |s, _| s.into_owned())
        .fold(Vec::new(), |mut a, v| { a.push(v.into_owned()); a});

    sink.send(42);
    sink.send(33);
    sink.feed(0..5);
    println!("{:?}", signal.sample());
}
