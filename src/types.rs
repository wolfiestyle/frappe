use std::borrow::Cow;
use helpers::retain_mut;

// callbacks use a Cow<T> argument so we can choose at runtime if
// we will send a ref or an owned value
pub struct Callbacks<T: Clone>
{
    fs: Vec<Box<FnMut(Cow<T>) -> bool>>,
}

impl<T: Clone> Callbacks<T>
{
    pub fn new() -> Self
    {
        Callbacks{ fs: Vec::new() }
    }

    pub fn push<F>(&mut self, cb: F)
        where F: FnMut(Cow<T>) -> bool + 'static
    {
        self.fs.push(Box::new(cb))
    }

    // sends a ref to the first N-1 callbacks, and the owned value to the last
    // this way we prevent tons of cloning
    pub fn call(&mut self, arg: T)
    {
        let maybe_last = self.fs.pop();
        self.call_ref(&arg);

        if let Some(mut last) = maybe_last
        {
            if last(Cow::Owned(arg))
            {
                self.fs.push(last);
            }
        }
    }

    fn call_ref(&mut self, arg: &T)
    {
        retain_mut(&mut self.fs, |f| f(Cow::Borrowed(arg)))
    }

    // we use this to passthrough an unprocessed value
    pub fn call_cow(&mut self, arg: Cow<T>)
    {
        match arg
        {
            Cow::Borrowed(r) => self.call_ref(r),
            Cow::Owned(v) => self.call(v),
        }
    }
}

// type erasure
pub trait Untyped {}
impl<T> Untyped for T {}
