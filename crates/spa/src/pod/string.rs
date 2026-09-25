use std::{ffi::CStr, hint, num::NonZero, ptr, slice, str::Utf8Error};

use crate::{
    mem::{AsBytes, AsBytesMut, Byte, as_bytes, as_bytes_mut},
    pod::PodError,
};

/// An enum describing the various "modes" of searching for a NUL terminator.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub enum NulSearch {
    /// This is the default mode, we ensure that the position of the NUL-terminator
    /// is at the end of the buffer, rather than scanning until we find one.
    ///
    /// This corresponds to the [`CStr::from_bytes_with_nul`] function.
    #[default]
    WithNul,

    /// This is the less restrictive mode, where we scan until the first NUL-terminator
    /// we find.
    ///
    /// This corresponds to the [`CStr::from_bytes_until_nul`] function.
    UntilNul,
}

impl NulSearch {
    /// Searches for the position of the NUL terminator according to the search method specified by
    /// `self`.
    #[inline(always)]
    #[track_caller]
    pub const fn search<B>(
        self,
        bytes: &B,
    ) -> Result<usize, PodError>
    where
        B: AsBytes + ?Sized,
    {
        let bytes = as_bytes(bytes);

        match self.search_inner(bytes) {
            Ok(nul_pos) => {
                // SAFETY: We know that the position is within the bounds of the buffer.
                unsafe { hint::assert_unchecked(nul_pos < bytes.len()) };

                // SAFETY: Since we know that the position is within the bounds of the buffer,
                //         we know that incrementing it is at most the length of the buffer.
                //
                //         It's stupid, but LLVM struggles to infer this, for some reason.
                unsafe {
                    hint::assert_unchecked(
                        nul_pos.unchecked_add(1) <= bytes.len(),
                    )
                };

                Ok(nul_pos)
            },
            // NOTE: Do not cease using bindings, it ***does*** materially affect the quality of the codegen.
            Err(err @ PodError::NotNulTerminated) => Err(err),
            // NOTE: The note above also applies here.
            Err(err @ PodError::InteriorNul { ref position }) => {
                // SAFETY: We know that the position is within the bounds of the buffer.
                unsafe { hint::assert_unchecked(*position < bytes.len()) };

                // SAFETY: Since we know this error to only occur when there's an interior NUL, which implies that it
                //         incrementing the position will ***still be less than the total length of the buffer***,
                //         as if it was equal to `bytes.len()` when incremented, we wouldn't be encountering this error
                //         at all.
                unsafe {
                    hint::assert_unchecked(
                        position.unchecked_add(1) < bytes.len(),
                    )
                };

                // FIXME: Insert assertions that establish that the difference between `bytes.len() - position >= 2`,
                //        and transitively `(bytes.len() + 1) - position >= 1`?

                Err(err)
            },
            Err(_) => {
                // SAFETY: We know that `InteriorNul` and `NotNulTerminated` are the only errors that
                //         we produce.
                unsafe { hint::unreachable_unchecked() };
            },
        }
    }

    /// Searches for the position of the NUL terminator according to the search method specified by `self`.
    ///
    /// This is the mutable variant of [`NulSearch::search`]. It is equivalent `self.search(as_bytes_mut(bytes))`,
    /// and exists purely for convenience.
    #[inline(always)]
    #[track_caller]
    pub const fn search_mut<B>(
        self,
        bytes: &mut B,
    ) -> Result<usize, PodError>
    where
        B: AsBytesMut + ?Sized,
    {
        self.search(as_bytes_mut(bytes))
    }

    /// Returns the position of the NUL terminator, if there is one.
    #[track_caller]
    #[inline(never)]
    const fn search_inner(
        self,
        bytes: &[Byte],
    ) -> Result<usize, PodError> {
        // FIXME: Use a custom `memchr` impl, or, something, specifically to avoid a possible
        //        subsequent call to `strlen` in the future.
        let nul_pos = match CStr::from_bytes_until_nul(Byte::as_u8_slice(bytes))
        {
            Ok(c_str) => {
                let nul_pos = c_str.count_bytes();

                // SAFETY: We know that the position of the NUL is within `bytes`.
                unsafe { hint::assert_unchecked(nul_pos < bytes.len()) };

                nul_pos
            },
            Err(_err) => {
                hint::cold_path();

                return Err(PodError::NotNulTerminated);
            },
        };

        // SAFETY: We know for a fact that the position of the NUL terminator lies within `bytes`.
        unsafe { hint::assert_unchecked(nul_pos < bytes.len()) };

        match self {
            NulSearch::UntilNul => Ok(nul_pos),
            NulSearch::WithNul if nul_pos == bytes.len().strict_sub(1) => {
                Ok(nul_pos)
            },
            NulSearch::WithNul => {
                hint::cold_path();
                Err(PodError::InteriorNul { position: nul_pos })
            },
        }
    }
}

