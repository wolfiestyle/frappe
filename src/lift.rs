//! Utilities for lifting functions into signals.

use crate::signal::Signal;
use crate::types::{SharedImpl, SharedSignal, Storage};

/// Maps a function over the value of signals.
///
/// This converts a function `Fn(A, B, ...) -> R` and the signals `Signal<A>, Signal<B>, ...`
/// into a `Signal<R>` that computes it's value by sampling the input signals and then
/// calling the supplied function.
#[macro_export]
macro_rules! signal_lift {
    ($($sig:expr),+ => $f:expr) => {
        signal_lift!($f, $($sig),+)
    };

    ($f:expr) => {
        $crate::Signal::from_fn($f)
    };

    ($f:expr, $sig1:expr) => {
        $crate::Signal::map(&$sig1, $f)
    };

    ($f:expr, $sig1:expr, $sig2:expr) => {
        $crate::lift::lift2($f, $sig1, $sig2)
    };

    ($f:expr, $sig1:expr, $sig2:expr, $sig3:expr) => {
        $crate::lift::lift3($f, $sig1, $sig2, $sig3)
    };

    ($f:expr, $sig1:expr, $sig2:expr, $sig3:expr, $sig4:expr) => {
        $crate::lift::lift4($f, $sig1, $sig2, $sig3, $sig4)
    };

    ($f:expr, $sig1:expr, $sig2:expr, $sig3:expr, $sig4:expr, $sig5:expr) => {
        $crate::lift::lift5($f, $sig1, $sig2, $sig3, $sig4, $sig5)
    };

    ($f:expr, $sig1:expr, $sig2:expr, $sig3:expr, $sig4:expr, $sig5:expr, $sig6:expr) => {
        $crate::lift::lift6($f, $sig1, $sig2, $sig3, $sig4, $sig5, $sig6)
    };
}

macro_rules! lift_impl {
    ($fname:ident ( $($vname:ident : $tname:ident),+ ) $($idx:tt)+) => (
        impl<T, $($tname,)+ F> SharedSignal<T> for SharedImpl<T, ($(Signal<$tname>),+), F>
        where
            F: Fn($($tname),+) -> T + 'static,
            $($tname: Clone + 'static),+
        {
            fn update(&self) {
                if !self.storage.must_update() && ($(self.source.$idx.has_changed())||+) {
                    self.storage.inc_root();
                }
            }

            fn get_storage(&self) -> &Storage<T> {
                &self.storage
            }

            fn sample(&self) -> &Storage<T> {
                if self.storage.must_update() {
                    let val = (self.f)($(self.source.$idx.sample()),+);
                    self.storage.set_local(val);
                }
                &self.storage
            }
        }

        /// Lifts a function into a signal.
        pub fn $fname<T, F, $($tname),+>(f: F, $($vname: Signal<$tname>),+) -> Signal<T>
        where
            F: Fn($($tname),+) -> T + Send + Sync + 'static,
            T: Send + 'static, $($tname: Clone + Send + Sync + 'static),+
        {
            Signal::shared(SharedImpl{
                storage: Default::default(),
                source: ($($vname),+),
                f: f,
            }.wrap())
        }
    );
}

lift_impl!(lift2(s1: S1, s2: S2)                                 0 1);
lift_impl!(lift3(s1: S1, s2: S2, s3: S3)                         0 1 2);
lift_impl!(lift4(s1: S1, s2: S2, s3: S3, s4: S4)                 0 1 2 3);
lift_impl!(lift5(s1: S1, s2: S2, s3: S3, s4: S4, s5: S5)         0 1 2 3 4);
lift_impl!(lift6(s1: S1, s2: S2, s3: S3, s4: S4, s5: S5, s6: S6) 0 1 2 3 4 5);
