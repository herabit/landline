use std::{borrow::Borrow, fmt, hash, hint::assert_unchecked, ops};

use crate::{
    mem::{AsBytes, AsBytesMut, Byte},
    pod::{AsPrimPod, AsPrimPodMut, PrimPod, kind::SpaKind, sealed},
};

/// A SPA Boolean ***without the padding***.
///
/// It wraps an [`i32`] (adhering to the Pipewire Spec), where
/// `0` is [`false`], and any other value (`1`, `2`, `-1`, ...) all
/// map to [`true`].
///
/// As such there is no canonical representation of [`true`]. It'll
/// usually be `1`, but it very well may not be.
///
/// # Representation
///
/// While this is a tuple struct over an [`i32`], it is aligned to 1-byte
/// boundaries. This is counter to how `libpipewire` and `libspa` handles things,
/// where everything is aligned to 8-byte boundaries.
///
/// We pack everything mainly because we don't expect the underlying data we
/// deserialize from, and serialize to, to exist for very long, and instead
/// we expect it to be treated as an intermediate representation.
///
/// We insert padding bytes ourselves to ensure that the actual size of a
/// POD is a multiple-of-8, but all of them will be byte-aligned. It just
/// helps to avoid unnecessary reads, and allows us to use our APIs in `const`
/// potentially, if say some SPA POD payload is static.
///
/// We do not seek compatibility with `libpipewire` or `libspa`. These libraries
/// seek to sever ourselves from them as dependencies, entirely.
#[derive(Clone, Copy)]
#[repr(C, packed)]
pub struct SpaBool(pub i32);

impl SpaBool {
    /// [`true`] as a [`SpaBool`].
    pub const TRUE: SpaBool = SpaBool(true as i32);

    /// [`false`] as a [`SpaBool`].
    pub const FALSE: SpaBool = SpaBool(false as i32);

    /// Create a [`SpaBool`] from a [`bool`].
    #[inline(always)]
    #[must_use]
    pub const fn new(b: bool) -> SpaBool {
        if b { SpaBool::TRUE } else { SpaBool::FALSE }
    }

    /// Cast this [`SpaBool`] to a [`bool`].
    #[inline(always)]
    #[must_use]
    pub const fn as_bool(self) -> bool {
        self.0 != 0
    }

    /// Cast this [`SpaBool`] to a reference to a [`bool`].
    #[inline(always)]
    #[must_use]
    pub const fn as_bool_ref(self) -> &'static bool {
        const TABLE: &[bool; 2] = &[false, true];

        let value = &TABLE[if self.as_bool() { 1 } else { 0 }];

        // SAFETY: We know the value of the bool in the lookup table is equivalent to `self.as_bool()`.
        unsafe { assert_unchecked(*value == self.as_bool()) };

        value
    }
}

impl sealed::PrimPod for SpaBool {}

unsafe impl PrimPod for SpaBool {
    type Padding = [Byte; 4];

    const DEFAULT: Self = SpaBool::FALSE;
    const SPA_KIND: SpaKind = SpaKind::Bool;
}

unsafe impl AsBytes for SpaBool {}
unsafe impl AsBytesMut for SpaBool {}

impl PartialEq for SpaBool {
    #[inline(always)]
    fn eq(
        &self,
        other: &Self,
    ) -> bool {
        self.as_bool() == other.as_bool()
    }
}

impl Eq for SpaBool {}

