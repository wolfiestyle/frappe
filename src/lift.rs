//! Utilities for lifting functions into signals.

/// Maps a function over the value of signals.
///
/// This converts a function `Fn(A, B, ...) -> R` and the signals `Signal<A>, Signal<B>, ...`
/// into a `Signal<R>` that computes it's value by sampling the input signals and then
/// calling the supplied function.
#[macro_export]
macro_rules! signal_lift {
    ($sig:expr => $f:expr) => {
        $crate::Signal::map(&$sig, $f)
    };

    ($($sig:expr),+ => $f:expr) => {
        $crate::Signal::from_fn(move || $f($($crate::Signal::sample(&$sig)),+))
    };
}

#[cfg(test)]
mod tests {
    use crate::{Signal, Sink};

    #[test]
    fn signal_lift1() {
        let sink = Sink::new();
        let sig = sink.stream().hold(0);
        let res: Signal<i32> = signal_lift!(sig => |a| a + 1);

        assert_eq!(res.sample(), 1);
        sink.send(12);
        assert_eq!(res.sample(), 13);
    }

    #[test]
    fn signal_lift2() {
        let sink1 = Sink::new();
        let sink2 = Sink::new();
        let sig1 = sink1.stream().hold(0);
        let sig2 = sink2.stream().hold(1);
        let res: Signal<i32> = signal_lift!(sig1, sig2 => |a, b| a + b);

        assert_eq!(res.sample(), 1);
        sink1.send(42);
        assert_eq!(res.sample(), 43);
        sink2.send(100);
        assert_eq!(res.sample(), 142);
    }
}
