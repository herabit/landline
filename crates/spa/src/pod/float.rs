use std::fmt;

use crate::{
    mem::{AsBytes, AsBytesMut},
    pod::{AsPrimPod, AsPrimPodMut, Byte, PrimPod, kind::SpaKind, sealed},
};

/// A SPA 32-bit IEE-754 single precision floating point number.
#[derive(Clone, Copy, PartialEq, PartialOrd, Default)]
#[repr(C, packed)]
pub struct SpaFloat(pub f32);

impl From<f32> for SpaFloat {
    #[inline(always)]
    fn from(value: f32) -> Self {
        SpaFloat(value)
    }
}

impl From<SpaFloat> for f32 {
    #[inline(always)]
    fn from(value: SpaFloat) -> Self {
        value.0
    }
}

impl fmt::Debug for SpaFloat {
    #[inline(always)]
    fn fmt(
        &self,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        { self.0 }.fmt(f)
    }
}

impl fmt::Display for SpaFloat {
    #[inline(always)]
    fn fmt(
        &self,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        { self.0 }.fmt(f)
    }
}

impl sealed::PrimPod for SpaFloat {}

unsafe impl PrimPod for SpaFloat {
    type Padding = [Byte; 4];

    const DEFAULT: Self = SpaFloat(0.0);
    const SPA_KIND: SpaKind = SpaKind::Float;
}

unsafe impl AsBytes for SpaFloat {}
unsafe impl AsBytesMut for SpaFloat {}

// SAFETY: `SpaFloat` is an unaligned wrapper for a `f32`.
unsafe impl AsPrimPod<SpaFloat> for f32 {}
unsafe impl AsPrimPodMut<SpaFloat> for f32 {}
