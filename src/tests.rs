use super::*;
use std::rc::Rc;
use std::borrow::Cow;
use std::fmt::Debug;

fn vec_cons<T: Clone>(mut v: Vec<T>, x: Cow<T>) -> Vec<T> { v.push(x.into_owned()); v }

#[test]
fn stream_basic()
{
    let sink = Sink::new();
    let stream = sink.stream();
    let rx = stream.channel();
    let signal = stream.hold(0);

    sink.send(42);
    sink.send(33);
    sink.feed(0..5);

    let result: Vec<i32> = rx.try_iter().collect();
    assert_eq!(result, [42, 33, 0, 1, 2, 3, 4]);
    assert_eq!(signal.sample(), 4);
}

#[test]
fn signal_basic()
{
    let signal = Signal::Constant(42);
    assert_eq!(signal.sample(), 42);
    signal.sample_with(|val| assert_eq!(*val, 42));

    let val = 33;
    let signal = Signal::from_fn(move || val);
    assert_eq!(signal.sample(), val);

    let sink = Sink::new();
    let stream = sink.stream();
    let s_double = stream.hold(0).map(|a| *a * 2);
    let s_odd = stream.hold_if(0, |a| *a % 2 != 0);

    sink.send(2);
    sink.send(3);
    sink.send(4);

    assert_eq!(s_double.sample(), 8);
    assert_eq!(s_odd.sample(), 3);
}

#[test]
fn stream_operations()
{
    let sink: Sink<i32> = Sink::new();
    let stream = sink.stream();

    let s_string = stream.map(|a| a.to_string()).fold(vec![], vec_cons);
    let s_odd = stream.filter(|a| a % 2 != 0).fold(vec![], vec_cons);
    let s_even_half = stream.filter_map(|a| if *a % 2 == 0 { Some(*a / 2) } else { None }).fold(vec![], vec_cons);
    let (pos, neg) = stream.map(|a| if *a > 0 { Ok(*a) } else { Err(*a) }).split();
    let s_pos = pos.fold(vec![], vec_cons);
    let s_neg = neg.fold(vec![], vec_cons);
    let s_merged = pos.merge(&neg.map(|a| -*a)).fold(vec![], vec_cons);
    let s_accum = stream.fold(vec![], vec_cons).snapshot(&stream, |s, _| s.into_owned()).fold(vec![], vec_cons);
    let s_cloned = stream.fold_clone(vec![], vec_cons);

    sink.feed(vec![5, 8, 13, -2, 42, -33]);

    assert_eq!(s_string.sample(), ["5", "8", "13", "-2", "42", "-33"]);
    assert_eq!(s_odd.sample(), [5, 13, -33]);
    assert_eq!(s_even_half.sample(), [4, -1, 21]);
    assert_eq!(s_pos.sample(), [5, 8, 13, 42]);
    assert_eq!(s_neg.sample(), [-2, -33]);
    assert_eq!(s_merged.sample(), [5, 8, 13, 2, 42, 33]);
    assert_eq!(s_accum.sample(), [vec![5], vec![5, 8], vec![5, 8, 13], vec![5, 8, 13, -2], vec![5, 8, 13, -2, 42], vec![5, 8, 13, -2, 42, -33]]);
    assert_eq!(s_cloned.sample(), [5, 8, 13, -2, 42, -33]);
}

#[cfg(feature="either")]
#[test]
fn merge_with()
{
    let sink1: Sink<i32> = Sink::new();
    let sink2: Sink<f32> = Sink::new();
    let stream: Stream<Result<_, _>> = sink1.stream().merge_with(&sink2.stream(), |e| e.either(|l| Ok(*l), |r| Err(*r)));
    let result = stream.fold(vec![], vec_cons);

    sink1.send(1);
    sink2.send(2.0);
    sink1.send(3);
    sink1.send(4);
    sink2.send(5.0);

    assert_eq!(result.sample(), [Ok(1), Err(2.0), Ok(3), Ok(4), Err(5.0)]);
}

#[test]
fn stream_channel()
{
    let sink = Sink::new();
    let rx = sink.stream().channel();

    let thread = std::thread::spawn(move || {
        let sink2 = Sink::new();
        let s_sum = sink2.stream().fold(0, |a, n| a + *n);
        sink2.feed(rx.iter());
        s_sum.into_rwlock().unwrap()
    });

    sink.feed(1..100);
    drop(sink);

    let result: Signal<i32> = thread.join().unwrap().into();
    assert_eq!(result.sample(), 4950);
}