impl PartialOrd for SpaBool {
    #[inline(always)]
    fn partial_cmp(
        &self,
        other: &Self,
    ) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SpaBool {
    #[inline(always)]
    fn cmp(
        &self,
        other: &Self,
    ) -> std::cmp::Ordering {
        self.as_bool().cmp(&other.as_bool())
    }
}

impl hash::Hash for SpaBool {
    #[inline(always)]
    fn hash<H>(
        &self,
        state: &mut H,
    ) where
        H: hash::Hasher,
    {
        self.as_bool().hash(state);
    }
}

impl fmt::Debug for SpaBool {
    #[inline(always)]
    #[track_caller]
    fn fmt(
        &self,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        self.as_bool().fmt(f)
    }
}

impl fmt::Display for SpaBool {
    #[inline(always)]
    #[track_caller]
    fn fmt(
        &self,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        self.as_bool().fmt(f)
    }
}

impl Default for SpaBool {
    #[inline(always)]
    fn default() -> Self {
        Self::FALSE
    }
}

impl AsRef<bool> for SpaBool {
    #[inline(always)]
    fn as_ref(&self) -> &bool {
        self.as_bool_ref()
    }
}

impl Borrow<bool> for SpaBool {
    #[inline(always)]
    fn borrow(&self) -> &bool {
        self.as_bool_ref()
    }
}

impl From<bool> for SpaBool {
    #[inline(always)]
    fn from(value: bool) -> Self {
        SpaBool::new(value)
    }
}

impl From<SpaBool> for bool {
    #[inline(always)]
    fn from(value: SpaBool) -> Self {
        value.as_bool()
    }
}

impl PartialEq<bool> for SpaBool {
    #[inline(always)]
    fn eq(
        &self,
        other: &bool,
    ) -> bool {
        self.as_bool() == *other
    }
}

impl PartialEq<SpaBool> for bool {
    #[inline(always)]
    fn eq(
        &self,
        other: &SpaBool,
    ) -> bool {
        *self == other.as_bool()
    }
}

impl PartialOrd<bool> for SpaBool {
    #[inline(always)]
    fn partial_cmp(
        &self,
        other: &bool,
    ) -> Option<std::cmp::Ordering> {
        Some(self.as_bool().cmp(other))
    }
}

impl PartialOrd<SpaBool> for bool {
    #[inline(always)]
    fn partial_cmp(
        &self,
        other: &SpaBool,
    ) -> Option<std::cmp::Ordering> {
        Some(self.cmp(&other.as_bool()))
    }
}

impl From<i32> for SpaBool {
    #[inline(always)]
    fn from(value: i32) -> Self {
        SpaBool(value as _)
    }
}

impl From<SpaBool> for i32 {
    #[inline(always)]
    fn from(value: SpaBool) -> Self {
        value.0
    }
}

impl From<u32> for SpaBool {
    #[inline(always)]
    fn from(value: u32) -> Self {
        SpaBool(value as i32)
    }
}

impl From<SpaBool> for u32 {
    #[inline(always)]
    fn from(value: SpaBool) -> Self {
        value.0 as u32
    }
}

/// This is a logical operation, not a bitwise one.
impl ops::Not for SpaBool {
    type Output = SpaBool;

    #[inline(always)]
    fn not(self) -> Self::Output {
        SpaBool::new(!self.as_bool())
    }
}

/// This is a logical operation, not a bitwise one.
impl ops::BitXor for SpaBool {
    type Output = SpaBool;

    #[inline(always)]
    fn bitxor(
        self,
        rhs: Self,
    ) -> Self::Output {
        SpaBool::new(self.as_bool() ^ rhs.as_bool())
    }
}

/// This is a logical operation, not a bitwise one.
impl ops::BitXorAssign for SpaBool {
    #[inline(always)]
    fn bitxor_assign(
        &mut self,
        rhs: Self,
    ) {
        *self = *self ^ rhs;
    }
}

/// This is a logical operation, not a bitwise one.
impl ops::BitOr for SpaBool {
    type Output = SpaBool;

    #[inline(always)]
    fn bitor(
        self,
        rhs: Self,
    ) -> Self::Output {
        SpaBool::new(self.as_bool() | rhs.as_bool())
    }
}

/// This is a logical operation, not a bitwise one.
impl ops::BitOrAssign for SpaBool {
    #[inline(always)]
    fn bitor_assign(
        &mut self,
        rhs: Self,
    ) {
        *self = *self | rhs;
    }
}

/// This is a logical operation, not a bitwise one.
impl ops::BitAnd for SpaBool {
    type Output = SpaBool;

