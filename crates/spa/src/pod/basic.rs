use std::{
    borrow::{Borrow, BorrowMut},
    fmt, mem,
    ops::{Deref, DerefMut},
};

use crate::{
    mem::{AsBytes, AsBytesMut, Byte},
    pod::{kind::SpaKind, sealed},
};

/// A trait for the types that are supported as padding for [`BasicPod`]s.
///
/// # Safety
///
/// Implementors must have an alignment of one, and have a size that is some multiple of four (including zero).
pub unsafe trait BasicPad:
    'static + Copy + AsBytes + AsBytesMut + Unpin + Default + fmt::Debug + sealed::BasicPad
{
    /// A default value for this type.
    const DEFAULT: Self;
}

impl sealed::BasicPad for [Byte; 0] {}
impl sealed::BasicPad for [Byte; 4] {}

unsafe impl<const N: usize> BasicPad for [Byte; N]
where
    [Byte; N]: sealed::BasicPad + Default,
{
    const DEFAULT: Self = [Byte::new(0); N];
}

/// A trait for POD types that are considered basic. Basic PODs are those with a fixed size known at compile time,
/// and a fixed stride, among other things.
///
/// # Safety
///
/// All POD types must have an alignment of `1`.
///
/// Likely some other stuff that needs to be hashed out.
pub unsafe trait BasicPod:
    'static + Copy + AsBytes + AsBytesMut + Unpin + Default + fmt::Debug + sealed::BasicPod
{
    /// The tail padding for this type.
    ///
    /// # Safety
    ///
    /// This must have an alignment of `1`.
    type Padding: BasicPad;

    /// This is a default value for `Self`.
    const DEFAULT: Self;

    /// A zero-initialized instance of `Padding`.
    const PADDING: Self::Padding = {
        // SAFETY: If something implements `AsBytesMut`, then we can safely fill it with zeroes, and it is inhabited.
        unsafe { mem::zeroed() }
    };

    /// The [`SpaKind`] for this type.
    const SPA_KIND: SpaKind;

    /// The [`SpaKind`] of this type, but as a [`u32`].
    const KIND: u32 = Self::SPA_KIND as u32;

    /// The expected size of this type.
    const SIZE: u32 = {
        let size = Self::SPA_KIND
            .expected_size()
            .expect("all basic PODs have a constant size");

        assert!(
            size as usize == size_of::<Self>(),
            "the expected size must match the actual size"
        );

        size
    };

    /// The expected size with paddings.
    const STRIDE: u32 = {
        let stride = Self::SIZE.checked_next_multiple_of(8).expect(
            "all basic PODs have a constant size which can be rounded up to a multiple of 8",
        );

        assert!(
            size_of::<BasicBody<Self>>() == stride as usize,
            "the stride must be equal to the size of `BasicBody<Self>`"
        );

        assert!(stride % 8 == 0, "the stride must be divisible by eight");

        let padding_size = stride.strict_sub(Self::SIZE);

        assert!(
            size_of::<Self::Padding>() == padding_size as usize,
            "the size of the padding is not equal to `stride - size`",
        );

        assert!(
            padding_size % 4 == 0 && padding_size <= 4,
            "the size of the padding must be zero or four",
        );

        stride
    };
}

/// A basic SPA POD without the header.
///
/// This is is used in as the body for basic PODs of `P`, or as the elements in an array of `P`.
#[repr(C, packed)]
pub struct BasicBody<P>
where
    P: BasicPod,
{
    /// The actual value of the body.
    pub value: P,
    /// The padding of the body.
    pub padding: P::Padding,
}

