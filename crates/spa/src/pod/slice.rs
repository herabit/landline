use std::{hint, marker::PhantomData, ptr::NonNull};

use crate::{
    mem::Byte,
    pod::{ParsePodError, PrimBody, PrimPod},
};

/// # Safety
///
/// Use your damn brain. Valid allocation blah blah blah.
#[inline(always)]
#[must_use]
#[allow(clippy::type_complexity)]
const unsafe fn split_at<T>(
    data: NonNull<[T]>,
    mid: usize,
) -> Option<(NonNull<[T]>, NonNull<[T]>)> {
    if mid <= data.len() {
        let head = NonNull::slice_from_raw_parts(data.cast::<T>(), mid);

        // SAFETY: We know that `data` is a valid allocation, and that `mid` is in bounds.
        let tail = NonNull::slice_from_raw_parts(unsafe { data.cast::<T>().add(mid) }, unsafe {
            data.len().unchecked_sub(mid)
        });

        Some((head, tail))
    } else {
        None
    }
}

/// Implementation detail for primitive slices.
#[repr(transparent)]
struct RawSlice<P>
where
    P: PrimPod,
{
    /// The length of this is always representable as a `u32`, and this
    /// must be a valid allocation to a `[P]` of said length, and any needed
    /// padding after.
    ///
    /// Additionally, we require that the size of the allocation, in bytes, including
    /// the padding, must fit within a `u32`.
    data: NonNull<[P]>,
    _padding: PhantomData<Option<NonNull<P::Padding>>>,
}

impl<P> RawSlice<P>
where
    P: PrimPod,
{
    const ASSERT: () = PrimBody::<P>::ensure_layout();

    /// Returns the length of this slice.
    #[inline(always)]
    #[must_use]
    const fn len(self) -> u32 {
        const { Self::ASSERT };

        // SAFETY: We require that the length of this slice is representable as a `u32`, and we only
        //         support 32-bit and 64-bit platforms.
        unsafe { hint::assert_unchecked(self.data.len() <= u32::MAX as usize) };

        let len = self.data.len() as u32;

        // SAFETY: We guarantee that the size of the slice fits in a u32.
        let size = unsafe { len.unchecked_mul(P::SIZE) };

        // NOTE: See the note in `RawSlide::decode` for `padding_size`.
        let padding_size = (len % 2) * P::PADDING_SIZE;

        // SAFETY: We guarantee that the stride (size + padding_len) also fits in a u32.
        let stride = unsafe { size.unchecked_add(padding_size) };

        // SAFETY: We guarantee that the stride is a multiple of 8.
        unsafe { hint::assert_unchecked(stride.is_multiple_of(8)) };

        len
    }

    /// Returns a pointer to the byte slice underlying this raw slice, including padding.
    #[inline(always)]
    #[must_use]
    const fn as_bytes(self) -> NonNull<[Byte]> {
        const { Self::ASSERT };
        // SAFETY: We know that this will never overflow due to the guarantees specified in `RawSlice::len`.
        let size = unsafe { self.len().unchecked_mul(P::SIZE) };

        // NOTE: See the note in `RawSlide::decode` for `padding_size`.
        let padding_size = (self.len() % 2) * P::PADDING_SIZE;

        // SAFETY: We know that this will never overflow due to the guarantees specified in `RawSlice::len`.
        let stride = unsafe { size.unchecked_add(padding_size) };

        // SAFETY: See `RawSlice::len` and `RawSlice::decode` for why the stride is always
        //         a multiple of 8.
        unsafe { hint::assert_unchecked(stride.is_multiple_of(8)) };

        NonNull::slice_from_raw_parts(self.data.cast::<Byte>(), stride as usize)
    }

    /// Decode a raw primitive slice from the start of the elements,
    /// and the amount of elements.
    ///
    /// # Safety
    ///
    /// The caller must ensure a bunch of things, but the one of priority
    /// is that `bytes` is *actually a valid allocation*.
    #[inline(always)]
    const unsafe fn decode(
        bytes: NonNull<[Byte]>,
        len: u32,
    ) -> Result<(RawSlice<P>, NonNull<[Byte]>), ParsePodError> {
        const { Self::ASSERT };

        // SAFETY: The caller promises that `bytes` is a valid allocation.
        // unsafe

        let Some(size) = len.checked_mul(P::SIZE) else {
            return Err(ParsePodError::InsufficientSpace);
        };

        // NOTE: This is a zero, always, on types that lack padding.
        //
        //       Types that lack padding are those that have sizes that
        //       are already a multiple of 8.
        //
        //       The only types that need padding are those whose size
        //       are not a multiple of 8. The only types that have this
        //       property are those whose size is 4.
        //
        //       Thus we need to increment the size by 4 if the length is odd,
        //       as `(4 * x) % 8 == 4` where `x` is odd. However,
        //       if we change the term `x` to `x + 1`, the identity
        //       becomes `(4 * (x + 1)) % 8 == 0`, or alternatively,
        //       `((4 * x) + 4) % 8 = 0`, which is what we're doing.
        let padding_size = (len % 2) * P::PADDING_SIZE;

        let Some(stride) = size.checked_add(padding_size) else {
            return Err(ParsePodError::InsufficientSpace);
        };

        // SAFETY: See the note above `padding_len`, as that illustrates why
        //         this is sound.
        unsafe { hint::assert_unchecked(stride.is_multiple_of(8)) };

        // SAFETY: The caller ensures that `bytes` is a valid allocation.
        let Some((this, rest)) = (unsafe { split_at(bytes, stride as usize) }) else {
            return Err(ParsePodError::InsufficientSpace);
        };

        // SAFETY: The caller ensures that `bytes` is a valid allocation (which these slices are derived from).
        let (slice, padding) = unsafe { split_at(this, size as usize) }.unwrap();

        // NOTE: These are sanity checks.
        if P::SIZE != 0 {
            assert!(slice.len() % P::SIZE as usize == 0);
        }
        assert!(padding.is_empty() || padding.len() == P::PADDING_SIZE as usize);

        let slice = RawSlice::<P> {
            data: NonNull::slice_from_raw_parts(this.cast::<P>(), len as usize),
            _padding: PhantomData,
        };

        Ok((slice, rest))
    }
}

