use std::{
    any::{Any, TypeId},
    cmp::Ordering,
    convert::Infallible,
    fmt, hash,
    num::{NonZero, TryFromIntError},
};

use crate::mem::{AsBytes, Byte, as_bytes};

#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
enum PadInner {
    _0 = 0,
    _1 = 1,
    _2 = 2,
    _3 = 3,
    _4 = 4,
    _5 = 5,
    _6 = 6,
    _7 = 7,
}

/// A type that represents the amount of padding required to pad get a multiple-of-eight value.
#[derive(Clone, Copy, PartialEq, Eq)]
#[repr(transparent)]
pub struct Pad8(PadInner);

impl Pad8 {
    /// The minimum amount of padding, zero.
    pub const MIN: Pad8 = Pad8(PadInner::_0);

    /// The maximum amount of padding, seven.
    pub const MAX: Pad8 = Pad8(PadInner::_7);

    /// Create a new [`Pad8`] from some value that can be treated as an integer.
    ///
    /// # Returns
    ///
    /// Returns [`None`] on error, if you need a more informative error, see the
    /// `TryInto<Pad8>` impl for `T`.
    #[inline(always)]
    #[must_use]
    pub const fn new_checked<T>(value: T) -> Option<Pad8>
    where
        T: Pad8Conv,
    {
        match conversions::pad8_from(value) {
            Ok(pad) => Some(pad),
            Err(_) => None,
        }
    }

    /// Create a new [`Pad8`] for some value that can always be turned into a [`Pad8`].
    #[inline(always)]
    #[must_use]
    pub const fn new<T>(value: T) -> Pad8
    where
        T: Pad8Conv + Into<Pad8>,
    {
        Pad8::new_checked(value)
            .expect("the conversion is expected to be fallible")
    }

    /// Attempt to create a `T` from a [`Pad8`].
    ///
    /// # Returns
    ///
    /// Returns [`None`] upon error. If you need an informative error,
    /// see `TryInto<T>` for [`Pad8`].
    #[inline(always)]
    #[must_use]
    pub const fn cast_checked<T>(self) -> Option<T>
    where
        T: Pad8Conv,
    {
        match conversions::pad8_into(self) {
            Ok(value) => Some(value),
            Err(_) => None,
        }
    }

    /// Create a `T` from a [`Pad8`] when errors are impossible.
    #[inline(always)]
    #[must_use]
    pub const fn cast<T>(self) -> T
    where
        T: Pad8Conv + From<Pad8>,
    {
        self.cast_checked()
            .expect("such a conversion shouldn't ever cause an error")
    }

    /// Attempt to calculate the amount of padding needed to round the provided
    /// value to a multiple of eight.
    ///
    /// # Returns
    ///
    /// Returns [`None`] if the calculation failed. If you need a meaningful error,
    /// see [`Pad8::try_needed`].
    #[inline(always)]
    #[must_use]
    pub const fn needed_checked<T>(value: T) -> Option<Pad8>
    where
        T: Pad8Conv,
    {
        match conversions::pad8_of(value) {
            Ok(pad) => Some(pad),
            Err(_) => None,
        }
    }

    /// Calculate the amount of padding needed to round the provided value to a multiple of eight.
    #[inline(always)]
    #[must_use]
    pub const fn needed<T>(value: T) -> Pad8
    where
        T: Pad8Conv<NeededError = Infallible>,
    {
        Pad8::needed_checked(value).expect("infallible conversions cannot fail")
    }

    /// Attempt to calculate the amount of padding needed to round the provided value
    /// to a multiple of eight.
    ///
    /// # Returns
    ///
    /// Returns [`Err`] upon error.
    #[inline(always)]
    #[allow(clippy::toplevel_ref_arg)]
    pub fn try_needed<T>(value: T) -> Result<Pad8, T::NeededError>
    where
        T: Pad8Conv,
    {
        // NOTE: I'm just getting lazy, but whatever, this can't be const rn anyways,
        //       so might as well abuse this.
        match conversions::pad8_of(value) {
            Ok(pad) => Ok(pad),
            Err(_)
                if TypeId::of::<T::NeededError>()
                    == TypeId::of::<Infallible>() =>
            {
                unreachable!("infallible calculation")
            },
            Err(err)
                if TypeId::of::<T::NeededError>()
                    == TypeId::of::<TryFromIntError>() =>
            {
                let ref err = ord_to_error(err);

                (err as &dyn Any)
                    .downcast_ref::<T::NeededError>()
                    .copied()
                    .map(Err)
                    .expect("this should be trivial to optimize away")
            },
            Err(_) => unreachable!("we only support two error types"),
        }
    }

