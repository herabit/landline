use std::{
    alloc::Allocator,
    convert::Infallible,
    error, fmt, hint,
    iter::FusedIterator,
    marker::PhantomData,
    mem::{DropGuard, MaybeUninit},
    net::{Ipv4Addr, Ipv6Addr},
    num::NonZero,
    ops::Deref,
    ptr::NonNull,
    rc::Rc,
    sync::Arc,
};

use crate::mem::Byte;

/// Helper function that aborts when the passed closure unwinds.
///
/// This exploits the fact that glue is automatically inserted for `extern "C"` functions
/// that prevents unwinding into the calling stack.
///
/// This is a polyfill.
#[inline(always)]
#[track_caller]
#[allow(clippy::let_unit_value)]
pub(crate) fn abort_on_unwind<F, R>(f: F) -> R
where
    F: FnOnce() -> R,
{
    /// We're exploiting the fact that `extern "C"` functions cannot unwind.
    #[inline(always)]
    extern "C" fn abort_on_unwind_impl<F, R>(f: F) -> R
    where
        F: FnOnce() -> R,
    {
        f()
    }

    // SAFETY: Since we know calling `f` with the above `extern "C"` function will make
    //         unwinding impossible, we're going to exploit a `DropGuard` to indicate that
    //         unwinding from this function is, impossible.
    let unwind_impossible = DropGuard::new((), |()| {
        // SAFETY: See above.
        unsafe { hint::unreachable_unchecked() }
    });

    // SAFETY: We know that this will never, ever unwind.
    let result = abort_on_unwind_impl(f);

    // SAFETY: We must dismiss the guard, as we don't want to call its destructor,
    //         as it'd cause UB.
    let _ = DropGuard::dismiss(unwind_impossible);

    result
}

/// An extension trait for iterators.
pub(crate) trait IterExt: Iterator {
    /// Returns whether this iterator has a trusted length.
    #[inline(always)]
    #[must_use]
    fn is_trusted_len(&self) -> bool {
        is_trusted_len::<Self>()
    }
}

impl<I> IterExt for I where I: ?Sized + Iterator {}

