use std::{
    any::TypeId,
    array::TryFromSliceError,
    borrow::Cow,
    collections::VecDeque,
    ffi::{CStr, CString},
    fmt, hint, iter,
    mem::{self, MaybeUninit},
    num::NonZero,
    ops::Deref,
    ptr::NonNull,
    rc::Rc,
    sync::Arc,
};

use crate::mem::{AsBytes, AsBytesMut, Byte, as_bytes, as_bytes_mut};

/// A SPA String that contains either no NUL terminators, or ***precisely one*** at the end.
///
/// The exact representation of this is kinda unspecified, but it permits us to insert the NUL when needed,
/// and include it when needed, if it already exists.
///
/// One such use case is being able to serialize a [`str`] that contains no NUL, by simply inserting it,
/// while still being able to, zero-copy, turn a [`CStr`] into a [`SpaStr`], to turn it back into a [`CStr`].
///
/// We guarantee that the size of string, when including a NUL terminator, is less than or equal to [`super::MAX_SIZE`].
#[repr(transparent)]
pub struct SpaStr([u8]);

impl SpaStr {
    /// Returns whether this string contains a NUL terminator.
    #[inline]
    #[must_use]
    #[doc(alias = "is_nul_terminated")]
    pub const fn has_nul(&self) -> bool {
        match self.0.len().checked_sub(self.as_bytes().len()).unwrap() {
            0 => false,
            1 => true,
            _ => panic!("edge case detected!"),
        }
    }

    /// Returns the length of the string, excluding any NUL terminator.
    #[inline]
    #[must_use]
    pub const fn len(&self) -> usize {
        self.as_bytes().len()
    }

