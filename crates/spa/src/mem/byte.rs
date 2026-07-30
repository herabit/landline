use std::{
    borrow::{Borrow, BorrowMut},
    cell::{Cell, UnsafeCell},
    fmt, hash,
    marker::PhantomData,
    mem::{self, ManuallyDrop, MaybeUninit},
    num::{NonZero, Saturating, Wrapping},
    ops::{Deref, DerefMut},
    os::fd::{BorrowedFd, OwnedFd},
    ptr::NonNull,
    slice,
};

/// A special [`u8`] that is always initialized, but isn't strictly a [`u8`], either.
///
/// This allows us to represent initialized regions of memory, without necessarily converting the underlying memory to integers.
///
/// This is particularly handy due to the fact that pipewire, unfortunately, allows the serialization and deserialization of raw pointers...
///
/// Yeah. It's not ideal.
///
/// So, this at least will make it less likely that we inadvertently fuck up provenance somewhere... Do note, that when we want to read a pointer
/// we have to use operations that do not lose provenance, such as reading from a raw pointer.
#[repr(transparent)]
pub struct Byte(MaybeUninit<u8>);

impl Clone for Byte {
    #[inline(always)]
    fn clone(&self) -> Self {
        *self
    }
}

impl Copy for Byte {}

unsafe impl AsBytes for Byte {}
unsafe impl AsBytesMut for Byte {}

// impl Byte {
//     #[inline(always)]
//     #[must_use]
//     pub const fn from_
// }

/// Trait that indicates it is safe to reinterpret the memory at a `&Self` to a `&[Byte]`.
///
/// # Safety
///
/// I need to flesh this out, but the TLDR is:
///
/// - The type must be "frozen"... This roughly means that the type must contain no interior mutability within the
///   memory region for this type. So `Cell`, `UnsafeCell`, atomics, ETC. are disallowed unless there is some
///   layer of indirection through a pointer, roughly.
/// - The type must not contain any uninitialized bytes.
/// - Probably some other things, don't implement this yourself.
pub unsafe trait AsBytes {}

/// Trait that indicates it is not safe to reinterpret the memory at a `&mut Self` to a `&mut [Byte]`.
///
/// # Safety
///
/// I need to flesh this out, but the TLDR is:
///
/// - The type must not contained uninitialized bytes.
/// - It is safe to mutate the underlying memory without violating any invariants.
/// - Probably other things, don't implement this yourself, you're gonna fuck it up.
pub unsafe trait AsBytesMut {}

/// Get a slice of the underlying bytes of some value.
#[inline(always)]
#[must_use]
pub const fn as_bytes<T>(val: &T) -> &[Byte]
where
    T: ?Sized + AsBytes,
{
    let len = size_of_val(val);
    let addr = (&raw const *val).cast::<Byte>();

    // SAFETY: `T` implements `AsBytes`, it is safe to reinterpret the value.
    unsafe { slice::from_raw_parts(addr, len) }
}

/// Get a mutable slice of the underlying bytes of some value.
#[inline(always)]
#[must_use]
pub const fn as_bytes_mut<T>(val: &mut T) -> &mut [Byte]
where
    T: ?Sized + AsBytesMut,
{
    let len = size_of_val(val);
    let addr = (&raw mut *val).cast::<Byte>();

    // SAFETY: `T` implements `AsBytesMut`, it is safe to reinterpret the value.
    unsafe { slice::from_raw_parts_mut(addr, len) }
}

/// Get a slice of the underlying bytes unsafely.
///
/// # Safety
///
/// The caller must ensure that the value has no uninitalized bytes, and that it is safe to
/// read any byte during the course of the borrow (no possibility of data races, etc.), and that
/// ensure that you know what you're doing when using the read data (such as when dealing with pointer provenance).
///
/// There's likely other stuff, don't use this.
#[inline(always)]
#[must_use]
pub const unsafe fn as_bytes_unchecked<T>(val: &T) -> &[Byte]
where
    T: ?Sized,
{
    // SAFETY: The caller ensures that is acceptable.
    unsafe { UnsafeBytes::<T, Read>::from_ref_unchecked(val) }.as_bytes()
}

/// Get a mutable slice of the underlying bytes unsafely.
///
/// # Safety
///
/// The caller must ensure that the value has uninitialized bytes, that it is safe to
/// read any byte during the course of the borrow (no possibility of data races, etc.),
/// and that the underlying memory is only modified in ways that are safe for `T` during the
/// course of the borrow. This, additionally, requires that by the end of the borrow, that the underlying
/// memory is a valid `T` (what this means is specific to each type, for example for types that contain pointers,
/// you must be very, very, very careful when handling provenance).
///
/// There's likely other stuff, don't use this.
#[inline(always)]
#[must_use]
pub const unsafe fn as_bytes_mut_unchecked<T>(val: &mut T) -> &mut [Byte]
where
    T: ?Sized,
{
    // SAFETY: The caller ensures that this is acceptable.
    unsafe { UnsafeBytes::<T, Write>::from_mut_unchecked(val) }.as_bytes_mut()
}