    #[inline(always)]
    fn bitand(
        self,
        rhs: Self,
    ) -> Self::Output {
        SpaBool::new(self.as_bool() & rhs.as_bool())
    }
}

/// This is a logical operation, not a bitwise one.
impl ops::BitAndAssign for SpaBool {
    #[inline(always)]
    fn bitand_assign(
        &mut self,
        rhs: Self,
    ) {
        *self = *self & rhs;
    }
}

macro_rules! ops_impl {
    (
        [ $($out:tt)* ]
        ($($init:tt)*)

        [$(,)?]
        $($rest:tt)*
    ) => {
        $($out)*
    };

    (
        $out:tt

        (
            $ops_init:tt,
            $assign_ops_init:tt $(,)?
        )

        [
            $(,)? ($type_a:ty, $type_b:ty)
            $($types_rest:tt)*
        ]

        [$(,)?]
        [$(,)?]
    ) => {
        ops_impl!(
            $out
            ($ops_init, $assign_ops_init)

            [$($types_rest)*]
            $ops_init
            $assign_ops_init
        );
    };

    (
        [ $($out:tt)* ]

        (
            $ops_init:tt,
            $assign_ops_init:tt
            $(,)?
        ) // We're caching the ops and assign ops values.

        [
            $(,)? ($type_a:ty, $type_b:ty)
            $($types_rest:tt)*
        ] // Types

        [
            $(,)? $op:ident => $op_method:ident
            $($ops_rest:tt)*
        ] // Normal operations

        [
            $(,)? $assign_op:ident => $assign_op_method:ident
            $($assign_ops_rest:tt)*
        ] // Assignment operations
    ) => {
        ops_impl!(
            [
                $( $out )*

                // Macro hell!
                impl ops::$op<$type_b> for $type_a {
                    type Output = $type_a;

                    #[inline(always)]
                    fn $op_method(self, rhs: $type_b) -> $type_a {
                        <$type_a as ops::$op>::$op_method(self, rhs.into())
                    }
                }

                impl ops::$assign_op<$type_b> for $type_a {
                    #[inline(always)]
                    fn $assign_op_method(&mut self, rhs: $type_b) {
                        <$type_a as ops::$assign_op>::$assign_op_method(self, rhs.into())
                    }
                }

                impl ops::$op<$type_a> for $type_b {
                    type Output = $type_b;

                    #[inline(always)]
                    fn $op_method(self, rhs: $type_a) -> $type_b {
                        <$type_b as ops::$op>::$op_method(self, rhs.into())
                    }
                }

                impl ops::$assign_op<$type_a> for $type_b {
                    #[inline(always)]
                    fn $assign_op_method(&mut self, rhs: $type_a) {
                        <$type_b as ops::$assign_op>::$assign_op_method(self, rhs.into())
                    }
                }
            ]
            ($ops_init, $assign_ops_init)

            [($type_a, $type_b) $($types_rest)* ]
            [ $($ops_rest)* ]
            [ $($assign_ops_rest)* ]
        );
    };
}

macro_rules! ops {
    (

        [ $($type_pairs:tt)* ] // Type pairs
        [ $($ops:tt)* ] // Normal operations
        [ $($assign_ops:tt)* ] // Assignment operations.
    ) => {
        ops_impl!(
            []
            ([ $($ops)* ], [ $($assign_ops)* ])

            [ $($type_pairs)* ]
            [ $($ops)* ]
            [ $($assign_ops)* ]
        );
    };
}

ops! {
    [
        (SpaBool, bool),
        (SpaBool, i32),
        (SpaBool, u32),
    ]

    [
        BitAnd => bitand,
        BitOr => bitor,
        BitXor => bitxor,
    ]
    [
        BitAndAssign => bitand_assign,
        BitOrAssign => bitor_assign,
        BitXorAssign => bitxor_assign,
    ]
}

// SAFETY: A `SpaBool` is an unaligned 32-bit value.
unsafe impl AsPrimPod<SpaBool> for u32 {}
unsafe impl AsPrimPodMut<SpaBool> for u32 {}

// SAFETY: A `SpaBool` is an unaligned 32-bit value.
unsafe impl AsPrimPod<SpaBool> for i32 {}
unsafe impl AsPrimPodMut<SpaBool> for i32 {}
