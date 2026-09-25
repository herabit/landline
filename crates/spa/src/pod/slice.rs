use std::{
    array,
    borrow::{Borrow, BorrowMut},
    fmt, hint,
    ops::{Index, IndexMut},
    ptr, slice,
};

use crate::{
    mem::{AsBytes, AsBytesMut, Byte},
    pod::{
        AsPrimPod, AsPrimPodMut, PodError, PrimPod, SpaHeader, kind::SpaKind,
    },
};

use private::MapIndex as _;

/// A slice of SPA POD primitives.
///
///
/// # Safety
///
/// We guarantee that the size of the underlying slice, in bytes, is less than or equal to [`super::MAX_SIZE`].
///
/// Transitively, this implies that no valid [`SpaSlice`] will have a length exceeding [`SpaSlice::MAX_LEN`], which
/// is a constant that is dependent on the size of the primitive.
///
/// For zero-sized primitives, however, the maximum length is [`u32::MAX`]. Since we require our types to be encodable
/// as [`u32`]s, we restrict ourself to slices whose lengths, and sizes, are encodable as a [`u32`]. So while the byte
/// size will always satisfy this requirement, no matter the length, we cannot say the same for the length. As such,
/// the maximum length is [`u32::MAX`].
///
/// Failure to comply with these invariants is considered undefined behavior.
#[repr(transparent)]
pub struct SpaSlice<P>([P])
where
    P: PrimPod;