    /// Returns whether this string is empty.
    ///
    /// This does not include the NUL terminator, if there is any.
    #[inline]
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.as_bytes().is_empty()
    }

    /// Returns the length of the string, including a NUL terminator.
    ///
    /// # Validity
    ///
    /// This is advisory, the underlying string may not actually have a NUL
    /// terminator.
    #[inline]
    #[must_use]
    pub const fn len_with_nul(&self) -> NonZero<usize> {
        let len_with_nul = self.len().checked_add(1).unwrap();

        assert!(
            len_with_nul <= super::MAX_SIZE as usize,
            "something has gone horribly wrong",
        );

        NonZero::new(len_with_nul).unwrap()
    }

    /// Returns the underlying string, excluding any NUL terminator.
    #[inline]
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8] {
        // SAFETY: We know that the source buffer is at most `MAX_SIZE` bytes.
        unsafe {
            hint::assert_unchecked(self.0.len() <= super::MAX_SIZE as usize)
        };

        // NOTE: Fancy pattern matching to remove the trailing NUL, but only if there is one.
        let (body, _nul) = {
            let dec_value = match &self.0 {
                // NOTE: The total length is at least one, decrementing cannot overflow.
                [.., 0x00] => 1_usize,
                // NOTE: Either an empty buffer or one without a NUL.
                [] | [.., _] => 0_usize,
            };

            // SAFETY: When there's no NUL, `dec_value` is zero, so no overflow. When there is a NUL,
            //         `dec_value` is one, and since we need at least one byte to store the NUL,
            //         decrementing the language too, will not overflow.
            let nul_pos = unsafe { self.0.len().unchecked_sub(dec_value) };

            // SAFETY: We know that `nul_pos` is at most `self.0.len()`.
            //
            //         `split_at` and `split_at_unchecked`, and the above subtraction
            //         all for some reason fail to establish this relationship.
            unsafe { hint::assert_unchecked(nul_pos <= self.0.len()) };

            // NOTE: No need for the unsafe version, the above assertion does the work for us,
            //       and more. Honestly, `split_at_unchecked` would be better as a `split_at_checked.unwrap()`.
            let (body, nul) = self.0.split_at(nul_pos);

            // SAFETY: We know that the length of the body is at most `MAX_SIZE - 1`, as we need
            //         one byte to store the NUL.
            unsafe {
                hint::assert_unchecked(body.len() < super::MAX_SIZE as usize)
            };

            // SAFETY: We know that the body's length when incremented is at most `MAX_SIZE`.
            unsafe {
                hint::assert_unchecked(
                    body.len().unchecked_add(1) <= super::MAX_SIZE as usize,
                )
            };

            // SAFETY: We know that the body's length, incremented, is at most one byte more than
            //         the buffer length.
            unsafe {
                hint::assert_unchecked(
                    body.len().unchecked_add(1).unchecked_sub(self.0.len())
                        <= 1,
                )
            };

            // SAFETY: Same as above, more or less.
            unsafe {
                hint::assert_unchecked(
                    body.len().unchecked_add(1) >= self.0.len(),
                )
            };

            if let [nul] = nul {
                // SAFETY: If the nul buffer isn't empty, then we know that byte is `0`.
                unsafe { hint::assert_unchecked(*nul == 0x00) };

                // SAFETY: Same as the above, but for the last element in the source buffer.
                unsafe {
                    hint::assert_unchecked(
                        self.0.last().copied().unwrap_unchecked() == 0x00,
                    )
                };

                // SAFETY: We know they're the same address.
                unsafe {
                    hint::assert_unchecked(
                        (&raw const *nul).offset_from_unsigned(
                            self.0.last().unwrap_unchecked(),
                        ) == 0,
                    )
                }
            }

            unsafe {
                hint::assert_unchecked(
                    body.len().unchecked_add(1) - 1 == body.len(),
                )
            };

            (body, nul)
        };

        body
    }

    /// Returns the underlying string mutably, excluding any NUL terminator.
    ///
    /// # Safety
    ///
    /// The caller must ensure that the buffer contains no NULs when the borrow ends.
    ///
    /// If you need a safe variant, see [`SpaStr::as_nonzero_bytes_mut`].
    #[inline]
    #[must_use]
    #[allow(unused_unsafe)]
    pub const unsafe fn as_bytes_mut(&mut self) -> &mut [u8] {
        let len = self.as_bytes().len();

        // SAFETY: The caller ensures this is safe.
        unsafe { self.0.split_at_mut(len).0 }
    }

    /// Returns the underlying string as a slice of [`NonZero`] bytes, excluding any NUL terminator.
    #[inline]
    #[must_use]
    pub const fn as_nonzero_bytes(&self) -> &[NonZero<u8>] {
        // SAFETY: We know that the slice up to the NUL terminator, if any,
        //         is entirely nonzero.
        unsafe {
            (&raw const *self.as_bytes() as *const [NonZero<u8>])
                .as_ref_unchecked()
        }
    }

    /// Returns the underlying string as a mutable slice of [`NonZero`] bytes, excluding any NUL terminator.
    #[inline]
    #[must_use]
    pub const fn as_nonzero_bytes_mut(&mut self) -> &mut [NonZero<u8>] {
        // SAFETY: We're returning a slice of nonzero bytes, and we know
        //         that the slice up to the NUL terminator is all nonzero.
        //
        //         Since NUL is zero, it is impossible to safely insert
        //         a zeroed byte.
        unsafe {
            (&raw mut *self.as_bytes_mut() as *mut [NonZero<u8>])
                .as_mut_unchecked()
        }
    }

    /// Get this [`SpaStr`] as a [`CStr`].
    ///
    /// # Returns
    ///
    /// Returns [`None`] if this string lacks a NUL terminator.
    #[inline]
    #[must_use]
    pub const fn as_c_str(&self) -> Option<&CStr> {
        if self.has_nul() {
            let length = self.len_with_nul().get();

            let (c_str, []) = self.0.split_at(length) else {
                panic!("this is just advisory");
            };

            // SAFETY: We know the underlying buffer is is a valid C string.
            Some(unsafe { CStr::from_bytes_with_nul_unchecked(c_str) })
        } else {
            None
        }
    }

    /// Allocate a box on the heap of this SPA string, including a NUL terminator.
    #[inline(never)]
    #[must_use]
    #[track_caller]
    fn make_box(&self) -> Box<[u8]> {
        let bytes = self.as_bytes();
        let mut alloc = Box::new_uninit_slice(bytes.len().strict_add(1));

        alloc[..bytes.len()].write_copy_of_slice(bytes);
        alloc[bytes.len()].write(0x00);

        // SAFETY: We've successfully initialized the buffer.
        unsafe { alloc.assume_init() }
    }

    /// Allocate a refcounted heap value of this SPA string, including a NUL terminator.
    #[inline(never)]
    #[must_use]
    #[track_caller]
    fn make_rc(&self) -> Rc<[u8]> {
        let bytes = self.as_bytes();
        let mut alloc = Rc::new_uninit_slice(bytes.len().strict_add(1));

        // SAFETY: We know the allocation is unique.
        let buffer = unsafe { Rc::get_mut(&mut alloc).unwrap_unchecked() };

        buffer[..bytes.len()].write_copy_of_slice(bytes);
        buffer[bytes.len()].write(0x00);

        // SAFETY: We know the buffer is initialized.
        unsafe { alloc.assume_init() }
    }

    /// Allocate an atomic refcounted heap value of this SPA string, including a NUL terminator.
    #[inline(never)]
    #[must_use]
    #[track_caller]
    fn make_arc(&self) -> Arc<[u8]> {
        let bytes = self.as_bytes();
        let alloc = Arc::new_uninit_slice(bytes.len().strict_add(1));

        let alloc = {
            // NOTE: I would be really surprised if the provenance for this was fucked up.
            let alloc = Arc::into_raw(alloc).cast_mut();

            // SAFETY: We know the allocation is unique.
            let buffer = unsafe { alloc.as_mut_unchecked() };

            buffer[..bytes.len()].write_copy_of_slice(bytes);
            buffer[bytes.len()].write(0x00);

            // SAFETY: We know `alloc` is a valid `Arc`.
            unsafe { Arc::from_raw(alloc) }
        };

        // SAFETY: We know the buffer is initialized.
        unsafe { alloc.assume_init() }
    }

    /// Allocate some heap allocated thing with a NUL terminator, and add additional hints.
    #[inline(always)]
    #[must_use]
    #[track_caller]
    unsafe fn make_heap<'a, H, A, As, AsI>(
        &'a self,
        alloc: A,
        additional_assertions: As,
    ) -> H
    where
        H: Deref<Target = [u8]>,
        A: FnOnce(&'a SpaStr) -> H,
        As: FnOnce(&mut H) -> AsI,
        AsI: IntoIterator<Item = bool>,
    {
        let mut heap_alloc = alloc(self);

        // SAFETY: We know that they have the same length when including
        //         a NUL.
        unsafe {
            hint::assert_unchecked(
                heap_alloc.len() == self.len_with_nul().get(),
            )
        };

        // SAFETY: We know the heap buffer is not empty.
        unsafe { hint::assert_unchecked(!heap_alloc.is_empty()) };

        // SAFETY: We know the last byte is NUL.
        unsafe {
            hint::assert_unchecked(heap_alloc.last().copied().unwrap() == 0x00)
        };

        // SAFETY: We know that the `strlen + 1` is equal to the
        //         size of the buffer.
        unsafe {
            hint::assert_unchecked(
                CStr::from_ptr(heap_alloc.as_ptr().cast())
                    .count_bytes()
                    .unchecked_add(1)
                    == heap_alloc.len(),
            )
        };

        // SAFETY: The caller ensures this is fine.
        additional_assertions(&mut heap_alloc)
            .into_iter()
            .for_each(|cond| unsafe { hint::assert_unchecked(cond) });

        heap_alloc
    }
}

