//! Just some types relating to the types/kinds of SPA POD values.

// NOTE: I am **very well aware** that this entire module would benefit from the utilization of macros.
//       I just, in my infinite wisdom, did not use one originally, and stubbornness dictates I continue
//       to avoid using macros.
//
//       Lol. Lmfao.

use crate::mem::AsBytes;
use std::mem;

/// A [`u32`] that is used as a tag denoting what type of data a SPA POD holds.
///
/// These values come from `spa/utils/type.h` from values matching the pattern `SPA_TYPE_[A-Z][a-z]+`.
///
/// We don't support `SPA_TYPE_Start`, as it's utterly useless. It's not actually used, `SPA_TYPE_Bitmap` has more of a chance
/// to actually be used (say, you run some old pipewire program).
//
//
// SAFETY: Do not reorder these variants at all.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[doc(alias = "SPA_TYPE")]
#[doc(alias = "SpaType")]
#[repr(u32)]
#[non_exhaustive]
pub enum SpaKind {
    /// No value or a NULL pointer.
    #[doc(alias("SPA_TYPE_None", "NONE"))]
    #[allow(clippy::identity_op)]
    #[default]
    None = 0x0000_0000 + 1,
    /// A boolean value stored as an [`i32`], where all nonzero values are [`true`], and `0` is [`false`].
    #[doc(alias("SPA_TYPE_Bool", "BOOL"))]
    Bool,
    /// An enumerated value stored as a [`u32`] `id` that acts as some index into a table.
    #[doc(alias("SPA_TYPE_Id", "ID"))]
    Id,
    /// A 32-bit signed integer ([`i32`]).
    #[doc(alias("SPA_TYPE_Int", "INT"))]
    Int,
    /// A 64-bit signed integer ([`i64`]).
    #[doc(alias("SPA_TYPE_Long", "LONG"))]
    Long,
    /// A 32-bit single precision floating point value ([`f32`]).
    #[doc(alias("SPA_TYPE_Float", "FLOAT"))]
    Float,
    /// A 64-bit double precision floating point value ([`f64`]).
    #[doc(alias("SPA_TYPE_Double", "DOUBLE"))]
    Double,
    /// A NUL-terminated string of an unspecified encoding.
    #[doc(alias("SPA_TYPE_String", "STRING"))]
    String,
    /// A byte array.
    #[doc(alias("SPA_TYPE_Bytes", "BYTES"))]
    Bytes,
    /// A rectangle.
    #[doc(alias("SPA_TYPE_Rectangle", "RECTANGLE"))]
    Rectangle,
    /// A fraction.
    #[doc(alias("SPA_TYPE_Fraction", "FRACTION"))]
    Fraction,
    /// An array of bits. Deprecated.
    #[doc(alias("SPA_TYPE_Bitmap", "BITMAP"))]
    #[deprecated = "This is a legacy POD type, and is currently unused and will remain so."]
    Bitmap,
    /// An array of POD values with a shared kind and size.
    #[doc(alias("SPA_TYPE_Array", "ARRAY"))]
    Array,
    /// The concatenation of multiple PODs into a single structure.
    #[doc(alias("SPA_TYPE_Struct", "STRUCT"))]
    Struct,
    /// A set of properties.
    #[doc(alias("SPA_TYPE_Object", "OBJECT"))]
    Object,
    /// A series of times events.
    #[doc(alias("SPA_TYPE_Sequence", "SEQUENCE"))]
    Sequence,
    /// A typed pointer somewhere in memory.
    #[doc(alias("SPA_TYPE_Pointer", "POINTER"))]
    Pointer,
    /// A file descriptor that is stored as an [`i64`]. When serialized, this is the index
    /// of the file descriptor in a message.
    #[doc(alias("SPA_TYPE_Fd", "FD"))]
    Fd,
    /// A choice between an array of possible values.
    #[doc(alias("SPA_TYPE_Choice", "CHOICE"))]
    Choice,
    /// The value id of the POD itself.
    #[doc(alias("SPA_TYPE_Pod", "POD"))]
    Pod,
}

/// [`u32`] constants for each [`SpaKind`]. These mainly exist to ease pattern matching
/// of [`u32`]s.
impl SpaKind {
    /// [`SpaKind::None`] as a [`u32`].
    #[doc(alias("SPA_TYPE_None", "None"))]
    pub const NONE: u32 = SpaKind::None as u32;

    /// [`SpaKind::Bool`] as a [`u32`].
    #[doc(alias("SPA_TYPE_Bool", "Bool"))]
    pub const BOOL: u32 = SpaKind::Bool as u32;

    /// [`SpaKind::Id`] as a [`u32`].
    #[doc(alias("SPA_TYPE_Id", "Id"))]
    pub const ID: u32 = SpaKind::Id as u32;

