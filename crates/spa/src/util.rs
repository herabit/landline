use std::{
    alloc::Allocator, convert::Infallible, error, fmt, iter::FusedIterator,
    marker::PhantomData, mem::MaybeUninit, ops::Deref, ptr::NonNull, rc::Rc,
    sync::Arc,
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

// TODO: Better docs.
/// Trait for possibly uninitialized types.
#[allow(clippy::missing_safety_doc)]
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