unsafe impl AsBytes for SpaStr {}

impl<const N: usize> TryFrom<&SpaStr> for [u8; N] {
    type Error = TryFromSliceError;

    #[inline(always)]
    #[track_caller]
    fn try_from(value: &SpaStr) -> Result<Self, Self::Error> {
        let bytes = value.as_bytes();

        if N == bytes.len().strict_add(1) {
            let mut output = [MaybeUninit::uninit(); N];

            output[..bytes.len()].write_copy_of_slice(bytes);
            output[bytes.len()].write(0x00);

            // SAFETY: We initialized the entire array.
            Ok(unsafe { mem::transmute_copy(&output) })
        } else {
            // SAFETY: We're conjuring a ZST slice, this is always fine.
            let slice = unsafe {
                NonNull::slice_from_raw_parts(
                    NonNull::<()>::dangling(),
                    bytes.len().strict_add(1),
                )
                .as_ref()
            };
            // NOTE: We're essentially reconstructing the error that we would expect.
            let array: Result<[(); N], _> = slice.try_into();

            // NOTE: This should never panic.
            Err(array.unwrap_err())
        }
    }
}

impl<const N: usize> TryFrom<&SpaStr> for [i8; N] {
    type Error = TryFromSliceError;

    #[inline(always)]
    #[track_caller]
    #[allow(clippy::toplevel_ref_arg)]
    fn try_from(value: &SpaStr) -> Result<Self, Self::Error> {
        let ref array: [u8; N] = value.try_into()?;

        // SAFETY: `i8` and `u8` are POD.
        Ok(unsafe { mem::transmute_copy(array) })
    }
}