impl<P> SpaSlice<P>
where
    P: PrimPod,
{
    /// The maximum length this kind of slice.
    pub const MAX_LEN: u32 = if P::SIZE == 0 {
        u32::MAX
    } else if super::MAX_SIZE.is_multiple_of(P::SIZE) {
        super::MAX_SIZE / P::SIZE
    } else {
        panic!("we somehow have an invalid state, fun.");
    };

    /// An empty [`SpaSlice`].
    #[inline(always)]
    #[must_use]
    pub const fn empty() -> &'static mut Self {
        const {
            match Self::from_prims_mut::<P>(&mut []) {
                Ok(empty) => empty,
                Err(..) => panic!("failed to create an empty slice"),
            }
        }
    }

    /// Returns the length of this slice, as a [`u32`].
    #[inline(always)]
    #[must_use]
    pub const fn len(&self) -> u32 {
        // SAFETY: We have a safety requirement that the size of the slice, in bytes, mustn't
        //         exceed `MAX_SIZE`.
        unsafe {
            hint::assert_unchecked(
                size_of_val(self) <= super::MAX_SIZE as usize,
            )
        };

        // SAFETY: Same as above, except for the maximum length of a SPA array of `P`s, which is
        //         derived from `MAX_SIZE`.
        unsafe {
            hint::assert_unchecked(self.0.len() <= Self::MAX_LEN as usize)
        };

        self.0.len() as u32
    }

    /// Returns whether this slice is empty.
    #[inline(always)]
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Borrow the underlying slice of primitives.
    #[inline(always)]
    #[must_use]
    pub const fn as_prims(&self) -> &[P] {
        // SAFETY: This is equivalent to `&self.0`, but it gives the compiler
        //         additional information.
        unsafe {
            slice::from_raw_parts(
                (&raw const *self).cast::<P>(),
                self.len() as usize,
            )
        }
    }

    /// Mutably borrow the underlying slice of primitives.
    #[inline(always)]
    #[must_use]
    pub const fn as_prims_mut(&mut self) -> &mut [P] {
        // SAFETY: This is equivalent to `&mut self.0`, but it gives the
        //         compiler additional information.
        unsafe {
            slice::from_raw_parts_mut(
                (&raw mut *self).cast::<P>(),
                self.len() as usize,
            )
        }
    }

    /// Attempt to construct a [`SpaSlice`] from a slice of things that can be
    /// treated as primitives.
    ///
    /// # Returns
    ///
    /// Returns an error if the slice is not representable in the pipewire protocol.
    #[inline(always)]
    pub const fn from_prims<T>(prims: &[T]) -> Result<&Self, PodError>
    where
        T: AsPrimPod<P>,
    {
        const { assert!(size_of::<T>() == P::SIZE as usize, "size mismatch") };

        if prims.len() <= Self::MAX_LEN as usize {
            // SAFETY: `T` implements `AsPrimPod`, and we know its length to lie within our limits.
            Ok(
                unsafe {
                    (&raw const *prims as *const Self).as_ref_unchecked()
                },
            )
        } else {
            Err(PodError::InvalidSize)
        }
    }

    /// Attempt to construct a mutable [`SpaSlice`] from a slice of things that
    /// can be treated as primitives.
    ///
    /// # Returns
    ///
    /// Returns an error if the slice is not representable in the pipewire protocol.
    #[inline(always)]
    pub const fn from_prims_mut<T>(
        prims: &mut [T]
    ) -> Result<&mut Self, PodError>
    where
        T: AsPrimPodMut<P>,
    {
        const { assert!(size_of::<T>() == P::SIZE as usize, "size mismatch") };

        if prims.len() <= Self::MAX_LEN as usize {
            // SAFETY: `T` implements `AsPrimPodMut`, and we know its length to lie within our limits.
            Ok(unsafe { (&raw mut *prims as *mut Self).as_mut_unchecked() })
        } else {
            Err(PodError::InvalidSize)
        }
    }

    /// Attempt to decode a SPA slice from a byte buffer.
    #[inline(always)]
    pub const fn decode(bytes: &[Byte]) -> Result<(&Self, &[Byte]), PodError> {
        let (pod_header, elem_header, rest) = match bytes
            .split_first_chunk::<{ size_of::<[SpaHeader; 2]>() }>()
        {
            Some((headers, rest)) => {
                let (pod_header, elem_header) =
                    headers.split_at(size_of::<SpaHeader>());

                let (pod_header, elem_header) = (
                    SpaHeader::from_bytes(pod_header).unwrap(),
                    SpaHeader::from_bytes(elem_header).unwrap(),
                );

                (pod_header, elem_header, rest)
            },
            None => {
                hint::cold_path();
                return Err(PodError::InsufficientSpace);
            },
        };

        if pod_header.kind != SpaKind::ARRAY || elem_header.kind != P::KIND {
            // If we're not parsing an array of `P`, return an error.
            hint::cold_path();

            return Err(PodError::InvalidKind);
        } else if pod_header.size > super::MAX_SIZE
            || elem_header.size != P::SIZE
        {
            // If the total size exceeds the max size, or the element header's size doesn't match
            // what we expect, we error.
            //
            // Note that since `MAX_SIZE` is already a multiple of eight, we don't need to round
            // up to the next multiple of eight for the above pod size check, as if the size is less than
            // or equal to `MAX_SIZE`, then it rounded up to the next multiple of eight will also be less than
            // or equal to `MAX_SIZE`.
            hint::cold_path();

            return Err(PodError::InvalidSize);
        }

        let Some(array_size) =
            pod_header.size.checked_sub(size_of::<SpaHeader>() as u32)
        else {
            // If the size of the POD cannot store the header describing the element type,
            // hard error.
            hint::cold_path();

            return Err(PodError::InvalidSize);
        };

        let array_len = if P::SIZE == 0 {
            // For zero sized primitives, we want to ensure that the size of the array is also, well, zero.
            //
            // It doesn't make sense for this to not be the case, and I think it's safe to assume any
            // POD with a zero-sized element, but not a zero sized size (excluding the element header),
            // is malformed, or potentially malicious.
            //
            // I'm making note of this assumption so that it's easier to fix if it ends up being problematic.
            if array_size == 0 {
                // For zero sized elements, we cannot derive a length from the size. But we won't hard error, either,
                // so we're doing what libpipewire does, and just return a slice with zero elements.
                0
            } else {
                hint::cold_path();

                // Malformed packet, how fun.
                return Err(PodError::InvalidSize);
            }
        } else {
            // For primitives whose size isn't zero, we're going to assume that anyone providing us an array whose size,
            // excluding the element header, is not exactly divisible by the element size, is malformed.
            //
            // I cannot think of any code patterns that would crate a size field of this nature within reasonable code.
            // I may, however, be wrong, so it's best to write down this assumption in case it ceases to be accurate.
            if array_size.is_multiple_of(P::SIZE) {
                let len = array_size / P::SIZE;
                // SAFETY: We know that `array_size` is evenly divisible by `P::SIZE`, and that `P::SIZE` is nonzero,
                //         so we can safely inform the compiler that we can reconstruct the value from the multiplication
                //         of the quotient and the element size.
                unsafe {
                    hint::assert_unchecked(
                        len.unchecked_mul(P::SIZE) == array_size,
                    )
                };

                len
            } else {
                hint::cold_path();

                // Malformed packet, how fun.
                return Err(PodError::InvalidSize);
            }
        };

        // FIXME: Use unsafe? Due to the checks above, particularly when we construct `array_size`,
        //        we know this will never overflow, but I doubt LLVM will be able to infer as such...
        //        maybe.. Anyways, maybe use unsafe if this proves to be problematic.
        let padded_size = array_size.next_multiple_of(8);

        // NOTE: While `body` contains any padding, we're simply going to ignore it when returning our values.
        //
        //       Why? Simplifies things tremendously, and nobody really wants to actually touch the padding,
        //       realistically.
        let Some((body, rest)) = rest.split_at_checked(padded_size as usize)
        else {
            hint::cold_path();

            // If we cannot get the body of the slice, we've run into issues.
            return Err(PodError::InsufficientSpace);
        };

        // SAFETY: We know that `P` is 1-byte aligned, and that we have sufficient space for `array_len` elements
        //         of `P`.
        let slice = unsafe {
            let slice = ptr::slice_from_raw_parts(
                (&raw const *body).cast::<P>(),
                array_len as usize,
            );

            (slice as *const Self).as_ref_unchecked()
        };

        Ok((slice, rest))
    }

    /// Attempt to mutably decode a primitive slice.
    #[inline(always)]
    pub const fn decode_mut(
        bytes: &mut [Byte]
    ) -> Result<(&mut Self, &mut [Byte]), PodError> {
        // NOTE: I'm lazy and I just want to implement this in terms of the immutable one.
        let (slice_offset, slice_len, rest_offset, rest_len) =
            match Self::decode(&*bytes) {
                Ok((slice, rest)) => {
                    // SAFETY: We know that `slice` is derived from `bytes`,
                    //         and is not before the start of `bytes`.
                    let slice_offset = unsafe {
                        (&raw const *slice)
                            .byte_offset_from_unsigned(&raw const *bytes)
                    };

                    // SAFETY: We know that `rest` is derived from `bytes`,
                    //         and is not before the start of `bytes`.
                    let rest_offset = unsafe {
                        (&raw const *slice)
                            .byte_offset_from_unsigned(&raw const *bytes)
                    };

                    (slice_offset, slice.len(), rest_offset, rest.len())
                },
                Err(err) => return Err(err),
            };

        let slice = {
            // SAFETY: We know that `slice_offset` will not overflow, and that the pointer will lie within
            //         `bytes`.
            let data =
                unsafe { (&raw mut *bytes).cast::<P>().byte_add(slice_offset) };
            let slice = ptr::slice_from_raw_parts_mut(data, slice_len as usize);

            // SAFETY: We know that there's no aliased mutability within `slice`, and that it is a valid
            //         allocation, and satisfies the invariants of `SpaSlice`.
            unsafe { (slice as *mut Self).as_mut_unchecked() }
        };

        let rest = {
            // SAFETY: We know that `rest_offset` will not overflow, and that the pointer will lie within
            //         `bytes`.
            let data = unsafe {
                (&raw mut *bytes).cast::<Byte>().byte_add(rest_offset)
            };
            let slice = ptr::slice_from_raw_parts_mut(data, rest_len);

            // SAFETY: We know that there's no aliased mutability within `rest`, and that it is a valid
            //         allocation, and so on.
            unsafe { slice.as_mut_unchecked() }
        };

        Ok((slice, rest))
    }
}

