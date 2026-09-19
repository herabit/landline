//! Types and tools for handling the raw representation of SPA PODs.
//!
//! Validation within this module is done lazily, and consumers are expected to validate payloads
//! themselves, usually using the tools provided within this crate.
//!
//! As for deserialization, we try to avoid copying. Parsing with this module is pull-based,
//! and copying should only ever be incurred by consumers, not us.
//!
//! As such, this module is considered low-level. It just helps to avoid needless allocations and implicit
//! recursion elsewhere.

use std::{convert::Infallible, fmt};

use crate::{
    mem::{AsBytes, AsBytesMut, Byte},
    pod::kind::SpaKind,
};

/// A header that is at the start of every SPA POD, storing the size of the payload,
/// and what kind of POD it is.
//
// TODO: Custom Eq/Ord/Hash impl, maybe, eventually.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(C, packed)]
pub struct SpaHeader {
    /// The size of the POD, excluding this header, in bytes, excluding padding.
    ///
    /// This is ***NOT*** the stride of the POD this refers to. It's really
    /// annoying, but pipewire differentiates between the two.
    ///
    /// The strides of POD data is the size aligned up to a multiple of 8.
    ///
    /// If you need the stride, use [`SpaHeader::stride`].
    pub size: u32,
    /// The kind of POD this header represents. This type does not protect against
    /// invalid kinds, use [`SpaHeader::spa_kind`] to get an enum that does protect
    /// against invalid kinds.
    pub kind: u32,
}

const _: () =
    assert!(size_of::<SpaHeader>() == 8 && align_of::<SpaHeader>() == 1);

impl SpaHeader {
    /// A `SpaHeader` corresponding to the `None` POD type.
    ///
    /// This is the default value.
    pub const NONE: SpaHeader =
        SpaHeader::from_spa_kind(SpaKind::None).unwrap();

    /// Create a [`SpaHeader`] for a given [`SpaKind`], given
    /// it's a primitive [`SpaKind`].
    #[inline(always)]
    #[must_use]
    pub const fn from_spa_kind(spa_kind: SpaKind) -> Option<SpaHeader> {
        match spa_kind.expected_size() {
            Some(size) => Some(SpaHeader {
                kind: spa_kind as u32,
                size,
            }),
            None => None,
        }
    }

    /// Get the [`SpaKind`] of this header, if it is valid.
    #[inline(always)]
    #[must_use]
    pub const fn spa_kind(&self) -> Option<SpaKind> {
        SpaKind::from_u32(self.kind)
    }

    /// Returns the expected size for this header.
    ///
    /// Equialent to `self.spa_kind().and_then(SpaKind::expected_size)`.
    #[inline(always)]
    #[must_use]
    pub const fn expected_size(&self) -> Option<u32> {
        match self.spa_kind() {
            Some(spa_kind) => spa_kind.expected_size(),
            None => None,
        }
    }

    /// Calculate the stride of the POD this header represents,
    /// aligning the size to 8-byte boundaries.
    #[inline(always)]
    #[must_use]
    pub const fn stride(&self) -> Option<u32> {
        self.size.checked_next_multiple_of(8)
    }

    /// Calculate the expected stride of the POD this represents.
    ///
    /// Equivalent to `self.expected_size().and_then(|size| size.checked_multiple_of(8))`.
    #[inline(always)]
    #[must_use]
    pub const fn expected_stride(&self) -> Option<u32> {
        match self.expected_size() {
            Some(size) => size.checked_next_multiple_of(8),
            None => None,
        }
    }

    /// Parse a [`SpaHeader`] given a slice of bytes, returning a tuple of the header
    /// and remaining bytes.
    ///
    /// # Returns
    ///
    /// Returns [`None`] if there's too little space in `bytes`.
    #[inline(always)]
    #[must_use]
    pub const fn split_bytes(bytes: &[Byte]) -> Option<(&SpaHeader, &[Byte])> {
        match bytes.split_first_chunk::<{ size_of::<SpaHeader>() }>() {
            Some((header_chunk, tail)) => Some({
                // SAFETY: `SpaHeader` is just plain old data without any padding bytes nor
                //         any strict alignment requirements, so it's safe to reinterpret
                //         a byte buffer of the same size as a `SpaHeader`.
                let header_chunk = unsafe {
                    (&raw const *header_chunk)
                        .cast::<SpaHeader>()
                        .as_ref_unchecked()
                };

                (header_chunk, tail)
            }),
            None => None,
        }
    }

