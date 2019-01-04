use std::sync::{self as arc, Arc};

pub fn arc_and_weak<T>(val: T) -> (Arc<T>, arc::Weak<T>) {
    let rc = Arc::new(val);
    let weak = Arc::downgrade(&rc);
    (rc, weak)
}

macro_rules! with_weak {
    ($weak:expr, $f:expr) => {
        $weak.upgrade().map($f).is_some()
    };
}