    /// [`SpaKind::Int`] as a [`u32`].
    #[doc(alias("SPA_TYPE_Int", "Int"))]
    pub const INT: u32 = SpaKind::Int as u32;

    /// [`SpaKind::Long`] as a [`u32`].
    #[doc(alias("SPA_TYPE_Long", "Long"))]
    pub const LONG: u32 = SpaKind::Long as u32;

    /// [`SpaKind::Float`] as a [`u32`].
    #[doc(alias("SPA_TYPE_Float", "Float"))]
    pub const FLOAT: u32 = SpaKind::Float as u32;

    /// [`SpaKind::Double`] as a [`u32`].
    #[doc(alias("SPA_TYPE_Double", "Double"))]
    pub const DOUBLE: u32 = SpaKind::Double as u32;

    /// [`SpaKind::String`] as a [`u32`].
    #[doc(alias("SPA_TYPE_String", "String"))]
    pub const STRING: u32 = SpaKind::String as u32;

    /// [`SpaKind::Bytes`] as a [`u32`].
    #[doc(alias("SPA_TYPE_Bytes", "Bytes"))]
    pub const BYTES: u32 = SpaKind::Bytes as u32;

    /// [`SpaKind::Rectangle`] as a [`u32`].
    #[doc(alias("SPA_TYPE_Rectangle", "Rectangle"))]
    pub const RECTANGLE: u32 = SpaKind::Rectangle as u32;

    /// [`SpaKind::Fraction`] as a [`u32`].
    #[doc(alias("SPA_TYPE_Fraction", "Fraction"))]
    pub const FRACTION: u32 = SpaKind::Fraction as u32;

    /// [`SpaKind::Bitmap`] as a [`u32`].
    #[doc(alias("SPA_TYPE_Bitmap", "Bitmap"))]
    #[allow(deprecated)]
    #[deprecated = "This is a legacy POD type, and is currently unused and will remain so."]
    pub const BITMAP: u32 = SpaKind::Bitmap as u32;

    /// [`SpaKind::Array`] as a [`u32`].
    #[doc(alias("SPA_TYPE_Array", "Array"))]
    pub const ARRAY: u32 = SpaKind::Array as u32;

    /// [`SpaKind::Struct`] as a [`u32`].
    #[doc(alias("SPA_TYPE_Struct", "Struct"))]
    pub const STRUCT: u32 = SpaKind::Struct as u32;

    /// [`SpaKind::Object`] as a [`u32`].
    #[doc(alias("SPA_TYPE_Object", "Object"))]
    pub const OBJECT: u32 = SpaKind::Object as u32;

    /// [`SpaKind::Sequence`] as a [`u32`].
    #[doc(alias("SPA_TYPE_Sequence", "Sequence"))]
    pub const SEQUENCE: u32 = SpaKind::Sequence as u32;

    /// [`SpaKind::Pointer`] as a [`u32`].
    #[doc(alias("SPA_TYPE_Pointer", "Pointer"))]
    pub const POINTER: u32 = SpaKind::Pointer as u32;

    /// [`SpaKind::Fd`] as a [`u32`].
    #[doc(alias("SPA_TYPE_Fd", "Fd"))]
    pub const FD: u32 = SpaKind::Fd as u32;

    /// [`SpaKind::Choice`] as a [`u32`].
    #[doc(alias("SPA_TYPE_Choice", "Choice"))]
    pub const CHOICE: u32 = SpaKind::Choice as u32;

    /// [`SpaKind::Pod`] as a [`u32`].
    #[doc(alias("SPA_TYPE_Pod", "Pod"))]
    pub const POD: u32 = SpaKind::Pod as u32;
}

impl SpaKind {
    /// Gets the [`SpaKind`] for a given primitive POD.
    ///
    /// This is equivalent to just doing `<P as PrimPod>::SPA_KIND`,
    /// but maybe this is more readable.
    #[inline(always)]
    #[must_use]
    #[track_caller]
    pub const fn of<P>() -> SpaKind
    where
        P: super::PrimPod,
    {
        <P as super::PrimPod>::SPA_KIND
    }

    /// Get the size of this SPA kind, if it is a known constant (a primitive SPA).
    #[inline(always)]
    #[must_use]
    pub const fn expected_size(self) -> Option<u32> {
        match self {
            // 0-sized types.
            SpaKind::None => Some(0),
            // 32-bit types.
            SpaKind::Bool | SpaKind::Id | SpaKind::Int | SpaKind::Float => {
                Some(4)
            },
            // 64-bit types.
            SpaKind::Long
            | SpaKind::Double
            | SpaKind::Rectangle
            | SpaKind::Fraction
            | SpaKind::Fd => Some(8),
            // 128-bit types (Currently only pointers, they contain a `SpaKind`, the pointer, and padding).
            SpaKind::Pointer => Some(16),
            // Variable-sized types don't have a constant size.
            _ => None,
        }
    }