/// This returns whether or not the specified iterator has a trusted length.
#[inline]
#[must_use]
pub(crate) fn is_trusted_len<I>() -> bool
where
    I: Iterator + ?Sized,
{
    struct Checker<'a, T>
    where
        T: ?Sized,
    {
        count: &'a mut u8,
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

    impl<T> FusedIterator for Checker<'_, T> where T: ?Sized + TrustedLen {}

    let mut count = 0_u8;
    let mut checker = Checker::<I> {
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

// TODO: Better docs.
/// Trait for possibly uninitialized types.
#[allow(clippy::missing_safety_doc)]
#[allow(dead_code)]
pub(crate) unsafe trait Uninit {
    type Init: ?Sized;

    /// Reinterpret a raw pointer as being initialized.
    #[track_caller]
    #[must_use]
    unsafe fn assume_init_raw(raw: NonNull<Self>) -> NonNull<Self::Init>;

    #[track_caller]
    #[must_use]
    #[inline(always)]
    unsafe fn assume_init_ref(&self) -> &Self::Init {
        // SAFETY: Shish.
        unsafe { Self::assume_init_raw(NonNull::from_ref(self)).as_ref() }
    }

    #[track_caller]
    #[must_use]
    #[inline(always)]
    unsafe fn assume_init_mut(&mut self) -> &mut Self::Init {
        // SAFETY: Shish.
        unsafe { Self::assume_init_raw(NonNull::from_mut(self)).as_mut() }
    }

    #[track_caller]
    #[must_use]
    unsafe fn assume_init(self) -> Self::Init
    where
        Self: Sized,
        Self::Init: Sized;
}

unsafe impl<T> Uninit for MaybeUninit<T> {
    type Init = T;

    #[inline(always)]
    unsafe fn assume_init_raw(raw: NonNull<Self>) -> NonNull<Self::Init> {
        raw.cast()
    }

    #[inline(always)]
    unsafe fn assume_init(self) -> Self::Init {
        // SAFETY: Shish.
        unsafe { <MaybeUninit<T>>::assume_init(self) }
    }
}

unsafe impl<T> Uninit for [MaybeUninit<T>] {
    type Init = [T];

    #[inline(always)]
    unsafe fn assume_init_raw(raw: NonNull<Self>) -> NonNull<Self::Init> {
        // SAFETY: Hello matey!
        unsafe { NonNull::new_unchecked(raw.as_ptr() as *mut [T]) }
    }
}

// TODO: Better docs.
/// An internal type for heap allocated types.
///
/// # Safety
///
/// We need to ensure this is safe, blah blah blah.
///
/// The constructors are guaranteed to return unique values.
#[allow(dead_code)]
pub(crate) unsafe trait HeapAlloc: Sized + Deref {
    /// Get an equivalent type for some target, preserving the allocator.
    type WithTarget<New: ?Sized>: HeapAlloc<Target = New, WithTarget<Self::Target> = Self>;

    /// An error that may be returned when trying to get the inner value.
    type TakeError: fmt::Debug;

    /// Get the inner pointer.
    #[track_caller]
    #[must_use]
    fn into_raw(self) -> NonNull<Self::Target>;

    /// Unsafely create `Self` from a raw pointer.
    #[track_caller]
    #[must_use]
    unsafe fn from_raw(raw: NonNull<Self::Target>) -> Self;

    /// Take the inner value.
    #[track_caller]
    fn take(self) -> Result<Self::Target, Self::TakeError>
    where
        Self::Target: Sized;

    #[track_caller]
    fn into_inner(self) -> Result<Self::Target, Self>
    where
        Self::Target: Sized;

    /// Allocate a sized value.
    #[track_caller]
    #[must_use]
    fn new(value: Self::Target) -> Self
    where
        Self::Target: Sized;

    /// Allocate a sized value that's uninitialized.
    #[track_caller]
    #[must_use]
    fn new_uninit() -> Self::WithTarget<MaybeUninit<Self::Target>>
    where
        Self::Target: Sized;

    /// Allocate a sized value that's zeroed.
    #[track_caller]
    #[must_use]
    fn new_zeroed() -> Self::WithTarget<MaybeUninit<Self::Target>>
    where
        Self::Target: Sized;

    /// Allocate an uninit slice.
    #[track_caller]
    #[must_use]
    fn new_uninit_slice(
        len: usize
    ) -> Self::WithTarget<[MaybeUninit<Self::Target>]>
    where
        Self::Target: Sized;

    /// Allocate a zeroed slice.
    #[track_caller]
    #[must_use]
    fn new_zeroed_slice(
        len: usize
    ) -> Self::WithTarget<[MaybeUninit<Self::Target>]>
    where
        Self::Target: Sized;

    /// Assumes this value as initialized.
    #[track_caller]
    #[must_use]
    #[inline(always)]
    unsafe fn assume_init(
        self
    ) -> Self::WithTarget<<Self::Target as Uninit>::Init>
    where
        Self::Target: Uninit,
    {
        // SAFETY: Hello, world!.
        unsafe { <_>::from_raw(Uninit::assume_init_raw(self.into_raw())) }
    }
}

// pub(crate) unsafe trait HeapAllocExt: HeapAlloc

unsafe impl<T> HeapAlloc for Box<T>
where
    T: ?Sized,
{
    type WithTarget<New: ?Sized> = Box<New>;
    type TakeError = Infallible;

    #[inline(always)]
    fn into_raw(self) -> NonNull<Self::Target> {
        Box::<T>::into_non_null(self)
    }

    #[inline(always)]
    unsafe fn from_raw(raw: NonNull<Self::Target>) -> Self {
        unsafe { Box::<T>::from_non_null(raw) }
    }

    #[inline(always)]
    fn take(self) -> Result<Self::Target, Self::TakeError>
    where
        Self::Target: Sized,
    {
        Ok(*self)
    }

    #[inline(always)]
    fn into_inner(self) -> Result<Self::Target, Self>
    where
        Self::Target: Sized,
    {
        Ok(*self)
    }

    #[inline(always)]
    fn new(value: Self::Target) -> Self
    where
        Self::Target: Sized,
    {
        Box::<T>::new(value)
    }

    #[inline(always)]
    fn new_uninit() -> Box<MaybeUninit<Self::Target>>
    where
        Self::Target: Sized,
    {
        Box::<T>::new_uninit()
    }

    #[inline(always)]
    fn new_zeroed() -> Box<MaybeUninit<Self::Target>>
    where
        Self::Target: Sized,
    {
        Box::<T>::new_zeroed()
    }

    #[inline(always)]
    fn new_uninit_slice(len: usize) -> Box<[MaybeUninit<Self::Target>]>
    where
        Self::Target: Sized,
    {
        Box::<[T]>::new_uninit_slice(len)
    }

    #[inline(always)]
    fn new_zeroed_slice(len: usize) -> Box<[MaybeUninit<Self::Target>]>
    where
        Self::Target: Sized,
    {
        Box::<[T]>::new_zeroed_slice(len)
    }
}

/// The reference counted type was not unique.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub(crate) struct NotUnique(pub(crate) ());

impl fmt::Display for NotUnique {
    #[inline(always)]
    #[track_caller]
    fn fmt(
        &self,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        write!(f, "the reference counted type is not unique")
    }
}

impl error::Error for NotUnique {}

unsafe impl<T> HeapAlloc for Rc<T>
where
    T: ?Sized,
{
    type WithTarget<New: ?Sized> = Rc<New>;
    type TakeError = NotUnique;

    #[inline(always)]
    fn into_raw(self) -> NonNull<Self::Target> {
        // SAFETY: Henlo.
        unsafe {
            NonNull::new_unchecked(
                Rc::<Self::Target>::into_raw(self).cast_mut(),
            )
        }
    }

    #[inline(always)]
    unsafe fn from_raw(raw: NonNull<Self::Target>) -> Self {
        // SAFETY: The caller ensures this is okay.
        unsafe { Rc::<Self::Target>::from_raw(raw.as_ptr()) }
    }

    #[inline(always)]
    fn take(self) -> Result<Self::Target, Self::TakeError>
    where
        Self::Target: Sized,
    {
        Rc::<Self::Target>::into_inner(self).ok_or(NotUnique(()))
    }

    #[inline(always)]
    fn into_inner(self) -> Result<Self::Target, Self>
    where
        Self::Target: Sized,
    {
        Rc::<Self::Target>::try_unwrap(self)
    }

    #[inline(always)]
    fn new(value: Self::Target) -> Self
    where
        Self::Target: Sized,
    {
        Rc::<Self::Target>::new(value)
    }

    #[inline(always)]
    fn new_uninit() -> Rc<MaybeUninit<Self::Target>>
    where
        Self::Target: Sized,
    {
        Rc::<Self::Target>::new_uninit()
    }

    #[inline(always)]
    fn new_zeroed() -> Rc<MaybeUninit<Self::Target>>
    where
        Self::Target: Sized,
    {
        Rc::<Self::Target>::new_zeroed()
    }

    #[inline(always)]
    fn new_uninit_slice(len: usize) -> Rc<[MaybeUninit<Self::Target>]>
    where
        Self::Target: Sized,
    {
        Rc::<[Self::Target]>::new_uninit_slice(len)
    }

    #[inline(always)]
    fn new_zeroed_slice(len: usize) -> Rc<[MaybeUninit<Self::Target>]>
    where
        Self::Target: Sized,
    {
        Rc::<[Self::Target]>::new_zeroed_slice(len)
    }
}

unsafe impl<T> HeapAlloc for Arc<T>
where
    T: ?Sized,
{
    type WithTarget<New: ?Sized> = Arc<New>;
    type TakeError = NotUnique;

    #[inline(always)]
    fn into_raw(self) -> NonNull<Self::Target> {
        // SAFETY: Henlo.
        unsafe {
            NonNull::new_unchecked(
                Arc::<Self::Target>::into_raw(self).cast_mut(),
            )
        }
    }

    #[inline(always)]
    unsafe fn from_raw(raw: NonNull<Self::Target>) -> Self {
        // SAFETY: The caller ensures this is okay.
        unsafe { Arc::<Self::Target>::from_raw(raw.as_ptr()) }
    }

    #[inline(always)]
    fn take(self) -> Result<Self::Target, Self::TakeError>
    where
        Self::Target: Sized,
    {
        Arc::<Self::Target>::into_inner(self).ok_or(NotUnique(()))
    }

    #[inline(always)]
    fn into_inner(self) -> Result<Self::Target, Self>
    where
        Self::Target: Sized,
    {
        Arc::<Self::Target>::try_unwrap(self)
    }

    #[inline(always)]
    fn new(value: Self::Target) -> Self
    where
        Self::Target: Sized,
    {
        Arc::<Self::Target>::new(value)
    }

    #[inline(always)]
    fn new_uninit() -> Arc<MaybeUninit<Self::Target>>
    where
        Self::Target: Sized,
    {
        Arc::<Self::Target>::new_uninit()
    }

    #[inline(always)]
    fn new_zeroed() -> Arc<MaybeUninit<Self::Target>>
    where
        Self::Target: Sized,
    {
        Arc::<Self::Target>::new_zeroed()
    }

    #[inline(always)]
    fn new_uninit_slice(len: usize) -> Arc<[MaybeUninit<Self::Target>]>
    where
        Self::Target: Sized,
    {
        Arc::<[Self::Target]>::new_uninit_slice(len)
    }

    #[inline(always)]
    fn new_zeroed_slice(len: usize) -> Arc<[MaybeUninit<Self::Target>]>
    where
        Self::Target: Sized,
    {
        Arc::<[Self::Target]>::new_zeroed_slice(len)
    }
}

/// A trait for iterators that are safe to flatten when yielded by some
/// other iterator.
///
/// This is ***not*** a trait for types that yield flattenable iterators.
///
/// Currently the only supported iterators are array iterators.
#[allow(dead_code, clippy::missing_safety_doc)]
pub(crate) unsafe trait TrustedFlatten:
    IntoIterator<IntoIter: TrustedLen>
{
}

unsafe impl<T, const N: usize> TrustedFlatten for [T; N] {}
unsafe impl<T, const N: usize> TrustedFlatten for &[T; N] {}
unsafe impl<T, const N: usize> TrustedFlatten for &mut [T; N] {}

/// A polyfill for the `TrustedStep` trait.
#[allow(dead_code, clippy::missing_safety_doc)]
pub(crate) unsafe trait TrustedStep: Copy + PartialOrd {}

macro_rules! trusted_step {
    ($($ty:ty),+ $(,)?) => {
        $(
            unsafe impl TrustedStep for $ty {}
        )+
    };
}

trusted_step!(Ipv4Addr, Ipv6Addr);
trusted_step!(
    NonZero<u8>,
    NonZero<u16>,
    NonZero<u32>,
    NonZero<u64>,
    NonZero<u128>,
    NonZero<usize>
);
trusted_step!(char);
trusted_step!(i8, i16, i32, i64, i128, isize);
trusted_step!(u8, u16, u32, u64, u128, usize);

/// A polyfill for the `TrustedLen` trait.
#[allow(dead_code, clippy::missing_safety_doc)]
pub(crate) unsafe trait TrustedLen: Iterator {}

unsafe impl TrustedLen for std::char::ToLowercase {}
unsafe impl TrustedLen for std::char::ToUppercase {}

unsafe impl<T, const N: usize> TrustedLen
    for std::boxed::BoxedArrayIntoIter<T, N>
{
}

unsafe impl<T, const N: usize> TrustedLen for std::array::IntoIter<T, N> {}

unsafe impl<T> TrustedLen for std::slice::Iter<'_, T> {}
unsafe impl<T> TrustedLen for std::slice::IterMut<'_, T> {}

unsafe impl<T> TrustedLen for std::slice::Chunks<'_, T> {}
unsafe impl<T> TrustedLen for std::slice::RChunks<'_, T> {}

unsafe impl<T> TrustedLen for std::slice::ChunksMut<'_, T> {}
unsafe impl<T> TrustedLen for std::slice::RChunksMut<'_, T> {}

unsafe impl<T> TrustedLen for std::slice::ChunksExact<'_, T> {}
unsafe impl<T> TrustedLen for std::slice::RChunksExact<'_, T> {}

unsafe impl<T> TrustedLen for std::slice::ChunksExactMut<'_, T> {}
unsafe impl<T> TrustedLen for std::slice::RChunksExactMut<'_, T> {}

unsafe impl<T> TrustedLen for std::slice::Windows<'_, T> {}
unsafe impl<T, const N: usize> TrustedLen
    for std::slice::ArrayWindows<'_, T, N>
{
}

unsafe impl<A> TrustedLen for std::option::IntoIter<A> {}
unsafe impl<A> TrustedLen for std::option::Iter<'_, A> {}
unsafe impl<A> TrustedLen for std::option::IterMut<'_, A> {}

unsafe impl<A> TrustedLen for std::result::IntoIter<A> {}
unsafe impl<A> TrustedLen for std::result::Iter<'_, A> {}
unsafe impl<A> TrustedLen for std::result::IterMut<'_, A> {}

unsafe impl<A> TrustedLen for std::ops::Range<A>
where
    A: TrustedStep,
    std::ops::Range<A>: Iterator<Item = A>,
{
}

unsafe impl<A> TrustedLen for std::range::RangeIter<A>
where
    A: TrustedStep,
    std::range::RangeIter<A>: Iterator<Item = A>,
{
}

unsafe impl<A> TrustedLen for std::ops::RangeFrom<A>
where
    A: TrustedStep,
    std::ops::RangeFrom<A>: Iterator<Item = A>,
{
}

unsafe impl<A> TrustedLen for std::range::RangeFromIter<A>
where
    A: TrustedStep,
    std::range::RangeFromIter<A>: Iterator<Item = A>,
{
}

unsafe impl<A> TrustedLen for std::ops::RangeInclusive<A>
where
    A: TrustedStep,
    std::ops::RangeInclusive<A>: Iterator<Item = A>,
{
}

unsafe impl<A> TrustedLen for std::range::RangeInclusiveIter<A>
where
    A: TrustedStep,
    std::range::RangeInclusiveIter<A>: Iterator<Item = A>,
{
}

unsafe impl<B, I, F> TrustedLen for std::iter::Map<I, F>
where
    I: TrustedLen,
    F: FnMut(I::Item) -> B,
{
}

// SAFETY: This may be unsound, but it ***shouldn't be***.
unsafe impl<I, U, F> TrustedLen for std::iter::FlatMap<I, U, F>
where
    I: TrustedLen,
    U: TrustedFlatten,
    F: FnMut(I::Item) -> U,
{
}

// SAFETY: This may be unsound, but it ***shouldn't be***.
unsafe impl<I> TrustedLen for std::iter::Flatten<I> where
    I: TrustedLen<Item: TrustedFlatten>
{
}

unsafe impl<I> TrustedLen for &mut I where I: TrustedLen + ?Sized {}

unsafe impl<T> TrustedLen for std::iter::Empty<T> {}
unsafe impl<A, F> TrustedLen for std::iter::OnceWith<F> where F: FnOnce() -> A {}
unsafe impl<T> TrustedLen for std::iter::Once<T> {}
unsafe impl<A, F> TrustedLen for std::iter::RepeatWith<F> where F: FnMut() -> A {}
unsafe impl<A> TrustedLen for std::iter::Repeat<A> where A: Clone {}
unsafe impl<A> TrustedLen for std::iter::RepeatN<A> where A: Clone {}
unsafe impl<I> TrustedLen for std::iter::Enumerate<I> where I: TrustedLen {}
unsafe impl<I> TrustedLen for std::iter::Fuse<I> where I: TrustedLen {}
unsafe impl<I> TrustedLen for std::iter::Peekable<I> where I: TrustedLen {}
unsafe impl<I> TrustedLen for std::iter::Take<I> where I: TrustedLen {}
unsafe impl<I> TrustedLen for std::iter::Rev<I> where
    I: TrustedLen + DoubleEndedIterator
{
}

unsafe impl<'a, I, T> TrustedLen for std::iter::Cloned<I>
where
    T: 'a + Clone,
    I: TrustedLen<Item = &'a T>,
{
}

unsafe impl<'a, I, T> TrustedLen for std::iter::Copied<I>
where
    T: 'a + Copy,
    I: TrustedLen<Item = &'a T>,
{
}

unsafe impl<A, B> TrustedLen for std::iter::Chain<A, B>
where
    A: TrustedLen,
    B: TrustedLen<Item = A::Item>,
{
}

unsafe impl<A, B> TrustedLen for std::iter::Zip<A, B>
where
    A: TrustedLen,
    B: TrustedLen,
{
}

unsafe impl<K, V> TrustedLen for std::collections::btree_map::IntoIter<K, V> {}
unsafe impl<K, V> TrustedLen for std::collections::btree_map::IntoKeys<K, V> {}
unsafe impl<K, V> TrustedLen for std::collections::btree_map::IntoValues<K, V> {}
unsafe impl<K, V> TrustedLen for std::collections::btree_map::Iter<'_, K, V> {}
unsafe impl<K, V> TrustedLen
    for std::collections::btree_map::IterMut<'_, K, V>
{
}
unsafe impl<K, V> TrustedLen for std::collections::btree_map::Keys<'_, K, V> {}
unsafe impl<K, V> TrustedLen for std::collections::btree_map::Values<'_, K, V> {}
unsafe impl<K, V> TrustedLen
    for std::collections::btree_map::ValuesMut<'_, K, V>
{
}

unsafe impl<T> TrustedLen for std::collections::btree_set::IntoIter<T> {}
unsafe impl<T> TrustedLen for std::collections::btree_set::Iter<'_, T> {}

unsafe impl<T> TrustedLen for std::collections::vec_deque::IntoIter<T> {}
unsafe impl<T> TrustedLen for std::collections::vec_deque::Iter<'_, T> {}
unsafe impl<T> TrustedLen for std::collections::vec_deque::IterMut<'_, T> {}

unsafe impl<T> TrustedLen for std::vec::IntoIter<T> {}
unsafe impl<T> TrustedLen for std::vec::Drain<'_, T> {}