#[test]
fn stream_switch()
{
    let stream_sink = Sink::new();
    let sink1 = Sink::new();
    let sink2 = Sink::new();

    let switched = stream_sink.stream().switch();
    let events = switched.channel();

    sink1.send(1);
    sink2.send(2);

    stream_sink.send(sink2.stream());
    sink1.send(3);
    sink2.send(4);
    assert_eq!(events.try_recv(), Ok(4));

    stream_sink.send(sink1.stream());
    sink1.send(5);
    sink2.send(6);
    assert_eq!(events.try_recv(), Ok(5));
}

#[test]
fn signal_switch()
{
    let signal_sink = Sink::new();
    let switched = signal_sink.stream().hold(Signal::Constant(0)).switch();

    signal_sink.send(Signal::Constant(1));
    assert_eq!(switched.sample(), 1);

    signal_sink.send(2.into());
    assert_eq!(switched.sample(), 2);
}

#[test]
fn cloning()
{
    #[derive(Debug)]
    struct Storage<T>(Vec<T>);

    impl<T> Storage<T>
    {
        fn new() -> Self { Storage(Vec::new()) }
        fn push(mut self, a: T) -> Self { self.0.push(a); self }
    }

    impl<T: Debug> Clone for Storage<T>
    {
        fn clone(&self) -> Self { panic!("storage cloned! {:?}", self.0) }
    }

    let sink = Sink::new();
    let accum = sink.stream().fold(Storage::new(), |a, v| a.push(*v));

    sink.feed(0..5);
    accum.sample_with(|res| assert_eq!(res.0, [0, 1, 2, 3, 4]));
}

#[test]
fn filter_extra()
{
    let sink = Sink::new();
    let stream = sink.stream();
    let sign_res = stream.map(|a| if *a >= 0 { Ok(*a) } else { Err(*a) });
    let even_opt = stream.map(|a| if *a % 2 == 0 { Some(*a) } else { None });
    let s_even = even_opt.filter_some().fold(vec![], vec_cons);
    let s_pos = sign_res.filter_first().fold(vec![], vec_cons);
    let s_neg = sign_res.filter_second().fold(vec![], vec_cons);

    sink.feed(vec![1, 8, -3, 42, -66]);

    assert_eq!(s_even.sample(), [8, 42, -66]);
    assert_eq!(s_pos.sample(), [1, 8, 42]);
    assert_eq!(s_neg.sample(), [-3, -66]);
}

#[test]
fn reentrant()
{
    let sink = Sink::new();
    let cloned = sink.clone();
    let sig = sink.stream()
        .filter_map(move |n| if *n < 10 { cloned.send(*n + 1); None } else { Some(*n) })
        .hold(0);

    sink.send(1);
    assert_eq!(sig.sample(), 10);
}

#[allow(unused_variables)]
#[test]
fn deletion()
{
    use std::cell::Cell;

    fn stream_cell(src: &Stream<i32>, i: i32) -> (Stream<i32>, Rc<Cell<i32>>)
    {
        let cell = Rc::new(Cell::new(0));
        let cloned = cell.clone();
        let stream = src.map(move |n| *n + i).inspect(move |n| cloned.set(*n));
        (stream, cell)
    }

    let sink = Sink::new();
    let stream = sink.stream();
    let (s1, c1) = stream_cell(&stream, 1);
    let (s2, c2) = stream_cell(&stream, 2);
    let (s3, c3) = stream_cell(&stream, 3);

    sink.send(10);
    assert_eq!(c1.get(), 11);
    assert_eq!(c2.get(), 12);
    assert_eq!(c3.get(), 13);

    drop(s2);
    sink.send(20);
    assert_eq!(c1.get(), 21);
    assert_eq!(c2.get(), 12);
    assert_eq!(c3.get(), 23);
}

#[test]
fn map_n()
{
    let sink = Sink::new();
    let s_out = sink.stream()
        .map_n(|a, sink| for _ in 0 .. *a { sink.send(*a) })
        .fold(vec![], vec_cons);

    sink.feed(0..4);

    assert_eq!(s_out.sample(), [1, 2, 2, 3, 3, 3]);
}
