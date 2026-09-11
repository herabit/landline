// NOTE: The size of pointers are always considered to be 16 bytes.

use std::ffi::c_void;

use crate::{
    mem::{AsBytes, AsBytesMut, Byte},
    pod::{PrimPod, kind::SpaKind, sealed},
};

const PADDING_SIZE: usize = cfg_select! {
    target_pointer_width = "32" => 4,
    target_pointer_width = "64" => 0,
};

/// A SPA Pointer.
#[derive(Clone, Copy, Debug)]
#[repr(C, packed)]
pub struct SpaPointer {
    /// The kind of this pointer.
    pub kind: u32,
    /// Padding bits after the `kind` field.
    ///
    /// This must be set to zero in a well formed POD.
    pub kind_padding: [Byte; 4],
    /// The actual pointer value. The `const` says nothing about whether the underlying pointer
    /// is actually immutable.
    ///
    /// Additionally this may be null.
    pub data: *const c_void,
    /// Trailing padding bits. The type of this is platform dependent:
    ///
    /// - On 32-bit platforms, it is a `[Byte; 4]`.
    /// - On 64-bit platforms, it is a `[Byte; 0]`.
    ///
    /// We don't specify this as the padding for this type in [`PrimPod::Padding`],
    /// as PipeWire specifies that [`SpaPointer`]s must be 16 bytes in size, always. So,
    /// on 32-bit platforms, this struct would suddenly cease to be 16 bytes in size, and
    /// instead be 12 bytes in size. Resulting in a compilation error.
    ///
    /// Rather than make this type special, or something else confusing, we just store the
    /// trailing padding internally.
    pub data_padding: [Byte; PADDING_SIZE],
}

const _: () = assert!(size_of::<SpaPointer>() == 16);

impl SpaPointer {
    /// Get the [`SpaKind`] for what this pointer allegedly points to, given
    /// it's valid.
    ///
    /// # Returns
    ///
    /// Returns [`None`] if the kind isn't supported or valid.
    #[inline(always)]
    #[must_use]
    pub const fn spa_kind(&self) -> Option<SpaKind> {
        SpaKind::from_u32(self.kind)
    }

    /// Create a new [`SpaPointer`] given some raw pointer and a [`SpaKind`].
    #[inline(always)]
    #[must_use]
    #[track_caller]
    pub const fn new(
        kind: SpaKind,
        data: *const c_void,
    ) -> SpaPointer {
        if cfg!(debug_assertions) {
            if data.is_null() {
                assert!(
                    kind as u32 == SpaKind::NONE,
                    "cannot create a null pointer to a `SpaKind` other than `None"
                );
            } else {
                assert!(
                    kind as u32 != SpaKind::NONE,
                    "cannot create a non-null pointer to a `SpaKind` that is `None`"
                )
            }
        }

        SpaPointer {
            kind: kind as u32,
            kind_padding: [Byte::new(0); _],
            data,
            data_padding: [Byte::new(0); _],
        }
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

unsafe impl PrimPod for SpaPointer {
    type Padding = [Byte; 0];

    const DEFAULT: Self = SpaPointer::new(SpaKind::None, std::ptr::null());
    const SPA_KIND: SpaKind = SpaKind::Pointer;
}
