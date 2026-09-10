use std::fmt;

use crate::{
    mem::{AsBytes, AsBytesMut, Byte},
    pod::{BasicPod, kind::SpaKind, sealed},
};

/// A SPA 64-bit IEE-754 double precision floating point number.
#[derive(Clone, Copy, PartialEq, PartialOrd, Default)]
#[repr(C, packed)]
pub struct SpaDouble(pub f64);

impl From<f64> for SpaDouble {
    #[inline(always)]
    fn from(value: f64) -> Self {
        SpaDouble(value)
    }
}

impl From<SpaDouble> for f64 {
    #[inline(always)]
    fn from(value: SpaDouble) -> Self {
        value.0
    }
}

impl fmt::Debug for SpaDouble {
    #[inline(always)]
    fn fmt(
        &self,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        { self.0 }.fmt(f)
    }
}

impl fmt::Display for SpaDouble {
    #[inline(always)]
    fn fmt(
        &self,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        { self.0 }.fmt(f)
    }
}

impl sealed::BasicPod for SpaDouble {}

unsafe impl BasicPod for SpaDouble {
    type Padding = [Byte; 0];

    const DEFAULT: Self = SpaDouble(0.0);
    const SPA_KIND: SpaKind = SpaKind::Double;
}

unsafe impl AsBytes for SpaDouble {}
unsafe impl AsBytesMut for SpaDouble {}
