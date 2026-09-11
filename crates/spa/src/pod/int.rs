use std::{fmt, num::TryFromIntError};

use crate::{
    mem::{AsBytes, AsBytesMut, Byte},
    pod::{PrimPod, kind::SpaKind, sealed},
};

/// A SPA 32-bit signed integer.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[repr(C, packed)]
pub struct SpaInt(pub i32);

impl fmt::Debug for SpaInt {
    #[inline(always)]
    fn fmt(
        &self,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        { self.0 }.fmt(f)
    }
}

impl fmt::Display for SpaInt {
    #[inline(always)]
    fn fmt(
        &self,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        { self.0 }.fmt(f)
    }
}

impl From<i32> for SpaInt {
    #[inline(always)]
    fn from(value: i32) -> Self {
        SpaInt(value)
    }
}

impl From<SpaInt> for i32 {
    #[inline(always)]
    fn from(value: SpaInt) -> Self {
        value.0
    }
}

impl TryFrom<isize> for SpaInt {
    type Error = TryFromIntError;

    #[inline(always)]
    fn try_from(value: isize) -> Result<Self, Self::Error> {
        value.try_into().map(SpaInt)
    }
}

impl From<SpaInt> for isize {
    #[inline(always)]
    fn from(value: SpaInt) -> Self {
        // NOTE: We only support platforms where `isize::BITS >= 32`.
        value.0.try_into().unwrap()
    }
}

impl sealed::PrimPod for SpaInt {}
unsafe impl PrimPod for SpaInt {
    type Padding = [Byte; 4];

    const DEFAULT: Self = SpaInt(0);
    const SPA_KIND: SpaKind = SpaKind::Int;
}

unsafe impl AsBytes for SpaInt {}
unsafe impl AsBytesMut for SpaInt {}