impl<P> Clone for RawSlice<P>
where
    P: PrimPod,
{
    #[inline(always)]
    fn clone(&self) -> Self {
        *self
    }
}

impl<P> Copy for RawSlice<P> where P: PrimPod {}

// /// A structure representing a primitive slice stored in-place.
#[repr(transparent)]
pub struct PrimSlice<'a, P>
where
    P: PrimPod,
{
    inner: RawSlice<P>,
    _slice: PhantomData<&'a [P]>,
    _padding: PhantomData<&'a P::Padding>,
}

#[unsafe(no_mangle)]
#[inline(never)]
pub fn decode<'a>(
    bytes: &'a [Byte],
    len: u32,
) -> Result<(PrimSlice<'a, super::SpaInt>, &'a [Byte]), ParsePodError> {
    PrimSlice::decode(bytes, len)
}

#[unsafe(no_mangle)]
#[inline(never)]
pub fn bytes_lol<'a>(slice: PrimSlice<'a, super::SpaInt>) -> &'a [Byte] {
    slice.as_bytes()
}

impl<'a, P> PrimSlice<'a, P>
where
    P: PrimPod,
{
    /// Get the length of the slice.
    #[inline(always)]
    #[must_use]
    pub const fn len(&self) -> u32 {
        self.inner.len()
    }

    /// Returns whether this slice is empty.
    #[inline(always)]
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Get the underlying bytes of this slice, including the padding.
    #[inline(always)]
    #[must_use]
    pub const fn as_bytes(&self) -> &'a [Byte] {
        // SAFETY: We know that the underlying bytes of the slice and padding are valid.
        unsafe { self.inner.as_bytes().as_ref() }
    }

    /// Decode a primitive slice from the start of the elements, and the amount of elements.
    #[inline(always)]
    #[allow(unused_unsafe)]
    pub const fn decode(
        bytes: &'a [Byte],
        len: u32,
    ) -> Result<(PrimSlice<'a, P>, &'a [Byte]), ParsePodError> {
        // SAFETY: We know an immutable slice is a valid allocation.
        match unsafe { RawSlice::decode(NonNull::from_ref(bytes), len) } {
            Ok((slice, rest)) => {
                // SAFETY: We know `slice` to be derived from `bytes`.
                let slice: PrimSlice<'a, P> = unsafe {
                    PrimSlice {
                        inner: slice,
                        _slice: PhantomData,
                        _padding: PhantomData,
                    }
                };

                // SAFETY: We know `rest` to be derived from `bytes`.
                let rest: &'a [Byte] = unsafe { rest.as_ref() };

                Ok((slice, rest))
            }
            Err(err) => Err(err),
        }
    }
}

impl<'a, P> Clone for PrimSlice<'a, P>
where
    P: PrimPod,
{
    #[inline(always)]
    fn clone(&self) -> Self {
        *self
    }
}

impl<'a, P> Copy for PrimSlice<'a, P> where P: PrimPod {}

// SAFETY: A `PrimSlice` is functionally a tuple over `&'a [P]` and `&'a P::Padding`.
unsafe impl<'a, P> Send for PrimSlice<'a, P>
where
    P: PrimPod,
    &'a [P]: Send,
    &'a P::Padding: Send,
{
}

unsafe impl<'a, P> Sync for PrimSlice<'a, P>
where
    P: PrimPod,
    &'a [P]: Sync,
    &'a P::Padding: Sync,
{
}

// #[inline(always)]
// #[must_use]
// pub const fn parse_slice_prim<P>(bytes: &[Byte]) -> Result<(&[P], Option<&P::Padding>)>
