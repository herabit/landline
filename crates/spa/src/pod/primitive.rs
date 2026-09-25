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
///
/// Other shit, this is private, so, lol, let me handle this.
pub unsafe trait PrimPad:
    'static
    + Copy
    + AsBytes
    + AsBytesMut
    + Unpin
    + Default
    + fmt::Debug
    + Send
    + Sync
    + sealed::PrimPad
{
    /// A default value for this type.
    const DEFAULT: Self;
}

impl sealed::PrimPad for [Byte; 0] {}
impl sealed::PrimPad for [Byte; 4] {}

/// This is only implemented for `[Byte; 0]` and `[Byte; 4]`.
unsafe impl<const N: usize> PrimPad for [Byte; N]
where
    [Byte; N]: sealed::PrimPad + Default,
{
    const DEFAULT: Self = [Byte::new(0); N];
}

/// Just a helper struct detailing the size, stride, and padding of the primitive types.
struct PrimLayout {
    /// The size of the primitive, in bytes, without padding to a multiple of eight.
    ///
    /// This is the size we expect to find in headers.
    size: u32,
    /// This is the size of the primitive, rounded up to a multiple of eight.
    stride: u32,
    /// This is the difference between the size and stride.
    padding: u32,
}

impl PrimLayout {
    #[track_caller]
    const fn of<P>() -> &'static PrimLayout
    where
        P: PrimPod,
    {
        &const {
            assert!(
                align_of::<P>() == 1,
                "primitive PODs must have an alignment of one"
            );

            assert!(
                align_of::<P::Padding>() == 1,
                "the padding of primitive PODs must have an alignment of one"
            );

            let size = {
                let size = P::SPA_KIND
                    .expected_size()
                    .expect("all primitive PODs have a known, constant size");

                assert!(
                    size as usize == size_of::<P>(),
                    "the expected size of a POD must match its actual size",
                );

                assert!(
                    size == 0 || size.is_power_of_two(),
                    "the size of a primitive POD must be zero or a power of two",
                );

                // NOTE: Probably not needed, but I'd like to check anyways.
                assert!(
                    size.is_multiple_of(4),
                    "all known primitive PODs have a size that is a multiple of four",
                );

                size
            };

            let stride = {
                let stride = size.checked_next_multiple_of(8).expect(
                    "all primitive PODs have a constant size which \
                     can be rounded up to a multiple of eight\
                    ",
                );

                let actual_stride = size_of::<P>()
                    .checked_add(size_of::<P::Padding>())
                    .expect("overflow when calculating the actual stride");

                assert!(
                    stride as usize == actual_stride,
                    "the stride must be equal to the padded size of the primitive POD",
                );

                assert!(
                    stride == 0 || stride.is_power_of_two(),
                    "the stride of a primitive POD must be zero or a power of two",
                );

                // NOTE: This is needless, as we create it with `checked_next_multiple_of(8)`, but whatever.
                assert!(
                    stride.is_multiple_of(8),
                    "the stride of a primitive POD must be a multiple of eight (otherwise, what's the point, lol)",
                );

                stride
            };

            let padding = {
                let padding = stride.checked_sub(size).expect(
                    "somehow we've calculated a size larger than the stride",
                );

                assert!(
                    padding as usize == size_of::<P::Padding>(),
                    "the size of the expected padding be equal to the actual size of the padding",
                );

                assert!(
                    padding == 0 || padding.is_power_of_two(),
                    "the size of the padding for all known primitive PODs is either zero, or a power of two",
                );

                assert!(
                    padding.is_multiple_of(4),
                    "the size of the padding for all known primitive PODs are multiples of four",
                );

                assert!(
                    padding <= 4,
                    "the size of the padding for all known primitive PODs is less than or equal to four",
                );

                assert!(
                    matches!(
                        (size, padding),
                        | (0, 0) // Zero sized primitives.
                        | (4, 4) // u32ish primitives.
                        | (8, 0) // u64ish primitives.
                        | (16, 0) // u128ish primitives.
                    ),
                    "we've encountered an unknown size/padding pair",
                );

                padding
            };

            PrimLayout {
                size,
                stride,
                padding,
            }
        }
    }
}