    /// Returns whether this is a primitive kind.
    #[inline(always)]
    #[must_use]
    pub const fn is_primitive(self) -> bool {
        self.expected_size().is_some()
    }

    /// Attempt to create a [`SpaKind`] from a [`u32`].
    #[inline(always)]
    #[must_use]
    pub const fn from_u32(kind: u32) -> Option<SpaKind> {
        // SAFETY: We need to ensure these bounds are kept in sync.
        const MIN: u32 = SpaKind::None as u32;
        const MAX: u32 = SpaKind::Pod as u32;

        if let MIN..=MAX = kind {
            // SAFETY: We know the value to be in bounds.
            Some(unsafe { mem::transmute::<u32, SpaKind>(kind) })
        } else {
            None
        }
    }
}

// SAFETY: `AsBytes` is only ever used to read the bytes of a type,
//         and `SpaKind` is just a `u32` with extra steps.
unsafe impl AsBytes for SpaKind {}

/// An enum representing what exactly a SPA POD pointer points to.
//
// SAFETY: Do not reorder the variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[doc(alias = "SPA_TYPE_POINTER")]
#[doc(alias = "SpaTypePointer")]
#[repr(u32)]
#[non_exhaustive]
pub enum SpaPointerKind {
    /// It points to a SPA buffer... I think.
    #[doc(alias = "SPA_TYPE_POINTER_Buffer")]
    #[default]
    Buffer = 0x0001_0000 + 1,
    /// It points to a SPA buffer's metadata... I think.
    #[doc(alias = "SPA_TYPE_POINTER_Meta")]
    Meta,
    /// It points to a SPA dictionary... I think.
    #[doc(alias = "SPA_TYPE_POINTER_Dict")]
    Dict,
}

impl SpaPointerKind {
    /// Attempt to create a [`SpaPointerKind`] from a [`u32`].
    #[inline(always)]
    #[must_use]
    pub const fn from_u32(kind: u32) -> Option<SpaPointerKind> {
        // SAFETY: We need to ensure these bounds are kept in sync.
        const MIN: u32 = SpaPointerKind::Buffer as u32;
        const MAX: u32 = SpaPointerKind::Dict as u32;

        if let MIN..=MAX = kind {
            // SAFETY: We know the value to be in bounds.
            Some(unsafe { mem::transmute::<u32, SpaPointerKind>(kind) })
        } else {
            None
        }
    }
}

unsafe impl AsBytes for SpaPointerKind {}

/// An enum representing the possible SPA POD events.
//
// SAFETY: Do not reorder the variants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[doc(alias = "SPA_TYPE_EVENT")]
#[doc(alias = "SpaTypeEvent")]
#[repr(u32)]
#[non_exhaustive]
pub enum SpaEventKind {
    /// Device event.
    #[doc(alias = "SPA_TYPE_EVENT_Device")]
    #[default]
    Device = 0x0002_0000 + 1,
    /// Node event.
    #[doc(alias = "SPA_TYPE_EVENT_Node")]
    Node,
}

impl SpaEventKind {
    /// Attempt to create a [`SpaEventKind`] from a [`u32`].
    #[inline(always)]
    #[must_use]
    pub const fn from_u32(kind: u32) -> Option<SpaEventKind> {
        // SAFETY: We need to ensure these bounds are kept in sync.
        const MIN: u32 = SpaEventKind::Device as u32;
        const MAX: u32 = SpaEventKind::Node as u32;

        if let MIN..=MAX = kind {
            // SAFETY: We know the value to be in bounds.
            Some(unsafe { mem::transmute::<u32, SpaEventKind>(kind) })
        } else {
            None
        }
    }
}

unsafe impl AsBytes for SpaEventKind {}

/// An enum representing the possible SPA POD commands.
//
// SAFETY: Do not reorder fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[doc(alias = "SPA_TYPE_COMMAND")]
#[doc(alias = "SpaTypeCommand")]
#[repr(u32)]
#[non_exhaustive]
pub enum SpaCommandKind {
    /// Device command.
    #[doc(alias = "SPA_TYPE_COMMAND_Device")]
    #[default]
    Device = 0x0003_0000 + 1,
    /// Node command.
    #[doc(alias = "SPA_TYPE_COMMAND_Node")]
    Node,
}

impl SpaCommandKind {
    /// Attempt to create a [`SpaCommandKind`] from a [`u32`].
    #[inline(always)]
    #[must_use]
    pub const fn from_u32(kind: u32) -> Option<SpaCommandKind> {
        // SAFETY: We need to ensure these bounds are kept in sync.
        const MIN: u32 = SpaCommandKind::Device as u32;
        const MAX: u32 = SpaCommandKind::Node as u32;

        if let MIN..=MAX = kind {
            // SAFETY: We know the value to be in bounds.
            Some(unsafe { mem::transmute::<u32, SpaCommandKind>(kind) })
        } else {
            None
        }
    }
}