    /// Calculate the needed padding for a type to align its size to a multiple of eight.
    #[inline(always)]
    #[must_use]
    pub const fn of<T>() -> Pad8 {
        Pad8::needed(size_of::<T>())
    }

    /// Calculate the needed padding for a value to align its size to a multiple of eight.
    #[inline(always)]
    #[must_use]
    pub const fn of_val<T>(val: &T) -> Pad8
    where
        T: ?Sized,
    {
        Pad8::needed(size_of_val(val))
    }
}

impl PartialOrd for Pad8 {
    #[inline(always)]
    fn partial_cmp(
        &self,
        other: &Self,
    ) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Pad8 {
    #[inline(always)]
    fn cmp(
        &self,
        other: &Self,
    ) -> Ordering {
        self.cast::<u8>().cmp(&other.cast::<u8>())
    }
}

impl hash::Hash for Pad8 {
    #[inline(always)]
    fn hash<H>(
        &self,
        state: &mut H,
    ) where
        H: hash::Hasher,
    {
        self.cast::<u8>().hash(state);
    }

    #[inline(always)]
    fn hash_slice<H>(
        data: &[Self],
        state: &mut H,
    ) where
        Self: Sized,
        H: hash::Hasher,
    {
        u8::hash_slice(Byte::as_u8_slice(as_bytes(data)), state);
    }
}

// SAFETY: `Pad8` is just a special `u8`... So long as we don't expose the underlying data mutably,
//         we're fine.
unsafe impl AsBytes for Pad8 {}

impl Default for Pad8 {
    #[inline(always)]
    fn default() -> Self {
        Pad8::MIN
    }
}

impl fmt::Debug for Pad8 {
    #[inline(always)]
    #[track_caller]
    fn fmt(
        &self,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        (self.0 as u8).fmt(f)
    }
}

impl fmt::Display for Pad8 {
    #[inline(always)]
    #[track_caller]
    fn fmt(
        &self,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        (self.0 as u8).fmt(f)
    }
}

impl fmt::Binary for Pad8 {
    #[inline(always)]
    #[track_caller]
    fn fmt(
        &self,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        (self.0 as u8).fmt(f)
    }
}

impl fmt::Octal for Pad8 {
    #[inline(always)]
    #[track_caller]
    fn fmt(
        &self,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        (self.0 as u8).fmt(f)
    }
}

impl fmt::LowerHex for Pad8 {
    #[inline(always)]
    #[track_caller]
    fn fmt(
        &self,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        (self.0 as u8).fmt(f)
    }
}

impl fmt::UpperHex for Pad8 {
    #[inline(always)]
    #[track_caller]
    fn fmt(
        &self,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        (self.0 as u8).fmt(f)
    }
}

impl fmt::LowerExp for Pad8 {
    #[inline(always)]
    #[track_caller]
    fn fmt(
        &self,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        (self.0 as u8).fmt(f)
    }
}

impl fmt::UpperExp for Pad8 {
    #[inline(always)]
    #[track_caller]
    fn fmt(
        &self,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        (self.0 as u8).fmt(f)
    }
}

/// Get a `TryFromIntError` for `Pad8 -> ?` conversions.
#[inline(always)]
fn ord_to_error(ord: Ordering) -> TryFromIntError {
    match ord {
        // We use `Less` to denote a negative overflow.
        Ordering::Less => usize::try_from(-1_isize).unwrap_err(),
        // We use `Equal` to represent getting zero for nonzero types.
        Ordering::Equal => NonZero::<usize>::try_from(0_usize).unwrap_err(),
        // We use `Greater` to represent getting a positive overflow.
        Ordering::Greater => isize::try_from(usize::MAX).unwrap_err(),
    }
}

macro_rules! convert_impl {
    (Pad8) => {};

    (bool) => {
        impl TryFrom<Pad8> for bool {
            type Error = TryFromIntError;

            #[inline(always)]
            fn try_from(pad: Pad8) -> Result<bool, TryFromIntError> {
                pad8_into(pad).map_err(super::ord_to_error)
            }
        }

        impl From<bool> for Pad8 {
            #[inline(always)]
            fn from(b: bool) -> Pad8 {
                pad8_from(b).unwrap()
            }
        }
    };

    (#[nonzero] $int:tt) => {
        impl TryFrom<NonZero<$int>> for Pad8 {
            type Error = TryFromIntError;

            #[inline(always)]
            fn try_from(int: NonZero<$int>) -> Result<Pad8, TryFromIntError> {
                pad8_from(int).map_err(super::ord_to_error)
            }
        }

        impl TryFrom<Pad8> for NonZero<$int> {
            type Error = TryFromIntError;

            #[inline(always)]
            fn try_from(pad: Pad8) -> Result<NonZero<$int>, TryFromIntError> {
                pad8_into(pad).map_err(super::ord_to_error)
            }
        }
    };

    ($int:tt) => {
        impl TryFrom<$int> for Pad8 {
            type Error = TryFromIntError;

            #[inline(always)]
            fn try_from(int: $int) -> Result<Pad8, TryFromIntError> {
                pad8_from(int).map_err(super::ord_to_error)
            }
        }

        impl From<Pad8> for $int {
            #[inline(always)]
            fn from(pad: Pad8) -> $int {
                pad8_into(pad).unwrap()
            }
        }
    };
}

macro_rules! get_type {
    (#[nonzero] $ty:ident) => { NonZero<$ty> };
    ($ty:ident) => { $ty };
}

macro_rules! transmute_copy {
    ($input:ident -> $type:ty) => {{
        let input: &_ = $input;

        assert!(size_of_val(input) == size_of::<$type>());
        assert!(align_of_val(input) == align_of::<$type>());

        // SAFETY: The caller ensures that this is safe.
        let value: $type = mem::transmute_copy(input);

        value
    }};
}

macro_rules! pad8_of {
    ($input:ident: Pad8) => {
        pad8_from(*$input)
    };
    ($input:ident: bool) => {
        pad8_from(*$input)
    };
    ($input:ident: #[nonzero] $int:ident) => {{
        // SAFETY: The caller ensures that it is safe to transmute to a `NonZero<$int>`.
        let int: NonZero<$int> = transmute_copy!($input -> NonZero<$int>);

        let pad = match int.get().strict_rem(8) {
            pad @ 0 => pad,
            rem @ 1..8 => (8 as $int).strict_sub(rem),
            8.. => unreachable!(),
            neg_rem => neg_rem,
        };

        match pad8_from(pad) {
            Ok(pad) => {
                if let Some(padded) = int.get().checked_add(pad.0 as _) {
                    // SAFETY: If we can calculate padding without overflow, then we know `padded`
                    //         is evenly divisble by eight.
                    unsafe { hint::assert_unchecked(padded % 8 == 0) };
                    // SAFETY: Same as above.
                    unsafe { hint::assert_unchecked((padded / 8).unchecked_mul(8) == padded) };
                }

                Ok(pad)
            },
            Err(err) => Err(err),
        }
    }};

    ($input:ident: $int:ident) => {{
        // SAFETY: The caller ensures that it is safe to transmute to an `$int`.
        let int: $int = transmute_copy!($input -> $int);

        let pad = match int.strict_rem(8) {
            pad @ 0 => pad,
            rem @ 1..8 => (8 as $int).strict_sub(rem),
            8.. => unreachable!(),
            neg_rem => neg_rem,
        };


        match pad8_from(pad) {
            Ok(pad) => {
                if let Some(padded) = int.checked_add(pad.0 as _) {
                    // SAFETY: If we can calculate padding without overflow, then we know `padded`
                    //         is evenly divisble by eight.
                    unsafe { hint::assert_unchecked(padded % 8 == 0) };
                    // SAFETY: Same as above.
                    unsafe { hint::assert_unchecked((padded / 8).unchecked_mul(8) == padded) };
                }

                Ok(pad)
            },
            Err(err) => Err(err),
        }
    }};
}

macro_rules! pad8_into {
    ($input:ident -> Pad8 == $output:ident) => {{
        let _: &Pad8 = $input;
        // SAFETY: The caller ensures `$output` is `Pad8`.
        let pad_8: $output = transmute_copy!($input -> $output);

        Ok(pad_8)
    }};

    ($input:ident -> bool == $output:ident) => {{
        let _: &Pad8 = $input;

        // SAFETY: The caller ensures that `$output` is `bool`.
        match $input {
            // SAFETY: The caller ensures that `$output` is `bool`, and we know these have
            //         equivalent representations in this case
            b @ Pad8(PadInner::_0 | PadInner::_1) => Ok(transmute_copy!(b -> $output)),
            _ => Err(Ordering::Greater),
        }
    }};

    ($input:ident -> #[nonzero] $int:ident == $output:ident) => {{
        let _: &Pad8 = $input;

        // SAFETY: The caller ensures that `$output` is `NonZero<$int>`.
        match NonZero::new($input.0 as $int) {
            Some(ref int) => Ok(transmute_copy!(int -> $output)),
            None => Err(Ordering::Equal),
        }
    }};

