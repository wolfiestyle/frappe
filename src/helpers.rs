use std::rc::{Rc, Weak};

pub fn rc_and_weak<T>(val: T) -> (Rc<T>, Weak<T>)
{
    let rc = Rc::new(val);
    let weak = Rc::downgrade(&rc);
    (rc, weak)
}

pub fn with_weak<T, F>(weak: &Weak<T>, f: F) -> bool
    where F: FnOnce(Rc<T>)
{
    weak.upgrade().map(f).is_some()
}