impl<const N: usize> TryFrom<&SpaStr> for [Byte; N] {
    type Error = TryFromSliceError;

    #[inline(always)]
    #[track_caller]
    #[allow(clippy::toplevel_ref_arg)]
    fn try_from(value: &SpaStr) -> Result<Self, Self::Error> {
        let ref array: [u8; N] = value.try_into()?;

        // SAFETY: `i8` and `u8` are POD.
        Ok(unsafe { mem::transmute_copy(array) })
    }
}

const TRUSTED_ARRAY_TYPES: &[TypeId] =
    &[TypeId::of::<u8>(), TypeId::of::<i8>(), TypeId::of::<Byte>()];

#[inline(always)]
fn is_trusted_array<T>() -> bool
where
    T: 'static + ?Sized,
{
    TRUSTED_ARRAY_TYPES
        .iter()
        .copied()
        .any(|trusted| trusted == TypeId::of::<T>())
}

impl<'a, const N: usize, T> TryFrom<&'a SpaStr> for Rc<[T; N]>
where
    T: 'static,
    &'a SpaStr: TryInto<[T; N], Error: Into<TryFromSliceError>> + Into<Rc<[T]>>,
{
    type Error = TryFromSliceError;

    #[inline(always)]
    #[track_caller]
    fn try_from(value: &'a SpaStr) -> Result<Self, Self::Error> {
        if is_trusted_array::<T>() {
            // NOTE: We're discarding the value, as we don't need to spill onto the stack.
            value.try_into().map(|_: [T; N]| ()).map_err(Into::into)?;

            let alloc: Rc<[T]> = value.into();

            // SAFETY: We trust `T`, thus we know the allocation to have a length of `N`.
            unsafe { hint::assert_unchecked(alloc.len() == N) };

            // SAFETY: See above.
            Ok(unsafe { Rc::from_raw(Rc::into_raw(alloc).cast()) })
        } else {
            value.try_into().map(Rc::new).map_err(Into::into)
        }
    }
}

impl<'a, const N: usize, T> TryFrom<&'a SpaStr> for Arc<[T; N]>
where
    T: 'static,
    &'a SpaStr:
        TryInto<[T; N], Error: Into<TryFromSliceError>> + Into<Arc<[T]>>,
{
    type Error = TryFromSliceError;

    #[inline(always)]
    #[track_caller]
    fn try_from(value: &'a SpaStr) -> Result<Self, Self::Error> {
        if is_trusted_array::<T>() {
            // NOTE: We're discarding the value, as we don't need to spill onto the stack.
            value.try_into().map(|_: [T; N]| ()).map_err(Into::into)?;

            let alloc: Arc<[T]> = value.into();

            // SAFETY: We trust `T`, so we know the length is `N`.
            unsafe { hint::assert_unchecked(alloc.len() == N) };

            // SAFETY: See above.
            Ok(unsafe { Arc::from_raw(Arc::into_raw(alloc).cast()) })
        } else {
            value.try_into().map(Arc::new).map_err(Into::into)
        }
    }
}