/// A NUL-terminated string that can be serialized as or deserialized from a SPA POD.
///
/// This has no guarantees of encoding besides its size in bytes being less than or equal to
/// [`MAX_SIZE`], and that it contains no interior NULs.
///
/// It's pretty similar to [`CStr`], however we have a fixed representation, whereas [`CStr`] has
/// a representation that is subject to change when/if the type system advances sufficiently.
///
/// # Safety
///
/// The encoding guarantees mentioned before must be upheld, always.
#[repr(transparent)]
pub struct SpaStr([Byte]);

impl SpaStr {
    /// This returns the size, in bytes, including the NUL terminator.
    #[inline(always)]
    #[must_use]
    pub const fn size(&self) -> NonZero<u32> {
        // SAFETY: We have a safety invariant that requires that the size musn't exceed `MAX_SIZE`.
        unsafe {
            hint::assert_unchecked(self.0.len() <= super::MAX_SIZE as usize)
        };

        // SAFETY: We know there's always at least a NUL terminator. Additionally, we know
        //         that we're not truncating the value.
        let size = unsafe { NonZero::new_unchecked(self.0.len() as u32) };

        // SAFETY: We know that the byte at the end of the buffer is a NUL.
        unsafe {
            hint::assert_unchecked(
                self.0[size.get().strict_sub(1) as usize].get() == 0x00,
            )
        };

        if self.0[0].get() == 0x00 {
            // SAFETY: If the first byte is a NUL-terminator, then we know it to be empty.
            unsafe { hint::assert_unchecked(size.get().strict_sub(1) == 0) };
        }

        size
    }

    /// This returns the length, in bytes, excluding the NUL-terminator.
    #[inline(always)]
    #[must_use]
    pub const fn len(&self) -> u32 {
        self.size().get().strict_sub(1)
    }

    /// Returns whether or not this string is empty, ignoring the NUL-terminator.
    #[inline(always)]
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns the underlying slice of [`Byte`]s, including the NUL-terminator.
    #[inline(always)]
    #[must_use]
    pub const fn as_bytes(&self) -> &[Byte] {
        // SAFETY: We know that it's safe to reborrow the underlying buffer
        //         immutably.
        unsafe {
            slice::from_raw_parts(
                (&raw const *self).cast::<Byte>(),
                self.size().get() as usize,
            )
        }
    }

    /// Returns a mutable reference to the underlying slice of [`Byte`]s, including the NUL-terminator.
    ///
    /// # Safety
    ///
    /// When the borrow ends, the caller must ensure that the NUL-terminator is still set to NUL,
    /// and that there's no NULs preceding it in the body of the string.
    ///
    /// Failure to do ensure the above is undefined behavior.
    #[inline(always)]
    #[must_use]
    pub const unsafe fn as_bytes_mut(&mut self) -> &mut [Byte] {
        // SAFETY: The caller ensures that the invariants of this type won't be violated,
        //         and we know that it's safe to reborrow the underlying buffer mutably
        //         as a result.
        unsafe {
            slice::from_raw_parts_mut(
                (&raw mut *self).cast::<Byte>(),
                self.size().get() as usize,
            )
        }
    }

    /// Returns the underlying slice of [`Byte`]s, excluding the NUL-terminator.
    #[inline(always)]
    #[must_use]
    pub const fn as_body(&self) -> &[Byte] {
        self.as_bytes()
            .split_last()
            .expect(
                "there was not a NUL terminator within our SPA string, somehow",
            )
            .1
    }

    /// Returns a mutable reference to the underlying slice of [`Byte`]s, excluding the NUL-terminator.
    ///
    /// # Safety
    ///
    /// When the borrow ends, the caller must ensure that the returned slice contains no NULs.
    ///
    /// Failure to ensure the above, is undefined behavior.
    #[inline(always)]
    #[must_use]
    pub const unsafe fn as_body_mut(&mut self) -> &mut [Byte] {
        // SAFETY: The caller ensures that the body will not be contain any NULs when the borrow ends.
        unsafe { self.as_bytes_mut() }
            .split_last_mut()
            .expect(
                "there was not a NUL terminator within our SPA string, somehow",
            )
            .1
    }

    /// Attempts to convert the underlying buffer to a [`str`], including the NUL-terminator.
    #[inline(always)]
    pub const fn to_str(&self) -> Result<&str, Utf8Error> {
        // NOTE: We're using the module version despite that no longer being preferred,
        //       as the constructor on `str` merely wraps it, but without inlining the call.
        //
        //       So this function call in reality becomes a chain of jumps, and it makes sense
        //       to eliminate one extra jump, if that is indeed possible.
        std::str::from_utf8(Byte::as_u8_slice(self.as_bytes()))
    }

