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

/// Perform an unchecked `div_rem`.
///
/// # Safety
///
/// The caller must ensure `rhs != 0`.
#[inline(always)]
#[must_use]
#[track_caller]
const unsafe fn unchecked_div_rem(
    lhs: u32,
    rhs: u32,
) -> (u32, u32) {
    let (Some(div), Some(rem)) = (lhs.checked_div(rhs), lhs.checked_rem(rhs)) else {
        // SAFETY: The caller promises `rhs != 0`.
        unsafe { hint::unreachable_unchecked() };
    };

    (div, rem)
}

/// Divide `rhs` by `lhs` exactly.
///
/// # Safety
///
/// The caller must ensure `rhs != 0 && lhs % rhs == 0`.
#[inline(always)]
#[must_use]
#[track_caller]
const unsafe fn unchecked_div_exact(
    lhs: u32,
    rhs: u32,
) -> u32 {
    // SAFETY: The caller ensures that `lhs` is a multiple of `rhs`.
    let (div, _rem @ 0) = (unsafe { unchecked_div_rem(lhs, rhs) }) else {
        // SAFETY: The caller ensures that `lhs` is a multiple of `rhs`.
        unsafe { hint::unreachable_unchecked() };
    };

    // SAFETY: The caller ensures that `lhs` is a multiple of `rhs`.
    unsafe { hint::assert_unchecked(div.unchecked_mul(rhs) == lhs) };

    div
}

