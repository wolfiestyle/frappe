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

/// Generic sum type of two elements.
///
/// It's used to provide generics over the `Option`/`Result`/`Either` types
pub trait SumType2
{
    type Type1;
    type Type2;

    fn from_type1(val: Self::Type1) -> Self;
    fn from_type2(val: Self::Type2) -> Self;

    fn is_type1(&self) -> bool;
    fn is_type2(&self) -> bool;

    fn into_type1(self) -> Option<Self::Type1>;
    fn into_type2(self) -> Option<Self::Type2>;
}

impl<T> SumType2 for Option<T>
{
    type Type1 = T;
    type Type2 = ();

    fn from_type1(val: Self::Type1) -> Self { Some(val) }
    fn from_type2(_: Self::Type2) -> Self { None }

    fn is_type1(&self) -> bool { self.is_some() }
    fn is_type2(&self) -> bool { self.is_none() }

    fn into_type1(self) -> Option<Self::Type1> { self }
    fn into_type2(self) -> Option<Self::Type2> { self.ok_or(()).err() }
}

impl<T, E> SumType2 for Result<T, E>
{
    type Type1 = T;
    type Type2 = E;

    fn from_type1(val: Self::Type1) -> Self { Ok(val) }
    fn from_type2(val: Self::Type2) -> Self { Err(val) }

    fn is_type1(&self) -> bool { self.is_ok() }
    fn is_type2(&self) -> bool { self.is_err() }

    fn into_type1(self) -> Option<Self::Type1> { self.ok() }
    fn into_type2(self) -> Option<Self::Type2> { self.err() }
}

#[cfg(feature="either")]
impl<L, R> SumType2 for ::either::Either<L, R>
{
    type Type1 = L;
    type Type2 = R;

    fn from_type1(val: Self::Type1) -> Self { ::either::Either::Left(val) }
    fn from_type2(val: Self::Type2) -> Self { ::either::Either::Right(val) }

    fn is_type1(&self) -> bool { self.is_left() }
    fn is_type2(&self) -> bool { self.is_right() }

    fn into_type1(self) -> Option<Self::Type1> { self.left() }
    fn into_type2(self) -> Option<Self::Type2> { self.right() }
}
