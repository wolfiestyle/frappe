#[macro_use]
extern crate frappe;
use frappe::{Sink, Stream, Signal};
use std::rc::Rc;
use std::fmt::Debug;

#[test]
fn stream_operations()
{
    let sink: Sink<i32> = Sink::new();
    let stream = sink.stream();

    let s_string = stream.map(|a| a.to_string()).collect::<Vec<_>>();
    let s_odd = stream.filter(|a| a % 2 != 0).collect::<Vec<_>>();
    let s_even_half = stream.filter_map(|a| if *a % 2 == 0 { Some(*a / 2) } else { None }).collect::<Vec<_>>();
    let (pos, neg) = stream.map(|a| if *a > 0 { Ok(*a) } else { Err(*a) }).split();
    let s_pos = pos.collect::<Vec<_>>();
    let s_neg = neg.collect::<Vec<_>>();
    let s_merged = pos.merge(&neg.map(|a| -*a)).collect::<Vec<_>>();
    let s_accum = stream.collect::<Vec<_>>().snapshot(&stream, |s, _| s.into_owned()).collect::<Vec<_>>();
    let s_cloned = stream.fold_clone(vec![], |mut a, v| { a.push(v.into_owned()); a });
    let s_last_pos = stream.hold_if(0, |a| *a > 0);

    sink.feed_ref(&[5, 8, 13, -2, 42, -33]);

    assert_eq!(s_string.sample(), ["5", "8", "13", "-2", "42", "-33"]);
    assert_eq!(s_odd.sample(), [5, 13, -33]);
    assert_eq!(s_even_half.sample(), [4, -1, 21]);
    assert_eq!(s_pos.sample(), [5, 8, 13, 42]);
    assert_eq!(s_neg.sample(), [-2, -33]);
    assert_eq!(s_merged.sample(), [5, 8, 13, 2, 42, 33]);
    assert_eq!(s_accum.sample(), [vec![5], vec![5, 8], vec![5, 8, 13], vec![5, 8, 13, -2], vec![5, 8, 13, -2, 42], vec![5, 8, 13, -2, 42, -33]]);
    assert_eq!(s_cloned.sample(), [5, 8, 13, -2, 42, -33]);
    assert_eq!(s_last_pos.sample(), 42);
}

#[test]
fn merge_with()
{
    let sink1: Sink<i32> = Sink::new();
    let sink2: Sink<f32> = Sink::new();
    let stream = sink1.stream().merge_with(&sink2.stream(), |l| Ok(*l), |r| Err(*r));
    let result = stream.collect::<Vec<_>>();

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
    use std::sync::mpsc::channel;

    let sink = Sink::new();
    let input = sink.stream().as_channel();
    let (output, result) = channel();
    let s_result = Signal::from_channel(0, result);

    let thread = std::thread::spawn(move || {
        let sink2 = Sink::new();
        let stream = sink2.stream();
        let s_sum = stream.fold(0, |a, n| a + *n);
        let doubles = stream.map(|n| *n * 2);
        let rx_doubles = doubles.as_channel();

        sink2.feed(input);

        output.send(s_sum.sample()).unwrap();
        rx_doubles
    });

    assert_eq!(s_result.sample(), 0);

    sink.feed(1..100);
    drop(sink);

    let doubles = thread.join().unwrap();
    let s_doubles = Signal::fold_channel(0, doubles, |a, n| a + n);
    assert_eq!(s_result.sample(), 4950);
    assert_eq!(s_doubles.sample(), 9900);
}


#[test]
fn signal_switch()
{
    let signal_sink = Sink::new();
    let switched = signal_sink.stream().hold(Default::default()).switch();
    let double = switched.map(|a| *a * 2);

    signal_sink.send(Signal::constant(1));
    assert_eq!(switched.sample(), 1);
    assert_eq!(double.sample(), 2);

    signal_sink.send(Signal::from_fn(|| 12));
    assert_eq!(switched.sample(), 12);
    assert_eq!(double.sample(), 24);
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
    let s_even = even_opt.filter_some().collect::<Vec<_>>();
    let s_pos = sign_res.filter_first().collect::<Vec<_>>();
    let s_neg = sign_res.filter_second().collect::<Vec<_>>();

    sink.feed_ref(&[1, 8, -3, 42, -66]);

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
        .collect::<Vec<_>>();

    sink.feed(0..4);

    assert_eq!(s_out.sample(), [1, 2, 2, 3, 3, 3]);
}

#[test]
fn lift()
{
    let sink1 = Sink::new();
    let res = signal_lift!(|a| *a + 1, sink1.stream().hold(0));

    assert_eq!(res.sample(), 1);
    sink1.send(12);
    assert_eq!(res.sample(), 13);

    let sink2 = Sink::new();
    let res = signal_lift!(sink1.stream().hold(0), sink2.stream().hold("a") => |a, b| a.to_string() + &b);
    let mapped = res.map(|s| format!("({})", s));

    assert_eq!(mapped.sample(), "(0a)");
    sink1.send(42);
    assert_eq!(mapped.sample(), "(42a)");
    sink2.send("xyz");
    assert_eq!(mapped.sample(), "(42xyz)");
}

#[test]
fn stream_collect()
{
    use std::collections::*;
    use std::cmp::Ordering;

    let sink = Sink::new();
    let stream = sink.stream();
    let s_vec: Signal<Vec<_>> = stream.collect();
    let s_vecdq: Signal<VecDeque<_>> = stream.collect();
    let s_list: Signal<LinkedList<_>> = stream.collect();
    let s_set: Signal<BTreeSet<_>> = stream.collect();
    let s_string: Signal<String> = stream.map(|v| format!("{} ", v)).collect();

    sink.feed_ref(&[1, 3, -42, 2]);

    assert_eq!(s_vec.sample(), [1, 3, -42, 2]);
    assert_eq!(s_vecdq.sample(), [1, 3, -42, 2]);
    assert_eq!(s_list.sample().iter().cmp([1, 3, -42, 2].iter()), Ordering::Equal);
    assert_eq!(s_set.sample().iter().cmp([-42, 1, 2, 3].iter()), Ordering::Equal);
    assert_eq!(s_string.sample(), "1 3 -42 2 ");

    let sink = Sink::new();
    let s_string: Signal<String> = sink.stream().collect();

    sink.feed("abZc".chars());

    assert_eq!(s_string.sample(), "abZc");
}

#[test]
fn signal_chain()
{
    use std::cell::Cell;

    let sink = Sink::new();
    let eval_count = Rc::new(Cell::new(0));
    let ev = eval_count.clone();

    let sig_a = sink.stream().hold(0);
    let sig_b = sig_a.map(move |a| { ev.set(ev.get() + 1); *a + 1 });
    let sig_c = sig_b.map(|a| *a * 2);
    let sig_d = sig_c.map(|a| format!("({})", a));
    let sig_e = sig_d.map(|s| s.into_owned() + ".-");

    assert_eq!(sig_e.has_changed(), true);
    assert_eq!(sig_e.sample(), "(2).-");
    assert_eq!(sig_e.has_changed(), false);
    assert_eq!(sig_e.sample(), "(2).-");
    assert_eq!(eval_count.get(), 1);

    sink.send(42);

    assert_eq!(sig_e.has_changed(), true);
    assert_eq!(sig_e.sample(), "(86).-");
    assert_eq!(sig_e.has_changed(), false);
    assert_eq!(sig_e.sample(), "(86).-");
    assert_eq!(eval_count.get(), 2);
}
