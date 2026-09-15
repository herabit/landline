use std::{
    array,
    borrow::{Borrow, BorrowMut},
    fmt, hint,
    marker::PhantomData,
    ops::{Index, IndexMut},
    ptr::NonNull,
    rc::Rc,
    sync::Arc,
};

use crate::{
    mem::{Byte, as_bytes, as_bytes_mut},
    pod::{ParsePodError, PrimBody, PrimPod, Result, SpaHeader, kind::SpaKind},
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
        let tail = NonNull::slice_from_raw_parts(
            unsafe { data.cast::<T>().add(mid) },
            unsafe { data.len().unchecked_sub(mid) },
        );

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
    let (Some(div), Some(rem)) = (lhs.checked_div(rhs), lhs.checked_rem(rhs))
    else {
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
        },
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
            unsafe {
                hint::assert_unchecked(self.data.len() <= u32::MAX as usize)
            };

            self.data.len() as u32
        };

        let (slice_size, padding_size) = {
            // SAFETY: We guarantee that the size of a slice fits in a u32.
            let slice_size = unsafe { len.unchecked_mul(P::SIZE) };

            // SAFETY: This is always `0` for types that don't need padding, and for those
            //         that do, it makes the stride divide exactly by `8`.
            //
            //         See `RawSlice::parse` for details.
            let padding_size =
                unsafe { (len % 2).unchecked_mul(P::PADDING_SIZE) };

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
                unsafe {
                    hint::assert_unchecked((slice_size == 0) == (len == 0))
                };

                // SAFETY: We know for a fact that `slice_size == len * P::SIZE` and
                //         that `P::SIZE != 0`, thus it is safe to perform an exact division.
                unsafe {
                    hint::assert_unchecked(
                        unchecked_div_exact(slice_size, P::SIZE) == len,
                    )
                };

                if len != 0 {
                    // SAFETY: We know for a fact that `slice_size == len * P::SIZE` and that
                    //         `P::SIZE != 0` and `len != 0`, thus it is safe to perform an exact division.
                    unsafe {
                        hint::assert_unchecked(
                            unchecked_div_exact(slice_size, len) == P::SIZE,
                        )
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
                //          See `RawSlice::parse` for more info.
                unsafe {
                    hint::assert_unchecked(
                        unchecked_div_exact(padding, P::PADDING_SIZE) <= 1,
                    )
                };

                // SAFETY: Same as above.
                unsafe {
                    hint::assert_unchecked(
                        padding == P::PADDING_SIZE || padding == 0,
                    )
                };
            }

            // SAFETY: We guarantee that `data` is a valid allocation that spanning `stride` bytes.
            NonNull::slice_from_raw_parts(
                unsafe { self.data.cast::<Byte>().add(size as usize) },
                padding as usize,
            )
        };

        let slice = NonNull::slice_from_raw_parts(
            self.data.cast::<Byte>(),
            size as usize,
        );

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

    /// Decode a raw primitive slice from the start of its header.
    ///
    /// # Safety
    ///
    /// The caller has to ensure a lot of things, I'm just too lazy to write them out
    /// at present.
    #[inline(always)]
    const unsafe fn decode(
        bytes: NonNull<[Byte]>
    ) -> Result<(RawSlice<P>, NonNull<[Byte]>)> {
        const { Self::ASSERT };

        // SAFETY: The caller ensures this is sound.
        let Some((headers, bytes)) =
            (unsafe { split_at(bytes, size_of::<[SpaHeader; 2]>()) })
        else {
            hint::cold_path();
            return Err(ParsePodError::InsufficientSpace);
        };

        // SAFETY: The caller ensures it's sound to treat the first 16 bytes as headers.
        let [pod_header, array_header] =
            unsafe { headers.cast::<[SpaHeader; 2]>().as_ref() };

        if P::SIZE == 0 {
            // NOTE: We're just going to error on arrays of zero sized types. They don't make sense within the context of
            //       pipewire, and yeah... Fuck it.
            Err(ParsePodError::Other)
        } else if pod_header.kind != SpaKind::ARRAY
            || array_header.kind != P::KIND
            || array_header.size != P::SIZE
        {
            // NOTE: We're confirming that we're actually parsing an array for our type, and that the child
            //       size matches what we expect. If it doesn't, then it's not expected.
            hint::cold_path();
            Err(ParsePodError::UnexpectedValue)
        } else if pod_header.size < (size_of::<SpaHeader>() as u32) {
            // NOTE: We somehow don't have enough room for the array's header.
            hint::cold_path();
            Err(ParsePodError::InsufficientSpace)
        } else if let array_size =
            pod_header.size.strict_sub(size_of::<SpaHeader>() as u32)
            && let Some(len) = checked_div_exact(array_size, P::SIZE)
        {
            // NOTE: Success!!!
            // SAFETY: The caller ensures this is fine and we just did a bunch of checks.
            unsafe { RawSlice::<P>::parse(bytes, len) }
        } else {
            // NOTE: AGH! Fuck more errors.
            hint::cold_path();
            Err(ParsePodError::InsufficientSpace)
        }
    }

    /// Parse a raw primitive slice from the start of the elements,
    /// and the amount of elements.
    ///
    /// # Safety
    ///
    /// The caller must ensure a bunch of things, but the one of priority
    /// is that `bytes` is *actually a valid allocation*.
    #[inline(always)]
    const unsafe fn parse(
        bytes: NonNull<[Byte]>,
        len: u32,
    ) -> Result<(RawSlice<P>, NonNull<[Byte]>)> {
        const { Self::ASSERT };

        let Some(size) = len.checked_mul(P::SIZE) else {
            hint::cold_path();
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
            hint::cold_path();
            return Err(ParsePodError::InsufficientSpace);
        };

        // SAFETY: See the note above `padding_len`, as that illustrates why
        //         this is sound.
        unsafe { hint::assert_unchecked(stride.is_multiple_of(8)) };

        // SAFETY: The caller ensures that `bytes` is a valid allocation.
        let Some((this, rest)) = (unsafe { split_at(bytes, stride as usize) })
        else {
            hint::cold_path();
            return Err(ParsePodError::InsufficientSpace);
        };

        // SAFETY: The caller ensures that `bytes` is a valid allocation (which these slices are derived from).
        let (slice, padding) =
            unsafe { split_at(this, size as usize) }.unwrap();

        // NOTE: These are sanity checks.
        if P::SIZE != 0 {
            assert!(slice.len() % P::SIZE as usize == 0);
        }
        assert!(
            padding.is_empty() || padding.len() == P::PADDING_SIZE as usize
        );

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
    /// Decode a primitive slice from the start of its header.
    #[inline(always)]
    #[allow(unused_unsafe)]
    pub const fn decode(
        bytes: &'a [Byte]
    ) -> Result<(PrimSlice<'a, P>, &'a [Byte])> {
        // SAFETY: We know an immutable slice is a valid allocation.
        match unsafe { RawSlice::decode(NonNull::from_ref(bytes)) } {
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
            },
            Err(err) => Err(err),
        }
    }

    /// Parse a primitive slice given a buffer and the amount of elements.
    #[inline(always)]
    #[allow(unused_unsafe)]
    pub const fn parse(
        bytes: &'a [Byte],
        len: u32,
    ) -> Result<(PrimSlice<'a, P>, &'a [Byte])> {
        // SAFETY: We know an immutable slice is a valid allocation.
        match unsafe { RawSlice::parse(NonNull::from_ref(bytes), len) } {
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
            },
            Err(err) => Err(err),
        }
    }

    /// Create an empty [`PrimSlice`].
    #[inline(always)]
    #[must_use]
    pub const fn empty() -> PrimSlice<'a, P> {
        const {
            match PrimSlice::parse(&[], 0) {
                Ok((slice, _)) => slice,
                Err(_) => unreachable!(),
            }
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

    /// Reborrow this slice for a shorter lifetime.
    #[inline(always)]
    #[must_use]
    pub const fn as_slice(&self) -> PrimSlice<'_, P> {
        *self
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
    pub const fn prims(&self) -> &'a [P] {
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

    /// Returns an iterator over the slice.
    ///
    /// This is equivalent to calling `self.prims().iter()`.
    #[inline(always)]
    pub fn iter(&self) -> std::slice::Iter<'_, P> {
        self.prims().iter()
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

// SAFETY: Since this is functionally a `(&'a [P], Option<&'a P::Padding>)`,
//         we follow the rules of references. References are only safe to send
//         to another thread if their referent is `Sync`. So, since our fields are covariant
//         over `&'a [P]` and `&'a P::Padding`, and transitively `P` and `P::Padding`,
//         we can implement `Send` when both `P` and `P::Padding` are `Sync`.
unsafe impl<'a, P> Send for PrimSlice<'a, P>
where
    P: PrimPod,
    P: Sync,
    P::Padding: Sync,
{
}

// SAFETY: Since this is functionally a `(&'a [P], Option<&'a P::Padding>)`,
//         we follow the rules of references. References are only safe to share
//         with another thread if their referent is `Sync`. Thus, since our fields
//         are covariant over `&'a [P]` and `Option<&'a P::Padding>`, and transitively
//         `P` and `P::Padding`, we can implement `Sync` when both `P` and `P::Padding`
//         are `Sync`.
unsafe impl<'a, P> Sync for PrimSlice<'a, P>
where
    P: PrimPod,
    P: Sync,
    P::Padding: Sync,
{
}

impl<'a, P> fmt::Debug for PrimSlice<'a, P>
where
    P: fmt::Debug + PrimPod,
    P::Padding: fmt::Debug,
{
    fn fmt(
        &self,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        let (slice, padding) = self.split();
        f.debug_struct("PrimSlice")
            .field("slice", &slice)
            .field("padding", &padding)
            .finish()
    }
}

impl<'a, P> Default for PrimSlice<'a, P>
where
    P: PrimPod,
{
    #[inline(always)]
    fn default() -> Self {
        PrimSlice::empty()
    }
}

impl<'a, P> AsRef<[P]> for PrimSlice<'a, P>
where
    P: PrimPod,
{
    #[inline(always)]
    fn as_ref(&self) -> &[P] {
        self.prims()
    }
}

impl<'a, P> Borrow<[P]> for PrimSlice<'a, P>
where
    P: PrimPod,
{
    #[inline(always)]
    fn borrow(&self) -> &[P] {
        self.prims()
    }
}

impl<'a, P, I> Index<I> for PrimSlice<'a, P>
where
    P: PrimPod,
    [P]: Index<I>,
{
    type Output = <[P] as Index<I>>::Output;

    #[inline(always)]
    #[track_caller]
    fn index(
        &self,
        index: I,
    ) -> &Self::Output {
        self.prims().index(index)
    }
}

impl<'a, P> From<PrimSlice<'a, P>> for &'a [P]
where
    P: PrimPod,
{
    #[inline(always)]
    fn from(value: PrimSlice<'a, P>) -> Self {
        value.prims()
    }
}

impl<'a, P> From<PrimSlice<'a, P>> for (&'a [P], Option<&'a P::Padding>)
where
    P: PrimPod,
{
    #[inline(always)]
    fn from(value: PrimSlice<'a, P>) -> Self {
        value.split()
    }
}

impl<'a, P, const N: usize> TryFrom<PrimSlice<'a, P>> for &'a [P; N]
where
    P: PrimPod,
{
    type Error = array::TryFromSliceError;

    #[inline(always)]
    fn try_from(value: PrimSlice<'a, P>) -> Result<Self, Self::Error> {
        value.prims().try_into()
    }
}

impl<'a, P, const N: usize> TryFrom<PrimSlice<'a, P>>
    for (&'a [P; N], Option<&'a P::Padding>)
where
    P: PrimPod,
{
    type Error = array::TryFromSliceError;

    #[inline(always)]
    fn try_from(value: PrimSlice<'a, P>) -> Result<Self, Self::Error> {
        let (slice, padding) = value.split();

        slice.try_into().map(|array| (array, padding))
    }
}

impl<'a, P> TryFrom<&'a [P]> for PrimSlice<'a, P>
where
    P: PrimPod,
{
    type Error = ParsePodError;

    #[inline(always)]
    fn try_from(value: &'a [P]) -> Result<Self, Self::Error> {
        match u32::try_from(value.len()) {
            Ok(len) => {
                PrimSlice::parse(as_bytes(value), len).map(|(slice, _)| slice)
            },
            Err(_) => {
                hint::cold_path();
                Err(ParsePodError::InsufficientSpace)
            },
        }
    }
}

impl<'a, P, const N: usize> TryFrom<&'a [P; N]> for PrimSlice<'a, P>
where
    P: PrimPod,
{
    type Error = ParsePodError;

    #[inline(always)]
    fn try_from(value: &'a [P; N]) -> Result<Self, Self::Error> {
        value.as_slice().try_into()
    }
}

impl<'a, P, const N: usize> TryFrom<PrimSlice<'a, P>> for [P; N]
where
    P: PrimPod,
{
    type Error = array::TryFromSliceError;

    #[inline(always)]
    fn try_from(value: PrimSlice<'a, P>) -> Result<Self, Self::Error> {
        value.prims().try_into()
    }
}

impl<'a, P> From<PrimSlice<'a, P>> for Vec<P>
where
    P: PrimPod,
{
    #[inline(always)]
    #[track_caller]
    fn from(value: PrimSlice<'a, P>) -> Self {
        value.prims().to_vec()
    }
}

impl<'a, P> From<PrimSlice<'a, P>> for Box<[P]>
where
    P: PrimPod,
{
    #[inline(always)]
    #[track_caller]
    fn from(value: PrimSlice<'a, P>) -> Self {
        value.prims().into()
    }
}

impl<'a, P> From<PrimSlice<'a, P>> for Rc<[P]>
where
    P: PrimPod,
{
    #[inline(always)]
    #[track_caller]
    fn from(value: PrimSlice<'a, P>) -> Self {
        value.prims().into()
    }
}

impl<'a, P> From<PrimSlice<'a, P>> for Arc<[P]>
where
    P: PrimPod,
{
    #[inline(always)]
    #[track_caller]
    fn from(value: PrimSlice<'a, P>) -> Self {
        value.prims().into()
    }
}

// impl<'a, P> From<PrimSlice<'a, P>> for Vec<P>
// where

impl<'a, P> IntoIterator for PrimSlice<'a, P>
where
    P: PrimPod,
{
    type Item = &'a P;
    type IntoIter = std::slice::Iter<'a, P>;

    #[inline(always)]
    fn into_iter(self) -> Self::IntoIter {
        self.prims().iter()
    }
}

impl<'a, P> IntoIterator for &PrimSlice<'a, P>
where
    P: PrimPod,
{
    type Item = &'a P;
    type IntoIter = std::slice::Iter<'a, P>;

    #[inline(always)]
    fn into_iter(self) -> Self::IntoIter {
        self.prims().iter()
    }
}

impl<'a, P> IntoIterator for &mut PrimSlice<'a, P>
where
    P: PrimPod,
{
    type Item = &'a P;
    type IntoIter = std::slice::Iter<'a, P>;

    #[inline(always)]
    fn into_iter(self) -> Self::IntoIter {
        self.prims().iter()
    }
}

/// A mutable slice of primitive PODs, as well as the trailing padding, if any.
#[repr(transparent)]
pub struct PrimSliceMut<'a, P>
where
    P: PrimPod,
{
    inner: RawSlice<P>,
    _slice: PhantomData<&'a mut [P]>,
    _padding: PhantomData<Option<&'a mut P::Padding>>,
}

impl<'a, P> PrimSliceMut<'a, P>
where
    P: PrimPod,
{
    /// Decode a mutable primitive slice from the start of its header.
    #[inline(always)]
    #[allow(unused_unsafe)]
    pub const fn decode(
        bytes: &'a mut [Byte]
    ) -> Result<(PrimSliceMut<'a, P>, &'a mut [Byte])> {
        // SAFETY: We know a mutable slice is a valid allocation.
        match unsafe { RawSlice::decode(NonNull::from_mut(bytes)) } {
            Ok((slice, mut rest)) => {
                // SAFETY: We know that `slice` is derived from `bytes`.
                let slice: PrimSliceMut<'a, P> = unsafe {
                    PrimSliceMut {
                        inner: slice,
                        _slice: PhantomData,
                        _padding: PhantomData,
                    }
                };

                // SAFETY: We know `rest` is derived from `bytes`.
                let rest: &'a mut [Byte] = unsafe { rest.as_mut() };

                Ok((slice, rest))
            },
            Err(err) => Err(err),
        }
    }

    /// Parse a primitive slice given a buffer and the amount of elements.
    #[inline(always)]
    #[allow(unused_unsafe)]
    pub const fn parse(
        bytes: &'a mut [Byte],
        len: u32,
    ) -> Result<(PrimSliceMut<'a, P>, &'a mut [Byte])> {
        // SAFETY: We know a mutable slice is a valid allocation.
        match unsafe { RawSlice::parse(NonNull::from_mut(bytes), len) } {
            Ok((slice, mut rest)) => {
                // SAFETY: We know that `slice` is derived from `bytes`.
                let slice: PrimSliceMut<'a, P> = unsafe {
                    PrimSliceMut {
                        inner: slice,
                        _slice: PhantomData,
                        _padding: PhantomData,
                    }
                };

                // SAFETY: We know that `rest` is derived from `bytes`.
                let rest: &'a mut [Byte] = unsafe { rest.as_mut() };

                Ok((slice, rest))
            },
            Err(err) => Err(err),
        }
    }

    /// Create an empty [`PrimSliceMut`].
    #[inline(always)]
    #[must_use]
    pub const fn empty() -> PrimSliceMut<'a, P> {
        const {
            match PrimSliceMut::parse(&mut [], 0) {
                Ok((slice, _)) => slice,
                Err(_) => unreachable!(),
            }
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

    /// Consume this slice and construct an [`PrimSlice`] from it.
    #[inline(always)]
    #[must_use]
    #[allow(unused_unsafe)]
    pub const fn into_slice(self) -> PrimSlice<'a, P> {
        // SAFETY: It is always sound to demote a mutable reference to
        //         an immutable one of the same lifetime if you
        //         take ownership of it.
        unsafe {
            PrimSlice {
                inner: self.inner,
                _slice: PhantomData,
                _padding: PhantomData,
            }
        }
    }

    /// Reborrow this slice as a [`PrimSlice`].
    #[inline(always)]
    #[must_use]
    #[allow(unused_unsafe)]
    pub const fn as_slice(&self) -> PrimSlice<'_, P> {
        // SAFETY: It is always sound to reborrow a mutable reference
        //         as an immutable one for some shorter lifetime.
        unsafe {
            PrimSlice {
                inner: self.inner,
                _slice: PhantomData,
                _padding: PhantomData,
            }
        }
    }

    /// Mutably reborrow this slice as a [`PrimSliceMut`].
    #[inline(always)]
    #[must_use]
    #[allow(unused_unsafe)]
    pub const fn as_slice_mut(&mut self) -> PrimSliceMut<'_, P> {
        // SAFETY: It is always sound to reborrow a mutable reference
        //         as a mutable reference for some shorter lifetime.
        unsafe {
            PrimSliceMut {
                inner: self.inner,
                _slice: PhantomData,
                _padding: PhantomData,
            }
        }
    }

    /// Get the underlying bytes of this slice, including the padding.
    #[inline(always)]
    #[must_use]
    pub const fn as_bytes(&self) -> &[Byte] {
        // SAFETY: We know that the underlying allocation is valid,
        //         and we're shortening the lifetime.
        unsafe { self.inner.as_bytes().as_ref() }
    }

    /// Mutably get the underlying bytes of this slice, including the padding.
    #[inline(always)]
    #[must_use]
    pub const fn as_bytes_mut(&mut self) -> &mut [Byte] {
        // SAFETY: We know that the underlying allocation is valid,
        //         and we're shortening the lifetime.
        unsafe { self.inner.as_bytes().as_mut() }
    }

    /// Consume this slice, returning the underlying the bytes, including the padding.
    #[inline(always)]
    #[must_use]
    pub const fn into_bytes(self) -> &'a mut [Byte] {
        // SAFETY: We know that the underlying allocation is valid,
        //         and we're consuming `self`, so we're not going to
        //         accidentally alias the underlying memory.
        unsafe { self.inner.as_bytes().as_mut() }
    }

    /// Split the underlying bytes at the start of the padding.
    #[inline(always)]
    #[must_use]
    pub const fn split_bytes(&self) -> (&[Byte], &[Byte]) {
        let (slice, padding) = self.inner.split_bytes();

        // SAFETY: We know that both allocations are valid, and
        //         we're shortening the lifetime, so there will
        //         be no aliased mutability.
        (unsafe { slice.as_ref() }, unsafe { padding.as_ref() })
    }

    /// Mutably split the underyling bytes at the start of the padding.
    #[inline(always)]
    #[must_use]
    pub const fn split_bytes_mut(&mut self) -> (&mut [Byte], &mut [Byte]) {
        let (mut slice, mut padding) = self.inner.split_bytes();

        // SAFETY: We know that both allocations are valid, and
        //         we're shortening the lifetime, so there
        //         will be no aliased mutability.
        (unsafe { slice.as_mut() }, unsafe { padding.as_mut() })
    }

    /// Consume this slice and split at the start of the padding.
    #[inline(always)]
    #[must_use]
    pub const fn into_split_bytes(self) -> (&'a mut [Byte], &'a mut [Byte]) {
        let (mut slice, mut padding) = self.inner.split_bytes();

        // SAFETY: We know that both allocations are valid,
        //         and we're allowed to use the lifetime of the
        //         underlying buffer as we're consuming `self`,
        //         thus there will be no aliased mutability.
        (unsafe { slice.as_mut() }, unsafe { padding.as_mut() })
    }

    /// Split into the underling primitive slice and the padding, if there is any.
    ///
    /// # Returns
    ///
    /// The padding will always be set to [`None`] for types that don't need padding.
    #[inline(always)]
    #[must_use]
    pub const fn split(&self) -> (&[P], Option<&P::Padding>) {
        let (slice, padding) = self.inner.split();

        // SAFETY: We know that the underlying slice is valid,
        //         and we're shortening the lifetime, so we're
        //         not going to encounter aliased mutability.
        let slice = unsafe { slice.as_ref() };

        let padding = match padding {
            // SAFETY: Same as above.
            Some(padding) => Some(unsafe { padding.as_ref() }),
            None => None,
        };

        (slice, padding)
    }

    /// Mutably split into the underlying primitive slice and the padding,
    /// if there is any.
    ///
    /// # Returns
    ///
    /// The padding will always be set to [`None`] for types that don't need
    /// padding.
    #[inline(always)]
    #[must_use]
    pub const fn split_mut(&mut self) -> (&mut [P], Option<&mut P::Padding>) {
        let (mut slice, mut padding) = self.inner.split();

        // SAFETY: We know that the underlying slice is valid, and
        //         we're shortening the lifetime, so we won't be introducing
        //         any aliased mutability.
        let slice = unsafe { slice.as_mut() };

        let padding = match padding {
            // SAFETY: Same as above.
            Some(ref mut padding) => Some(unsafe { padding.as_mut() }),
            None => None,
        };

        (slice, padding)
    }

    /// Consume this slice and split into the underlying primitive slice and
    /// the padding, if there is any.
    ///
    /// # Returns
    ///
    /// The padding will always be set to [`None`] for types that don't need padding.
    #[inline(always)]
    #[must_use]
    pub const fn into_split(self) -> (&'a mut [P], Option<&'a mut P::Padding>) {
        let (mut slice, mut padding) = self.inner.split();

        // SAFETY: We know that the underlying slice is valid, and
        //         we're consuming `self` so it's sound to borrow for `'a`.
        let slice = unsafe { slice.as_mut() };

        let padding = match padding {
            // SAFETY: Same as above.
            Some(ref mut padding) => Some(unsafe { padding.as_mut() }),
            None => None,
        };

        (slice, padding)
    }

    /// Get a reference to the underlying primitive slice.
    #[inline(always)]
    #[must_use]
    pub const fn prims(&self) -> &[P] {
        self.split().0
    }

    /// Get a reference to the underlying padding, if there's any.
    ///
    /// # Returns
    ///
    /// This will always return [`None`] for types that don't need padding.
    #[inline(always)]
    #[must_use]
    pub const fn padding(&self) -> Option<&P::Padding> {
        self.split().1
    }

    /// Get a mutable reference to the underlying primitive slice.
    #[inline(always)]
    #[must_use]
    pub const fn prims_mut(&mut self) -> &mut [P] {
        self.split_mut().0
    }

    /// Get a mutable reference to the underlying padding, if there's any.
    ///
    /// # Returns
    ///
    /// This will always return [`None`] for types that don't need padding.
    #[inline(always)]
    #[must_use]
    pub const fn padding_mut(&mut self) -> Option<&mut P::Padding> {
        self.split_mut().1
    }

    /// Consume this slice and return the underlying primitive slice.
    #[inline(always)]
    #[must_use]
    pub const fn into_prims(self) -> &'a mut [P] {
        self.into_split().0
    }

    /// Consume this slice and return the underlying padding, if there's any.
    ///
    /// # Returns
    ///
    /// This will always return [`None`] for types that don't need padding.
    #[inline(always)]
    #[must_use]
    pub const fn into_padding(self) -> Option<&'a mut P::Padding> {
        self.into_split().1
    }

    /// Returns an iterator over the slice.
    ///
    /// This is equivalent to calling `self.prims().iter()`.
    #[inline(always)]
    pub fn iter(&self) -> std::slice::Iter<'_, P> {
        self.prims().iter()
    }

    /// Returns a mutable iterator over the slice.
    ///
    /// This is equivalent to calling `self.prims_mut().iter_mut()`.
    #[inline(always)]
    pub fn iter_mut(&mut self) -> std::slice::IterMut<'_, P> {
        self.prims_mut().iter_mut()
    }
}