    ($input:ident -> $int:ident == $output:ident) => {{
        let _: &Pad8 = $input;

        let ref int = $input.0 as $int;

        // SAFETY: The caller ensures that `$output` is `$int`.
        Ok(transmute_copy!(int -> $output))
    }};
}

macro_rules! pad8_from {
    ($input:ident: Pad8) => {{
        // SAFETY: The caller ensures that it is safe to transmute to a `Pad8`.
        let pad_8: Pad8 = transmute_copy!($input -> Pad8);

        Ok(pad_8)
    }};

    ($input:ident: bool) => {{
        // SAFETY: The caller ensures that it is safe to transmute to a `bool`.
        let b: bool = transmute_copy!($input -> bool);

        Ok(Pad8(
            if b {
                PadInner::_1
            } else {
                PadInner::_0
            }
        ))
    }};

    ($input:ident: #[nonzero] $int:ident) => {{
        // SAFETY: The caller ensures that it is safe to transmute to a `NonZero<$int>`.
        let int: NonZero<$int> = transmute_copy!($input -> NonZero<$int>);


        match (int.get() < 0, int.get() >= 8) {
            (true, false) => Err(Ordering::Less),
            (false, true) => Err(Ordering::Greater),
            (false, false) => {
                assert!(matches!(int.get(), 0..8));

                // SAFETY: We know that the integer is in range, and thus truncating to
                //         a `u8`, which a `Pad8` is internally, will yield a valid `PadU8`.
                let pad_8: Pad8 = mem::transmute(int.get() as u8);

                assert!(pad_8.0 as u8 != 0);

                Ok(pad_8)
            },
            (true, true) => unreachable!(),
        }
    }};

    ($input:ident: $int:ident) => {{
        // SAFETY: The caller ensures that it is safe to transmute to a `$int`.
        let int: $int = transmute_copy!($input -> $int);

        match (int < 0, int >= 8) {
            (true, false) => Err(Ordering::Less),
            (false, true) => Err(Ordering::Greater),
            (false, false) => {
                assert!(matches!(int, 0..8));

                // SAFETY: We know that the integer is in range, and thus truncating to
                //         a `u8`, which a `Pad8` is internally, will yield a valid `PadU8`.
                let pad_8: Pad8 = mem::transmute(int as u8);

                Ok(pad_8)
            },
            (true, true) => unreachable!(),
        }
    }};
}

