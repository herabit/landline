use std::{
    alloc::Layout,
    any::Any,
    borrow::{Borrow, BorrowMut},
    error, fmt,
    iter::FusedIterator,
    marker::PhantomData,
    mem::{self, ManuallyDrop, MaybeUninit},
    num::NonZero,
    ops::{Deref, Index, IndexMut},
    ptr::NonNull,
};

use crate::mem::Byte;

/// This returns, without `'static` being required, or, anything, whether a type is an [`i8`], [`u8`],
/// or [`Byte`]. In other words, can it be used as a C char, without caring about the sign.
///
/// See https://goldstein.lol/posts/stable-specialization/ for the source
/// of this trick!
#[inline]
#[must_use]
pub(crate) fn is_char_ish<T>() -> bool
where
    T: ?Sized,
{
    /// # Safety
    ///
    /// Only implement this for `i8`, `u8`, or `Byte`
    unsafe trait CharIsh {}

    unsafe impl CharIsh for u8 {}
    unsafe impl CharIsh for i8 {}
    unsafe impl CharIsh for Byte {}

    struct Checker<'a, T>
    where
        T: ?Sized,
    {
        count: &'a mut u8,
        // NOTE: We're using invariance as it makes more sense.
        _type: PhantomData<fn(T) -> T>,
    }

    impl<T> Iterator for Checker<'_, T>
    where
        T: ?Sized,
    {
        type Item = ();

        #[inline(always)]
        fn next(&mut self) -> Option<Self::Item> {
            *self.count += 1;

            // We return `None` once, if `T` is a char-ish type, then
            // the `Fused` specialization will only call this a second time
            // when we're fused.
            None
        }
    }

    impl<T> FusedIterator for Checker<'_, T> where T: ?Sized + CharIsh {}

    let mut count = 0_u8;
    let mut checker = Checker::<T> {
        count: &mut count,
        _type: PhantomData,
    }
    .fuse();

    // Attempt to increment twice...
    _ = checker.next();
    _ = checker.next();

    // If we succeeded, yey!
    count == 2
}