// SAFETY: Since this is functionally a `(&'a mut [P], Option<&'a mut P::Padding>)`,
//         we follow the rules of mutable references. Mutable references are only safe
//         to send to another thread if their referent is `Send`. So, since our fields
//         are covariant over `&'a mut [P]` and `&'a mut P::Padding`, but invariant
//         over `P` and `P::Padding`, we can only implement `Send` when both `P` and
//         `P::Padding` are `Send`.
unsafe impl<'a, P> Send for PrimSliceMut<'a, P>
where
    P: PrimPod + Send,
    P::Padding: Send,
{
}

// SAFETY: Since this is functionally a `(&'a mut [P], Option<&'a mut P::Padding>)`,
//         we follow the rules of mutable references. Mutable references are only
//         safe to share with another thread if their referent is `Sync`. Thus, since
//         our fields are covariant over `&'a mut [P]` and `&'a mut P::Padding`, but
//         invariant over `P` and `P::Padding`, we can only implement `Sync` when both
//         `P` and `P::Padding` are `Sync`.
unsafe impl<'a, P> Sync for PrimSliceMut<'a, P>
where
    P: PrimPod + Sync,
    P::Padding: Sync,
{
}

impl<'a, P> fmt::Debug for PrimSliceMut<'a, P>
where
    P: fmt::Debug + PrimPod,
    P::Padding: fmt::Debug,
{
    fn fmt(
        &self,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        let (slice, padding) = self.split();

        f.debug_struct("PrimSliceMut")
            .field("slice", &slice)
            .field("padding", &padding)
            .finish()
    }
}

