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

// unlike the original Vec::retain, we do not preserve the order
pub fn retain_swap<T, F>(vec: &mut Vec<T>, pred: F)
    where F: Fn(&T) -> bool
{
    let mut i = 0;
    while i < vec.len()
    {
        if pred(&vec[i])
        {
            i += 1;
        }
        else
        {
            vec.swap_remove(i);
        }
    }
}
