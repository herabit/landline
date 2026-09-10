use std::{
    cmp::Ordering,
    hash,
    num::{NonZero, TryFromIntError},
};

use crate::{
    mem::{AsBytes, AsBytesMut, Byte, as_bytes},
    pod::{BasicPod, kind::SpaKind, sealed},
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
        if let Some(denom) = NonZero::new(self.denom) {
            Some((self.numer / denom.get(), self.numer % denom.get()))
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
        let lhs = as_bytes(self)
            .as_array::<8>()
            .copied()
            .map(Byte::into_u8_array)
            .map(u64::from_ne_bytes)
            .unwrap();

        let rhs = as_bytes(other)
            .as_array::<8>()
            .copied()
            .map(Byte::into_u8_array)
            .map(u64::from_ne_bytes)
            .unwrap();

        lhs == rhs
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

impl hash::Hash for SpaFraction {
    #[inline(always)]
    fn hash<H>(
        &self,
        state: &mut H,
    ) where
        H: hash::Hasher,
    {
        { self.numer }.hash(state);
        { self.denom }.hash(state);
    }
}

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

impl sealed::BasicPod for SpaFraction {}

unsafe impl BasicPod for SpaFraction {
    type Padding = [Byte; 0];

    const DEFAULT: Self = SpaFraction { numer: 0, denom: 0 };
    const SPA_KIND: SpaKind = SpaKind::Fraction;
}

unsafe impl AsBytes for SpaFraction {}
unsafe impl AsBytesMut for SpaFraction {}
