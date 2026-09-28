use std::{ffi::CStr, hint, num::NonZero};

use crate::mem::{AsBytes, AsBytesMut, Byte, as_bytes, as_bytes_mut};

/// A SPA String that contains either no NUL terminators, or ***precisely one*** at the end.
///
/// The exact representation of this is kinda unspecified, but it permits us to insert the NUL when needed,
/// and include it when needed, if it already exists.
///
/// One such use case is being able to serialize a [`str`] that contains no NUL, by simply inserting it,
/// while still being able to, zero-copy, turn a [`CStr`] into a [`SpaStr`], to turn it back into a [`CStr`].
///
/// We guarantee that the size of string, when including a NUL terminator, is less than or equal to [`super::MAX_SIZE`].
#[repr(transparent)]
pub struct SpaStr([u8]);

impl SpaStr {
    /// Returns the underlying byte buffer, excluding any NUL terminator.
    #[inline(always)]
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8] {
        let len = self.len();

        self.0.split_at(len as usize).0
    }

    /// Returns the underlying byte buffer mutably, excluding any NUL terminator.
    ///
    /// # Safety
    ///
    /// The caller must ensure that no NULs are within the buffer when the borrow ends.
    ///
    /// If you need a safe variant of this, see [`SpaStr::as_nonzer_bytes_mut`].
    #[inline(always)]
    #[must_use]
    #[allow(unused_unsafe)]
    pub const unsafe fn as_bytes_mut(&mut self) -> &mut [u8] {
        let len = self.len();

        // SAFETY: The caller ensures this is sound.
        unsafe { self.0.split_at_mut(len as usize).0 }
    }

    /// Returns the underlying byte buffer, excluding any NUL terminator,
    /// but with [`NonZero`] bytes instead.
    #[inline(always)]
    #[must_use]
    pub const fn as_nonzero_bytes(&self) -> &[NonZero<u8>] {
        // SAFETY: We know for a fact that the bytes before the NUL, if any,
        //         are nonzero.
        unsafe {
            (&raw const *self.as_bytes() as *const [NonZero<u8>])
                .as_ref_unchecked()
        }
    }

    /// Returns the underlying byte buffer mutably, excluding any NUL terminator,
    /// but with [`NonZero`] bytes instead.
    #[inline(always)]
    #[must_use]
    pub const fn as_nonzero_bytes_mut(&mut self) -> &mut [NonZero<u8>] {
        // SAFETY: We know for a fact that the bytes before the NUL, if any,
        //         are nonzero. Additionally, since we're returning
        //         nonzero bytes, it is impossible to insert a NUL during the borrow.
        unsafe {
            (&raw mut *self.as_bytes_mut() as *mut [NonZero<u8>])
                .as_mut_unchecked()
        }
    }

    /// Returns whether this string has an included NUL terminator.
    #[inline(always)]
    #[must_use]
    pub const fn has_nul(&self) -> bool {
        matches!(self, SpaStr([.., 0x00]))
    }

    /// Returns whether this string is empty.
    #[inline(always)]
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns the length of the SPA string including the NUL terminator.
    ///
    /// This is not necessarily the size of the underlying buffer.
    #[inline(always)]
    #[must_use]
    pub const fn len_with_nul(&self) -> NonZero<u32> {
        NonZero::new(self.len().strict_add(1)).unwrap()
    }

    /// Returns the length of the SPA string, excluding the NUL terminator.
    ///
    /// Note that this is not necessarily the size of the buffer.
    #[inline(always)]
    #[must_use]
    pub const fn len(&self) -> u32 {
        // SAFETY: We know that all SPA strings, with our withour their NUL terminator
        //         are within bounds of `MAX_SIZE`.
        unsafe {
            hint::assert_unchecked(self.0.len() <= super::MAX_SIZE as usize)
        };

        let len_without_nul = {
            let has_nul = matches!(&self.0, [.., 0x00]);

            let len_without_nul = if has_nul {
                // SAFETY: If there's a NUL, then we can decrement the length.
                unsafe { self.0.len().unchecked_sub(1) }
            } else {
                // NOTE: No NUL was found, so we always just return the length of the original buffer.
                self.0.len()
            };

            // SAFETY: We know that the length without the NUL terminator will be in bounds of
            //         the original buffer. Additionally we know the difference will be at most 1.
            unsafe { hint::assert_unchecked(len_without_nul <= self.0.len()) };

            // SAFETY: We know that the length without the NUL terminator is less than the maximum SPA POD
            //         size, as we need to be able to store the NUL terminator in a SPA POD.
            unsafe {
                hint::assert_unchecked(
                    len_without_nul < super::MAX_SIZE as usize,
                )
            };

            // SAFETY: Same as above, but additionally we know that the difference is at most 1.
            unsafe {
                hint::assert_unchecked(
                    self.0.len().strict_sub(len_without_nul) <= 1,
                )
            };

            // SAFETY: We for certain that the length with a NUL is at either greater than or equal
            //         to the original buffer size.
            unsafe {
                hint::assert_unchecked(
                    len_without_nul.strict_add(1) >= self.0.len(),
                );
            };

            // SAFETY: Same as above but we know that the length with a NUL is at most one more
            //         than the original buffer size.
            unsafe {
                hint::assert_unchecked(
                    len_without_nul.strict_add(1).strict_sub(self.0.len()) <= 1,
                )
            }

            if has_nul {
                // SAFETY: If we have a NUL terminator, then we know the length including it,
                //         is equivalent to the underlying buffer length.
                unsafe {
                    hint::assert_unchecked(
                        len_without_nul.strict_add(1) == self.0.len(),
                    )
                };
            } else {
                // SAFETY: If we lack a NUL terminator, then we know the length including it,
                //         is greater than the underlying buffer length.
                unsafe {
                    hint::assert_unchecked(
                        len_without_nul.strict_add(1) > self.0.len(),
                    )
                };

                // SAFETY: Same as above, but we know that the difference is one.
                unsafe {
                    hint::assert_unchecked(
                        len_without_nul.strict_add(1).strict_sub(self.0.len())
                            == 1,
                    )
                };
            }

            len_without_nul
        };

        len_without_nul as u32
    }
}

