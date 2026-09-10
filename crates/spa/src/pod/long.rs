use std::{fmt, num::TryFromIntError};

use crate::{
    mem::{AsBytes, AsBytesMut, Byte},
    pod::{BasicPod, kind::SpaKind, sealed},
};

/// A SPA 64-bit signed integer.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[repr(C, packed)]
pub struct SpaLong(pub i64);

impl fmt::Debug for SpaLong {
    #[inline(always)]
    fn fmt(
        &self,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        { self.0 }.fmt(f)
    }
}

impl fmt::Display for SpaLong {
    #[inline(always)]
    fn fmt(
        &self,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        { self.0 }.fmt(f)
    }
}

impl From<i64> for SpaLong {
    #[inline(always)]
    fn from(value: i64) -> Self {
        SpaLong(value)
    }
}

impl From<SpaLong> for i64 {
    #[inline(always)]
    fn from(value: SpaLong) -> Self {
        value.0
    }
}

// In the unlikely event pipewire eventually supports CHERI... lol.
//
// I mean currently, we do just force a compiler error outside of when the
// bit width of a pointer is neither 32 or 64 bits.
//
// Fuck I just remembered the god damn x32 ABI... lord. It shouldn't matter...
// Probably.
impl TryFrom<isize> for SpaLong {
    type Error = TryFromIntError;

    #[inline(always)]
    fn try_from(value: isize) -> Result<Self, Self::Error> {
        value.try_into().map(SpaLong)
    }
}

impl TryFrom<SpaLong> for isize {
    type Error = TryFromIntError;

    #[inline(always)]
    fn try_from(value: SpaLong) -> Result<Self, Self::Error> {
        value.0.try_into()
    }
}

impl sealed::BasicPod for SpaLong {}

unsafe impl BasicPod for SpaLong {
    type Padding = [Byte; 0];

    const DEFAULT: Self = SpaLong(0);
    const SPA_KIND: SpaKind = SpaKind::Long;
}

unsafe impl AsBytes for SpaLong {}
unsafe impl AsBytesMut for SpaLong {}