// SAFETY: A `SpaSlice` is a fancy wrapper over a slice of primitives.
unsafe impl<P> AsBytes for SpaSlice<P> where P: PrimPod {}
unsafe impl<P> AsBytesMut for SpaSlice<P> where P: PrimPod {}

impl<P> Default for &SpaSlice<P>
where
    P: PrimPod,
{
    #[inline(always)]
    fn default() -> Self {
        SpaSlice::empty()
    }
}

impl<P> Default for &mut SpaSlice<P>
where
    P: PrimPod,
{
    #[inline(always)]
    fn default() -> Self {
        SpaSlice::empty()
    }
}

impl<P> AsRef<[P]> for SpaSlice<P>
where
    P: PrimPod,
{
    #[inline(always)]
    fn as_ref(&self) -> &[P] {
        self.as_prims()
    }
}

impl<P> AsMut<[P]> for SpaSlice<P>
where
    P: PrimPod,
{
    #[inline(always)]
    fn as_mut(&mut self) -> &mut [P] {
        self.as_prims_mut()
    }
}

impl<P> Borrow<[P]> for SpaSlice<P>
where
    P: PrimPod,
{
    #[inline(always)]
    fn borrow(&self) -> &[P] {
        self.as_prims()
    }
}

impl<P> BorrowMut<[P]> for SpaSlice<P>
where
    P: PrimPod,
{
    #[inline(always)]
    fn borrow_mut(&mut self) -> &mut [P] {
        self.as_prims_mut()
    }
}

impl<P> fmt::Debug for SpaSlice<P>
where
    P: PrimPod,
    [P]: fmt::Debug,
{
    #[inline(always)]
    #[track_caller]
    fn fmt(
        &self,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        self.as_prims().fmt(f)
    }
}

impl<'a, P> IntoIterator for &'a SpaSlice<P>
where
    P: PrimPod,
{
    type Item = &'a P;
    type IntoIter = slice::Iter<'a, P>;

    #[inline(always)]
    fn into_iter(self) -> Self::IntoIter {
        self.as_prims().iter()
    }
}