impl<P> BasicBody<P>
where
    P: BasicPod,
{
    /// A default `BasicBody`, containing the default value for `P` and the default padding.
    pub const DEFAULT: BasicBody<P> = BasicBody::new(P::DEFAULT);

    #[inline(always)]
    #[track_caller]
    const fn ensure_layout() {
        const { assert!(align_of::<P>() == 1, "basic pods are always 1-byte aligned") };

        const {
            assert!(
                align_of::<P::Padding>() == 1,
                "basic pod paddings are always 1-byte aligned"
            )
        };

        // Run the checks in these constants.
        let _ = const { (P::SIZE, P::STRIDE) };
    }

    /// Get a reference to the corresponding value and padding.
    #[inline(always)]
    #[must_use]
    pub const fn split(&self) -> (&P, &P::Padding) {
        Self::ensure_layout();

        // SAFETY: We know `P` and `P::Padding` to be 1-byte aligned.
        unsafe {
            (
                (&raw const self.value).as_ref_unchecked(),
                (&raw const self.padding).as_ref_unchecked(),
            )
        }
    }

    /// Get a mutable reference to the corresponding value and padding.
    #[inline(always)]
    #[must_use]
    pub const fn split_mut(&mut self) -> (&mut P, &mut P::Padding) {
        Self::ensure_layout();

        // SAFETY: We know `P` and `P::Padding` to be 1-byte aligned.
        unsafe {
            (
                (&raw mut self.value).as_mut_unchecked(),
                (&raw mut self.padding).as_mut_unchecked(),
            )
        }
    }

    /// Get a reference to the underlying value.
    #[inline(always)]
    #[must_use]
    pub const fn as_value(&self) -> &P {
        self.split().0
    }

    /// Get a mutable reference to the underlying value.
    #[inline(always)]
    #[must_use]
    pub const fn as_value_mut(&mut self) -> &mut P {
        self.split_mut().0
    }

    /// Get a reference to the underlying padding.
    #[inline(always)]
    #[must_use]
    pub const fn as_padding(&self) -> &P::Padding {
        self.split().1
    }

    /// Get a mutable reference to the underlying padding.
    #[inline(always)]
    #[must_use]
    pub const fn as_padding_mut(&mut self) -> &mut P::Padding {
        self.split_mut().1
    }

    /// Create a new [`BasicBody`] given some value.
    #[inline(always)]
    #[must_use]
    pub const fn new(value: P) -> BasicBody<P> {
        Self::ensure_layout();

        BasicBody {
            value,
            padding: P::PADDING,
        }
    }
}

impl<P> Clone for BasicBody<P>
where
    P: BasicPod,
{
    #[inline(always)]
    fn clone(&self) -> Self {
        *self
    }
}

impl<P> Copy for BasicBody<P> where P: BasicPod {}

impl<P> Default for BasicBody<P>
where
    P: BasicPod,
{
    #[inline(always)]
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl<P> fmt::Debug for BasicBody<P>
where
    P: BasicPod,
{
    fn fmt(
        &self,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        let (value, padding) = self.split();

        f.debug_struct("BasicBody")
            .field("value", value)
            .field("padding", padding)
            .finish()
    }
}

impl<P> AsRef<P> for BasicBody<P>
where
    P: BasicPod,
{
    #[inline(always)]
    fn as_ref(&self) -> &P {
        self.as_value()
    }
}

impl<P> AsMut<P> for BasicBody<P>
where
    P: BasicPod,
{
    #[inline(always)]
    fn as_mut(&mut self) -> &mut P {
        self.as_value_mut()
    }
}

impl<P> Borrow<P> for BasicBody<P>
where
    P: BasicPod,
{
    #[inline(always)]
    fn borrow(&self) -> &P {
        self.as_value()
    }
}

impl<P> BorrowMut<P> for BasicBody<P>
where
    P: BasicPod,
{
    #[inline(always)]
    fn borrow_mut(&mut self) -> &mut P {
        self.as_value_mut()
    }
}

impl<P> Deref for BasicBody<P>
where
    P: BasicPod,
{
    type Target = P;

    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        self.as_value()
    }
}

impl<P> DerefMut for BasicBody<P>
where
    P: BasicPod,
{
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.as_value_mut()
    }
}

// SAFETY: We know that `BasicBody` contains no padding and that its component parts
//         are `AsBytes + AsBytesMut`.

unsafe impl<P> AsBytes for BasicBody<P> where P: BasicPod {}
unsafe impl<P> AsBytesMut for BasicBody<P> where P: BasicPod {}
