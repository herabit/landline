use std::{
    cmp::Ordering,
    hint,
    num::{NonZero, TryFromIntError},
};

use crate::{
    mem::{AsBytes, AsBytesMut, Byte},
    pod::{PrimPod, kind::SpaKind, sealed},
};

/// A SPA Fraction.
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct SpaFraction {
    /// The numerator.
    pub numer: u32,
    /// The denomiator.
    pub denom: u32,
}

impl SpaFraction {
    /// Perform a checked `(numer / denom, numer % denom)`.
    #[inline(always)]
    #[must_use]
    pub const fn checked_div_rem(self) -> Option<(u32, u32)> {
        match (
            self.numer.checked_div(self.denom),
            self.numer.checked_rem(self.denom),
        ) {
            (Some(div), Some(rem @ 0)) => {
                // SAFETY: If the remainder is zero, then we know `numer` is exactly divisible by `denom`.
                unsafe { hint::assert_unchecked(div.unchecked_mul(self.denom) == self.numer) };
                Some((div, rem))
            },
            (Some(div), Some(rem @ 1..)) => Some((div, rem)),
            (None, None) => {
                hint::cold_path();
                None
            },
            (None, Some(_)) | (Some(_), None) => unreachable!(),
        }
        // match NonZero::new(self.denom) {
        //     Some(denom) => Some((self.numer / denom.get(), self.numer % denom.get())),
        //     None => None,
        // }
    }

    /// Returns whether this fraction is defined. A fraction is considered undefined
    /// if its denominator is zero.
    #[inline(always)]
    #[must_use]
    pub const fn is_defined(self) -> bool {
        self.denom != 0
    }

    /// Returns whether this fraction is representable as an integer.
    #[inline(always)]
    #[must_use]
    pub const fn is_integer(self) -> bool {
        matches!(self.checked_div_rem(), Some((_, 0)))
    }

    /// Returns whether this fraction is equivalent to the specified [`u32`].
    #[inline(always)]
    #[must_use]
    pub const fn eq_u32(
        self,
        value: u32,
    ) -> bool {
        self.is_defined() && (self.denom as u64).strict_mul(value as u64) == self.numer as u64
    }

    /// Returns this fraction as a [`u32`], if it can be represented as one losslessly.
    ///
    /// # Returns
    ///
    /// Returns [`None`] if `denom == 0 || numer % denom != 0`.
    #[inline(always)]
    #[must_use]
    pub const fn to_u32(self) -> Option<u32> {
        match self.checked_div_rem() {
            Some((int_part, 0)) => Some(int_part),
            None | Some((_, 1..)) => {
                hint::cold_path();

                None
            },
        }
    }

    /// Rounds this fraction towards zero, returning the integer part
    /// of it.
    ///
    /// # Returns
    ///
    /// Returns [`None`] if `denom == 0`.
    #[inline(always)]
    #[must_use]
    pub const fn trunc(self) -> Option<SpaFraction> {
        match self.checked_div_rem() {
            Some((int_part, _rem)) => Some(SpaFraction {
                numer: int_part,
                denom: 1,
            }),
            None => None,
        }
    }

    /// Returns the fractional part of this fraction, where the corollary form of
    /// division rounds towards zero. `self.trunc() + self.fract() == self` is true.
    #[inline(always)]
    #[must_use]
    pub const fn fract(self) -> Option<SpaFraction> {
        match self.checked_div_rem() {
            Some((_int_part, rem)) => Some(SpaFraction {
                numer: rem,
                denom: self.denom,
            }),
            None => None,
        }
    }

    /// Rounds this fraction towards negative infinity. Since our fractions
    /// are unsigned, this is equivalent to [`SpaFraction::trunc`].
    ///
    /// # Returns
    ///
    /// Returns [`None`] if `denom == 0`.
    #[inline(always)]
    #[must_use]
    pub const fn floor(self) -> Option<SpaFraction> {
        self.trunc()
    }

    /// Rounds this fraction towards positive infinity.
    ///
    /// # Returns
    ///
    /// Returns [`None`] if `denom == 0`.
    #[inline(always)]
    #[must_use]
    pub const fn ceil(self) -> Option<SpaFraction> {
        match self.checked_div_rem() {
            Some((int_part, 0)) => Some(SpaFraction {
                numer: int_part,
                denom: 1,
            }),
            Some((int_part, 1..)) => Some(SpaFraction {
                // SAFETY: The existence of a remainder at all implies it is sound to increment
                //         by up to and including, the remainder. We're only incrementing by one,
                //         and the remainder is at least one, thus this is sound.
                numer: unsafe { int_part.unchecked_add(1) },
                denom: 1,
            }),
            None => None,
        }
    }

    /// Flips the numerator and denominator.
    #[inline(always)]
    #[must_use]
    pub const fn flip(self) -> SpaFraction {
        SpaFraction {
            numer: self.denom,
            denom: self.numer,
        }
    }

    /// Returns the reciprocal of this fraction.
    ///
    /// # Returns
    ///
    /// Returns [`None`] if this fraction is equal to zero, or undefined. In other words,
    /// this will only return [`Some`] if `numer != 0 && denom != 0`.
    ///
    /// If you want to flip the numerator and denominator without regard of the validity,
    /// use [`SpaFraction::flip`].
    #[inline(always)]
    #[must_use]
    pub const fn recip(self) -> Option<SpaFraction> {
        if self.numer != 0 && self.denom != 0 {
            Some(self.flip())
        } else {
            None
        }
    }
}

impl PartialEq for SpaFraction {
    #[inline(always)]
    fn eq(
        &self,
        other: &Self,
    ) -> bool {
        matches!(self.partial_cmp(other), Some(Ordering::Equal))
    }