unsafe impl<T> AsBytes for [T] where T: AsBytes {}
unsafe impl<T> AsBytesMut for [T] where T: AsBytesMut {}

unsafe impl<T, const N: usize> AsBytes for [T; N] where T: AsBytes {}
unsafe impl<T, const N: usize> AsBytesMut for [T; N] where T: AsBytesMut {}

unsafe impl<T> AsBytes for Saturating<T> where T: AsBytes {}
unsafe impl<T> AsBytesMut for Saturating<T> where T: AsBytesMut {}

unsafe impl<T> AsBytes for Wrapping<T> where T: AsBytes {}
unsafe impl<T> AsBytesMut for Wrapping<T> where T: AsBytesMut {}

unsafe impl<T> AsBytes for PhantomData<T> where T: ?Sized {}
unsafe impl<T> AsBytesMut for PhantomData<T> where T: ?Sized {}

unsafe impl<T> AsBytes for ManuallyDrop<T> where T: ?Sized + AsBytes {}
unsafe impl<T> AsBytesMut for ManuallyDrop<T> where T: ?Sized + AsBytesMut {}

// NOTE: With these interior mutability types that share a memory layout with what they contain,
//       so long as we have an exclusive reference, we won't run into UB.
unsafe impl<T> AsBytesMut for UnsafeCell<T> where T: ?Sized + AsBytesMut {}
unsafe impl<T> AsBytesMut for Cell<T> where T: ?Sized + AsBytesMut {}

unsafe impl<T> AsBytes for *const T {}
unsafe impl<T> AsBytesMut for *const T {}

unsafe impl<T> AsBytes for *mut T {}
unsafe impl<T> AsBytesMut for *mut T {}

unsafe impl<T> AsBytes for NonNull<T> {}
unsafe impl<T> AsBytes for Option<NonNull<T>> {}
unsafe impl<T> AsBytesMut for Option<NonNull<T>> {}

// NOTE: Pointers can have any bit pattern, so long as we don't fuck up provenance.
//
//       Additionally, if we have exclusive access to an atomic, we can access the underlying data without races.
#[cfg(target_has_atomic = "ptr")]
unsafe impl<T> AsBytesMut for std::sync::atomic::AtomicPtr<T> {}

unsafe impl<T> AsBytes for &T {}
unsafe impl<T> AsBytes for Option<&T> {}

unsafe impl<T> AsBytes for &mut T {}
unsafe impl<T> AsBytes for Option<&mut T> {}

unsafe impl<T> AsBytes for Box<T> {}
unsafe impl<T> AsBytes for Option<Box<T>> {}

unsafe impl AsBytes for () {}
unsafe impl AsBytesMut for () {}

// NOTE: `AsBytesMut` cannot be implemented as we may invalidate the underlying UTF-8.
unsafe impl AsBytes for str {}

// NOTE: `AsBytesMut` cannot be implemented as `char` imposes additional bit validity constraints.
unsafe impl AsBytes for char {}
unsafe impl AsBytes for NonZero<char> {}

// NOTE: `AsBytesMut` cannot be implemented as `bool` imposees additional bit validity constraints.
unsafe impl AsBytes for bool {}

unsafe impl AsBytes for f32 {}
unsafe impl AsBytesMut for f32 {}

unsafe impl AsBytes for f64 {}
unsafe impl AsBytesMut for f64 {}

macro_rules! integers {
    ($($int:ident),+) => {
        $(
            unsafe impl AsBytes for $int {}
            unsafe impl AsBytesMut for $int {}

            unsafe impl AsBytes for std::num::NonZero<$int> {}

            unsafe impl AsBytes for Option<std::num::NonZero<$int>> {}
            unsafe impl AsBytesMut for Option<std::num::NonZero<$int>> {}
        )+
    };
}

integers!(u8, u16, u32, u64, u128, usize);
integers!(i8, i16, i32, i64, i128, isize);

macro_rules! atomic_integers {
    ($($atomic_int:ident => $cfg:tt),+) => {
        $(
            #[cfg(target_has_atomic = $cfg)]
            unsafe impl AsBytesMut for std::sync::atomic::$atomic_int {}
        )+
    };
}

atomic_integers!(AtomicU8 => "8", AtomicU16 => "16", AtomicU32 => "32", AtomicU64 => "64", AtomicUsize => "ptr");
atomic_integers!(AtomicI8 => "8", AtomicI16 => "16", AtomicI32 => "32", AtomicI64 => "64", AtomicIsize => "ptr");

// macro_rules! {
//     () => {

//     };
// }

// unsafe impl AsBytes for BorrowedFd<'_> {}
// unsafe impl AsBytes for OwnedFd {}
//

mod unsafe_bytes;

#[doc(inline)]
pub use unsafe_bytes::*;
// #[doc(inline)]
// pub use access::*;