/// This just exactly divides a `u32` by another `u32`.
///
/// # Returns
///
/// Returns [`None`] if `rhs == 0 || lhs % rhs != 0`.
#[inline(always)]
#[must_use]
#[track_caller]
const fn checked_div_exact(
    lhs: u32,
    rhs: u32,
) -> Option<u32> {
    match lhs.checked_rem(rhs) {
        // SAFETY: We know that we're not dividing by zero, and that `lhs` is a multiple of `rhs`.
        Some(0) => Some(unsafe { unchecked_div_exact(lhs, rhs) }),
        None | Some(1..) => {
            hint::cold_path();
            None
        }
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

    /// Returns the size and stride (in bytes) of this slice.
    #[inline(always)]
    const fn size_stride_len(self) -> (u32, u32, u32) {
        const { Self::ASSERT };

        let len = {
            // SAFETY: We only support 32-bit and 64-bit platforms, and we require that
            //         the length, size, and stride must all fit in a u32.
            unsafe { hint::assert_unchecked(self.data.len() <= u32::MAX as usize) };

            self.data.len() as u32
        };

        let (slice_size, padding_size) = {
            // SAFETY: We guarantee that the size of a slice fits in a u32.
            let slice_size = unsafe { len.unchecked_mul(P::SIZE) };

            // SAFETY: This is always `0` for types that don't need padding, and for those
            //         that do, it makes the stride divide exactly by `8`.
            //
            //         See `RawSlice::decode` for details.
            let padding_size = unsafe { (len % 2).unchecked_mul(P::PADDING_SIZE) };

            (slice_size, padding_size)
        };

        let stride_size = {
            // SAFETY: We guarantee that the stride of a slice fits in a u32.
            let stride_size = unsafe { slice_size.unchecked_add(padding_size) };

            // SAFETY: We guarantee that the stride of a slice is always divisible by eight.
            let _ = unsafe { unchecked_div_exact(stride_size, 8) };

            stride_size
        };

        let len = {
            if P::SIZE != 0 {
                // SAFETY: Since we know that `slice_size == len * P::SIZE` and
                //         that `P::SIZE != 0`, we know for a *fact* that `slice_size == 0`
                //         only ever when `len == 0`.
                unsafe { hint::assert_unchecked((slice_size == 0) == (len == 0)) };

                // SAFETY: We know for a fact that `slice_size == len * P::SIZE` and
                //         that `P::SIZE != 0`, thus it is safe to perform an exact division.
                unsafe { hint::assert_unchecked(unchecked_div_exact(slice_size, P::SIZE) == len) };

                if len != 0 {
                    // SAFETY: We know for a fact that `slice_size == len * P::SIZE` and that
                    //         `P::SIZE != 0` and `len != 0`, thus it is safe to perform an exact division.
                    unsafe {
                        hint::assert_unchecked(unchecked_div_exact(slice_size, len) == P::SIZE)
                    };
                }
            }

            len
        };

        (slice_size, stride_size, len)
    }

    /// Returns the length of this slice.
    #[inline(always)]
    #[must_use]
    const fn len(self) -> u32 {
        const { Self::ASSERT };

        let (_size, _stride, len) = self.size_stride_len();

        len
    }

    /// Split the underlying byte slice to a byte slice of the raw slice, and a raw slice of
    /// the padding (if any).
    #[inline(always)]
    #[must_use]
    const fn split_bytes(self) -> (NonNull<[Byte]>, NonNull<[Byte]>) {
        const { Self::ASSERT };

        let (size, stride, _len) = self.size_stride_len();

        let padding = {
            // SAFETY: We know `stride >= size`.
            let padding = unsafe { stride.unchecked_sub(size) };

            if P::PADDING_SIZE != 0 {
                // SAFETY: If the size of the padding type is not zero, then we know
                //         that the length of the padding, in bytes, is a multiple of
                //         `P::PADDING_SIZE`.
                //
                //          Additionally, we know that the padding is only ever not
                //          zero sized ***when*** the size of the POD is 4, and since
                //          we pad to multiples of eight, we know that `padding` is only
                //          ever `0` or `1`.
                //
                //          See `RawSlice::decode` for more info.
                unsafe {
                    hint::assert_unchecked(unchecked_div_exact(padding, P::PADDING_SIZE) <= 1)
                };

                // SAFETY: Same as above.
                unsafe { hint::assert_unchecked(padding == P::PADDING_SIZE || padding == 0) };
            }

            // SAFETY: We guarantee that `data` is a valid allocation that spanning `stride` bytes.
            NonNull::slice_from_raw_parts(
                unsafe { self.data.cast::<Byte>().add(size as usize) },
                padding as usize,
            )
        };

        let slice = NonNull::slice_from_raw_parts(self.data.cast::<Byte>(), size as usize);

        (slice, padding)
    }

    /// Split the raw slice into the underlying slice, and the padding, if there is any.
    #[inline(always)]
    #[must_use]
    const fn split(self) -> (NonNull<[P]>, Option<NonNull<P::Padding>>) {
        const { Self::ASSERT };

        let (slice, padding) = self.split_bytes();

        let slice = {
            let len = checked_div_exact(slice.len() as u32, P::SIZE).unwrap();

            NonNull::slice_from_raw_parts(slice.cast::<P>(), len as usize)
        };

        let padding = if padding.is_empty() {
            None
        } else if padding.len() == P::PADDING_SIZE as usize {
            Some(padding.cast::<P::Padding>())
        } else {
            // NOTE: This should be optimized away due to me abusing the `assume` intrinsic.
            panic!("something horrible has gone wrong");
        };

        (slice, padding)
    }

    /// Returns a pointer to the byte slice underlying this raw slice, including padding.
    #[inline(always)]
    #[must_use]
    const fn as_bytes(self) -> NonNull<[Byte]> {
        const { Self::ASSERT };

        let (_size, stride, _len) = self.size_stride_len();

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

/// A slice of primitive PODs, as well as the trailing padding, if any.
#[repr(transparent)]
pub struct PrimSlice<'a, P>
where
    P: PrimPod,
{
    inner: RawSlice<P>,
    _slice: PhantomData<&'a [P]>,
    _padding: PhantomData<Option<&'a P::Padding>>,
}

impl<'a, P> PrimSlice<'a, P>
where
    P: PrimPod,
{
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

    /// Split the underlying bytes of this slice at the start of the padding.
    #[inline(always)]
    #[must_use]
    pub const fn split_bytes(&self) -> (&'a [Byte], &'a [Byte]) {
        let (slice, padding) = self.inner.split_bytes();

        // SAFETY: We know that both allocations are valid.
        (unsafe { slice.as_ref() }, unsafe { padding.as_ref() })
    }

    /// Split into the underlying primitive slice and the padding, if there is any.
    ///
    /// # Returns
    ///
    /// The padding will always be set to [`None`] for types that don't need padding.
    #[inline(always)]
    #[must_use]
    pub const fn split(&self) -> (&'a [P], Option<&'a P::Padding>) {
        let (slice, padding) = self.inner.split();

        // SAFETY: We know that the underlying slice is valid.
        let slice = unsafe { slice.as_ref() };

        let padding = match padding {
            // SAFETY: We know that the underlying padding is valid.
            Some(padding) => Some(unsafe { padding.as_ref() }),
            None => None,
        };

        (slice, padding)
    }

    /// Get a reference to the underlying primitive slice.
    #[inline(always)]
    #[must_use]
    pub const fn as_ref(&self) -> &'a [P] {
        self.split().0
    }

    /// Get a reference to the underlying padding, if there's any.
    ///
    /// # Returns
    ///
    /// This will always return [`None`] for types that don't need padding.
    #[inline(always)]
    #[must_use]
    pub const fn padding(&self) -> Option<&'a P::Padding> {
        self.split().1
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

// SAFETY: A `PrimSlice` is functionally a tuple over `&'a [P]` and `Option<&'a P::Padding>`.
unsafe impl<'a, P> Send for PrimSlice<'a, P>
where
    P: PrimPod,
    &'a [P]: Send,
    Option<&'a P::Padding>: Send,
{
}

// SAFETY: A `PrimSlice` is functionally a tuple over `&'a [P]` and `Option<&'a P::Padding>`.
unsafe impl<'a, P> Sync for PrimSlice<'a, P>
where
    P: PrimPod,
    &'a [P]: Sync,
    Option<&'a P::Padding>: Sync,
{
}
