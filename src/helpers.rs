use std::rc::{Rc, Weak};
use std::cell::RefCell;

pub fn rc_and_weak<T>(val: T) -> (Rc<RefCell<T>>, Weak<RefCell<T>>)
{
    let rc = Rc::new(RefCell::new(val));
    let weak = Rc::downgrade(&rc);
    (rc, weak)
}

pub fn with_weak<T, F>(weak: &Weak<T>, f: F) -> bool
    where F: FnOnce(Rc<T>)
{
    weak.upgrade().map(f).is_some()
}

// unlike the original Vec::retain, we do not preserve the order
pub fn retain_mut<T, F>(vec: &mut Vec<T>, pred: F)
    where F: Fn(&mut T) -> bool
{
    let mut i = 0;
    while i < vec.len()
    {
        if pred(&mut vec[i])
        {
            i += 1;
        }
        else
        {
            vec.swap_remove(i);
        }
    }
}