/// A trait for POD types that are considered primitive. Primitive PODs are those with a fixed size known at compile time,
/// and a fixed stride, among other things.
///
/// # TODO
///
/// > Introduce a new trait that permits converting between primitives and their raw underyling, aligned types,
/// > if they have such a type.
/// >
/// > This would assist in conversions, for example permit in-place conversions of a `[f32]` to a `PrimSlice`,
/// > and so on.
/// >
/// > I'm tired, and likely will be taking a break for a bit, but, this will be added sometime soon.
/// >
/// > ... It may also be entirely unecessary... In *most* scenarios it's, fine. Idk. I *may* add this later.
/// > I shouldn't keep bikeshedding.
///
/// # Safety
///
/// All POD types must have an alignment of `1`.
///
/// Likely some other stuff that needs to be hashed out, hence us keeping this permanently sealed.
pub unsafe trait PrimPod:
    'static
    + Copy
    + AsBytes
    + AsBytesMut
    + Unpin
    + Default
    + fmt::Debug
    + sealed::PrimPod
{
    /// The tail padding for this type.
    ///
    /// # Safety
    ///
    /// This must have an alignment of `1`.
    ///
    /// Other things too, hence why we keep this permanently sealed.
    type Padding: PrimPad;

    /// This is a default value for `Self`.
    const DEFAULT: Self;

    /// The default padding value.
    const PADDING_DEFAULT: Self::Padding = <Self::Padding as PrimPad>::DEFAULT;

    /// The [`SpaKind`] for this type.
    const SPA_KIND: SpaKind;

    /// The [`SpaKind`] of this type, but as a [`u32`].
    ///
    /// This is equivalent to `Self::SPA_KIND as u32`.
    const KIND: u32 = Self::SPA_KIND as u32;

    /// The expected size of this type.
    ///
    /// This is equivalent to `Self::SPA_KIND.expected_size().unwrap()`.
    const SIZE: u32 = PrimLayout::of::<Self>().size;

    /// The expected size with padding.
    ///
    /// This is equivalent to `Self::SIZE.checked_next_multiple_of(8).unwrap()`.
    const STRIDE: u32 = PrimLayout::of::<Self>().stride;

    /// The size of the padding.
    ///
    /// This is equivalent to `Self::STRIDE.strict_sub(Self::SIZE)`.
    const PADDING_SIZE: u32 = PrimLayout::of::<Self>().padding;
}

// const A: () = assert!(<super::SpaBool as PrimPod>::SIZE != 4);

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

    /// Get a reference to the corresponding value and padding.
    #[inline(always)]
    #[must_use]
    pub const fn split(&self) -> (&P, &P::Padding) {
        // NOTE: This just does compile time checks for us.
        const { _ = P::SIZE };

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
        // NOTE: This just does compile time checks for us.
        const { _ = P::SIZE };

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
        // NOTE: This just does compile time checks for us.
        const { _ = P::SIZE };

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

/// Trait for types that can be immutably represented as some primitive POD.
///
/// # Safety
///
/// Implementors must have the same size as the primitive POD they can be coerced into.
/// The coercions must also be safe to do inversely, given that the primitive POD is in fact,
/// adequately aligned. So if we construct a [`SpaDouble`] from a [`f64`], given the value is sufficiently
/// aligned, it must also be safe to transmute back into a [`f64`].
///
/// This being implemented does ***not*** imply that it is safe to reinterpret mutably.
/// For that, see [`AsPrimPodMut`].
///
/// Implementors are allowed to have stricter alignment requirements than their PODs.
///
/// Probably some other things, potentially avoid implementing this yourself, until I hash out the
/// exact invariants more at a later date.
///
pub unsafe trait AsPrimPod<P>: AsBytes
where
    P: PrimPod,
{
}

// SAFETY: It's safe to reinterpret a POD as itself.
unsafe impl<P> AsPrimPod<P> for P where P: PrimPod {}

/// Trait for types that can be mutabled represented as some primitive POD.
///
/// # Safety
///
/// Pretty much the same as [`AsPrimPod`], except having this trait implemented
/// does not imply that it is sound to immutably reinterpret.
///
/// Avoid implementing this until I fully hash out the details of the invariants,
/// I just need to get moving on my project. This is pre-alpha software, don't expect
/// stability, or use this at all. I need to fully flesh things out significantly. But until then,
/// I'm focusing on getting things operational.
///
/// Seriously, do not rely on my code, like, ever.
pub unsafe trait AsPrimPodMut<P>: AsBytesMut
where
    P: PrimPod,
{
}

// SAFETY: It's safe to reinterpret a POD as itself.
unsafe impl<P> AsPrimPodMut<P> for P where P: PrimPod {}