impl<'a, P> Default for PrimSliceMut<'a, P>
where
    P: PrimPod,
{
    #[inline(always)]
    fn default() -> Self {
        PrimSliceMut::empty()
    }
}

impl<'a, P> AsRef<[P]> for PrimSliceMut<'a, P>
where
    P: PrimPod,
{
    #[inline(always)]
    fn as_ref(&self) -> &[P] {
        self.prims()
    }
}

impl<'a, P> AsMut<[P]> for PrimSliceMut<'a, P>
where
    P: PrimPod,
{
    #[inline(always)]
    fn as_mut(&mut self) -> &mut [P] {
        self.prims_mut()
    }
}

impl<'a, P> Borrow<[P]> for PrimSliceMut<'a, P>
where
    P: PrimPod,
{
    #[inline(always)]
    fn borrow(&self) -> &[P] {
        self.prims()
    }
}

impl<'a, P> BorrowMut<[P]> for PrimSliceMut<'a, P>
where
    P: PrimPod,
{
    #[inline(always)]
    fn borrow_mut(&mut self) -> &mut [P] {
        self.prims_mut()
    }
}

impl<'a, P, I> Index<I> for PrimSliceMut<'a, P>
where
    P: PrimPod,
    [P]: Index<I>,
{
    type Output = <[P] as Index<I>>::Output;

    #[inline(always)]
    #[track_caller]
    fn index(
        &self,
        index: I,
    ) -> &Self::Output {
        self.prims().index(index)
    }
}