unsafe impl AsBytes for SpaCommandKind {}

/// An enum representing the possible SPA POD objects.
//
// SAFETY: Do not reorder fields.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[doc(alias = "SPA_TYPE_OBJECT")]
#[doc(alias = "SpaTypeObject")]
#[repr(u32)]
#[non_exhaustive]
pub enum SpaObjectKind {
    /// PropInfo.
    #[doc(alias = "SPA_TYPE_OBJECT_PropInfo")]
    #[default]
    PropInfo = 0x0004_0000 + 1,
    /// Props.
    #[doc(alias = "SPA_TYPE_OBJECT_Props")]
    Props,
    /// Format.
    #[doc(alias = "SPA_TYPE_OBJECT_Format")]
    Format,
    /// ParamBuffers.
    #[doc(alias = "SPA_TYPE_OBJECT_ParamBuffers")]
    ParamBuffers,
    /// ParamMeta.
    #[doc(alias = "SPA_TYPE_OBJECT_ParamMeta")]
    ParamMeta,
    /// ParamIO.
    #[doc(alias = "SPA_TYPE_OBJECT_ParamIO")]
    #[doc(alias = "SPA_TYPE_OBJECT_ParamIo")]
    ParamIo,
    /// ParamProfile.
    #[doc(alias = "SPA_TYPE_OBJECT_ParamProfile")]
    ParamProfile,
    /// ParamPortConfig.
    #[doc(alias = "SPA_TYPE_OBJECT_ParamPortConfig")]
    ParamPortConfig,
    /// ParamRoute.
    #[doc(alias = "SPA_TYPE_OBJECT_ParamRoute")]
    ParamRoute,
    /// Profiler.
    #[doc(alias = "SPA_TYPE_OBJECT_Profiler")]
    Profiler,
    /// ParamLatency.
    #[doc(alias = "SPA_TYPE_OBJECT_ParamLatency")]
    ParamLatency,
    /// ParamProcessLatency.
    #[doc(alias = "SPA_TYPE_OBJECT_ParamProcessLatency")]
    ParamProcessLatency,
}

impl SpaObjectKind {
    /// Alias for [`SpaObjectKind::ParamIo`].
    #[doc(alias = "SPA_TYPE_OBJECT_ParamIO")]
    #[doc(alias = "SPA_TYPE_OBJECT_ParamIo")]
    #[allow(non_upper_case_globals)]
    pub const ParamIO: SpaObjectKind = SpaObjectKind::ParamIo;

    /// Attempt to create a [`SpaObjectKind`] from a [`u32`].
    #[inline(always)]
    #[must_use]
    pub const fn from_u32(kind: u32) -> Option<SpaObjectKind> {
        // SAFETY: We need to ensure these bounds are kept in sync.
        const MIN: u32 = SpaObjectKind::PropInfo as u32;
        const MAX: u32 = SpaObjectKind::ParamProcessLatency as u32;

        if let MIN..=MAX = kind {
            // SAFETY: We know the value to be in bounds.
            Some(unsafe { mem::transmute::<u32, SpaObjectKind>(kind) })
        } else {
            None
        }
    }
}

unsafe impl AsBytes for SpaObjectKind {}

/// An enum representing the possible SPA POD vendors.
//
// SAFETY: Do not edit the field values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
#[doc(alias = "SPA_TYPE_VENDOR")]
#[doc(alias = "SpaTypeVendor")]
#[repr(u32)]
#[non_exhaustive]
pub enum SpaVendorKind {
    /// PipeWire vendor.
    #[doc(alias = "SPA_TYPE_VENDOR_PipeWire")]
    #[default]
    PipeWire = 0x0200_0000,
    /// Other vendor.
    #[doc(alias = "SPA_TYPE_VENDOR_Other")]
    Other = 0x7f00_0000,
}

impl SpaVendorKind {
    /// Attempt to create a [`SpaVendorKind`] from a [`u32`].
    #[inline(always)]
    #[must_use]
    pub const fn from_u32(kind: u32) -> Option<SpaVendorKind> {
        const PIPE_WIRE: u32 = SpaVendorKind::PipeWire as u32;
        const OTHER: u32 = SpaVendorKind::Other as u32;

        if let PIPE_WIRE | OTHER = kind {
            // SAFETY: We know the value to be a valid vendor.
            Some(unsafe { mem::transmute::<u32, SpaVendorKind>(kind) })
        } else {
            None
        }
    }
}

unsafe impl AsBytes for SpaVendorKind {}
