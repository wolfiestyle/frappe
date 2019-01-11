//! Utilities for lifting functions into signals.

/// Maps a function over the value of signals.
///
/// This converts a function `Fn(A, B, ...) -> R` and the signals `Signal<A>, Signal<B>, ...`
/// into a `Signal<R>` that computes it's value by sampling the input signals and then
/// calling the supplied function.
#[macro_export]
macro_rules! signal_lift {
    ($($sig:expr),+ => | $($arg:ident),+ | $body:expr) => {
        $crate::signal_lift!(@closure $($sig),+ ; $($arg)+ ; $body)
    };

    ($($sig:expr),+ => $f:expr) => {
        $crate::signal_lift!(@expr $f; $($sig),+ ;)
    };

    (@expr $f:expr ; ; $($arg:ident)*) => {
        $f($($arg),*)
    };

    (@expr $f:expr ; $sig:expr $(,$tail:expr)* ; $($arg:ident)*) => {
        $sig.map(move |arg| $crate::signal_lift!(@expr $f ; $($tail),* ; $($arg)* arg))
    };

    (@closure ; ; $body:expr) => ($body);

    (@closure $sig:expr $(,$stail:expr)* ; $arg:ident $($atail:ident)* ; $body:expr) => {
        $sig.map(move |$arg| $crate::signal_lift!(@closure $($stail),* ; $($atail)* ; $body))
    }
}

#[cfg(test)]
mod tests {
    use crate::stream::Sink;

    fn incr(a: i32) -> i32 {
        a + 1
    }

    fn append(a: i32, b: &str) -> String {
        a.to_string() + b
    }

    #[test]
    fn signal_lift1() {
        let sink = Sink::new();
        let sig = sink.stream().hold(0);
        let res_e = signal_lift!(sig => incr);
        let res_c = signal_lift!(sig => |a| a + 1);

        assert_eq!(res_e.sample(), 1);
        assert_eq!(res_c.sample(), 1);
        sink.send(12);
        assert_eq!(res_e.sample(), 13);
        assert_eq!(res_c.sample(), 13);
    }

    #[test]
    fn signal_lift2() {
        let sink1 = Sink::new();
        let sink2 = Sink::new();
        let sig1 = sink1.stream().hold(0);
        let sig2 = sink2.stream().hold("a");
        let sig2_ = sig2.clone();
        let lifted_e = signal_lift!(sig1, sig2 => append);
        let lifted_c = signal_lift!(sig1, sig2_ => |a, b| a.to_string() + b);
        let res_e = lifted_e.map(|s| format!("({})", s));
        let res_c = lifted_c.map(|s| format!("({})", s));

        assert_eq!(res_e.sample(), "(0a)");
        assert_eq!(res_c.sample(), "(0a)");
        sink1.send(42);
        assert_eq!(res_e.sample(), "(42a)");
        assert_eq!(res_c.sample(), "(42a)");
        sink2.send("xyz");
        assert_eq!(res_e.sample(), "(42xyz)");
        assert_eq!(res_c.sample(), "(42xyz)");
    }
}
