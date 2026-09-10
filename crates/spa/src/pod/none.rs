// use std::{hint, mem};

// use crate::{
//     mem::{Byte, as_bytes},
//     pod::{SpaHeader, kind::SpaKind},
// };

// /// A `None` SPA POD, representing nothing or a null pointer, sorta.
// ///
// /// Has no payload.
// #[repr(C, packed)]
// pub struct SpaNone {
//     /// The size is guaranteed to be `0`,
//     /// and the `kind` `SpaKind::None`.
//     header: SpaHeader,
//     /// The payload of a `None` POD, which is
//     /// always an empty byte array.
//     payload: [u8; 0],
// }

// impl SpaNone {
//     pub const NONE: &SpaNone = SpaNone::parse(&mut as_bytes(&SpaHeader::NONE)).unwrap();

//     /// Get a reference to this this `SpaNone`.
//     #[inline(always)]
//     #[must_use]
//     pub const fn header(&self) -> &SpaHeader {
//         // SAFETY: We know that `self.header == SpaHeader { size: 0, kind: SpaKind::None as u32, }`.
//         unsafe {
//             hint::assert_unchecked(matches!(
//                 self.header,
//                 SpaHeader {
//                     size: 0,
//                     kind: SpaKind::NONE,
//                 },
//             ))
//         };

//         &self.header
//     }

//     /// Parse a [`SpaNone`] from a given byte buffer.
//     #[inline(always)]
//     #[must_use]
//     pub const fn parse<'a>(input: &mut &'a [Byte]) -> Option<&'a SpaNone> {
//         if let Some((
//             this @ SpaHeader {
//                 size: 0,
//                 kind: SpaKind::NONE,
//             },
//             rest,
//         )) = SpaHeader::split_bytes(input)
//         {
//             *input = rest;

//             // SAFETY: `SpaNone` is just a fancy `SpaHeader` and we just validated said header.
//             Some(unsafe { (&raw const *this).cast::<SpaNone>().as_ref_unchecked() })
//         } else {
//             None
//         }
//     }

//     /// Parse a [`SpaNone`] from a mutable byte buffer.
//     #[inline(always)]
//     #[unsafe(no_mangle)]
//     #[must_use]
//     pub const fn parse_mut<'a>(input: &mut &'a mut [Byte]) -> Option<&'a mut SpaNone> {
//         if let Some(SpaHeader {
//             size: 0,
//             kind: SpaKind::NONE,
//         }) = SpaHeader::from_bytes(input)
//         {
//             let (this, rest) = mem::replace(input, &mut []).split_at_mut(size_of::<SpaHeader>());
//             *input = rest;

//             // SAFETY: `SpaNone` is just a fancy `SpaHeader` and we just validated said header.
//             Some(unsafe { (&raw mut *this).cast::<SpaNone>().as_mut_unchecked() })
//         } else {
//             None
//         }
//     }
// }
