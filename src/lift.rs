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
    ($sig1:expr => $f:expr) => {
        $crate::Signal::map(&$sig1, $f)
    };

    ($sig1:expr, $sig2:expr => $f:expr) => {
        $crate::lift::lift2(($sig1, $sig2), $f)
    };

    ($sig1:expr, $sig2:expr, $sig3:expr => $f:expr) => {
        $crate::lift::lift3(($sig1, $sig2, $sig3), $f)
    };

    ($sig1:expr, $sig2:expr, $sig3:expr, $sig4:expr => $f:expr) => {
        $crate::lift::lift4(($sig1, $sig2, $sig3, $sig4), $f)
    };

    ($sig1:expr, $sig2:expr, $sig3:expr, $sig4:expr, $sig5:expr => $f:expr) => {
        $crate::lift::lift5(($sig1, $sig2, $sig3, $sig4, $sig5), $f)
    };

    ($sig1:expr, $sig2:expr, $sig3:expr, $sig4:expr, $sig5:expr, $sig6:expr => $f:expr) => {
        $crate::lift::lift6(($sig1, $sig2, $sig3, $sig4, $sig5, $sig6), $f)
    };

    ($sig1:expr, $sig2:expr, $sig3:expr, $sig4:expr, $sig5:expr, $sig6:expr, $sig7:expr => $f:expr) => {
        $crate::lift::lift7(($sig1, $sig2, $sig3, $sig4, $sig5, $sig6, $sig7), $f)
    };

    ($sig1:expr, $sig2:expr, $sig3:expr, $sig4:expr, $sig5:expr, $sig6:expr, $sig7:expr, $sig8:expr => $f:expr) => {
        $crate::lift::lift8(($sig1, $sig2, $sig3, $sig4, $sig5, $sig6, $sig7, $sig8), $f)
    };

    ($sig1:expr, $sig2:expr, $sig3:expr, $sig4:expr, $sig5:expr, $sig6:expr, $sig7:expr, $sig8:expr, $sig9:expr => $f:expr) => {
        $crate::lift::lift9(
            (
                $sig1, $sig2, $sig3, $sig4, $sig5, $sig6, $sig7, $sig8, $sig9,
            ),
            $f,
        )
    };

    ($sig1:expr, $sig2:expr, $sig3:expr, $sig4:expr, $sig5:expr, $sig6:expr, $sig7:expr, $sig8:expr, $sig9:expr, $sig10:expr => $f:expr) => {
        $crate::lift::lift10(
            (
                $sig1, $sig2, $sig3, $sig4, $sig5, $sig6, $sig7, $sig8, $sig9, $sig10,
            ),
            $f,
        )
    };
}

//FIXME: someday we'll have variadic generics..
macro_rules! lift_impl {
    ($fname:ident ( $($tname:ident),+ ) $($idx:tt)+) => {
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
        pub fn $fname<T, F, $($tname),+>(source: ($(Signal<$tname>),+), f: F) -> Signal<T>
        where
            F: Fn($($tname),+) -> T + Send + Sync + 'static,
            $($tname: Clone + Send + Sync + 'static,)+
            T: Send + 'static,
        {
            Signal::shared(SharedImpl{
                storage: Default::default(),
                source,
                f,
            }.wrap())
        }
    };
}

lift_impl!( lift2(S0, S1)                                 0 1);
lift_impl!( lift3(S0, S1, S2)                             0 1 2);
lift_impl!( lift4(S0, S1, S2, S3)                         0 1 2 3);
lift_impl!( lift5(S0, S1, S2, S3, S4)                     0 1 2 3 4);
lift_impl!( lift6(S0, S1, S2, S3, S4, S5)                 0 1 2 3 4 5);
lift_impl!( lift7(S0, S1, S2, S3, S4, S5, S6)             0 1 2 3 4 5 6);
lift_impl!( lift8(S0, S1, S2, S3, S4, S5, S6, S7)         0 1 2 3 4 5 6 7);
lift_impl!( lift9(S0, S1, S2, S3, S4, S5, S6, S7, S8)     0 1 2 3 4 5 6 7 8);
lift_impl!(lift10(S0, S1, S2, S3, S4, S5, S6, S7, S8, S9) 0 1 2 3 4 5 6 7 8 9);