impl<'a, P, I> IndexMut<I> for PrimSliceMut<'a, P>
where
    P: PrimPod,
    [P]: IndexMut<I>,
{
    #[inline(always)]
    #[track_caller]
    fn index_mut(
        &mut self,
        index: I,
    ) -> &mut Self::Output {
        self.prims_mut().index_mut(index)
    }
}

impl<'a, P> IntoIterator for PrimSliceMut<'a, P>
where
    P: PrimPod,
{
    type Item = &'a mut P;
    type IntoIter = std::slice::IterMut<'a, P>;

    #[inline(always)]
    fn into_iter(self) -> Self::IntoIter {
        self.into_prims().iter_mut()
    }
}

impl<'a, 'b, P> IntoIterator for &'b PrimSliceMut<'a, P>
where
    P: PrimPod,
{
    type Item = &'b P;
    type IntoIter = std::slice::Iter<'b, P>;

    #[inline(always)]
    fn into_iter(self) -> Self::IntoIter {
        self.prims().iter()
    }
}

impl<'a, 'b, P> IntoIterator for &'b mut PrimSliceMut<'a, P>
where
    P: PrimPod,
{
    type Item = &'b mut P;
    type IntoIter = std::slice::IterMut<'b, P>;

    #[inline(always)]
    fn into_iter(self) -> Self::IntoIter {
        self.prims_mut().iter_mut()
    }
}

