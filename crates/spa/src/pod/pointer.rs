// NOTE: The size of pointers are always considered to be 16 bytes.

use std::ffi::c_void;

use crate::{
    mem::{AsBytes, AsBytesMut, Byte},
    pod::{PrimPod, kind::SpaKind, sealed},
};

/// A SPA Pointer.
#[derive(Clone, Copy, Debug)]
#[repr(C, packed)]
pub struct SpaPointer {
    /// The kind of this pointer.
    pub kind: u32,
    /// Inner padding bits (these must all be set to zero).
    pub inner_padding: [Byte; 4],
    /// The actual pointer value. The `const` says nothing about whether the underlying pointer
    /// is actually immutable.
    ///
    /// Additionally this may be null.
    pub data: *const c_void,
}

impl SpaPointer {
    /// Get the [`SpaKind`] for what this pointer allegedly points to, given
    /// it's valid.
    #[inline(always)]
    #[must_use]
    pub const fn spa_kind(&self) -> Option<SpaKind> {
        SpaKind::from_u32(self.kind)
    }
}

impl Default for SpaPointer {
    #[inline(always)]
    fn default() -> Self {
        SpaPointer::DEFAULT
    }
}

unsafe impl AsBytes for SpaPointer {}
unsafe impl AsBytesMut for SpaPointer {}

impl sealed::PrimPod for SpaPointer {}

const PADDING_SIZE: usize = cfg_select! {
    target_pointer_width = "32" => 4,
    target_pointer_width = "64" => 0,
};

unsafe impl PrimPod for SpaPointer {
    type Padding = [Byte; PADDING_SIZE];

    const DEFAULT: Self = SpaPointer {
        kind: SpaKind::NONE,
        inner_padding: [Byte::new(0); _],
        data: std::ptr::null(),
    };

    const SPA_KIND: SpaKind = SpaKind::Pointer;
}
