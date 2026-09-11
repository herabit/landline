use std::{
    hash,
    num::{NonZero, TryFromIntError},
};

use crate::{
    mem::{AsBytes, AsBytesMut, Byte, as_bytes},
    pod::{PrimPod, kind::SpaKind, sealed},
};

/// A SPA Rectangle.
#[derive(Debug, Clone, Copy)]
#[repr(C, packed)]
pub struct SpaRectangle {
    /// The width of the rectangle.
    pub width: u32,
    /// The height of the rectangle.
    pub height: u32,
}

impl PartialEq for SpaRectangle {
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

impl Eq for SpaRectangle {}

impl hash::Hash for SpaRectangle {
    #[inline(always)]
    fn hash<H>(
        &self,
        state: &mut H,
    ) where
        H: hash::Hasher,
    {
        { self.width }.hash(state);
        { self.height }.hash(state);
    }
}

impl From<(u32, u32)> for SpaRectangle {
    #[inline(always)]
    fn from(value: (u32, u32)) -> Self {
        SpaRectangle {
            width: value.0,
            height: value.1,
        }
    }
}

impl From<SpaRectangle> for (u32, u32) {
    #[inline(always)]
    fn from(value: SpaRectangle) -> Self {
        (value.width, value.height)
    }
}

impl From<(u32, NonZero<u32>)> for SpaRectangle {
    #[inline(always)]
    fn from(value: (u32, NonZero<u32>)) -> Self {
        SpaRectangle {
            width: value.0,
            height: value.1.get(),
        }
    }
}

impl TryFrom<SpaRectangle> for (u32, NonZero<u32>) {
    type Error = TryFromIntError;

    #[inline(always)]
    fn try_from(value: SpaRectangle) -> Result<Self, Self::Error> {
        Ok((value.width, value.height.try_into()?))
    }
}

impl From<(NonZero<u32>, u32)> for SpaRectangle {
    #[inline(always)]
    fn from(value: (NonZero<u32>, u32)) -> Self {
        SpaRectangle {
            width: value.0.get(),
            height: value.1,
        }
    }
}

impl TryFrom<SpaRectangle> for (NonZero<u32>, u32) {
    type Error = TryFromIntError;

    #[inline(always)]
    fn try_from(value: SpaRectangle) -> Result<Self, Self::Error> {
        Ok((value.width.try_into()?, value.height))
    }
}

impl From<(NonZero<u32>, NonZero<u32>)> for SpaRectangle {
    #[inline(always)]
    fn from(value: (NonZero<u32>, NonZero<u32>)) -> Self {
        SpaRectangle {
            width: value.0.get(),
            height: value.1.get(),
        }
    }
}

impl TryFrom<SpaRectangle> for (NonZero<u32>, NonZero<u32>) {
    type Error = TryFromIntError;

    #[inline(always)]
    fn try_from(value: SpaRectangle) -> Result<Self, Self::Error> {
        Ok((value.width.try_into()?, value.height.try_into()?))
    }
}

impl From<[u32; 2]> for SpaRectangle {
    #[inline(always)]
    fn from(value: [u32; 2]) -> Self {
        SpaRectangle {
            width: value[0],
            height: value[1],
        }
    }
}

impl From<SpaRectangle> for [u32; 2] {
    #[inline(always)]
    fn from(value: SpaRectangle) -> Self {
        [value.width, value.height]
    }
}

impl From<[NonZero<u32>; 2]> for SpaRectangle {
    #[inline(always)]
    fn from(value: [NonZero<u32>; 2]) -> Self {
        SpaRectangle {
            width: value[0].get(),
            height: value[1].get(),
        }
    }
}

impl TryFrom<SpaRectangle> for [NonZero<u32>; 2] {
    type Error = TryFromIntError;

    #[inline(always)]
    fn try_from(value: SpaRectangle) -> Result<Self, Self::Error> {
        let fract: (NonZero<u32>, NonZero<u32>) = value.try_into()?;

        Ok(fract.into())
    }
}

impl Default for SpaRectangle {
    #[inline(always)]
    fn default() -> Self {
        SpaRectangle::DEFAULT
    }
}

impl sealed::PrimPod for SpaRectangle {}

unsafe impl PrimPod for SpaRectangle {
    type Padding = [Byte; 0];

    const DEFAULT: Self = SpaRectangle {
        width: 0,
        height: 0,
    };

    const SPA_KIND: SpaKind = SpaKind::Rectangle;
}

unsafe impl AsBytes for SpaRectangle {}
unsafe impl AsBytesMut for SpaRectangle {}
