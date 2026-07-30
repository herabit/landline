use std::{
    borrow::{Borrow, BorrowMut},
    cmp::Ordering,
    fmt, hash, hint,
    marker::PhantomData,
    mem::{self, ManuallyDrop},
    ops::{Deref, DerefMut},
};

use crate::mem::{AsBytes, AsBytesMut, Byte, as_bytes, as_bytes_mut};

mod sealed {
    pub trait SealAccess {}
}

/// Just an enum representing the possible [`Access`] kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[repr(u8)]
pub enum AccessKind {
    /// This indicates that it is safe to read from a shared reference (`&T`).
    Read = 0b01,
    /// This indicates that it is safe to write to an exclusive reference (`&mut T`).
    Write = 0b10,
    #[default]
    /// This indicates that it is safe to read from a shared reference or exclusive reference (`&T` | `&mut T`),
    /// or write to an exclusive reference (`&mut T`).
    ReadWrite = 0b11,
}

impl AccessKind {
    /// Returns whether this access kind indicates readability.
    #[inline(always)]
    #[must_use]
    pub const fn can_read(self) -> bool {
        matches!(self, AccessKind::Read | AccessKind::ReadWrite)
    }

    /// Returns whether this access kind indicates writability.
    #[inline(always)]
    #[must_use]
    pub const fn can_write(self) -> bool {
        matches!(self, AccessKind::Write | AccessKind::ReadWrite)
    }
}

impl PartialOrd for AccessKind {
    #[inline(always)]
    #[unsafe(no_mangle)]
    fn partial_cmp(
        &self,
        other: &Self,
    ) -> Option<Ordering> {
        // NOTE: We need to find some branchless function, `@`, that results in the following:
        //
        //       - (Read, Read)           => `1 @ 1 =>  0` # Eq
        //       - (Read, Write)          => `1 @ 2 =>  2` # None
        //       - (Read, ReadWrite)      => `1 @ 3 => -1` # Less
        //
        //       - (Write, Read)          => `2 @ 1 =>  2` # None
        //       - (Write, Write)         => `2 @ 2 =>  0` # Eq
        //       - (Write, ReadWrite)     => `2 @ 3 => -1` # Less
        //
        //       - (ReadWrite, Read)      => `3 @ 1 =>  1` # Greater
        //       - (ReadWrite, Write)     => `3 @ 2 =>  1` # Greater
        //       - (ReadWrite, ReadWrite) => `3 @ 3 =>  0` # Equal
        //
        //
        //
        // NOTE: We're relying on `Option::<Ordering>::None == 2` for optimal codegen...
        //
        //       Even if this, changes for some reason, this should still be faster in a loop.

        // NOTE: This is just a cool hack for `x == AccessKind::ReadWrite` that
        //       seems to optimize better.
        let is_rw = |kind: AccessKind| match (kind as u8 - 1) >> 1 {
            0 => false,
            1 => true,
            _ => unreachable!(),
        };

        // NOTE: Another hack that seems to optimize better.
        let to_mask = |cond: bool| (cond as u8) * 0b11;

        // NOTE: This value is only actually used if `self == ReadWrite`.
        let rw_case = {
            let value = (*self as i8).wrapping_sub(*other as i8);

            // NOTE: If we're actually dealing with a RW case, then
            //       this will be `0` if the rhs is RW, and `1` if
            //       it isn't.
            //
            //       If we're not in a RW case, this will have a junk value we discard.
            (value >> 1) ^ (value & 0b01)
        };

        todo!("implement for the `Read` and `Write` cases (optimally using the same code).")
    }
}

/// A marker trait indicating the access rights an [`UnsafeBytes`] has.
#[allow(clippy::missing_safety_doc)]
pub unsafe trait Access:
    'static
    + Copy
    + Send
    + Sync
    + Ord
    + Default
    + AsBytes
    + AsBytesMut
    + hash::Hash
    + fmt::Debug
    + sealed::SealAccess
{
    const ACCESS: Self;
    const KIND: AccessKind;
}

/// A marker trait that indicates some [`Access`] contains some other [`Access`].
#[allow(clippy::missing_safety_doc)]
pub unsafe trait Has<A = Self>: Access
where
    A: Access,
{
}

unsafe impl<A> Has<A> for A where A: Access {}

/// A marker trait indicating that some [`Access`] can be safely used on some [`T`].
#[allow(clippy::missing_safety_doc)]
pub unsafe trait CanAccess<T>: Access
where
    T: ?Sized,
{
}

unsafe impl<T> CanAccess<T> for Read where T: ?Sized + AsBytes {}
unsafe impl<T> CanAccess<T> for Write where T: ?Sized + AsBytesMut {}
unsafe impl<T> CanAccess<T> for ReadWrite where T: ?Sized + AsBytes + AsBytesMut {}

/// An [`Access`] that indicates it is safe to read the underlying memory of something.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Read;

unsafe impl AsBytes for Read {}
unsafe impl AsBytesMut for Read {}

impl sealed::SealAccess for Read {}
unsafe impl Access for Read {
    const ACCESS: Self = Read;
    const KIND: AccessKind = AccessKind::Read;
}

/// An [`Access`] that indicates it is safe to write to the underlying memory of something.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Write;

unsafe impl AsBytes for Write {}
unsafe impl AsBytesMut for Write {}

impl sealed::SealAccess for Write {}
unsafe impl Access for Write {
    const ACCESS: Self = Write;
    const KIND: AccessKind = AccessKind::Write;
}