impl<'a, const N: usize, T> TryFrom<&'a SpaStr> for Box<[T; N]>
where
    T: 'static,
    &'a SpaStr:
        TryInto<[T; N], Error: Into<TryFromSliceError>> + Into<Box<[T]>>,
{
    type Error = TryFromSliceError;

    #[inline(always)]
    #[track_caller]
    fn try_from(value: &'a SpaStr) -> Result<Self, Self::Error> {
        if is_trusted_array::<T>() {
            // NOTE: We're discarding the value, as we don't need to spill onto the stack.
            value.try_into().map(|_: [T; N]| ()).map_err(Into::into)?;

            let alloc: Box<[T]> = value.into();

            // SAFETY: We trust `T`, we know its length to be `N`.
            unsafe { hint::assert_unchecked(alloc.len() == N) };

            // SAFETY: See above.
            Ok(unsafe { Box::from_raw(Box::into_raw(alloc).cast()) })
        } else {
            value.try_into().map(Box::new).map_err(Into::into)
        }
    }
}

impl From<&SpaStr> for Box<[u8]> {
    #[inline(always)]
    #[track_caller]
    fn from(spa_str: &SpaStr) -> Self {
        // SAFETY: We know this is valid.
        unsafe { spa_str.make_heap(SpaStr::make_box, |_| iter::empty()) }
    }
}

impl From<&SpaStr> for Rc<[u8]> {
    #[inline(always)]
    #[track_caller]
    fn from(spa_str: &SpaStr) -> Self {
        // SAFETY: We know this is valid.
        unsafe {
            spa_str.make_heap(SpaStr::make_rc, |rc| {
                [Rc::weak_count(rc) == 0, Rc::strong_count(rc) == 1]
            })
        }
    }
}

impl From<&SpaStr> for Arc<[u8]> {
    #[inline(always)]
    #[track_caller]
    fn from(spa_str: &SpaStr) -> Self {
        // SAFETY: We know this is valid.
        unsafe { spa_str.make_heap(SpaStr::make_arc, |_arc| []) }
    }
}

impl From<&SpaStr> for Box<[i8]> {
    #[inline(always)]
    #[track_caller]
    fn from(value: &SpaStr) -> Self {
        let bytes: Box<[u8]> = value.into();

        // SAFETY: `i8` and `u8` are POD.
        unsafe { Box::from_raw(Box::into_raw(bytes) as *mut [i8]) }
    }
}

impl From<&SpaStr> for Arc<[i8]> {
    #[inline(always)]
    fn from(value: &SpaStr) -> Self {
        let bytes: Arc<[u8]> = value.into();

        // SAFETY: `i8` and `u8` are POD.
        unsafe { Arc::from_raw(Arc::into_raw(bytes) as *const [i8]) }
    }
}

impl From<&SpaStr> for Rc<[i8]> {
    #[inline(always)]
    fn from(value: &SpaStr) -> Self {
        let bytes: Rc<[u8]> = value.into();

        // SAFETY: `i8` and `u8` are POD.
        unsafe { Rc::from_raw(Rc::into_raw(bytes) as *const [i8]) }
    }
}

impl From<&SpaStr> for Box<[Byte]> {
    #[inline(always)]
    #[track_caller]
    fn from(value: &SpaStr) -> Self {
        let bytes: Box<[u8]> = value.into();

        // SAFETY: `Byte` and `u8` are POD.
        unsafe { Box::from_raw(Box::into_raw(bytes) as *mut [Byte]) }
    }
}

impl From<&SpaStr> for Arc<[Byte]> {
    #[inline(always)]
    fn from(value: &SpaStr) -> Self {
        let bytes: Arc<[u8]> = value.into();

        // SAFETY: `Byte` and `u8` are POD.
        unsafe { Arc::from_raw(Arc::into_raw(bytes) as *const [Byte]) }
    }
}

