use std::{
    borrow::{Borrow, BorrowMut},
    fmt, hash,
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
    /// Borrow a `T` with some access rights, without any checks.
    ///
    /// # Safety
    ///
    /// The caller must ensure that it the access rights granted are not misused
    /// in any capacity that violates any invariants of the underlying `T`.
    #[inline(always)]
    #[must_use]
    pub const unsafe fn from_ref_unchecked(val: &T) -> &UnsafeBytes<T, A> {
        // SAFETY: The caller ensures this is acceptable.
        unsafe {
            (&raw const *val as *const UnsafeBytes<T, A>).as_ref_unchecked()
        }
    }

    /// Borrow a `T` with some access rights.
    #[inline(always)]
    #[must_use]
    pub const fn from_ref(val: &T) -> &UnsafeBytes<T, A>
    where
        A: CanAccess<T>,
    {
        // SAFETY: We know that it's sound to access `T` for `A`.
        unsafe { UnsafeBytes::from_ref_unchecked(val) }
    }

    /// Get the underlying `T`.
    #[inline(always)]
    #[must_use]
    pub const fn as_ref(&self) -> &T {
        &self.val
    }

    /// Mutably borrow a `T` with some access rights, without any checks.
    ///
    /// # Safety
    ///
    /// The caller must ensure that it the access rights granted are not misused
    /// in any capacity that violates any invariants of the underlying `T`.
    #[inline(always)]
    #[must_use]
    pub const unsafe fn from_mut_unchecked(
        val: &mut T
    ) -> &mut UnsafeBytes<T, A> {
        // SAFETY: The caller ensures that it's safe to grant `A` to the `T`.
        unsafe { (&raw mut *val as *mut UnsafeBytes<T, A>).as_mut_unchecked() }
    }

    /// Mutably borrow some `T` with some set of access rights.
    #[inline(always)]
    #[must_use]
    pub const fn from_mut(val: &mut T) -> &mut UnsafeBytes<T, A>
    where
        A: CanAccess<T>,
    {
        // SAFETY: We know that `A` can access `T`.
        unsafe { UnsafeBytes::from_mut_unchecked(val) }
    }

    /// Mutably borrow the underlying `T`.
    #[inline(always)]
    #[must_use]
    pub const fn as_mut(&mut self) -> &mut T {
        &mut self.val
    }

    /// Without any checks, grant access rights to some underlying `T`.
    ///
    /// # Safety
    ///
    /// The caller must ensure that it the access rights granted are not misused
    /// in any capacity that violates any invariants of the underlying `T`.
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

    /// Grant access rights to some underlying `T`.
    #[inline(always)]
    #[must_use]
    pub const fn new(val: T) -> UnsafeBytes<T, A>
    where
        T: Sized,
        A: CanAccess<T>,
    {
        // SAFETY: We know that `A` has adequate rights to `T`.
        unsafe { UnsafeBytes::new_unchecked(val) }
    }

    #[inline(always)]
    #[must_use]
    pub const fn into_inner(self) -> T
    where
        T: Sized,
    {
        let this = ManuallyDrop::new(self);
        // SAFETY: We know that `UnsafeBytes` shares a memory layout with `T`.
        unsafe { mem::transmute_copy(&this) }
    }

    /// If the access rights grant read access, get the underlying memory as a byte buffer.
    #[inline(always)]
    #[must_use]
    pub const fn as_bytes(&self) -> &[Byte]
    where
        A: Has<Read>,
    {
        as_bytes(self)
    }

    /// If the access rights grant write access, get the underlying memory as a byte buffer.
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
