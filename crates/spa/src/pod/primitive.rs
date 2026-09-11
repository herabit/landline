use std::{
    borrow::{Borrow, BorrowMut},
    fmt,
    ops::{Deref, DerefMut},
};

use crate::{
    mem::{AsBytes, AsBytesMut, Byte},
    pod::{kind::SpaKind, sealed},
};

/// A trait for the types that are supported as padding for [`PrimPod`]s.
///
/// # Safety
///
/// Implementors must have an alignment of one, and have a size that is some multiple of four (including zero).
pub unsafe trait PrimPad:
    'static + Copy + AsBytes + AsBytesMut + Unpin + Default + fmt::Debug + Send + Sync + sealed::PrimPad
{
    /// A default value for this type.
    const DEFAULT: Self;
}

impl sealed::PrimPad for [Byte; 0] {}
impl sealed::PrimPad for [Byte; 4] {}

unsafe impl<const N: usize> PrimPad for [Byte; N]
where
    [Byte; N]: sealed::PrimPad + Default,
{
    const DEFAULT: Self = [Byte::new(0); N];
}

/// A trait for POD types that are considered primitive. Primitive PODs are those with a fixed size known at compile time,
/// and a fixed stride, among other things.
///
/// # Safety
///
/// All POD types must have an alignment of `1`.
///
/// Likely some other stuff that needs to be hashed out.
pub unsafe trait PrimPod:
    'static + Copy + AsBytes + AsBytesMut + Unpin + Default + fmt::Debug + sealed::PrimPod
{
    /// The tail padding for this type.
    ///
    /// # Safety
    ///
    /// This must have an alignment of `1`.
    type Padding: PrimPad;

    /// This is a default value for `Self`.
    const DEFAULT: Self;

    /// The default padding value.
    const PADDING_DEFAULT: Self::Padding = <Self::Padding as PrimPad>::DEFAULT;

    /// The [`SpaKind`] for this type.
    const SPA_KIND: SpaKind;

    /// The [`SpaKind`] of this type, but as a [`u32`].
    const KIND: u32 = Self::SPA_KIND as u32;

    /// The expected size of this type.
    const SIZE: u32 = {
        let size = Self::SPA_KIND
            .expected_size()
            .expect("all primitive PODs have a constant size");

        assert!(
            size as usize == size_of::<Self>(),
            "the expected size must match the actual size"
        );

        size
    };

    /// The expected size with padding.
    const STRIDE: u32 = {
        let stride = Self::SIZE.checked_next_multiple_of(8).expect(
            "all primitive PODs have a constant size which can be rounded up to a multiple of 8",
        );

        assert!(
            size_of::<PrimBody<Self>>() == stride as usize,
            "the stride must be equal to the size of `PrimBody<Self>`"
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

        assert!(
            matches!(
                (Self::SIZE, padding_size),
                | (0, 0) // Zero sized primitives are known.
                | (4, 4) // u32-ish sized primitives are known.
                | (8, 0) // u64-ish sized primitives are known.
                | (16, 0) // u128-ish sized primitives are known.
            ),
            "we're not aware of this padding/size pair, and therefore the stride."
        );

        stride
    };

    /// The size of the padding.
    const PADDING_SIZE: u32 = Self::STRIDE.strict_sub(Self::SIZE);
}

/// A primitive SPA POD without the header, but containing the padding.
///
/// This is is used in as the body for primitive PODs of `P`, or as the elements in an array of `P`.
#[repr(C, packed)]
pub struct PrimBody<P>
where
    P: PrimPod,
{
    /// The actual value of the body.
    pub value: P,
    /// The padding of the body.
    pub padding: P::Padding,
}

impl<P> PrimBody<P>
where
    P: PrimPod,
{
    /// A default [`PrimBody`], containing the default value for `P` and the default padding.
    pub const DEFAULT: PrimBody<P> = PrimBody::new(P::DEFAULT);

    #[inline(always)]
    #[track_caller]
    pub(crate) const fn ensure_layout() {
        const {
            assert!(
                align_of::<P>() == 1,
                "primitive pods are always 1-byte aligned"
            )
        };

        const {
            assert!(
                align_of::<P::Padding>() == 1,
                "primitive pod paddings are always 1-byte aligned"
            )
        };

        // Run the checks in these constants.
        let _ = const { (P::SIZE, P::PADDING_SIZE, P::STRIDE) };
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

    /// Create a new [`PrimBody`] given some value.
    #[inline(always)]
    #[must_use]
    pub const fn new(value: P) -> PrimBody<P> {
        Self::ensure_layout();

        PrimBody {
            value,
            padding: P::PADDING_DEFAULT,
        }
    }
}

impl<P> Clone for PrimBody<P>
where
    P: PrimPod,
{
    #[inline(always)]
    fn clone(&self) -> Self {
        *self
    }
}

impl<P> Copy for PrimBody<P> where P: PrimPod {}

impl<P> Default for PrimBody<P>
where
    P: PrimPod,
{
    #[inline(always)]
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl<P> fmt::Debug for PrimBody<P>
where
    P: PrimPod,
{
    fn fmt(
        &self,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        let (value, padding) = self.split();

        f.debug_struct("PrimBody")
            .field("value", value)
            .field("padding", padding)
            .finish()
    }
}

impl<P> AsRef<P> for PrimBody<P>
where
    P: PrimPod,
{
    #[inline(always)]
    fn as_ref(&self) -> &P {
        self.as_value()
    }
}

impl<P> AsMut<P> for PrimBody<P>
where
    P: PrimPod,
{
    #[inline(always)]
    fn as_mut(&mut self) -> &mut P {
        self.as_value_mut()
    }
}

impl<P> Borrow<P> for PrimBody<P>
where
    P: PrimPod,
{
    #[inline(always)]
    fn borrow(&self) -> &P {
        self.as_value()
    }
}

impl<P> BorrowMut<P> for PrimBody<P>
where
    P: PrimPod,
{
    #[inline(always)]
    fn borrow_mut(&mut self) -> &mut P {
        self.as_value_mut()
    }
}

impl<P> Deref for PrimBody<P>
where
    P: PrimPod,
{
    type Target = P;

    #[inline(always)]
    fn deref(&self) -> &Self::Target {
        self.as_value()
    }
}

impl<P> DerefMut for PrimBody<P>
where
    P: PrimPod,
{
    #[inline(always)]
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.as_value_mut()
    }
}

// SAFETY: We know that `PrimBody` contains no padding and that its component parts
//         are `AsBytes + AsBytesMut`.

unsafe impl<P> AsBytes for PrimBody<P> where P: PrimPod {}
unsafe impl<P> AsBytesMut for PrimBody<P> where P: PrimPod {}
