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

    ($($sig:expr),+ => | $($args:ident),+ | $body:expr) => {
        $crate::signal_lift!(@closure $body; $($args)+ ;; $($sig),+)
    };

    ($($sig:expr),+ => $f:expr) => ({
        let f = $f;
        $crate::signal_lift!(@expr f;; $($sig),+)
    });

    (@closure $body:expr ; $($args:ident)* ; $($vars:ident)* ;) => {
        $crate::Signal::from_fn(move || {
            let ($($args),*) = ($($crate::Signal::sample(&$vars)),*);
            $body
        })
    };

    (@closure $body:expr ; $($args:ident)* ; $($vars:ident)* ; $sig:expr $(,$stail:expr)*) => ({
        let sig = $sig;
        $crate::signal_lift!(@closure $body; $($args)* ; $($vars)* sig ; $($stail),*)
    });

    (@expr $f:expr ; $($vars:ident)* ;) => {
        $crate::Signal::from_fn(move || $f($($crate::Signal::sample(&$vars)),*))
    };

    (@expr $f:expr ; $($vars:ident)* ; $sig:expr $(,$stail:expr)*) => ({
        let sig = $sig;
        $crate::signal_lift!(@expr $f ; $($vars)* sig ; $($stail),*)
    });
}

#[cfg(test)]
mod tests {
    use crate::{Signal, Sink};

    #[test]
    fn signal_lift1() {
        let sink = Sink::new();
        let res: Signal<i32> = signal_lift!(sink.stream().hold(0) => |a| a + 1);

        assert_eq!(res.sample(), 1);
        sink.send(12);
        assert_eq!(res.sample(), 13);
    }

    #[test]
    fn signal_lift2_closure() {
        let sink1 = Sink::new();
        let sink2 = Sink::new();
        let res: Signal<String> = signal_lift!(sink1.stream().hold(0), sink2.stream().hold("a") => |a, b| a.to_string() + b);

        assert_eq!(res.sample(), "0a");
        sink1.send(42);
        assert_eq!(res.sample(), "42a");
        sink2.send("xyz");
        assert_eq!(res.sample(), "42xyz");
    }

    #[test]
    fn signal_lift2_expr() {
        fn append<T: ToString>(a: T, b: &str) -> String {
            a.to_string() + b
        }

        let sink1 = Sink::new();
        let sink2 = Sink::new();
        let res: Signal<String> =
            signal_lift!(sink1.stream().hold(0), sink2.stream().hold("a") => append);

        assert_eq!(res.sample(), "0a");
        sink1.send(42);
        assert_eq!(res.sample(), "42a");
        sink2.send("xyz");
        assert_eq!(res.sample(), "42xyz");
    }
}