/// An enum that represents where a NUL terminator was found
/// within some byte buffer.
///
/// This mainly exists to encapsulate the various possible, equally valid (depending on context),
/// ways we wish to construct a [`SpaStr`].
///
/// Since a [`SpaStr`] has at most one NUL at the end of it, we want to be able to construct one
/// from say, [`str`]s we know to contain no NULs (and thus, when are being encoded, we can just automatically insert one),
/// or [`CStr`]s without any checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NulPos {
    /// There was no NUL terminator within the buffer, at all.
    ///
    /// Buffers that return this variant cannot be safely reinterpreted as
    /// a [`CStr`], nor can any subslice of them, as there exists no NUL terminator.
    None {
        /// The length, in bytes, of the original input buffer.
        len: usize,
    },

    /// There's only one NUL terminator, at the end of the buffer.
    ///
    /// Buffers that return this variant can be safely reinterpreted as a
    /// [`CStr`].
    End {
        /// The byte position of the NUL, which is the length of the buffer,
        /// in bytes, minus one.
        pos: usize,
    },

    /// There's a NUL terminator that lies before the end of the buffer.
    ///
    /// Buffers that return this variant may have more than one NUL terminator,
    /// but we know for certain that we can reinterpet the buffer, up to and including
    /// the position, as a [`CStr`].
    Interior {
        /// The byte position of the first NUL in the buffer.
        pos: usize,
    },
}

impl NulPos {
    /// Create a [`NulPos::None`], given some length.
    #[inline(always)]
    #[must_use]
    pub const fn none(len: usize) -> NulPos {
        NulPos::None { len }
    }

    /// Returns whether this is a [`NulPos::None`].
    #[inline(always)]
    #[must_use]
    pub const fn is_none(&self) -> bool {
        matches!(self, NulPos::None { .. })
    }

    /// Create a [`NulPos::End`], given some NUL position.
    #[inline(always)]
    #[must_use]
    pub const fn end(pos: usize) -> NulPos {
        NulPos::End { pos }
    }

    /// Returns whether this is a [`NulPos::End`].
    #[inline(always)]
    #[must_use]
    pub const fn is_end(&self) -> bool {
        matches!(self, NulPos::End { .. })
    }

    /// Create a [`NulPos::Interior`], given some NUL position.
    #[inline(always)]
    #[must_use]
    pub const fn interior(pos: usize) -> NulPos {
        NulPos::Interior { pos }
    }

    /// Returns whether this is a [`NulPos::Interior`].
    #[inline(always)]
    #[must_use]
    pub const fn is_interior(&self) -> bool {
        matches!(self, NulPos::Interior { .. })
    }

    /// Returns the position of the NUL terminator in the byte buffer,
    /// if there was one.
    #[inline(always)]
    #[must_use]
    pub const fn pos(&self) -> Option<usize> {
        match self {
            NulPos::None { .. } => None,
            NulPos::End { pos } | NulPos::Interior { pos } => Some(*pos),
        }
    }

    /// Returns the length of a byte buffer, including a NUL terminator,
    /// that would be needed to allocate a [`std::ffi::CStr`] for the
    /// original buffer.
    ///
    /// # Returns
    ///
    /// We do not differentiate between buffers that have a NUL, or lack one. We return [`None`]
    /// if the size calculation overflowed.
    #[inline(always)]
    #[must_use]
    pub const fn size_needed(&self) -> Option<NonZero<usize>> {
        match self.len_with_nul() {
            Ok(needed) | Err(needed) => needed,
        }
    }