    /// Attempts to convert the underlying buffer to a mutable [`str`], including the NUL-terminator.
    ///
    /// # Safety
    ///
    /// The caller needs to, before the borrow ends, ensure that the NUL-terminator remains in tact,
    /// and that there are no preceding NULs inserted.
    #[inline(always)]
    pub const unsafe fn to_str_mut(&mut self) -> Result<&mut str, Utf8Error> {
        // NOTE: The note within `to_str` applies here, as well.
        //
        // SAFETY: The caller ensures that the invariants of this type will remain in tact before
        //         the borrow ends.
        std::str::from_utf8_mut(Byte::as_u8_slice_mut(unsafe {
            self.as_bytes_mut()
        }))
    }

    /// Attempts to convert the underlying buffer to a [`str`], excluding the NUL-terminator.
    #[inline(always)]
    pub const fn to_body_str(&self) -> Result<&str, Utf8Error> {
        // NOTE: The note within `to_str` applies here, as well.
        std::str::from_utf8(Byte::as_u8_slice(self.as_body()))
    }

    /// Attempts to convert the underlying buffer to a mutable [`str`], excluding the NUL-terminator.
    ///
    /// # Safety
    ///
    /// The caller must ensure that by the time the borrow ends, that it contains no NUL bytes within it,
    /// whatsoever.
    ///
    /// Failure to ensure this is undefined behavior.
    #[inline(always)]
    pub const unsafe fn to_body_str_mut(
        &mut self
    ) -> Result<&mut str, Utf8Error> {
        // NOTE: The note within `to_str` applies here, as well.
        //
        // SAFETY: We know that there's no chance of the NUL being removed, and the caller
        //         ensures that no NULs will be inserted into the body.
        std::str::from_utf8_mut(Byte::as_u8_slice_mut(unsafe {
            self.as_body_mut()
        }))
    }

    /// Attempt to create a [`SpaStr`] from a [`CStr`].
    #[inline(always)]
    pub const fn from_c_str(c_str: &CStr) -> Result<&SpaStr, PodError> {
        // SAFETY: `CStr` guarantees that `count_bytes` returns the size, ignoring the NUL terminator, so incrementing
        //         is always sound.
        let size = unsafe { c_str.count_bytes().unchecked_add(1) };

        if size <= super::MAX_SIZE as usize {
            let string = ptr::slice_from_raw_parts(
                (&raw const *c_str).cast::<Byte>(),
                size,
            );

            // SAFETY: We know the size to be within bounds.
            Ok(unsafe { (string as *const SpaStr).as_ref_unchecked() })
        } else {
            Err(PodError::InvalidSize)
        }
    }

    /// Attempt to create a mutable [`SpaStr`] from a mutable [`CStr`].
    #[inline(always)]
    pub const fn from_c_str_mut(
        c_str: &mut CStr
    ) -> Result<&mut SpaStr, PodError> {
        // SAFETY: `CStr` guarantees that `count_bytes` returns the size, ignoring the NUL terminator, so incrementing
        //         is always sound.
        let size = unsafe { c_str.count_bytes().unchecked_add(1) };

        if size <= super::MAX_SIZE as usize {
            let string = ptr::slice_from_raw_parts_mut(
                (&raw mut *c_str).cast::<Byte>(),
                size,
            );

            // SAFETY: We know the size to be within bounds.
            Ok(unsafe { (string as *mut SpaStr).as_mut_unchecked() })
        } else {
            Err(PodError::InvalidSize)
        }
    }

    /// Borrow this [`SpaStr`] as a borrowed C string.
    #[inline(always)]
    #[must_use]
    pub const fn as_c_str(&self) -> &CStr {
        // SAFETY: We know all `SpaStr`s to be valid, NUL-terminated C strings.
        //
        //         Additionally, we're not strictly relying on the representation
        //         of `CStr` outside of it either being a wrapper around a `[u8]`,
        //         that includes the NUL terminator, or it being a magical thin DST,
        //         where the loss of the pointer metadata *shouldn't cause issues*.
        //
        //         This is technically undefined behavior, but realistically no issues will,
        //         ever, occur.
        //
        //         A bunch of code relies on this, and I figure they'll emit lints or hard errors
        //         if it becomes an issue in the future.
        unsafe {
            (&raw const *self.as_bytes() as *const CStr).as_ref_unchecked()
        }
    }

    /// Mutable borrow this [`SpaStr`] as a mutable C string.
    #[inline(always)]
    #[must_use]
    pub const fn as_c_str_mut(&mut self) -> &mut CStr {
        // SAFETY: We're converting to a `CStr`, we know our invariants will be upheld.
        //
        //         Additionally, the same things from `as_c_str` apply here.
        unsafe {
            (&raw mut *self.as_bytes_mut() as *mut CStr).as_mut_unchecked()
        }
    }
}

// SAFETY: It is safe to borrow the underlying bytes immutably, but not mutably, due to the possibility
//         of the insertion or removal of NUL-terminators.
unsafe impl AsBytes for SpaStr {}