impl<'a, P> From<PrimSliceMut<'a, P>> for PrimSlice<'a, P>
where
    P: PrimPod,
{
    #[inline(always)]
    fn from(value: PrimSliceMut<'a, P>) -> Self {
        value.into_slice()
    }
}

impl<'a, P> TryFrom<&'a mut [P]> for PrimSliceMut<'a, P>
where
    P: PrimPod,
{
    type Error = ParsePodError;

    #[inline(always)]
    fn try_from(value: &'a mut [P]) -> Result<Self, Self::Error> {
        match u32::try_from(value.len()) {
            Ok(len) => PrimSliceMut::parse(as_bytes_mut(value), len)
                .map(|(slice, _)| slice),
            Err(_) => {
                hint::cold_path();
                Err(ParsePodError::InsufficientSpace)
            },
        }
    }
}

impl<'a, P> From<PrimSliceMut<'a, P>> for &'a mut [P]
where
    P: PrimPod,
{
    #[inline(always)]
    fn from(value: PrimSliceMut<'a, P>) -> Self {
        value.into_prims()
    }
}

impl<'a, P> From<PrimSliceMut<'a, P>>
    for (&'a mut [P], Option<&'a mut P::Padding>)
where
    P: PrimPod,
{
    #[inline(always)]
    fn from(value: PrimSliceMut<'a, P>) -> Self {
        value.into_split()
    }
}