impl From<&SpaStr> for Rc<[Byte]> {
    #[inline(always)]
    fn from(value: &SpaStr) -> Self {
        let bytes: Rc<[u8]> = value.into();

        // SAFETY: `Byte` and `u8` are POD.
        unsafe { Rc::from_raw(Rc::into_raw(bytes) as *const [Byte]) }
    }
}

impl<'a, T> From<&'a SpaStr> for Vec<T>
where
    &'a SpaStr: Into<Box<[T]>>,
{
    #[inline(always)]
    #[track_caller]
    fn from(value: &'a SpaStr) -> Self {
        Vec::from(value.into())
    }
}

impl<'a, T> From<&'a SpaStr> for VecDeque<T>
where
    &'a SpaStr: Into<Vec<T>>,
{
    #[inline(always)]
    #[track_caller]
    fn from(value: &'a SpaStr) -> Self {
        VecDeque::from(value.into())
    }
}

impl From<&SpaStr> for Box<CStr> {
    #[inline(always)]
    #[track_caller]
    fn from(value: &SpaStr) -> Self {
        let bytes: Box<[u8]> = value.into();

        // SAFETY: We know that the above allocation is a valid C string.
        unsafe { Box::from_raw(Box::into_raw(bytes) as *mut CStr) }
    }
}

impl From<&SpaStr> for Rc<CStr> {
    #[inline(always)]
    #[track_caller]
    fn from(value: &SpaStr) -> Self {
        let bytes: Rc<[u8]> = value.into();

        // SAFETY: We know that the above allocation is a valid C string.
        unsafe { Rc::from_raw(Rc::into_raw(bytes) as *const CStr) }
    }
}

impl From<&SpaStr> for Arc<CStr> {
    #[inline(always)]
    #[track_caller]
    fn from(value: &SpaStr) -> Self {
        let bytes: Arc<[u8]> = value.into();

        // SAFETY: We know that the above allocation is a valid C string.
        unsafe { Arc::from_raw(Arc::into_raw(bytes) as *const CStr) }
    }
}

impl From<&SpaStr> for CString {
    #[inline(always)]
    #[track_caller]
    fn from(value: &SpaStr) -> Self {
        let c_str: Box<CStr> = value.into();

        c_str.into()
    }
}

impl<'a> From<&'a SpaStr> for Cow<'a, CStr> {
    #[inline(always)]
    #[track_caller]
    fn from(value: &'a SpaStr) -> Self {
        match value.as_c_str() {
            Some(c_str) => Cow::Borrowed(c_str),
            None => Cow::Owned(value.into()),
        }
    }
}

impl fmt::Debug for SpaStr {
    #[track_caller]
    fn fmt(
        &self,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        // FIXME: Avoid an allocation.
        String::from_utf8_lossy(self.as_bytes()).fmt(f)
    }
}

/// An enum that represents where a NUL terminator was found
/// within some byte buffer.
///
/// This mainly exists to encapsulate the various possible, equally valid (depending on context),
/// ways we wish to construct a [`SpaStr`].
///
/// Since a [`SpaStr`] has at most one NUL at the end of it, we want to be able to construct one
/// from say, [`str`]s we know to contain no NULs (and thus, when are being encoded, we can just automatically insert one),
/// or [`CStr`]s without any checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NulPos {
    /// There was no NUL terminator within the buffer, at all.
    ///
    /// Buffers that return this variant cannot be safely reinterpreted as
    /// a [`CStr`], nor can any subslice of them, as there exists no NUL terminator.
    None {
        /// The length, in bytes, of the original input buffer.
        len: usize,
    },

    /// There's only one NUL terminator, at the end of the buffer.
    ///
    /// Buffers that return this variant can be safely reinterpreted as a
    /// [`CStr`].
    End {
        /// The byte position of the NUL, which is the length of the buffer,
        /// in bytes, minus one.
        pos: usize,
    },

    /// There's a NUL terminator that lies before the end of the buffer.
    ///
    /// Buffers that return this variant may have more than one NUL terminator,
    /// but we know for certain that we can reinterpet the buffer, up to and including
    /// the position, as a [`CStr`].
    Interior {
        /// The byte position of the first NUL in the buffer.
        pos: usize,
    },
}

