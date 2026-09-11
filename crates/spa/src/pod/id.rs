use std::{fmt, num::TryFromIntError};

use crate::{
    mem::{AsBytes, AsBytesMut, Byte},
    pod::{PrimPod, kind::SpaKind, sealed},
};

/// A SPA id ***without the padding***.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[repr(C, packed)]
pub struct SpaId(pub u32);

impl fmt::Display for SpaId {
    #[inline(always)]
    fn fmt(
        &self,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        { self.0 }.fmt(f)
    }
}

impl From<u32> for SpaId {
    #[inline(always)]
    fn from(value: u32) -> Self {
        SpaId(value)
    }
}

impl From<SpaId> for u32 {
    #[inline(always)]
    fn from(value: SpaId) -> Self {
        value.0
    }
}

impl TryFrom<usize> for SpaId {
    type Error = TryFromIntError;

    #[inline(always)]
    fn try_from(value: usize) -> Result<Self, Self::Error> {
        value.try_into().map(SpaId)
    }
}

impl From<SpaId> for usize {
    #[inline(always)]
    fn from(value: SpaId) -> Self {
        // NOTE: We only support platforms whose `usize::BITS >= 32`.
        value.0.try_into().unwrap()
    }
}

impl sealed::PrimPod for SpaId {}

unsafe impl PrimPod for SpaId {
    type Padding = [Byte; 4];

    const DEFAULT: Self = SpaId(0);
    const SPA_KIND: SpaKind = SpaKind::Id;
}

unsafe impl AsBytes for SpaId {}
unsafe impl AsBytesMut for SpaId {}