impl<'a, P> From<PrimSliceMut<'a, P>> for &'a [P]
where
    P: PrimPod,
{
    #[inline(always)]
    fn from(value: PrimSliceMut<'a, P>) -> Self {
        value.into_prims()
    }
}

impl<'a, P> From<PrimSliceMut<'a, P>> for (&'a [P], Option<&'a P::Padding>)
where
    P: PrimPod,
{
    #[inline(always)]
    fn from(value: PrimSliceMut<'a, P>) -> Self {
        let (slice, padding) = value.into_split();

        (slice, padding.map(|pad| &*pad))
    }
}

impl<'a, P, const N: usize> TryFrom<&'a mut [P; N]> for PrimSliceMut<'a, P>
where
    P: PrimPod,
{
    type Error = ParsePodError;

    #[inline(always)]
    fn try_from(value: &'a mut [P; N]) -> Result<Self, Self::Error> {
        value.as_mut_slice().try_into()
    }
}

impl<'a, P, const N: usize> TryFrom<PrimSliceMut<'a, P>> for &'a mut [P; N]
where
    P: PrimPod,
{
    type Error = array::TryFromSliceError;

    #[inline(always)]
    fn try_from(value: PrimSliceMut<'a, P>) -> Result<Self, Self::Error> {
        value.into_prims().try_into()
    }
}