impl NulPos {
    /// Create a [`NulPos::None`], given some length.
    #[inline(always)]
    #[must_use]
    pub const fn none(len: usize) -> NulPos {
        NulPos::None { len }
    }

    /// Returns whether this is a [`NulPos::None`].
    #[inline(always)]
    #[must_use]
    pub const fn is_none(&self) -> bool {
        matches!(self, NulPos::None { .. })
    }

    /// Create a [`NulPos::End`], given some NUL position.
    #[inline(always)]
    #[must_use]
    pub const fn end(pos: usize) -> NulPos {
        NulPos::End { pos }
    }

    /// Returns whether this is a [`NulPos::End`].
    #[inline(always)]
    #[must_use]
    pub const fn is_end(&self) -> bool {
        matches!(self, NulPos::End { .. })
    }

    /// Create a [`NulPos::Interior`], given some NUL position.
    #[inline(always)]
    #[must_use]
    pub const fn interior(pos: usize) -> NulPos {
        NulPos::Interior { pos }
    }

    /// Returns whether this is a [`NulPos::Interior`].
    #[inline(always)]
    #[must_use]
    pub const fn is_interior(&self) -> bool {
        matches!(self, NulPos::Interior { .. })
    }

    /// Returns the position of the NUL terminator in the byte buffer,
    /// if there was one.
    #[inline(always)]
    #[must_use]
    pub const fn pos(&self) -> Option<usize> {
        match self {
            NulPos::None { .. } => None,
            NulPos::End { pos } | NulPos::Interior { pos } => Some(*pos),
        }
    }

    /// Returns the length of a byte buffer, including a NUL terminator,
    /// that would be needed to allocate a [`std::ffi::CStr`] for the
    /// original buffer.
    ///
    /// # Returns
    ///
    /// We do not differentiate between buffers that have a NUL, or lack one. We return [`None`]
    /// if the size calculation overflowed.
    #[inline(always)]
    #[must_use]
    pub const fn size_needed(&self) -> Option<NonZero<usize>> {
        match self.len_with_nul() {
            Ok(needed) | Err(needed) => needed,
        }
    }

    /// Returns the length of the byte buffer, including the NUL terminator.
    ///
    /// # Returns
    ///
    /// The actual calculated value is a [`Option<NonZero<usize>>`]. The [`None`] case
    /// indicates that there was an overflow when calculating the length.
    ///
    /// However, we wrap the actual value within a [`Result`].
    ///
    /// For [`NulPos::None`], we return `Err(length)`, as while the original source
    /// buffer may lack a NUL, we may be intending to allocate a buffer than can store
    /// the NUL terminator.
    ///
    /// For [`NulPos::End`] and [`NulPos::Interior`], we return `Ok(length)`, as the original
    /// source buffer *does* contain a NUL.
    #[inline(always)]
    pub const fn len_with_nul(
        &self
    ) -> Result<Option<NonZero<usize>>, Option<NonZero<usize>>> {
        let with_nul = match self.len_without_nul().checked_add(1) {
            Some(with_nul) => Some(NonZero::new(with_nul).unwrap()),
            None => None,
        };

        match self {
            NulPos::None { .. } => Err(with_nul),
            NulPos::End { .. } | NulPos::Interior { .. } => Ok(with_nul),
        }
    }

    /// Returns the length of the byte buffer, excluding the NUL terminator.
    ///
    /// # Returns
    ///
    /// This does *not* differentiate on buffers who lack a NUL terminator,
    /// as as far as we're considered, their length is by definition,
    /// a length without a NUL terminator.
    ///
    /// For [`NulPos::None`], we return the `len` field.
    ///
    /// For [`NulPos::End`] and [`NulPos::Interior`], we return the position
    /// of the NUL terminator, as getting the subslice up to that offset,
    /// but not including it (so a `..pos` range), gives you a buffer without
    /// any NUL terminator, where the length is `pos`.
    #[inline(always)]
    #[must_use]
    pub const fn len_without_nul(&self) -> usize {
        match self {
            NulPos::None {
                len: len_without_nul,
            }
            | NulPos::End {
                pos: len_without_nul,
            }
            | NulPos::Interior {
                pos: len_without_nul,
            } => *len_without_nul,
        }
    }