    /// Parse a [`SpaHeader`] given a slice of bytes.
    ///
    /// # Returns
    ///
    /// Returns [`None`] if there's too little space in `bytes`.
    #[inline(always)]
    #[must_use]
    pub const fn from_bytes(bytes: &[Byte]) -> Option<&SpaHeader> {
        match SpaHeader::split_bytes(bytes) {
            Some((header, _)) => Some(header),
            None => None,
        }
    }

    /// Parse a [`SpaHeader`] given a mutable slice of bytes, returning a tuple of the header
    /// and remaining bytes.
    ///
    /// # Returns
    ///
    /// Returns [`None`] if there's too little space in `bytes`.
    #[inline(always)]
    #[must_use]
    pub const fn split_bytes_mut(
        bytes: &mut [Byte]
    ) -> Option<(&mut SpaHeader, &mut [Byte])> {
        match bytes.split_first_chunk_mut::<{ size_of::<SpaHeader>() }>() {
            Some((header_chunk, tail)) => Some({
                // SAFETY: See the safety info in `SpaHeader::split_bytes`, the same applies here.
                let header_chunk = unsafe {
                    (&raw mut *header_chunk)
                        .cast::<SpaHeader>()
                        .as_mut_unchecked()
                };

                (header_chunk, tail)
            }),
            None => None,
        }
    }

    /// Parse a [`SpaHeader`] given a mutable slice of bytes.
    ///
    /// # Returns
    ///
    /// Returns [`None`] if there's too little space in `bytes`.
    #[inline(always)]
    #[must_use]
    pub const fn from_bytes_mut(bytes: &mut [Byte]) -> Option<&mut SpaHeader> {
        match SpaHeader::split_bytes_mut(bytes) {
            Some((header, _)) => Some(header),
            None => None,
        }
    }
}

// SAFETY: `SpaHeader` is just a `[u32; 2]`.
unsafe impl AsBytes for SpaHeader {}
unsafe impl AsBytesMut for SpaHeader {}

impl Default for SpaHeader {
    #[inline(always)]
    fn default() -> Self {
        SpaHeader::NONE
    }
}

impl fmt::Debug for SpaHeader {
    fn fmt(
        &self,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        let kind = fmt::from_fn(|f| match self.spa_kind() {
            Some(ref spa_kind) => {
                f.debug_tuple("Known").field(&spa_kind).finish()
            },
            None => f.debug_tuple("Unknown").field(&{ self.kind }).finish(),
        });

        f.debug_struct("SpaHeader")
            .field("kind", &kind)
            .field("size", &{ self.size })
            .field("expected_size", &self.expected_size())
            .field("stride", &self.stride())
            .field("expected_stride", &self.expected_stride())
            .finish_non_exhaustive()
    }
}

pub mod kind;

mod primitive;
#[doc(inline)]
pub use primitive::*;

mod none;
#[doc(inline)]
pub use none::*;

mod r#bool;
#[doc(inline)]
pub use r#bool::*;

mod id;
#[doc(inline)]
pub use id::*;

mod int;
#[doc(inline)]
pub use int::*;

mod long;
#[doc(inline)]
pub use long::*;

mod float;
#[doc(inline)]
pub use float::*;

mod double;
#[doc(inline)]
pub use double::*;

mod string;
#[doc(inline)]
pub use string::*;

mod bytes;
#[doc(inline)]
pub use bytes::*;

mod rectangle;
#[doc(inline)]
pub use rectangle::*;

mod fraction;
#[doc(inline)]
pub use fraction::*;

mod bitmap;
#[doc(inline)]
pub use bitmap::*;

// mod array;
// #[doc(inline)]
// pub use array::*;

mod r#struct;
#[doc(inline)]
pub use r#struct::*;

mod object;
#[doc(inline)]
pub use object::*;

mod sequence;
#[doc(inline)]
pub use sequence::*;

mod pointer;
#[doc(inline)]
pub use pointer::*;

mod fd;
#[doc(inline)]
pub use fd::*;

mod choice;
#[doc(inline)]
pub use choice::*;

mod slice;
#[doc(inline)]
pub use slice::*;

mod sealed;

/// Self explanatory, a [`Result`](std::result::Result) that exists
/// to reduce boilerplate.
pub type Result<T, E = ParsePodError> = std::result::Result<T, E>;

/// An error that can occur when parsing a POD.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum ParsePodError {
    /// There is insufficient space to parse the buffer.
    InsufficientSpace,

    /// We encountered an unexpected value.
    UnexpectedValue,

    /// Some other, not yet named error.
    Other,
}

impl From<Infallible> for ParsePodError {
    #[inline(always)]
    fn from(value: Infallible) -> Self {
        match value {}
    }
}