impl<'a, P, const N: usize> TryFrom<PrimSliceMut<'a, P>>
    for (&'a mut [P; N], Option<&'a mut P::Padding>)
where
    P: PrimPod,
{
    type Error = array::TryFromSliceError;

    #[inline(always)]
    fn try_from(value: PrimSliceMut<'a, P>) -> Result<Self, Self::Error> {
        let (slice, padding) = value.into_split();

        slice.try_into().map(|array| (array, padding))
    }
}

impl<'a, P, const N: usize> TryFrom<PrimSliceMut<'a, P>> for &'a [P; N]
where
    P: PrimPod,
{
    type Error = array::TryFromSliceError;

    #[inline(always)]
    fn try_from(value: PrimSliceMut<'a, P>) -> Result<Self, Self::Error> {
        value.into_slice().try_into()
    }
}

impl<'a, P, const N: usize> TryFrom<PrimSliceMut<'a, P>>
    for (&'a [P; N], Option<&'a P::Padding>)
where
    P: PrimPod,
{
    type Error = array::TryFromSliceError;

    #[inline(always)]
    fn try_from(value: PrimSliceMut<'a, P>) -> Result<Self, Self::Error> {
        let (slice, padding) = value.into_split();

        (&*slice)
            .try_into()
            .map(|array| (array, padding.map(|pad| &*pad)))
    }
}