    /// Searches the specified byte buffer for a NUL terminator.
    ///
    /// # Specialization and Performance
    ///
    /// Currently we do *not* specialize. So if you pass along a [`CStr`],
    /// whatever implementation of `memchr` the Rust standard library uses,
    /// will be called.
    ///
    /// Realistically this shouldn't be an issue, as SPA PODs are pretty small,
    /// and the strings within them are also, pretty small. Still, this need to be
    /// kept in mind.
    #[inline(always)]
    #[must_use]
    pub const fn search<B>(bytes: &B) -> NulPos
    where
        B: AsBytes + ?Sized,
    {
        NulPos::search_inner(as_bytes(bytes))
    }

    /// Searches the specified mutable byte buffer for a NUL terminator.
    ///
    /// This is the mutable variant of [`NulPos::search`]. See that for
    /// details on usage.
    ///
    /// This is equivalent to `NulPos::search(&*as_bytes_mut(bytes))`.
    #[inline(always)]
    #[must_use]
    pub const fn search_mut<B>(bytes: &mut B) -> NulPos
    where
        B: AsBytesMut + ?Sized,
    {
        NulPos::search_inner(as_bytes_mut(bytes))
    }

    #[inline(always)]
    #[must_use]
    #[track_caller]
    const fn search_inner(bytes: &[Byte]) -> NulPos {
        let len_with_nul =
            match CStr::from_bytes_until_nul(Byte::as_u8_slice(bytes)) {
                Ok(c_str) => Some({
                    let nul_pos = c_str.count_bytes();

                    // SAFETY: We know for a fact the `nul_pos + 1 <= len`.
                    //         Additionally, we know it to be nonzero.
                    let len_with_nul = unsafe {
                        NonZero::new_unchecked(nul_pos.unchecked_add(1))
                    };

                    // SAFETY: Same as above.
                    unsafe {
                        hint::assert_unchecked(
                            len_with_nul.get() <= bytes.len(),
                        )
                    };

                    // SAFETY: We know for a fact that the byte at  `len_with_nul - 1` is zero.
                    unsafe {
                        hint::assert_unchecked(
                            bytes[len_with_nul.get().strict_sub(1)].get()
                                == 0x00,
                        )
                    };

                    len_with_nul
                }),
                Err(_) => None,
            };

        match len_with_nul {
            Some(len_with_nul) => {
                // SAFETY: We know that if there's a NUL terminator, that the length of our buffer is nonzero.
                let buffer_len = NonZero::new(bytes.len())
                    .expect("the buffer length should be nonzero");

                assert!(
                    len_with_nul.get() <= buffer_len.get(),
                    "the length including a nul must be within the bounds of the original length",
                );

                let position = len_with_nul.get().strict_sub(1);

                assert!(
                    bytes[position].get() == 0x00,
                    "the last byte in the buffer with the nul, must be nul",
                );

                match (
                    len_with_nul.get() < buffer_len.get(),
                    len_with_nul.get() > buffer_len.get(),
                ) {
                    (false, false) => {
                        assert!(
                            len_with_nul.get() == buffer_len.get(),
                            "the length with nul in this case must be equal to the original buffer length",
                        );

                        NulPos::End { pos: position }
                    },
                    (true, false) => {
                        assert!(
                            len_with_nul.get() < buffer_len.get(),
                            "the length with nul in this case must be less than the original buffer length",
                        );

                        NulPos::Interior { pos: position }
                    },
                    (_, true) => unreachable!(),
                }
            },
            None => NulPos::None { len: bytes.len() },
        }
    }
}