/// An [`Access`]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct ReadWrite;

unsafe impl AsBytes for ReadWrite {}
unsafe impl AsBytesMut for ReadWrite {}

impl sealed::SealAccess for ReadWrite {}
unsafe impl Access for ReadWrite {
    const ACCESS: Self = ReadWrite;
    const KIND: AccessKind = AccessKind::ReadWrite;
}

unsafe impl Has<Read> for ReadWrite {}
unsafe impl Has<Write> for ReadWrite {}

/// A Wrapper type for some [`T`] that implements [`AsBytes`] and/or [`AsBytesMut`] according
/// to the [`Access`] specified.
///
/// - [`Read`]: Implements [`AsBytes`].
/// - [`Write`]: Implements [`AsBytesMut`].
/// - [`ReadWrite`]: Implements [`AsBytes`] and [`AsBytesMut`].
#[repr(transparent)]
pub struct UnsafeBytes<T, A = ReadWrite>
where
    T: ?Sized,
    A: Access,
{
    _marker: PhantomData<A>,
    val: T,
}

impl<T, A> UnsafeBytes<T, A>
where
    T: ?Sized,
    A: Access,
{
    #[inline(always)]
    #[must_use]
    pub const unsafe fn from_ref_unchecked(val: &T) -> &UnsafeBytes<T, A> {
        unsafe { (&raw const *val as *const UnsafeBytes<T, A>).as_ref_unchecked() }
    }

    #[inline(always)]
    #[must_use]
    pub const fn from_ref(val: &T) -> &UnsafeBytes<T, A>
    where
        A: CanAccess<T>,
    {
        unsafe { UnsafeBytes::from_ref_unchecked(val) }
    }

    #[inline(always)]
    #[must_use]
    pub const fn as_ref(&self) -> &T {
        &self.val
    }

    #[inline(always)]
    #[must_use]
    pub const unsafe fn from_mut_unchecked(val: &mut T) -> &mut UnsafeBytes<T, A> {
        unsafe { (&raw mut *val as *mut UnsafeBytes<T, A>).as_mut_unchecked() }
    }

    #[inline(always)]
    #[must_use]
    pub const fn from_mut(val: &mut T) -> &mut UnsafeBytes<T, A>
    where
        A: CanAccess<T>,
    {
        unsafe { UnsafeBytes::from_mut_unchecked(val) }
    }

    #[inline(always)]
    #[must_use]
    pub const fn as_mut(&mut self) -> &mut T {
        &mut self.val
    }

    #[inline(always)]
    #[must_use]
    pub const unsafe fn new_unchecked(val: T) -> UnsafeBytes<T, A>
    where
        T: Sized,
    {
        UnsafeBytes {
            _marker: PhantomData,
            val,
        }
    }

    #[inline(always)]
    #[must_use]
    pub const fn new(val: T) -> UnsafeBytes<T, A>
    where
        T: Sized,
        A: CanAccess<T>,
    {
        unsafe { UnsafeBytes::new_unchecked(val) }
    }

    #[inline(always)]
    #[must_use]
    pub const fn into_inner(self) -> T
    where
        T: Sized,
    {
        let this = ManuallyDrop::new(self);
        unsafe { mem::transmute_copy(&this) }
    }

    #[inline(always)]
    #[must_use]
    pub const fn as_bytes(&self) -> &[Byte]
    where
        A: Has<Read>,
    {
        as_bytes(self)
    }

    #[inline(always)]
    #[must_use]
    pub const fn as_bytes_mut(&mut self) -> &mut [Byte]
    where
        A: Has<Write>,
    {
        as_bytes_mut(self)
    }
}

impl<T> UnsafeBytes<T, Read> where T: ?Sized {}

unsafe impl<T, A> AsBytes for UnsafeBytes<T, A>
where
    T: ?Sized,
    A: Has<Read>,
{
}

unsafe impl<T, A> AsBytesMut for UnsafeBytes<T, A>
where
    T: ?Sized,
    A: Has<Write>,
{
}

impl<T, A> Deref for UnsafeBytes<T, A>
where
    T: ?Sized,
    A: Access,
{
    type Target = T;

    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        <Self>::as_ref(self)
    }
}

impl<T, A> DerefMut for UnsafeBytes<T, A>
where
    T: ?Sized,
    A: Access,
{
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut Self::Target {
        <Self>::as_mut(self)
    }
}

impl<T, A> Borrow<[Byte]> for UnsafeBytes<T, A>
where
    T: ?Sized,
    A: Has<Read>,
{
    #[inline(always)]
    fn borrow(&self) -> &[Byte] {
        self.as_bytes()
    }
}

impl<T, A> BorrowMut<[Byte]> for UnsafeBytes<T, A>
where
    T: ?Sized,
    A: Has<Read> + Has<Write>,
{
    #[inline(always)]
    fn borrow_mut(&mut self) -> &mut [Byte] {
        self.as_bytes_mut()
    }
}

impl<T, A> AsRef<[Byte]> for UnsafeBytes<T, A>
where
    T: ?Sized,
    A: Has<Read>,
{
    #[inline(always)]
    fn as_ref(&self) -> &[Byte] {
        self.as_bytes()
    }
}

impl<T, A> AsMut<[Byte]> for UnsafeBytes<T, A>
where
    T: ?Sized,
    A: Has<Write>,
{
    #[inline(always)]
    fn as_mut(&mut self) -> &mut [Byte] {
        self.as_bytes_mut()
    }
}