impl<'a, P, const N: usize> TryFrom<PrimSliceMut<'a, P>> for [P; N]
where
    P: PrimPod,
{
    type Error = array::TryFromSliceError;

    #[inline(always)]
    fn try_from(value: PrimSliceMut<'a, P>) -> Result<Self, Self::Error> {
        value.into_prims().try_into()
    }
}

impl<'a, P> From<PrimSliceMut<'a, P>> for Vec<P>
where
    P: PrimPod,
{
    #[inline(always)]
    #[track_caller]
    fn from(value: PrimSliceMut<'a, P>) -> Self {
        value.into_prims().to_vec()
    }
}

impl<'a, P> From<PrimSliceMut<'a, P>> for Box<[P]>
where
    P: PrimPod,
{
    #[inline(always)]
    #[track_caller]
    fn from(value: PrimSliceMut<'a, P>) -> Self {
        value.into_prims().into()
    }
}

impl<'a, P> From<PrimSliceMut<'a, P>> for Rc<[P]>
where
    P: PrimPod,
{
    #[inline(always)]
    #[track_caller]
    fn from(value: PrimSliceMut<'a, P>) -> Self {
        value.into_prims().into()
    }
}

impl<'a, P> From<PrimSliceMut<'a, P>> for Arc<[P]>
where
    P: PrimPod,
{
    #[inline(always)]
    #[track_caller]
    fn from(value: PrimSliceMut<'a, P>) -> Self {
        value.into_prims().into()
    }
}

#[test]
fn test_fuck_me() {
    #[repr(C, packed)]
    struct Packet {
        pod_header: SpaHeader,
        array_header: SpaHeader,
        data: [super::SpaInt; 127],
        padding: [Byte; 4],
    }

    unsafe impl crate::mem::AsBytes for Packet {}

    let packet = Packet {
        pod_header: SpaHeader {
            kind: SpaKind::ARRAY,
            size: size_of::<Packet>()
                .strict_sub(size_of::<SpaHeader>())
                .strict_sub(size_of::<<super::SpaInt as PrimPod>::Padding>())
                .try_into()
                .unwrap(),
        },
        array_header: SpaHeader {
            kind: SpaKind::INT,
            size: super::SpaInt::SIZE,
        },
        data: [0.into(); _],
        padding: [Byte::new(0); _],
    };

    let (ints, rest) =
        PrimSlice::<super::SpaInt>::decode(as_bytes(&packet)).unwrap();

    for &super::SpaInt(int) in ints {
        println!("Integer go brrr: {int}");
    }

    assert!(rest.is_empty());

    assert_eq!(ints.len() as usize, packet.data.len());
}