impl<'a, P> IntoIterator for &'a mut SpaSlice<P>
where
    P: PrimPod,
{
    type Item = &'a mut P;
    type IntoIter = slice::IterMut<'a, P>;

    #[inline(always)]
    fn into_iter(self) -> Self::IntoIter {
        self.as_prims_mut().iter_mut()
    }
}

impl<'a, P, T> TryFrom<&'a [T]> for &'a SpaSlice<P>
where
    P: PrimPod,
    T: AsPrimPod<P>,
{
    type Error = PodError;

    #[inline(always)]
    fn try_from(prims: &'a [T]) -> Result<Self, Self::Error> {
        SpaSlice::from_prims(prims)
    }
}

impl<'a, P, T> TryFrom<&'a mut [T]> for &'a mut SpaSlice<P>
where
    P: PrimPod,
    T: AsPrimPodMut<P>,
{
    type Error = PodError;

    #[inline(always)]
    fn try_from(prims: &'a mut [T]) -> Result<Self, Self::Error> {
        SpaSlice::from_prims_mut(prims)
    }
}

impl<'a, P> From<&'a SpaSlice<P>> for &'a [P]
where
    P: PrimPod,
{
    #[inline(always)]
    fn from(slice: &'a SpaSlice<P>) -> Self {
        slice.as_prims()
    }
}

impl<'a, P> From<&'a mut SpaSlice<P>> for &'a mut [P]
where
    P: PrimPod,
{
    #[inline(always)]
    fn from(prims: &'a mut SpaSlice<P>) -> Self {
        prims.as_prims_mut()
    }
}

impl<'a, P, const N: usize> TryFrom<&'a SpaSlice<P>> for &'a [P; N]
where
    P: PrimPod,
{
    type Error = array::TryFromSliceError;

    #[inline(always)]
    fn try_from(slice: &'a SpaSlice<P>) -> Result<Self, Self::Error> {
        slice.as_prims().try_into()
    }
}

impl<'a, P, const N: usize> TryFrom<&'a mut SpaSlice<P>> for &'a mut [P; N]
where
    P: PrimPod,
{
    type Error = array::TryFromSliceError;

    #[inline(always)]
    fn try_from(slice: &'a mut SpaSlice<P>) -> Result<Self, Self::Error> {
        slice.as_prims_mut().try_into()
    }
}

impl<'a, P, const N: usize> TryFrom<&'a SpaSlice<P>> for [P; N]
where
    P: PrimPod,
{
    type Error = array::TryFromSliceError;

    #[inline(always)]
    fn try_from(slice: &'a SpaSlice<P>) -> Result<Self, Self::Error> {
        slice.as_prims().try_into()
    }
}

impl<'a, P, const N: usize> TryFrom<&'a mut SpaSlice<P>> for [P; N]
where
    P: PrimPod,
{
    type Error = array::TryFromSliceError;

    #[inline(always)]
    fn try_from(slice: &'a mut SpaSlice<P>) -> Result<Self, Self::Error> {
        slice.as_prims().try_into()
    }
}

mod private {
    use crate::pod::{PrimPod, SpaSlice};

    pub trait MapIndex: 'static {
        type Mapped: 'static + ?Sized;

        fn mapped(&self) -> &Self::Mapped;
        fn mapped_mut(&mut self) -> &mut Self::Mapped;
    }

    impl<P> MapIndex for P
    where
        P: PrimPod,
    {
        type Mapped = P;

        #[inline(always)]
        #[track_caller]
        fn mapped(&self) -> &Self::Mapped {
            self
        }

        #[inline(always)]
        #[track_caller]
        fn mapped_mut(&mut self) -> &mut Self::Mapped {
            self
        }
    }

    impl<P> MapIndex for [P]
    where
        P: PrimPod,
    {
        type Mapped = SpaSlice<P>;

        #[inline(always)]
        #[track_caller]
        fn mapped(&self) -> &Self::Mapped {
            self.try_into().unwrap()
        }

        #[inline(always)]
        #[track_caller]
        fn mapped_mut(&mut self) -> &mut Self::Mapped {
            self.try_into().unwrap()
        }
    }
}

impl<P, I> Index<I> for SpaSlice<P>
where
    P: PrimPod,
    [P]: Index<I, Output: private::MapIndex>,
{
    type Output = <<[P] as Index<I>>::Output as private::MapIndex>::Mapped;

    #[inline(always)]
    #[track_caller]
    fn index(
        &self,
        index: I,
    ) -> &Self::Output {
        self.as_prims().index(index).mapped()
    }
}

impl<P, I> IndexMut<I> for SpaSlice<P>
where
    P: PrimPod,
    [P]: IndexMut<I, Output: private::MapIndex>,
{
    #[inline(always)]
    #[track_caller]
    fn index_mut(
        &mut self,
        index: I,
    ) -> &mut Self::Output {
        self.as_prims_mut().index_mut(index).mapped_mut()
    }
}