pub struct Cond<const COND: bool>;

pub trait Pick<T, F>
where
    T: ?Sized,
    F: ?Sized,
{
    type Output: ?Sized;
}

impl<T, F> Pick<T, F> for Cond<false>
where
    T: ?Sized,
    F: ?Sized,
{
    type Output = F;
}

impl<T, F> Pick<T, F> for Cond<true>
where
    T: ?Sized,
    F: ?Sized,
{
    type Output = T;
}

macro_rules! of_error {
    (Pad8) => {
        Infallible
    };
    (bool) => {
        Infallible
    };
    (#[nonzero] $int:ident) => {
        of_error!($int)
    };
    ($int:ident) => {
        <Cond<{ $int::MIN >= 0 }> as Pick<Infallible, TryFromIntError>>::Output
    };
}

macro_rules! conversions {
    (
        $(
            $variant:ident
                => $( #[nonzero $($nonzero:lifetime)?] )? $type:ident
        ),*
        $(,)?
    ) => {
        mod conversions {
            use std::{
                error,
                cmp::Ordering,
                convert::Infallible,
                num::{TryFromIntError, NonZero},
                mem,
                hint,
            };
            use super::{PadInner, Pad8, Pick, Cond};

            enum ConvKind {
                $($variant,)+
            }

            #[doc(hidden)]
            pub struct ConvWit(ConvKind);

            pub trait Sealed {}

            /// Trait for describing the types that the [`Pad8`] constructor supports.
            #[allow(clippy::missing_safety_doc)]
            pub unsafe trait Pad8Conv:
                'static
                + Copy
                + TryInto<Pad8, Error: 'static + Copy + error::Error + From<Infallible> + Eq>
                + TryFrom<Pad8, Error: 'static + Copy + error::Error + From<Infallible> + Eq>
                + Sealed
            {
                /// An error that is returned when trying to calculate the needed padding for a type.
                type NeededError: 'static + Copy + error::Error + From<Infallible> + Eq;

                /// Implementation detail.
                #[doc(hidden)]
                const WIT: ConvWit;
            }

            #[inline(always)]
            #[track_caller]
            #[allow(unused_comparisons, clippy::toplevel_ref_arg)]
            pub const fn pad8_from<T>(ref input: T) -> Result<Pad8, Ordering>
            where
                T: Pad8Conv,
            {
                match T::WIT.0 {
                    $(
                        ConvKind::$variant => unsafe {
                            pad8_from!(input: $(#[nonzero] $($nonzero)?)? $type)
                        },
                    )*
                }
            }

            #[inline(always)]
            #[track_caller]
            #[allow(clippy::toplevel_ref_arg, unreachable_patterns, unused_unsafe)]
            pub const fn pad8_of<T>(ref input: T) -> Result<Pad8, Ordering>
            where
                T: Pad8Conv,
            {
                match T::WIT.0 {
                    $(
                        ConvKind::$variant => unsafe {
                            pad8_of!(input: $(#[nonzero] $($nonzero)?)? $type)
                        },
                    )*
                }
            }

            #[inline(always)]
            #[track_caller]
            #[allow(clippy::toplevel_ref_arg)]
            pub const fn pad8_into<T>(ref input: Pad8) -> Result<T, Ordering>
            where
                T: Pad8Conv,
            {
                match T::WIT.0 {
                    $(
                        ConvKind::$variant => unsafe {
                            pad8_into!(input -> $(#[nonzero] $($nonzero)?)? $type == T)
                        }
                    )*
                }
            }

            $(
                #[allow(unused_comparisons)]
                const _: () = {
                    #[allow(dead_code)]
                    type This = get_type!($(#[nonzero] $($nonzero)?)? $type);

                    impl Sealed for This {}
                    unsafe impl Pad8Conv for This {
                        type NeededError = of_error!($type);
                        const WIT: ConvWit = ConvWit(ConvKind::$variant);
                    }

                    convert_impl!($(#[nonzero] $($nonzero)?)? $type);
                };
            )*
        }
    };
}

conversions! {
    Pad8 => Pad8,
    Bool => bool,

    U8 => u8, NzU8 => #[nonzero] u8,
    U16 => u16, NzU16 => #[nonzero] u16,
    U32 => u32, NzU32 => #[nonzero] u32,
    U64 => u64, NzU64 => #[nonzero] u64,
    U128 => u128, NzU128 => #[nonzero] u128,
    Usize => usize, NzUsize => #[nonzero] usize,


    I8 => i8, NzI8 => #[nonzero] i8,
    I16 => i16, NzI16 => #[nonzero] i16,
    I32 => i32, NzI32 => #[nonzero] i32,
    I64 => i64, NzI64 => #[nonzero] i64,
    I128 => i128, NzI128 => #[nonzero] i128,
    Isize => isize, NzIsize => #[nonzero] isize,
}

#[doc(inline)]
pub use conversions::Pad8Conv;