    #[inline(always)]
    #[allow(clippy::partialeq_ne_impl)]
    fn ne(
        &self,
        other: &Self,
    ) -> bool {
        matches!(
            self.partial_cmp(other),
            None | Some(Ordering::Greater) | Some(Ordering::Less)
        )
    }
}

impl Eq for SpaFraction {}

impl PartialOrd for SpaFraction {
    #[inline(always)]
    fn partial_cmp(
        &self,
        other: &Self,
    ) -> Option<Ordering> {
        let &SpaFraction {
            numer: lhs_numer,
            denom: lhs_denom,
        } = self;
        let &SpaFraction {
            numer: rhs_numer,
            denom: rhs_denom,
        } = other;

        // Dividing by zero is undefined.
        let (lhs_denom, rhs_denom) = NonZero::new(lhs_denom).zip(NonZero::new(rhs_denom))?;

        if (lhs_numer == 0) && (rhs_numer == 0) {
            // If both numerators are zero, then we know them to be equivalent regardless of the denominator.
            Some(Ordering::Equal)
        } else if lhs_denom == rhs_denom {
            // If the denominator is equivalent, then we must compare the numerators.
            Some(lhs_numer.cmp(&rhs_numer))
        } else if lhs_numer == rhs_numer {
            // If the numerator is equivalent, then we must compare the denominators in reverse order (larger denominators are smaller numbers
            // in this case).
            Some(rhs_denom.cmp(&lhs_denom))
        } else {
            // If they're all not equal, then we upcast to a `u64`, and compare the multiplied numerators. This should
            // be faster than doing a `div` or similar.
            let lhs = (lhs_numer as u64).strict_mul(rhs_denom.get() as u64);
            let rhs = (rhs_numer as u64).strict_mul(lhs_denom.get() as u64);

            Some(lhs.cmp(&rhs))
        }
    }
}

// TODO: Fix the hash implementation. Until then, no hashing!

// impl hash::Hash for SpaFraction {
//     #[inline(always)]
//     fn hash<H>(
//         &self,
//         state: &mut H,
//     ) where
//         H: hash::Hasher,
//     {
//         { self.numer }.hash(state);
//         { self.denom }.hash(state);
//     }
// }

impl Default for SpaFraction {
    #[inline(always)]
    fn default() -> Self {
        Self::DEFAULT
    }
}

impl From<(u32, u32)> for SpaFraction {
    #[inline(always)]
    fn from(value: (u32, u32)) -> Self {
        SpaFraction {
            numer: value.0,
            denom: value.1,
        }
    }
}

impl From<SpaFraction> for (u32, u32) {
    #[inline(always)]
    fn from(value: SpaFraction) -> Self {
        (value.numer, value.denom)
    }
}

impl From<(u32, NonZero<u32>)> for SpaFraction {
    #[inline(always)]
    fn from(value: (u32, NonZero<u32>)) -> Self {
        SpaFraction {
            numer: value.0,
            denom: value.1.get(),
        }
    }
}

impl TryFrom<SpaFraction> for (u32, NonZero<u32>) {
    type Error = TryFromIntError;

    #[inline(always)]
    fn try_from(value: SpaFraction) -> Result<Self, Self::Error> {
        Ok((value.numer, value.denom.try_into()?))
    }
}

impl From<(NonZero<u32>, u32)> for SpaFraction {
    #[inline(always)]
    fn from(value: (NonZero<u32>, u32)) -> Self {
        SpaFraction {
            numer: value.0.get(),
            denom: value.1,
        }
    }
}

impl TryFrom<SpaFraction> for (NonZero<u32>, u32) {
    type Error = TryFromIntError;

    #[inline(always)]
    fn try_from(value: SpaFraction) -> Result<Self, Self::Error> {
        Ok((value.numer.try_into()?, value.denom))
    }
}

impl From<(NonZero<u32>, NonZero<u32>)> for SpaFraction {
    #[inline(always)]
    fn from(value: (NonZero<u32>, NonZero<u32>)) -> Self {
        SpaFraction {
            numer: value.0.get(),
            denom: value.1.get(),
        }
    }
}

impl TryFrom<SpaFraction> for (NonZero<u32>, NonZero<u32>) {
    type Error = TryFromIntError;

    #[inline(always)]
    fn try_from(value: SpaFraction) -> Result<Self, Self::Error> {
        Ok((value.numer.try_into()?, value.denom.try_into()?))
    }
}

impl From<[u32; 2]> for SpaFraction {
    #[inline(always)]
    fn from(value: [u32; 2]) -> Self {
        SpaFraction {
            numer: value[0],
            denom: value[1],
        }
    }
}

impl From<SpaFraction> for [u32; 2] {
    #[inline(always)]
    fn from(value: SpaFraction) -> Self {
        [value.numer, value.denom]
    }
}

impl From<[NonZero<u32>; 2]> for SpaFraction {
    #[inline(always)]
    fn from(value: [NonZero<u32>; 2]) -> Self {
        SpaFraction {
            numer: value[0].get(),
            denom: value[1].get(),
        }
    }
}

impl TryFrom<SpaFraction> for [NonZero<u32>; 2] {
    type Error = TryFromIntError;

    #[inline(always)]
    fn try_from(value: SpaFraction) -> Result<Self, Self::Error> {
        let fract: (NonZero<u32>, NonZero<u32>) = value.try_into()?;

        Ok(fract.into())
    }
}

impl sealed::PrimPod for SpaFraction {}

unsafe impl PrimPod for SpaFraction {
    type Padding = [Byte; 0];

    const DEFAULT: Self = SpaFraction { numer: 0, denom: 0 };
    const SPA_KIND: SpaKind = SpaKind::Fraction;
}

unsafe impl AsBytes for SpaFraction {}
unsafe impl AsBytesMut for SpaFraction {}