    /// Returns the length of the byte buffer, including the NUL terminator.
    ///
    /// # Returns
    ///
    /// The actual calculated value is a [`Option<NonZero<usize>>`]. The [`None`] case
    /// indicates that there was an overflow when calculating the length.
    ///
    /// However, we wrap the actual value within a [`Result`].
    ///
    /// For [`NulPos::None`], we return `Err(length)`, as while the original source
    /// buffer may lack a NUL, we may be intending to allocate a buffer than can store
    /// the NUL terminator.
    ///
    /// For [`NulPos::End`] and [`NulPos::Interior`], we return `Ok(length)`, as the original
    /// source buffer *does* contain a NUL.
    #[inline(always)]
    pub const fn len_with_nul(
        &self
    ) -> Result<Option<NonZero<usize>>, Option<NonZero<usize>>> {
        let with_nul = match self.len_without_nul().checked_add(1) {
            Some(with_nul) => Some(NonZero::new(with_nul).unwrap()),
            None => None,
        };

        match self {
            NulPos::None { .. } => Err(with_nul),
            NulPos::End { .. } | NulPos::Interior { .. } => Ok(with_nul),
        }
    }

    /// Returns the length of the byte buffer, excluding the NUL terminator.
    ///
    /// # Returns
    ///
    /// This does *not* differentiate on buffers who lack a NUL terminator,
    /// as as far as we're considered, their length is by definition,
    /// a length without a NUL terminator.
    ///
    /// For [`NulPos::None`], we return the `len` field.
    ///
    /// For [`NulPos::End`] and [`NulPos::Interior`], we return the position
    /// of the NUL terminator, as getting the subslice up to that offset,
    /// but not including it (so a `..pos` range), gives you a buffer without
    /// any NUL terminator, where the length is `pos`.
    #[inline(always)]
    #[must_use]
    pub const fn len_without_nul(&self) -> usize {
        match self {
            NulPos::None {
                len: len_without_nul,
            }
            | NulPos::End {
                pos: len_without_nul,
            }
            | NulPos::Interior {
                pos: len_without_nul,
            } => *len_without_nul,
        }
    }

    /// Searches the specified byte buffer for a NUL terminator.
    ///
    /// # Specialization and Performance
    ///
    /// Currently we do *not* specialize. So if you pass along a [`CStr`],
    /// whatever implementation of `memchr` the Rust standard library uses,
    /// will be called.
    ///
    /// Realistically this shouldn't be an issue, as SPA PODs are pretty small,
    /// and the strings within them are also, pretty small. Still, this need to be
    /// kept in mind.
    #[inline(always)]
    #[must_use]
    pub const fn search<B>(bytes: &B) -> NulPos
    where
        B: AsBytes + ?Sized,
    {
        NulPos::search_inner(as_bytes(bytes))
    }

    /// Searches the specified mutable byte buffer for a NUL terminator.
    ///
    /// This is the mutable variant of [`NulPos::search`]. See that for
    /// details on usage.
    ///
    /// This is equivalent to `NulPos::search(&*as_bytes_mut(bytes))`.
    #[inline(always)]
    #[must_use]
    pub const fn search_mut<B>(bytes: &mut B) -> NulPos
    where
        B: AsBytesMut + ?Sized,
    {
        NulPos::search_inner(as_bytes_mut(bytes))
    }

    #[inline(always)]
    #[must_use]
    #[track_caller]
    const fn search_inner(bytes: &[Byte]) -> NulPos {
        let len_with_nul =
            match CStr::from_bytes_until_nul(Byte::as_u8_slice(bytes)) {
                Ok(c_str) => Some({
                    let nul_pos = c_str.count_bytes();

                    // SAFETY: We know for a fact the `nul_pos + 1 <= len`.
                    //         Additionally, we know it to be nonzero.
                    let len_with_nul = unsafe {
                        NonZero::new_unchecked(nul_pos.unchecked_add(1))
                    };

                    // SAFETY: Same as above.
                    unsafe {
                        hint::assert_unchecked(
                            len_with_nul.get() <= bytes.len(),
                        )
                    };

                    // SAFETY: We know for a fact that the byte at  `len_with_nul - 1` is zero.
                    unsafe {
                        hint::assert_unchecked(
                            bytes[len_with_nul.get().strict_sub(1)].get()
                                == 0x00,
                        )
                    };

                    len_with_nul
                }),
                Err(_) => None,
            };

        match len_with_nul {
            Some(len_with_nul) => {
                // SAFETY: We know that if there's a NUL terminator, that the length of our buffer is nonzero.
                let buffer_len = NonZero::new(bytes.len())
                    .expect("the buffer length should be nonzero");

                assert!(
                    len_with_nul.get() <= buffer_len.get(),
                    "the length including a nul must be within the bounds of the original length",
                );

                let position = len_with_nul.get().strict_sub(1);

                assert!(
                    bytes[position].get() == 0x00,
                    "the last byte in the buffer with the nul, must be nul",
                );

                match (
                    len_with_nul.get() < buffer_len.get(),
                    len_with_nul.get() > buffer_len.get(),
                ) {
                    (false, false) => {
                        assert!(
                            len_with_nul.get() == buffer_len.get(),
                            "the length with nul in this case must be equal to the original buffer length",
                        );

                        NulPos::End { pos: position }
                    },
                    (true, false) => {
                        assert!(
                            len_with_nul.get() < buffer_len.get(),
                            "the length with nul in this case must be less than the original buffer length",
                        );

                        NulPos::Interior { pos: position }
                    },
                    (_, true) => unreachable!(),
                }
            },
            None => NulPos::None { len: bytes.len() },
        }
    }
}
