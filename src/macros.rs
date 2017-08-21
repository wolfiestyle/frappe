/// Maps a function over the value of signals.
///
/// This converts a function `Fn(A, B, ...) -> R` and the signals `Signal<A>, Signal<B>, ...`
/// into a `Signal<R>` that computes it's value by sampling the input signals and then
/// calling the supplied function.
#[macro_export]
macro_rules! signal_lift
{
    (@replace $a:expr, $sub:expr) => ($sub);

    ($f:expr) => {
        $crate::Signal::from_fn(f)
    };

    ($f:expr, $($sig:expr),+) => ({
        let f = $f;
        let sig = [$($sig),+];
        $crate::Signal::from_fn(move || {
            let mut i = 0;
            f($(
                $crate::Signal::sample(
                    signal_lift!(@replace $sig, &sig[{ i += 1; i - 1 }])
                )
            ),+)
        })
    });
}
