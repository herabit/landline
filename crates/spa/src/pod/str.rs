use std::{
    alloc::{Allocator, Global, Layout},
    borrow::{Borrow, BorrowMut, Cow},
    cmp::Ordering,
    convert::Infallible,
    error,
    ffi::{CStr, CString, FromBytesWithNulError},
    fmt, hash, hint,
    iter::{self, FusedIterator},
    mem::{self, DropGuard, MaybeUninit},
    num::{NonZero, TryFromIntError},
    ops::{self, BitOrAssign, Deref, Index, IndexMut},
    ptr::{self, NonNull},
    range::Range,
    rc::Rc,
    slice::{self, SliceIndex},
    str::{FromStr, Utf8Error},
    sync::Arc,
};

use crate::{
    mem::{AsBytes, AsBytesMut, Byte, as_bytes, as_bytes_mut},
    pod::{
        PodError,
        error::{WithPodError, WithUtf8Error},
    },
    util::{self, HeapAlloc},
};

// FIXME: Write better docs.
//
/// A SPA String that contains either no NUL terminators, or ***precisely one*** at the end.
///
/// The exact representation of this is kinda unspecified, but it permits us to insert the NUL when needed,
/// and include it when needed, if it already exists.
///
/// One such use case is being able to serialize a [`str`] that contains no NUL, by simply inserting it,
/// while still being able to, zero-copy, turn a [`CStr`] into a [`SpaStr`], to turn it back into a [`CStr`].
///
/// We guarantee that the size of string, when including a NUL terminator, is less than or equal to [`super::MAX_SIZE`].
///
/// Conversions will tend to preserve or even insert NULs, but borrows by default will not include them.
#[repr(transparent)]
pub struct SpaStr {
    whole: [u8],
}

impl SpaStr {
    /// Returns an empty [`SpaStr`].
    #[inline(always)]
    #[must_use]
    pub const fn empty() -> &'static SpaStr {
        const {
            match SpaStr::from_c_str(c"") {
                Ok(spa_str) => spa_str,
                Err(..) => unreachable!(),
            }
        }
    }

    /// Create a [`SpaStr`] without any checks.
    ///
    /// # Safety
    ///
    /// The caller must ensure:
    ///
    /// - That the buffer contains either no NULs, or that it has only one NUL,
    ///   stored at the end of the buffer.
    /// - That the length, trimming any terminating NUL, is less than [`super::MAX_SIZE`].
    /// - Probably some other things I am forgetting.
    #[inline(always)]
    #[must_use]
    pub const unsafe fn from_bytes_unchecked<B>(bytes: &B) -> &SpaStr
    where
        B: ?Sized + AsBytes,
    {
        let bytes = as_bytes(bytes);

        // SAFETY: We know that since `bytes` may contain a NUL, that it is at most `MAX_SIZE`.
        unsafe {
            hint::assert_unchecked(bytes.len() <= super::MAX_SIZE as usize)
        };

        // SAFETY: The caller ensures this is acceptable.
        unsafe { (&raw const *bytes as *const SpaStr).as_ref_unchecked() }
    }

    /// Create a mutable [`SpaStr`] without any checks.
    ///
    /// # Safety
    ///
    /// The caller must ensure:
    ///
    /// - That the buffer contains either no NULs, or that it has only one NUL,
    ///   stored at the end of the buffer.
    /// - That the length, trimming any terminating NUL, is less than [`super::MAX_SIZE`].
    /// - Probably some other things I am forgetting.
    #[inline(always)]
    #[must_use]
    pub const unsafe fn from_bytes_mut_unchecked<B>(
        bytes: &mut B
    ) -> &mut SpaStr
    where
        B: ?Sized + AsBytesMut,
    {
        let bytes = as_bytes_mut(bytes);

        // SAFETY: We know that since `bytes` may contain a NUL, that it is at most `MAX_SIZE`.
        unsafe {
            hint::assert_unchecked(bytes.len() <= super::MAX_SIZE as usize)
        };

        // SAFETY: The caller ensures this is acceptable.
        unsafe { (&raw mut *bytes as *mut SpaStr).as_mut_unchecked() }
    }

    /// This is an implementation detail for the `from_bytes` methods and similar.
    ///
    /// It returns the byte offset the caller needs to split at to create a valid SPA String meeting
    /// the criteria specified by `setting`.
    ///
    /// # Safety
    ///
    /// It is ***always*** safe to split the source buffer at the returned location.
    ///
    /// Additionally, if the setting permits the found NUL (if there is one), the returned
    /// value is `nul_pos + 1`, as we want the split buffer to ***contain*** the NUL.
    const fn find_split_pos(
        bytes: &[Byte],
        setting: NulSetting,
    ) -> Result<usize, PodError> {
        let nul_pos = NulPos::search(bytes);

        if let Err(err) = setting.validate_pos(&nul_pos) {
            hint::cold_path();

            return Err(err);
        }

        let split_pos = match nul_pos {
            NulPos::None { len } => len,
            // NOTE: We're incrementing as this stores the position of the NUL,
            //       not the length of the buffer including the NUL.
            NulPos::End { pos } | NulPos::Interior { pos } => pos.strict_add(1),
        };

        debug_assert!(
            split_pos <= bytes.len(),
            "undefined behavior: the split position is outside the bounds of the containing buffer"
        );

        // SAFETY: Do not remove this, it is needed for soundness.
        if split_pos > super::MAX_SIZE as usize {
            hint::cold_path();

            return Err(PodError::InvalidSize);
        }

        Ok(split_pos)
    }

    #[track_caller]
    #[inline(always)]
    const fn from_bytes_inner(
        bytes: &[Byte],
        setting: NulSetting,
    ) -> Result<&SpaStr, PodError> {
        let split_pos = match SpaStr::find_split_pos(bytes, setting) {
            Ok(split_pos) => split_pos,
            Err(err) => return Err(err),
        };

        let (spa_str, _rest) = bytes.split_at(split_pos);

        // SAFETY: We know `bytes[..split_pos]` is a valid SPA string.
        Ok(unsafe { SpaStr::from_bytes_unchecked(spa_str) })
    }

    /// Create a [`SpaStr`] given some byte buffer, and some setting for how to handle
    /// NULs.
    ///
    /// # Returns
    ///
    /// Returns [`Err`] if there was a conflict with the specified NUL setting,
    /// or if the size of the string would be too large.
    #[inline(always)]
    #[track_caller]
    pub const fn from_bytes_with_setting<B>(
        bytes: &B,
        setting: NulSetting,
    ) -> Result<&SpaStr, PodError>
    where
        B: ?Sized + AsBytes,
    {
        SpaStr::from_bytes_inner(as_bytes(bytes), setting)
    }

    /// Create a [`SpaStr`] given some byte buffer.
    ///
    /// # Returns
    ///
    /// Returns [`Err`] if there was an interior NUL, or if the SPA string would
    /// be too large.
    #[inline(always)]
    #[track_caller]
    pub const fn from_bytes<B>(bytes: &B) -> Result<&SpaStr, PodError>
    where
        B: ?Sized + AsBytes,
    {
        SpaStr::from_bytes_inner(as_bytes(bytes), NulSetting::Optional)
    }

    #[track_caller]
    #[inline(always)]
    const fn from_bytes_mut_inner(
        bytes: &mut [Byte],
        setting: NulSetting,
    ) -> Result<&mut SpaStr, PodError> {
        let split_pos = match SpaStr::find_split_pos(&*bytes, setting) {
            Ok(split_pos) => split_pos,
            Err(err) => return Err(err),
        };

        let (spa_str, _rest) = bytes.split_at_mut(split_pos);

        // SAFETY: We know `bytes[..split_pos]` is a valid SPA string.
        Ok(unsafe { SpaStr::from_bytes_mut_unchecked(spa_str) })
    }

    /// Create a mutable [`SpaStr`] given some byte buffer, and some setting for how to handle
    /// NULs.
    ///
    /// # Returns
    ///
    /// Returns [`Err`] if there was a conflict with the specified NUL setting,
    /// or if the size of the string would be too large.
    #[inline(always)]
    #[track_caller]
    pub const fn from_bytes_mut_with_setting<B>(
        bytes: &mut B,
        setting: NulSetting,
    ) -> Result<&mut SpaStr, PodError>
    where
        B: ?Sized + AsBytesMut,
    {
        SpaStr::from_bytes_mut_inner(as_bytes_mut(bytes), setting)
    }

    /// Create a mutable [`SpaStr`] given some byte buffer.
    ///
    /// # Returns
    ///
    /// Returns [`Err`] if there was an interior NUL, or if the SPA string would
    /// be too large.
    #[inline(always)]
    #[track_caller]
    pub const fn from_bytes_mut<B>(
        bytes: &mut B
    ) -> Result<&mut SpaStr, PodError>
    where
        B: ?Sized + AsBytesMut,
    {
        SpaStr::from_bytes_mut_inner(as_bytes_mut(bytes), NulSetting::Optional)
    }

    /// Create a new [`SpaStr`] from a C string.
    ///
    /// # Performance
    ///
    /// This ***may*** inadvertently call `strlen` in future versions of Rust.
    ///
    /// Not much we can do.
    ///
    /// # Returns
    ///
    /// Returns [`Err`] if the specified C string was too large.
    #[inline(always)]
    pub const fn from_c_str(c_str: &CStr) -> Result<&SpaStr, PodError> {
        // SAFETY: We know that it is always safe to increment the `strlen` of a `CStr`.
        let len_with_nul = unsafe { c_str.count_bytes().unchecked_add(1) };

        if len_with_nul <= super::MAX_SIZE as usize {
            // NOTE:  We're creating a `[u8]` as `to_bytes_with_nul` may call `strlen` in the future.
            //
            // SAFETY: We know that there's an underlying buffer spanning `c_str[..=len_with_nul]`.
            let buf = unsafe {
                slice::from_raw_parts(
                    (&raw const *c_str).cast::<u8>(),
                    len_with_nul,
                )
            };

            // SAFETY: We know that `buf` is a valid SPA string.
            Ok(unsafe { SpaStr::from_bytes_unchecked(buf) })
        } else {
            Err(PodError::InvalidSize)
        }
    }

    /// Create a new mutable [`SpaStr`] from a C string.
    ///
    /// # Performance
    ///
    /// This ***may*** inadvertently call `strlen` in future versions of Rust.
    ///
    /// Not much we can do.
    ///
    /// # Returns
    ///
    /// Returns [`Err`] if the specified C string was too large.
    #[inline(always)]
    pub const fn from_c_str_mut(
        c_str: &mut CStr
    ) -> Result<&mut SpaStr, PodError> {
        // SAFETY: We know that it is always safe to increment the `strlen` of a `CStr`.
        let len_with_nul = unsafe { c_str.count_bytes().unchecked_add(1) };

        if len_with_nul <= super::MAX_SIZE as usize {
            // NOTE:  We're creating a `[u8]` as `to_bytes_with_nul` may call `strlen` in the future.
            //
            // SAFETY: We know that there's an underlying buffer spanning `c_str[..=len_with_nul]`.
            let buf = unsafe {
                slice::from_raw_parts_mut(
                    (&raw mut *c_str).cast::<u8>(),
                    len_with_nul,
                )
            };

            // SAFETY: We know that `buf` is a valid SPA string.
            Ok(unsafe { SpaStr::from_bytes_mut_unchecked(buf) })
        } else {
            Err(PodError::InvalidSize)
        }
    }

    const fn split_inner(&self) -> (&[NonZero<u8>], Option<&Nul>) {
        let (body, nul) = match &self.whole {
            [body @ .., nul @ 0x00] => (body, Some(nul)),
            body => (body, None),
        };

        // SAFETY: We know the body to be nonzero.
        let body = unsafe {
            (&raw const *body as *const [NonZero<u8>]).as_ref_unchecked()
        };

        let nul = match nul {
            // SAFETY: We know this byte is NUL.
            Some(nul) => Some(unsafe {
                (&raw const *nul).cast::<Nul>().as_ref_unchecked()
            }),
            None => None,
        };

        (body, nul)
    }

    const fn split_mut_inner(
        &mut self
    ) -> (&mut [NonZero<u8>], Option<&mut Nul>) {
        let (body, nul) = match &mut self.whole {
            [body @ .., nul @ 0x00] => (body, Some(nul)),
            body => (body, None),
        };

        // SAFETY: We know the body to be nonzero.
        let body = unsafe {
            (&raw mut *body as *mut [NonZero<u8>]).as_mut_unchecked()
        };

        let nul = match nul {
            // SAFETY: WE know this byte is NUL
            Some(nul) => Some(unsafe {
                (&raw mut *nul).cast::<Nul>().as_mut_unchecked()
            }),
            None => None,
        };

        (body, nul)
    }

    /// Splits this SPA string at its body, which is guaranteed to contain no NULs (which is just zero),
    /// and the NUL terminator at the end of the string, if there's any.
    #[inline(always)]
    #[must_use]
    pub const fn split(&self) -> (&[NonZero<u8>], Option<&Nul>) {
        let (body, nul) = self.split_inner();

        // SAFETY: We know the size without a NUL to be less than the maximum SPA size.
        unsafe {
            hint::assert_unchecked(body.len() < super::MAX_SIZE as usize)
        };

        // SAFETY: We know that if there's a NUL, that the body length is equal to `whole - 1`, otherwise
        //         we know it is equal to `whole`.
        unsafe {
            hint::assert_unchecked({
                let n = body.len().unchecked_add(if let Some(Nul) = nul {
                    1_usize
                } else {
                    0_usize
                });

                n == self.whole.len()
            })
        };

        (body, nul)
    }

    /// Mutably splits this SPA string at its body, which is guaranteed to contain no NULs (which are just zero),
    /// and the NUL terminator at the end of the string, if there's any.
    #[inline(always)]
    #[must_use]
    pub const fn split_mut(
        &mut self
    ) -> (&mut [NonZero<u8>], Option<&mut Nul>) {
        let whole_len = self.whole.len();
        let (body, nul) = self.split_mut_inner();

        // SAFETY: We know the size without a NUL to be less than the maximum SPA size.
        unsafe {
            hint::assert_unchecked(body.len() < super::MAX_SIZE as usize)
        };

        // SAFETY: We know that if there's a NUL, that the body length is equal to `whole - 1`, otherwise
        //         we know it is equal to `whole`.
        unsafe {
            hint::assert_unchecked({
                let n = body.len().unchecked_add(if let Some(Nul) = nul {
                    1_usize
                } else {
                    0_usize
                });

                n == whole_len
            })
        };

        (body, nul)
    }

    /// The [`u8`] correlary to [`SpaStr::split`]. The body is guaranteed to contain no NULs, and the optional NUL
    /// is guaranteed to be NUL.
    #[inline(always)]
    #[must_use]
    pub const fn split_bytes(&self) -> (&[u8], Option<&u8>) {
        let (body, nul) = self.split();

        // SAFETY: Mutation of the underlying buffer is impossible, and `u8` has less restrictive
        //         bit validity requirements.
        let body =
            unsafe { (&raw const *body as *const [u8]).as_ref_unchecked() };

        let nul = match nul {
            // SAFETY: Mutation is impossible, and `u8` can represent any initialized byte.
            Some(nul) => Some(unsafe {
                (&raw const *nul).cast::<u8>().as_ref_unchecked()
            }),
            None => None,
        };

        (body, nul)
    }

    /// The [`u8`] correlary to  [`SpaStr::split_mut`]. The body is guaranteed to contain no NULs, and the optional NUL
    /// is guaranteed to be NUL.
    ///
    /// # Safety
    ///
    /// Body musn't contain NULs before the borrow ends, and the NUL must be a NUL when the borrow ends. If you want a
    /// type-safe variant of this, see  [`SpaStr::split_mut`].
    #[inline(always)]
    #[must_use]
    pub const unsafe fn split_bytes_mut(
        &mut self
    ) -> (&mut [u8], Option<&mut u8>) {
        let (body, nul) = self.split_mut();

        // SAFETY: The caller ensures this is acceptable.
        let body = unsafe { (&raw mut *body as *mut [u8]).as_mut_unchecked() };

        let nul = match nul {
            // SAFETY: The caller ensures this is acceptable.
            Some(nul) => {
                Some(unsafe { (&raw mut *nul).cast::<u8>().as_mut_unchecked() })
            },
            None => None,
        };

        (body, nul)
    }

    /// Returns the body of the SPA string.
    #[inline(always)]
    #[must_use]
    pub const fn body(&self) -> &[NonZero<u8>] {
        self.split().0
    }

    /// Mutably returns the body of the SPA string.
    #[inline(always)]
    #[must_use]
    pub const fn body_mut(&mut self) -> &mut [NonZero<u8>] {
        self.split_mut().0
    }

    /// Returns the body of the SPA string, [`u8`] correlary to [`SpaStr::body`].
    #[inline(always)]
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8] {
        self.split_bytes().0
    }

    /// Returns the body of the SPA string, [`u8`] correlary to [`SpaStr::body_mut`].
    ///
    /// # Safety
    ///
    /// The caller must ensure that the body contains no NULs when the borrow ends.
    #[inline(always)]
    #[must_use]
    pub const unsafe fn as_bytes_mut(&mut self) -> &mut [u8] {
        // SAFETY: The caller ensures this is acceptable.
        unsafe { self.split_bytes_mut().0 }
    }

    /// Returns the entire underlying buffer of the SPA string.
    #[inline(always)]
    #[must_use]
    pub const fn as_whole(&self) -> &[u8] {
        // SAFETY: We know this to always be true.
        unsafe {
            hint::assert_unchecked(self.whole.len() <= super::MAX_SIZE as usize)
        };

        &self.whole
    }

    /// Returns the entire underlying buffer of the SPA string.
    ///
    /// # Safety
    ///
    /// The caller must ensure before the borrow ends:
    ///
    /// 1. The NUL, if any, remains unchanged.
    /// 2. That all bytes up to the NUL or end of the buffer are not NUL.
    /// 3. Other stuff probably, avoid using this. It exists mainly for consistency.
    ///
    /// There are *technically* some sound usages that violate these rules, however they're
    /// niche, and considered bad practice. So, uphold them.
    ///
    /// I'll document them, later.
    #[inline(always)]
    #[must_use]
    #[allow(unused_unsafe)]
    pub const unsafe fn as_whole_mut(&mut self) -> &mut [u8] {
        // SAFETY: We know this to always be true.
        unsafe {
            hint::assert_unchecked(self.whole.len() <= super::MAX_SIZE as usize)
        };

        // SAFETY: The caller ensures this is okay.
        unsafe { &mut self.whole }
    }

    /// Returns whether or not SPA string has an encoded NUL terminator already.
    #[inline(always)]
    #[must_use]
    pub const fn has_nul(&self) -> bool {
        self.split().1.is_some()
    }

    /// Returns the length of the body of the SPA string.
    #[inline(always)]
    #[must_use]
    pub const fn len(&self) -> usize {
        self.body().len()
    }

    /// Returns whether the body of this SPA string is empty.
    ///
    /// # Safety
    ///
    /// This is guaranteed to be less than [`super::MAX_SIZE`].
    #[inline(always)]
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns the length of the body, plus the NUL terminator.
    ///
    /// This is always `self.len() + 1`, regardless of whether or not
    /// the underlying buffer is actually NUL terminated.
    ///
    /// It's a convenience method.
    ///
    /// # Safety
    ///
    /// This is guaranteed to be less than or equal to [`super::MAX_SIZE`].
    #[inline(always)]
    #[must_use]
    pub const fn len_with_nul(&self) -> NonZero<usize> {
        NonZero::new(self.len().strict_add(1)).unwrap()
    }

    /// Borrows the underlying buffer as a C string.
    ///
    /// # Returns
    ///
    /// Returns [`None`] if the underlying buffer is not NUL terminated.
    #[inline(always)]
    #[must_use]
    pub const fn as_c_str(&self) -> Option<&CStr> {
        if self.has_nul() {
            // SAFETY: We just checked that `self` is NUL-terminated.
            Some(unsafe {
                (&raw const *self as *const CStr).as_ref_unchecked()
            })
        } else {
            None
        }
    }

    /// Mutably borrows the underlying buffer as a C string.
    ///
    /// # Returns
    ///
    /// Returns [`None`] if the underlying buffer is not NUL terminated.
    #[inline(always)]
    #[must_use]
    pub const fn as_c_str_mut(&mut self) -> Option<&mut CStr> {
        if self.has_nul() {
            // SAFETY: We just checked that `self` is NUL-terminated.
            Some(unsafe { (&raw mut *self as *mut CStr).as_mut_unchecked() })
        } else {
            None
        }
    }

    /// Returns the body of the string as a UTF-8 string, without the NUL terminator, if any.
    ///
    /// # Returns
    ///
    /// Returns [`Err`] if the string contains invalid UTF-8.
    #[inline(always)]
    pub const fn to_str(&self) -> Result<&str, Utf8Error> {
        std::str::from_utf8(self.as_bytes())
    }

    /// Returns the body of the string as a mutable UTF-8 string, without the NUL terminator, if any.
    ///
    /// # Returns
    ///
    /// Returns [`Err`] if the string contains invalid UTF-8.
    ///
    /// # Safety
    ///
    /// The caller must ensure that no NUL is within the string when the borrow ends.
    #[inline(always)]
    pub const unsafe fn to_str_mut(&mut self) -> Result<&mut str, Utf8Error> {
        // SAFETY: The caller ensures this is sound.
        std::str::from_utf8_mut(unsafe { self.as_bytes_mut() })
    }

    /// Returns the whole string as a UTF-8 string, including the NUL terminator, if any.
    ///
    /// # Returns
    ///
    /// Returns [`Err`] if the string contains invalid UTF-8.
    #[inline(always)]
    pub const fn to_str_whole(&self) -> Result<&str, Utf8Error> {
        std::str::from_utf8(self.as_whole())
    }

    /// Returns the whole string as a UTF-8 string, including the NUL terminator, if any.
    ///
    /// # Returns
    ///
    /// Returns [`Err`] if the string contains invalid UTF-8.
    ///
    /// # Safety
    ///
    /// The caller must ensure that the body of the string has no NUL,
    /// and that the trailing NUL terminator, if there is one, remains intact
    /// when the borrow ends.
    ///
    /// There are *technically* some sound usages that violate these rules, however they're
    /// niche, and considered bad practice. So, uphold them.
    ///
    /// I'll document them, later.
    #[inline(always)]
    pub const unsafe fn to_str_whole_mut(
        &mut self
    ) -> Result<&mut str, Utf8Error> {
        // SAFETY: The caller ensures this is sound.
        std::str::from_utf8_mut(unsafe { self.as_whole_mut() })
    }

    /// This returns whether this SPA string is ASCII.
    #[inline(always)]
    #[must_use]
    pub const fn is_ascii(&self) -> bool {
        // NOTE: NULs are always ASCII, so including it will always yield an equivalent result
        //       to `self.as_bytes().is_ascii()`.
        self.as_whole().is_ascii()
    }

    /// Makes this SPA string ASCII lowercase in-place, leaving non-ASCII
    /// bytes unchanged.
    ///
    /// # Returns
    ///
    /// Returns `self` for convenience.
    #[inline(always)]
    pub const fn make_ascii_lowercase(&mut self) -> &mut SpaStr {
        // SAFETY: The invariants of this type are left intact.
        unsafe { self.as_whole_mut().make_ascii_lowercase() };

        self
    }

    #[track_caller]
    fn to_ascii_lowercase_inner(&self) -> Box<SpaStr> {
        let body = self.as_bytes();
        let mut buf: Box<[MaybeUninit<u8>]> =
            Box::new_uninit_slice(body.len().strict_add(1));

        for i in 0..body.len() {
            buf[i].write(body[i].to_ascii_lowercase());
        }

        buf[body.len()].write(Nul as u8);

        // SAFETY: We initialized the buffer properly.
        unsafe {
            Box::from_raw(
                Box::into_raw(Box::<[_]>::assume_init(buf)) as *mut SpaStr
            )
        }
    }

    /// Returns an owned SPA string where each byte that is ASCII, is replaced with
    /// its lowercase counterpart.
    ///
    /// Bytes that are non-ASCII, or lack such a counterpart, much like [`SpaStr::make_ascii_lowercase`],
    /// are left unchanged.
    #[inline]
    #[must_use]
    #[track_caller]
    pub fn to_ascii_lowercase(&self) -> Box<SpaStr> {
        self.body()
            .iter()
            .copied()
            .map(NonZero::get)
            .map(|b| b.to_ascii_lowercase())
            .map(NonZero::new)
            .map(Option::unwrap)
            .collect()
    }

    /// Makes this SPA string ASCII uppercase in-place, leaving non-ASCII
    /// bytes unchanged.
    ///
    /// # Returns
    ///
    /// Returns `self` for convenience.
    #[inline(always)]
    pub const fn make_ascii_uppercase(&mut self) -> &mut SpaStr {
        // SAFETY: The invariants of this type are left intact.
        unsafe { self.as_whole_mut().make_ascii_uppercase() };

        self
    }

    /// Returns an owned SPA string where each byte that is ASCII, is replaced with
    /// its uppercase counterpart.
    ///
    /// Bytes that are non-ASCII, or lack such a counterpart, much like [`SpaStr::make_ascii_uppercase`],
    /// are left unchanged.
    #[inline]
    #[must_use]
    #[track_caller]
    pub fn to_ascii_uppercase(&self) -> Box<SpaStr> {
        self.body()
            .iter()
            .copied()
            .map(NonZero::get)
            .map(|b| b.to_ascii_uppercase())
            .map(NonZero::new)
            .map(Option::unwrap)
            .collect()
    }

    /// Allocate a heap value of this SPA string, including a NUL terminator.
    #[inline(never)]
    #[must_use]
    #[track_caller]
    fn make_heap_inner<H>(&self) -> H
    where
        H: HeapAlloc<Target = SpaStr>,
    {
        let bytes = self.as_bytes();

        // SAFETY: This is a buffer we have unique access to, and is the length
        //         of the body of our string, plus one for the NUL.
        let mut alloc: NonNull<[MaybeUninit<u8>]> = {
            let len_with_nul = bytes.len().strict_add(1);

            H::WithTarget::<u8>::new_uninit_slice(len_with_nul).into_raw()
        };

        // SAFETY: We have unique access to the underlying memory.
        unsafe {
            alloc
                .as_mut()
                .get_unchecked_mut(..bytes.len())
                .write_copy_of_slice(bytes)
        };
        // SAFETY: Same as above.
        unsafe {
            alloc
                .as_mut()
                .get_unchecked_mut(bytes.len())
                .write(Nul as u8)
        };

        // SAFETY: We know the buffer is a nonnull pointer.
        let alloc =
            unsafe { NonNull::new_unchecked(alloc.as_ptr() as *mut SpaStr) };

        // SAFETY: We know we've initialized the buffer properly.
        unsafe { H::from_raw(alloc) }
    }

    /// This does the work of allocating this type in addition to inserting some compiler hints,
    /// as `make_heap_inner` is not inlined, so any hints are ultimately lost.
    #[inline(always)]
    #[must_use]
    #[track_caller]
    fn make_heap<H>(&self) -> H
    where
        H: HeapAlloc<Target = SpaStr>,
    {
        let alloc = self.make_heap_inner::<H>();

        // SAFETY: We know `alloc` is the size of `self` with a NUL, as it has a NUL.
        unsafe {
            hint::assert_unchecked(alloc.has_nul() && alloc.len() == self.len())
        };

        alloc
    }

    /// Insert the NUL into a `SpaStr` that lacks one.
    ///
    /// We're not inlining this function body as it gets quite big, and realistically we don't expect this
    /// code to be called very often.
    ///
    /// So why pollute the instruction cache?
    ///
    /// # Safety
    ///
    /// Caller must ensure there is no NUL terminator.
    #[must_use]
    #[inline(never)]
    #[track_caller]
    unsafe fn insert_nul_boxed(self: Box<SpaStr>) -> Box<SpaStr> {
        // SAFETY: We know a `self` is just a byte buffer.
        let buf: Box<[u8]> =
            unsafe { Box::from_raw(Box::into_raw(self) as *mut [u8]) };

        let buf: Box<[u8]> = {
            let mut buf = buf.into_vec();

            // NOTE: We're avoiding amortized growths, as we'd prefer to only allocate one additional byte,
            //       as we're immediately shrinking the buffer to the exact required length...
            //
            //       We ***could*** just use `std::alloc::realloc`, and write the nul at the end,
            //       but this realistically should be fine, and reuses vec machinery which is likely
            //       already in the instruction cache.
            //
            //       Plus, `Vec`s will handle the `[u8; 0]` case for us. So, yeah.
            buf.reserve_exact(1);

            // SAFETY: Do not remove this, we're relying on the existence of a NUL terminator elsewhere.
            buf.push(Nul as u8);

            buf.into_boxed_slice()
        };

        let buf = Box::into_raw(buf) as *mut SpaStr;

        // SAFETY: We know the NUL has been inserted successfully.
        unsafe { Box::from_raw(buf) }
    }

    /// This is the implementation of the `Box<SpaStr> -> Box<CStr>` conversion,
    /// and the derivitives thereof.
    #[inline(always)]
    #[must_use]
    #[track_caller]
    fn make_boxed_cstr(self: Box<SpaStr>) -> Box<CStr> {
        let len_with_nul = self.len_with_nul();
        let spa_str = if self.has_nul() {
            self
        } else {
            // SAFETY: We know it lacks a NUL terminator.
            unsafe { self.insert_nul_boxed() }
        };

        // SAFETY: We know it has a NUL terminator.
        unsafe {
            hint::assert_unchecked(
                spa_str.has_nul()
                    && spa_str.whole.len() == len_with_nul.get()
                    && spa_str.len() == len_with_nul.get() - 1,
            )
        };

        // SAFETY: We know `spa_str` now has the required terminating NUL.
        unsafe { Box::from_raw(Box::into_raw(spa_str) as *mut CStr) }
    }

    /// This will insert a NUL into this `Rc<SpaStr>`.
    ///
    /// We avoid inlining to prevent polluting the caller with drop glue.
    ///
    /// This function is safe as it relies on on the `From<&SpaStr>` impl, which automatically
    /// inserts a NUL.
    #[must_use]
    #[track_caller]
    #[inline(never)]
    fn insert_nul_rc(self: Rc<SpaStr>) -> Rc<SpaStr> {
        <&SpaStr>::into(&self)
    }

    /// This will take ownership of this this `Rc<SpaStr>`, and if it contains a NUL,
    /// reinterpret it as a `Rc<CStr>`.
    ///
    /// Otherwise, it'll allocate a new C string.
    #[inline(always)]
    #[must_use]
    #[track_caller]
    fn make_rc_cstr(self: Rc<SpaStr>) -> Rc<CStr> {
        let len_with_nul = self.len_with_nul();
        let spa_str = if self.has_nul() {
            // SAFETY: We already have a NUL, no need to reallocate.
            self
        } else {
            // SAFETY: This internally uses `make_heap`, which inserts a NUL for us.
            self.insert_nul_rc()
        };

        // SAFETY: We know the new string contains a NUL, and its whole buffer length is equal
        //         to our length with a NUL.
        unsafe {
            hint::assert_unchecked(
                spa_str.has_nul() && spa_str.whole.len() == len_with_nul.get(),
            )
        };

        // SAFETY: We know this buffer now has a NUL terminator.
        unsafe { Rc::from_raw(Rc::into_raw(spa_str) as *const CStr) }
    }

    /// This will insert a NUL into this `Arc<SpaStr>`.
    ///
    /// We avoid inlining to prevent polluting the caller with drop glue.
    ///
    /// This function is safe as it relies on on the `From<&SpaStr>` impl, which automatically
    /// inserts a NUL.
    #[must_use]
    #[track_caller]
    #[inline(never)]
    fn insert_nul_arc(self: Arc<SpaStr>) -> Arc<SpaStr> {
        <&SpaStr>::into(&self)
    }

    /// This will take ownership of this this `Arc<SpaStr>`, and if it contains a NUL,
    /// reinterpret it as a `Arc<CStr>`.
    ///
    /// Otherwise, it'll allocate a new C string.
    #[inline(always)]
    #[must_use]
    #[track_caller]
    fn make_arc_cstr(self: Arc<SpaStr>) -> Arc<CStr> {
        let len_with_nul = self.len_with_nul();
        let spa_str = if self.has_nul() {
            // SAFETY: We already have a NUL, no need to reallocate.
            self
        } else {
            // SAFETY: This internally uses `make_heap`, which inserts a NUL for us.
            self.insert_nul_arc()
        };

        // SAFETY: We know the new string contains a NUL, and its whole buffer length is equal
        //         to our length with a NUL.
        unsafe {
            hint::assert_unchecked(
                spa_str.has_nul() && spa_str.whole.len() == len_with_nul.get(),
            )
        };

        // SAFETY: We know this buffer now has a NUL terminator.
        unsafe { Arc::from_raw(Arc::into_raw(spa_str) as *const CStr) }
    }

    /// This just returns a boxed `SpaStr` that is empty but ***also*** contains no NUL.
    ///
    /// This won't allocate, we just use it as a placeholder value.
    #[track_caller]
    #[inline(always)]
    fn empty_boxed() -> Box<SpaStr> {
        let buf: Box<[u8]> = [].into();

        // SAFETY: We know an empty boxed byte slice is a valid `SpaStr`.
        unsafe { Box::from_raw(Box::into_raw(buf) as *mut SpaStr) }
    }

    /// # Safety
    ///
    /// The caller must ensure that the size of the allocation for `dest` != `src.len_with_nul()`.
    unsafe fn clone_from_boxed_realloc(
        dest: &mut Box<SpaStr>,
        buf: Box<SpaStr>,
        len_with_nul: NonZero<usize>,
    ) -> Box<[MaybeUninit<u8>]> {
        // SAFETY: We know that `buf` is an initialized buffer, but since we're discarding
        //         the underlying memory, it's safe to treat it as uninitialized.
        let buf: NonNull<[u8]> =
            unsafe { NonNull::new_unchecked(Box::into_raw(buf) as *mut [u8]) };

        let (old_layout, new_layout) = {
            // SAFETY: We know the value in `buf` is a valid allocation, and thus is a valid
            //         allocation.
            let old_layout =
                unsafe { Layout::array::<u8>(buf.len()).unwrap_unchecked() };

            // SAFETY: We know that the length of a new buffer with a NUL is is less than
            //         `MAX_SIZE`, and thus is less than `isize::MAX`, so this is always safe.
            let new_layout = unsafe {
                Layout::array::<u8>(len_with_nul.get()).unwrap_unchecked()
            };

            (old_layout, new_layout)
        };

        // SAFETY: Just in case the allocator unwinds, we want to abort to prevent unsoundness.
        //
        //         This is unlikely to ever be needed, ***however***, it's still good to have.
        //
        //         Additionally, we need to adjust the resulting new slice if allocation succeeded,
        //         as the returned slice *may be larger than the requested size*.
        let alloc_result = util::abort_on_unwind(|| {
            // NOTE: We're comparing the sizes of the old and new layouts.
            let order = old_layout.size().cmp(&new_layout.size());

            match order {
                // SAFETY: The caller ensures us the layouts are not equal.
                Ordering::Equal => unsafe { hint::unreachable_unchecked() },
                // SAFETY: We have ownership of `buf`, we know `old_layout < new_layout`,
                //         thus it is safe to grow the allocation.
                Ordering::Less => unsafe {
                    Global.grow(buf.cast(), old_layout, new_layout)
                },
                // SAFETY: We have ownership of `buf`, we know `old_layout > new_layout`,
                //         thus it is safe to shrink the allocation.
                Ordering::Greater => unsafe {
                    Global.shrink(buf.cast(), old_layout, new_layout)
                },
            }
        });

        // SAFETY: To prevent the caller observing our intermediate, empty buffer on an unwind,
        //         we're going to replace the underlying buffer with `buf`.
        //
        //         The destructor of this guard will only be called when unwinding the stack if
        //         `alloc_result` is `Err`.
        //
        //         To me it makes more sense to have this update happen in the unwinding code,
        //         rather than before we start to unwind, as we consider the failure case to be
        //         ***highly unlikely***, and we'd rather it not pollute anything.
        let unwind_guard = DropGuard::new(buf, |buf| {
            // SAFETY: We know this to only ever be called ***when unwinding the stack***,
            //         and as per the contract for `Allocator::grow` and `Allocator::shrink`,
            //         if it returns an error, the original allocation must remain intact.
            //
            //         As such, since this will only run in response to unwinding the stack
            //         in response to such an error, we know `buf` to remain unchanged, and
            //         still, valid.
            let buf: Box<SpaStr> =
                unsafe { Box::from_raw(buf.as_ptr() as *mut SpaStr) };

            // NOTE: We know the placeholder value in `dest` to be a zero-sized allocation,
            //       and thus can be leaked without actually any risk of a memory leak.
            let _ = Box::leak(mem::replace(dest, buf));
        });

        // NOTE: We need to adjust the length of this buffer to `len_with_nul`.
        let Ok(buf) = alloc_result else {
            // NOTE: This will call the destructor for the above guard.
            std::alloc::handle_alloc_error(new_layout);
        };

        // SAFETY: There was no failure when allocating the new buffer, so we
        //         must dismiss the `unwind_guard` to avoid UB, as the original `buf`
        //         is now invalidated.
        let _ = DropGuard::dismiss(unwind_guard);

        // SAFETY: We know that `buf` is big enough for `len_with_nul`, and that
        //         we can deallocate it with `new_layout`, which is derived from it.
        let buf: NonNull<[MaybeUninit<u8>]> =
            NonNull::slice_from_raw_parts(buf.cast(), len_with_nul.get());

        // SAFETY: We know `buf` is a valid boxed uninit slice.
        unsafe { Box::from_non_null(buf) }
    }

    /// This is the implementation of `clone_into_boxed`.
    #[allow(let_underscore_drop)]
    #[inline(never)]
    fn clone_from_boxed(
        dest: &mut Box<SpaStr>,
        src: &SpaStr,
    ) {
        let buf = mem::replace(dest, SpaStr::empty_boxed());

        let body = src.as_bytes();
        let len_with_nul = NonZero::new(body.len().strict_add(1)).unwrap();

        // NOTE: This is a buffer whose length is equal to the length of `self` plus one, for the NUL.
        //
        //       This buffer may be uninitialized, all we guarantee is that it has enough room.
        let mut buf = if buf.as_whole().len() == len_with_nul.get() {
            // SAFETY: We know there's enough room in `buf`.
            unsafe {
                Box::from_raw(Box::into_raw(buf) as *mut [MaybeUninit<u8>])
            }
        } else {
            // SAFETY: We know the size of the buffers differ.
            unsafe { SpaStr::clone_from_boxed_realloc(dest, buf, len_with_nul) }
        };

        // SAFETY: We know the length of `buf` to be `len_with_nul`.
        unsafe {
            buf.get_unchecked_mut(..body.len())
                .write_copy_of_slice(body)
        };
        // SAFETY: Same as above.
        unsafe { buf.get_unchecked_mut(body.len()).write(Nul as u8) };

        // SAFETY: We know that we've initialized `buf` properly as a `SpaStr`.
        let buf = unsafe { Box::from_raw(Box::into_raw(buf) as *mut SpaStr) };

        // NOTE: We're writing the new buffer, and leaking the old one, as we know it to be
        //       zero sized, and we don't want unecessary deallocation code.
        let _ = Box::leak(mem::replace(dest, buf));
    }

    /// Clones `self` into the the provided boxed `SpaStr`, while also inserting a NUL if required.
    #[track_caller]
    #[inline(always)]
    fn clone_into_boxed(
        &self,
        dest: &mut Box<SpaStr>,
    ) {
        SpaStr::clone_from_boxed(dest, self);

        // SAFETY: We know `dest` to contain a NUL and the body of `self` now.
        unsafe {
            hint::assert_unchecked(dest.has_nul() && dest.len() == self.len())
        };
    }
}

unsafe impl AsBytes for SpaStr {}

impl AsRef<[u8]> for SpaStr {
    #[inline(always)]
    fn as_ref(&self) -> &[u8] {
        self.as_bytes()
    }
}

impl Borrow<[u8]> for SpaStr {
    #[inline(always)]
    fn borrow(&self) -> &[u8] {
        self.as_bytes()
    }
}

impl AsRef<[NonZero<u8>]> for SpaStr {
    #[inline(always)]
    fn as_ref(&self) -> &[NonZero<u8>] {
        self.body()
    }
}

impl AsMut<[NonZero<u8>]> for SpaStr {
    #[inline(always)]
    fn as_mut(&mut self) -> &mut [NonZero<u8>] {
        self.body_mut()
    }
}

impl Borrow<[NonZero<u8>]> for SpaStr {
    #[inline(always)]
    fn borrow(&self) -> &[NonZero<u8>] {
        self.body()
    }
}

impl BorrowMut<[NonZero<u8>]> for SpaStr {
    #[inline(always)]
    fn borrow_mut(&mut self) -> &mut [NonZero<u8>] {
        self.body_mut()
    }
}

impl Default for &SpaStr {
    #[inline(always)]
    fn default() -> Self {
        SpaStr::empty()
    }
}

impl Default for Box<SpaStr> {
    #[inline(always)]
    #[track_caller]
    fn default() -> Self {
        SpaStr::empty().into()
    }
}

impl Clone for Box<SpaStr> {
    #[inline(always)]
    fn clone(&self) -> Self {
        <&SpaStr>::into(self)
    }

    #[inline(always)]
    fn clone_from(
        &mut self,
        source: &Self,
    ) {
        source.clone_into_boxed(self);
    }
}

impl ToOwned for SpaStr {
    type Owned = Box<SpaStr>;

    #[inline(always)]
    #[track_caller]
    fn to_owned(&self) -> Self::Owned {
        self.into()
    }
}

impl fmt::Debug for SpaStr {
    #[track_caller]
    fn fmt(
        &self,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        // FIXME: Avoid an allocation.
        String::from_utf8_lossy(self.as_bytes()).fmt(f)
    }
}

impl PartialEq for SpaStr {
    #[inline(always)]
    fn eq(
        &self,
        other: &Self,
    ) -> bool {
        self.as_bytes() == other.as_bytes()
    }
}

impl Eq for SpaStr {}

impl PartialEq<CStr> for SpaStr {
    #[inline(always)]
    fn eq(
        &self,
        other: &CStr,
    ) -> bool {
        self.as_bytes() == other.to_bytes()
    }
}

impl PartialEq<SpaStr> for CStr {
    #[inline(always)]
    fn eq(
        &self,
        other: &SpaStr,
    ) -> bool {
        self.to_bytes() == other.as_bytes()
    }
}

impl PartialOrd for SpaStr {
    #[inline(always)]
    fn partial_cmp(
        &self,
        other: &Self,
    ) -> Option<Ordering> {
        Some(self.cmp(other))
    }

    #[inline(always)]
    fn ge(
        &self,
        other: &Self,
    ) -> bool {
        self.as_bytes() >= other.as_bytes()
    }

    #[inline(always)]
    fn gt(
        &self,
        other: &Self,
    ) -> bool {
        self.as_bytes() > other.as_bytes()
    }

    #[inline(always)]
    fn le(
        &self,
        other: &Self,
    ) -> bool {
        self.as_bytes() <= other.as_bytes()
    }

    #[inline(always)]
    fn lt(
        &self,
        other: &Self,
    ) -> bool {
        self.as_bytes() < other.as_bytes()
    }
}

impl Ord for SpaStr {
    #[inline(always)]
    fn cmp(
        &self,
        other: &Self,
    ) -> Ordering {
        self.as_bytes().cmp(other.as_bytes())
    }
}

impl PartialOrd<CStr> for SpaStr {
    #[inline(always)]
    fn partial_cmp(
        &self,
        other: &CStr,
    ) -> Option<Ordering> {
        Some(self.as_bytes().cmp(other.to_bytes()))
    }

    #[inline(always)]
    fn ge(
        &self,
        other: &CStr,
    ) -> bool {
        self.as_bytes() >= other.to_bytes()
    }

    #[inline(always)]
    fn gt(
        &self,
        other: &CStr,
    ) -> bool {
        self.as_bytes() > other.to_bytes()
    }

    #[inline(always)]
    fn le(
        &self,
        other: &CStr,
    ) -> bool {
        self.as_bytes() <= other.to_bytes()
    }

    #[inline(always)]
    fn lt(
        &self,
        other: &CStr,
    ) -> bool {
        self.as_bytes() < other.to_bytes()
    }
}

impl PartialOrd<SpaStr> for CStr {
    #[inline(always)]
    fn partial_cmp(
        &self,
        other: &SpaStr,
    ) -> Option<Ordering> {
        Some(self.to_bytes().cmp(other.as_bytes()))
    }

    #[inline(always)]
    fn lt(
        &self,
        other: &SpaStr,
    ) -> bool {
        self.to_bytes() < other.as_bytes()
    }

    #[inline(always)]
    fn le(
        &self,
        other: &SpaStr,
    ) -> bool {
        self.to_bytes() <= other.as_bytes()
    }

    #[inline(always)]
    fn gt(
        &self,
        other: &SpaStr,
    ) -> bool {
        self.to_bytes() > other.as_bytes()
    }

    #[inline(always)]
    fn ge(
        &self,
        other: &SpaStr,
    ) -> bool {
        self.to_bytes() >= other.as_bytes()
    }
}

impl hash::Hash for SpaStr {
    #[inline(always)]
    #[track_caller]
    fn hash<H>(
        &self,
        state: &mut H,
    ) where
        H: hash::Hasher,
    {
        self.as_bytes().hash(state);
    }
}

/// Calculates the range of the whole slice needed for `I`.
///
/// This helps us bypass the fact we cannot directly determine the bounds of `I`.
#[inline(always)]
#[track_caller]
fn index_range<I>(
    spa_str: &SpaStr,
    index: I,
) -> Range<usize>
where
    [u8]: Index<I, Output = [u8]>,
{
    let (body, nul) = spa_str.split_bytes();

    // Since we can't actually get access to the bounds we need,
    // we're indexing first, then we're going to convert it into indexes.
    let Range {
        start: start_ptr,
        end: end_ptr,
    } = body.index(index).as_ptr_range().into();

    // SAFETY: We know `start_ptr` lies within `body`.
    let start = unsafe { start_ptr.offset_from_unsigned(body.as_ptr()) };

    let end = {
        // SAFETY: We know `end_ptr` lies within `body`.
        let end = unsafe { end_ptr.offset_from_unsigned(body.as_ptr()) };

        // SAFETY: `spa_str` has a NUL terminator AND `end` is equal to the body length,
        //          then we want to include the NUL in the total range we're indexing by.
        //
        //          Since we only increment when we need to, this is sound.
        unsafe {
            end.unchecked_add(if nul.is_some() && end == body.len() {
                1_usize
            } else {
                0_usize
            })
        }
    };

    // SAFETY: We know `start` is never after `end`, and that both lie within `spa_str.whole`.
    unsafe {
        hint::assert_unchecked(
            start <= end
                && start <= spa_str.whole.len()
                && end <= spa_str.whole.len(),
        )
    };

    (start..end).into()
}

// SAFETY: This is sound for all subslices, as there's no possibility of mutating the buffer.
impl<I> Index<I> for SpaStr
where
    I: SliceIndex<[u8], Output = [u8]>,
{
    type Output = SpaStr;

    #[inline(always)]
    #[track_caller]
    fn index(
        &self,
        index: I,
    ) -> &SpaStr {
        let range = index_range(self, index);
        // SAFETY: We know the range is valid.
        let sub_str = unsafe { self.as_whole().get_unchecked(range) };

        // SAFETY: Any substring of a `SpaStr` is a valid `SpaStr`.
        unsafe { (&raw const *sub_str as *const SpaStr).as_ref_unchecked() }
    }
}

// SAFETY: This is sound for all subslices, as since we're protecting the underlying slice through a `SpaStr`,
//         and the rules of a `SpaStr` require that we cannot insert a NUL if there isn't already one,
//         or remove an existing NUL, and so on, even though we're providing mutable access, callers
//         require unsafe to invalidate the underlying buffer.
impl<I> IndexMut<I> for SpaStr
where
    I: SliceIndex<[u8], Output = [u8]>,
{
    #[inline(always)]
    #[track_caller]
    fn index_mut(
        &mut self,
        index: I,
    ) -> &mut Self::Output {
        let range = index_range(self, index);
        // SAFETY: We're not invalidating the underlying buffer, and we know the range is valid.
        let sub_str = unsafe { self.as_whole_mut().get_unchecked_mut(range) };

        // SAFETY: Any substring of a `SpaStr` is a valid `SpaStr`.
        unsafe { (&raw mut *sub_str as *mut SpaStr).as_mut_unchecked() }
    }
}

impl<'a> TryFrom<&'a SpaStr> for &'a str {
    type Error = Utf8Error;

    #[inline(always)]
    fn try_from(spa_str: &'a SpaStr) -> Result<Self, Self::Error> {
        spa_str.to_str()
    }
}

impl<'a> TryFrom<&'a mut SpaStr> for &'a str {
    type Error = Utf8Error;

    #[inline(always)]
    fn try_from(spa_str: &'a mut SpaStr) -> Result<Self, Self::Error> {
        spa_str.to_str()
    }
}

impl<'a> TryFrom<&'a SpaStr> for String {
    type Error = Utf8Error;

    #[inline(always)]
    fn try_from(spa_str: &'a SpaStr) -> Result<Self, Self::Error> {
        spa_str.try_into().map(str::to_owned)
    }
}

impl<'a> TryFrom<&'a mut SpaStr> for String {
    type Error = Utf8Error;

    #[inline(always)]
    fn try_from(spa_str: &'a mut SpaStr) -> Result<Self, Self::Error> {
        spa_str.try_into().map(str::to_owned)
    }
}

impl<'a> TryFrom<&'a SpaStr> for Box<str> {
    type Error = Utf8Error;

    #[inline(always)]
    fn try_from(spa_str: &'a SpaStr) -> Result<Self, Self::Error> {
        spa_str.to_str().map(Box::from)
    }
}

impl<'a> TryFrom<&'a mut SpaStr> for Box<str> {
    type Error = Utf8Error;

    #[inline(always)]
    fn try_from(spa_str: &'a mut SpaStr) -> Result<Self, Self::Error> {
        spa_str.to_str().map(Box::from)
    }
}

impl<'a> TryFrom<&'a SpaStr> for Rc<str> {
    type Error = Utf8Error;

    #[inline(always)]
    fn try_from(spa_str: &'a SpaStr) -> Result<Self, Self::Error> {
        spa_str.to_str().map(Rc::from)
    }
}

impl<'a> TryFrom<&'a mut SpaStr> for Rc<str> {
    type Error = Utf8Error;

    #[inline(always)]
    fn try_from(spa_str: &'a mut SpaStr) -> Result<Self, Self::Error> {
        spa_str.to_str().map(Rc::from)
    }
}

impl TryFrom<Box<SpaStr>> for Rc<str> {
    type Error = Utf8Error;

    #[inline(always)]
    fn try_from(spa_str: Box<SpaStr>) -> Result<Self, Self::Error> {
        spa_str.to_str().map(Rc::from)
    }
}

impl<'a> TryFrom<&'a SpaStr> for Arc<str> {
    type Error = Utf8Error;

    #[inline(always)]
    fn try_from(spa_str: &'a SpaStr) -> Result<Self, Self::Error> {
        spa_str.to_str().map(Arc::from)
    }
}

impl<'a> TryFrom<&'a mut SpaStr> for Arc<str> {
    type Error = Utf8Error;

    #[inline(always)]
    fn try_from(spa_str: &'a mut SpaStr) -> Result<Self, Self::Error> {
        spa_str.to_str().map(Arc::from)
    }
}

impl TryFrom<Box<SpaStr>> for Arc<str> {
    type Error = Utf8Error;

    #[inline(always)]
    fn try_from(spa_str: Box<SpaStr>) -> Result<Self, Self::Error> {
        spa_str.to_str().map(Arc::from)
    }
}

impl<'a> TryFrom<&'a SpaStr> for Cow<'a, str> {
    type Error = Utf8Error;

    #[inline(always)]
    fn try_from(spa_str: &'a SpaStr) -> Result<Self, Self::Error> {
        spa_str.try_into().map(Cow::Borrowed)
    }
}

impl<'a> TryFrom<&'a mut SpaStr> for Cow<'a, str> {
    type Error = Utf8Error;

    #[inline(always)]
    fn try_from(spa_str: &'a mut SpaStr) -> Result<Self, Self::Error> {
        spa_str.try_into().map(Cow::Borrowed)
    }
}

impl TryFrom<Box<SpaStr>> for Cow<'_, str> {
    type Error = WithUtf8Error<Box<SpaStr>>;

    #[inline(always)]
    #[track_caller]
    fn try_from(spa_str: Box<SpaStr>) -> Result<Self, Self::Error> {
        spa_str.try_into().map(Cow::Owned)
    }
}

impl TryFrom<Box<SpaStr>> for String {
    type Error = WithUtf8Error<Box<SpaStr>>;

    #[inline(always)]
    fn try_from(spa_str: Box<SpaStr>) -> Result<Self, Self::Error> {
        // NOTE: It doesn't matter whether we use `to_str` or `to_str_whole` for the validation,
        //       as NULs are always valid UTF-8.
        match spa_str.to_str_whole() {
            Ok(_) => Ok({
                let buf = {
                    // SAFETY: We know `spa_str` is a byte buffer.
                    let buf = unsafe {
                        Box::from_raw(Box::into_raw(spa_str) as *mut [u8])
                    };
                    let mut buf = buf.to_vec();

                    // NOTE: We're removing the NUL if it exists.
                    buf.pop_if(|&mut last| last == Nul as u8);

                    buf
                };

                // SAFETY: We know the buffer contains valid UTF-8.
                unsafe { String::from_utf8_unchecked(buf) }
            }),
            Err(error) => Err(WithUtf8Error {
                error,
                value: spa_str,
            }),
        }
    }
}

impl TryFrom<Box<SpaStr>> for Box<str> {
    type Error = WithUtf8Error<Box<SpaStr>>;

    #[inline(always)]
    #[track_caller]
    fn try_from(spa_str: Box<SpaStr>) -> Result<Self, Self::Error> {
        String::try_from(spa_str).map(String::into_boxed_str)
    }
}

impl<'a> TryFrom<&'a str> for &'a SpaStr {
    type Error = PodError;

    #[inline(always)]
    fn try_from(str: &'a str) -> Result<Self, Self::Error> {
        SpaStr::from_bytes(str)
    }
}

impl<'a> TryFrom<&'a mut str> for &'a SpaStr {
    type Error = PodError;

    #[inline(always)]
    fn try_from(str: &'a mut str) -> Result<Self, Self::Error> {
        SpaStr::from_bytes(str)
    }
}

#[allow(clippy::try_from_instead_of_from_str)]
impl<'a> TryFrom<&'a str> for Box<SpaStr> {
    type Error = PodError;

    #[inline(always)]
    fn try_from(str: &'a str) -> Result<Self, Self::Error> {
        str.try_into().map(SpaStr::to_owned)
    }
}

impl<'a> TryFrom<&'a mut str> for Box<SpaStr> {
    type Error = PodError;

    #[inline(always)]
    fn try_from(str: &'a mut str) -> Result<Self, Self::Error> {
        str.try_into().map(SpaStr::to_owned)
    }
}

impl FromStr for Box<SpaStr> {
    type Err = PodError;

    #[inline(always)]
    #[track_caller]
    fn from_str(str: &str) -> Result<Self, Self::Err> {
        str.try_into()
    }
}

impl TryFrom<Box<str>> for Box<SpaStr> {
    type Error = WithPodError<Box<str>>;

    #[inline(always)]
    fn try_from(str: Box<str>) -> Result<Self, Self::Error> {
        match SpaStr::from_bytes(&*str) {
            Ok(_) => {
                // SAFETY: We know the entirety of `str` is a valid SPA string.
                Ok(unsafe { Box::from_raw(Box::into_raw(str) as *mut SpaStr) })
            },
            Err(pod_error) => Err(WithPodError {
                pod_error,
                value: str,
            }),
        }
    }
}

impl TryFrom<String> for Box<SpaStr> {
    type Error = WithPodError<String>;

    #[inline(always)]
    fn try_from(string: String) -> Result<Self, Self::Error> {
        match SpaStr::from_bytes(&*string) {
            Ok(_) => {
                let string: Box<str> = string.into();

                // SAFETY: We know the entirety of `str` is a valid SPA string.
                Ok(unsafe {
                    Box::from_raw(Box::into_raw(string) as *mut SpaStr)
                })
            },
            Err(pod_error) => Err(WithPodError {
                pod_error,
                value: string,
            }),
        }
    }
}

impl<'a> TryFrom<Cow<'a, str>> for Box<SpaStr> {
    type Error = WithPodError<Cow<'a, str>>;

    #[inline(always)]
    fn try_from(str: Cow<'a, str>) -> Result<Self, Self::Error> {
        match SpaStr::from_bytes(&*str) {
            Ok(_) => {
                let str = Box::<str>::from(str);

                // SAFETY: We know the entirety of `str` is a valid SPA string.
                Ok(unsafe { Box::from_raw(Box::into_raw(str) as *mut SpaStr) })
            },
            Err(pod_error) => Err(WithPodError {
                pod_error,
                value: str,
            }),
        }
    }
}

impl TryFrom<Rc<str>> for Box<SpaStr> {
    type Error = WithPodError<Rc<str>>;

    #[inline(always)]
    fn try_from(str: Rc<str>) -> Result<Self, Self::Error> {
        match SpaStr::from_bytes(&*str) {
            Ok(spa_str) => Ok(spa_str.to_owned()),
            Err(pod_error) => Err(WithPodError {
                pod_error,
                value: str,
            }),
        }
    }
}

impl TryFrom<Arc<str>> for Box<SpaStr> {
    type Error = WithPodError<Arc<str>>;

    #[inline(always)]
    fn try_from(str: Arc<str>) -> Result<Self, Self::Error> {
        match SpaStr::from_bytes(&*str) {
            Ok(spa_str) => Ok(spa_str.to_owned()),
            Err(pod_error) => Err(WithPodError {
                pod_error,
                value: str,
            }),
        }
    }
}

// impl<'a> TryFrom<Cow<'a, str>> for Cow<'a, SpaStr> {}

// impl<'a> TryFrom<&'a str> for Box<SpaStr> {
//     type Error = $;

//     fn try_from(value: &'a str) -> Result<Self, Self::Error> {
//         todo!()
//     }
// }

/// This will automatically insert a NUL terminator.
impl From<&SpaStr> for Box<SpaStr> {
    #[inline(always)]
    fn from(spa_str: &SpaStr) -> Self {
        spa_str.make_heap()
    }
}

/// This will automatically insert a NUL terminator.
impl From<&mut SpaStr> for Box<SpaStr> {
    #[inline(always)]
    #[track_caller]
    fn from(spa_str: &mut SpaStr) -> Self {
        <&SpaStr>::into(spa_str)
    }
}

/// This will automatically insert a NUL terminator.
impl From<&SpaStr> for Rc<SpaStr> {
    #[inline(always)]
    fn from(spa_str: &SpaStr) -> Self {
        spa_str.make_heap()
    }
}

/// This will automatically insert a NUL terminator.
impl From<&mut SpaStr> for Rc<SpaStr> {
    #[inline(always)]
    #[track_caller]
    fn from(spa_str: &mut SpaStr) -> Self {
        <&SpaStr>::into(spa_str)
    }
}

/// This will automatically insert a NUL terminator.
impl From<&SpaStr> for Arc<SpaStr> {
    #[inline(always)]
    fn from(spa_str: &SpaStr) -> Self {
        spa_str.make_heap()
    }
}

/// This will automatically insert a NUL terminator.
impl From<&mut SpaStr> for Arc<SpaStr> {
    #[inline(always)]
    #[track_caller]
    fn from(spa_str: &mut SpaStr) -> Self {
        <&SpaStr>::into(spa_str)
    }
}

impl From<Box<SpaStr>> for Box<CStr> {
    #[inline(always)]
    fn from(spa_str: Box<SpaStr>) -> Self {
        spa_str.make_boxed_cstr()
    }
}

impl From<&SpaStr> for Box<CStr> {
    #[inline(always)]
    #[track_caller]
    fn from(spa_str: &SpaStr) -> Self {
        Box::<SpaStr>::from(spa_str).into()
    }
}

impl From<&mut SpaStr> for Box<CStr> {
    #[inline(always)]
    #[track_caller]
    fn from(spa_str: &mut SpaStr) -> Self {
        Box::<SpaStr>::from(spa_str).into()
    }
}

impl From<&SpaStr> for Rc<CStr> {
    #[inline(always)]
    #[track_caller]
    fn from(spa_str: &SpaStr) -> Self {
        Rc::<SpaStr>::from(spa_str).make_rc_cstr()
    }
}

impl From<&mut SpaStr> for Rc<CStr> {
    #[inline(always)]
    #[track_caller]
    fn from(spa_str: &mut SpaStr) -> Self {
        Rc::<SpaStr>::from(spa_str).make_rc_cstr()
    }
}

impl From<Box<SpaStr>> for Rc<CStr> {
    #[inline(always)]
    #[track_caller]
    fn from(spa_str: Box<SpaStr>) -> Self {
        Rc::<SpaStr>::from(&*spa_str).make_rc_cstr()
    }
}

impl From<&SpaStr> for Arc<CStr> {
    #[inline(always)]
    #[track_caller]
    fn from(spa_str: &SpaStr) -> Self {
        Arc::<SpaStr>::from(spa_str).make_arc_cstr()
    }
}

impl From<&mut SpaStr> for Arc<CStr> {
    #[inline(always)]
    #[track_caller]
    fn from(spa_str: &mut SpaStr) -> Self {
        Arc::<SpaStr>::from(spa_str).make_arc_cstr()
    }
}

impl From<Box<SpaStr>> for Arc<CStr> {
    #[inline(always)]
    #[track_caller]
    fn from(spa_str: Box<SpaStr>) -> Self {
        Arc::<SpaStr>::from(&*spa_str).make_arc_cstr()
    }
}

impl From<Box<SpaStr>> for CString {
    #[inline(always)]
    #[track_caller]
    fn from(spa_str: Box<SpaStr>) -> Self {
        Box::<CStr>::from(spa_str).into()
    }
}

impl From<&SpaStr> for CString {
    #[inline(always)]
    #[track_caller]
    fn from(spa_str: &SpaStr) -> Self {
        Box::<CStr>::from(spa_str).into()
    }
}

impl From<&mut SpaStr> for CString {
    #[inline(always)]
    #[track_caller]
    fn from(spa_str: &mut SpaStr) -> Self {
        Box::<CStr>::from(spa_str).into()
    }
}

impl<'a> TryFrom<&'a SpaStr> for &'a CStr {
    type Error = FromBytesWithNulError;

    #[inline(always)]
    fn try_from(spa_str: &'a SpaStr) -> Result<Self, Self::Error> {
        spa_str
            .as_c_str()
            .ok_or(FromBytesWithNulError::NotNulTerminated)
    }
}

impl<'a> TryFrom<&'a CStr> for &'a SpaStr {
    type Error = PodError;

    #[inline(always)]
    fn try_from(c_str: &'a CStr) -> Result<Self, Self::Error> {
        SpaStr::from_c_str(c_str)
    }
}

impl<'a> TryFrom<&'a mut SpaStr> for &'a mut CStr {
    type Error = FromBytesWithNulError;

    #[inline(always)]
    fn try_from(spa_str: &'a mut SpaStr) -> Result<Self, Self::Error> {
        spa_str
            .as_c_str_mut()
            .ok_or(FromBytesWithNulError::NotNulTerminated)
    }
}

impl<'a> TryFrom<&'a mut CStr> for &'a mut SpaStr {
    type Error = PodError;

    #[inline(always)]
    fn try_from(c_str: &'a mut CStr) -> Result<Self, Self::Error> {
        SpaStr::from_c_str_mut(c_str)
    }
}

impl<'a> TryFrom<&'a mut CStr> for &'a SpaStr {
    type Error = PodError;

    #[inline(always)]
    fn try_from(c_str: &'a mut CStr) -> Result<Self, Self::Error> {
        SpaStr::from_c_str(c_str)
    }
}

impl<'a> TryFrom<&'a mut SpaStr> for &'a CStr {
    type Error = FromBytesWithNulError;

    #[inline(always)]
    fn try_from(spa_str: &'a mut SpaStr) -> Result<Self, Self::Error> {
        spa_str
            .as_c_str()
            .ok_or(FromBytesWithNulError::NotNulTerminated)
    }
}

impl<'a> From<&'a SpaStr> for Cow<'a, CStr> {
    #[inline(always)]
    #[track_caller]
    fn from(spa_str: &'a SpaStr) -> Self {
        match spa_str.as_c_str() {
            Some(c_str) => Cow::Borrowed(c_str),
            None => Cow::Owned(spa_str.into()),
        }
    }
}

impl<'a> From<&'a mut SpaStr> for Cow<'a, CStr> {
    #[inline(always)]
    #[track_caller]
    fn from(spa_str: &'a mut SpaStr) -> Self {
        <&SpaStr>::into(spa_str)
    }
}

impl<'a> From<&'a SpaStr> for Cow<'a, SpaStr> {
    #[inline(always)]
    fn from(spa_str: &'a SpaStr) -> Self {
        Cow::Borrowed(spa_str)
    }
}

impl<'a> From<&'a mut SpaStr> for Cow<'a, SpaStr> {
    #[inline(always)]
    fn from(spa_str: &'a mut SpaStr) -> Self {
        Cow::Borrowed(spa_str)
    }
}

impl From<Box<SpaStr>> for Cow<'_, SpaStr> {
    #[inline(always)]
    fn from(spa_str: Box<SpaStr>) -> Self {
        Cow::Owned(spa_str)
    }
}

const _: () = assert!(
    size_of::<Box<CStr>>() == size_of::<CString>(),
    "they changed the representation in a weird way",
);

impl TryFrom<Box<CStr>> for Box<SpaStr> {
    type Error = WithPodError<Box<CStr>>;

    #[inline(always)]
    fn try_from(c_str: Box<CStr>) -> Result<Self, Self::Error> {
        match SpaStr::from_c_str(&c_str) {
            Ok(spa_str) => Ok({
                // NOTE: We're reconstructing a slice as `CStr` may be thin in the future, but `SpaStr`
                //       never will be, so while we have this information, we ought to use it.
                let len_with_nul = spa_str.len_with_nul();

                let spa_str = ptr::slice_from_raw_parts_mut(
                    Box::into_raw(c_str).cast::<u8>(),
                    len_with_nul.get(),
                ) as *mut SpaStr;

                // SAFETY: We know the body of `spa_str` to be a valid SPA string.
                unsafe { Box::from_raw(spa_str) }
            }),
            Err(pod_error) => Err(WithPodError {
                pod_error,
                value: c_str,
            }),
        }
    }
}

impl<'a> TryFrom<Cow<'a, CStr>> for Box<SpaStr> {
    type Error = WithPodError<Cow<'a, CStr>>;

    #[inline(always)]
    fn try_from(c_str: Cow<'a, CStr>) -> Result<Self, Self::Error> {
        match SpaStr::from_c_str(&c_str) {
            Ok(spa_str) => Ok({
                // NOTE: This is the size of the allocation, and caching it
                //       spares additional `strlen` calls that may get added
                //       in the future.
                let len_with_nul = spa_str.len_with_nul();

                let c_str: Box<CStr> = match c_str {
                    Cow::Borrowed(c_str) => c_str.into(),
                    // NOTE: We're avoiding the `From` impl as it adds code bloat.
                    Cow::Owned(c_str) => {
                        // NOTE: We're kinda assuming that `CString` internally is just a `Box<CStr>`,
                        //       which it pretty much is currently, and I see little to no reason to
                        //       assume that'll ever change. There's other reasons, if this changes,
                        //       then pigs will fly.
                        //
                        // FIXME: Actually determine if this is okay.
                        let c_str = ptr::slice_from_raw_parts_mut(
                            c_str.into_raw().cast::<u8>(),
                            len_with_nul.get(),
                        ) as *mut CStr;

                        // SAFETY: See the above assumption.
                        unsafe { Box::from_raw(c_str) }
                    },
                };

                // NOTE: We're doing this in case a `CStr -> SpaStr` pointer cast ever becomes invalid,
                //       say, in the event `CStr` becomes thin.
                let spa_str = ptr::slice_from_raw_parts_mut(
                    Box::into_raw(c_str).cast::<u8>(),
                    len_with_nul.get(),
                ) as *mut SpaStr;

                // SAFETY: We know the body of the original C string is a valid SPA string.
                unsafe { Box::from_raw(spa_str) }
            }),
            Err(pod_error) => Err(WithPodError {
                pod_error,
                value: c_str,
            }),
        }
    }
}

impl TryFrom<CString> for Box<SpaStr> {
    type Error = WithPodError<CString>;

    #[inline(always)]
    fn try_from(c_str: CString) -> Result<Self, Self::Error> {
        Cow::<CStr>::Owned(c_str).try_into().map_err(
            |WithPodError { pod_error, value }| WithPodError {
                pod_error,
                value: match value {
                    Cow::Owned(c_str) => c_str,
                    Cow::Borrowed(_) => unreachable!(),
                },
            },
        )
    }
}

impl<'a> TryFrom<&'a CStr> for Box<SpaStr> {
    type Error = PodError;

    #[inline(always)]
    fn try_from(c_str: &'a CStr) -> Result<Self, Self::Error> {
        Cow::Borrowed(c_str)
            .try_into()
            .map_err(|WithPodError { pod_error, .. }| pod_error)
    }
}

impl<'a> TryFrom<&'a mut CStr> for Box<SpaStr> {
    type Error = PodError;

    #[inline(always)]
    fn try_from(c_str: &'a mut CStr) -> Result<Self, Self::Error> {
        Cow::Borrowed(c_str)
            .try_into()
            .map_err(|WithPodError { pod_error, .. }| pod_error)
    }
}

impl TryFrom<Rc<CStr>> for Box<SpaStr> {
    type Error = WithPodError<Rc<CStr>>;

    #[inline(always)]
    fn try_from(c_str: Rc<CStr>) -> Result<Self, Self::Error> {
        match SpaStr::from_c_str(&c_str) {
            Ok(spa_str) => Ok(spa_str.to_owned()),
            Err(pod_error) => Err(WithPodError {
                pod_error,
                value: c_str,
            }),
        }
    }
}

impl TryFrom<Arc<CStr>> for Box<SpaStr> {
    type Error = WithPodError<Arc<CStr>>;

    #[inline(always)]
    fn try_from(c_str: Arc<CStr>) -> Result<Self, Self::Error> {
        match SpaStr::from_c_str(&c_str) {
            Ok(spa_str) => Ok(spa_str.to_owned()),
            Err(pod_error) => Err(WithPodError {
                pod_error,
                value: c_str,
            }),
        }
    }
}

/// # Safety
///
/// Do not update `str_body` or `str_body_bytes`.
unsafe trait AsSpaStr {
    fn get_str(&self) -> Result<&SpaStr, &CStr>;

    /// This this will always contain nonzero bytes.
    #[inline(always)]
    fn str_body(&self) -> &[NonZero<u8>] {
        match self.get_str() {
            Ok(spa_str) => spa_str.body(),
            // SAFETY: We know the body of a C string to lack a NUL (which is a zero).
            Err(c_str) => unsafe {
                (&raw const *c_str.to_bytes() as *const [NonZero<u8>])
                    .as_ref_unchecked()
            },
        }
    }

    /// Wrapper around `str_body`.
    #[inline(always)]
    fn str_body_bytes(&self) -> &[u8] {
        // SAFETY: Use your brain.
        unsafe {
            (&raw const *self.str_body() as *const [u8]).as_ref_unchecked()
        }
    }
}

unsafe impl AsSpaStr for SpaStr {
    #[inline(always)]
    fn get_str(&self) -> Result<&SpaStr, &CStr> {
        Ok(self)
    }
}

unsafe impl AsSpaStr for Nul {
    #[inline(always)]
    fn get_str(&self) -> Result<&SpaStr, &CStr> {
        Ok(self.as_ref())
    }
}

unsafe impl AsSpaStr for CStr {
    #[inline(always)]
    fn get_str(&self) -> Result<&SpaStr, &CStr> {
        Err(self)
    }
}

unsafe impl AsSpaStr for CString {
    #[inline(always)]
    fn get_str(&self) -> Result<&SpaStr, &CStr> {
        Err(self.as_c_str())
    }
}

unsafe impl<S> AsSpaStr for &S
where
    S: AsSpaStr + ?Sized,
{
    #[inline(always)]
    fn get_str(&self) -> Result<&SpaStr, &CStr> {
        S::get_str(self)
    }
}

unsafe impl<S> AsSpaStr for &mut S
where
    S: AsSpaStr + ?Sized,
{
    #[inline(always)]
    fn get_str(&self) -> Result<&SpaStr, &CStr> {
        S::get_str(self)
    }
}

unsafe impl<S, A> AsSpaStr for Box<S, A>
where
    A: Allocator,
    S: AsSpaStr + ?Sized,
{
    #[inline(always)]
    fn get_str(&self) -> Result<&SpaStr, &CStr> {
        S::get_str(self)
    }
}

unsafe impl<S> AsSpaStr for Rc<S>
where
    S: AsSpaStr + ?Sized,
{
    #[inline(always)]
    fn get_str(&self) -> Result<&SpaStr, &CStr> {
        S::get_str(self)
    }
}

unsafe impl<S> AsSpaStr for Arc<S>
where
    S: AsSpaStr + ?Sized,
{
    #[inline(always)]
    fn get_str(&self) -> Result<&SpaStr, &CStr> {
        S::get_str(self)
    }
}

unsafe impl<S> AsSpaStr for Cow<'_, S>
where
    S: ?Sized + AsSpaStr + ToOwned,
{
    #[inline(always)]
    fn get_str(&self) -> Result<&SpaStr, &CStr> {
        S::get_str(self.borrow())
    }
}

#[track_caller]
#[inline(always)]
fn collect_spa_str<I>(iter: I) -> Vec<u8>
where
    I: IntoIterator<Item: AsSpaStr>,
{
    let mut iter = iter.into_iter();

    let body = if let Some(first) = iter.next() {
        let mut body = iter.fold(
            {
                let body = first.str_body_bytes().to_vec();

                drop(first);

                body
            },
            |mut acc, s| {
                acc.extend_from_slice(s.str_body_bytes());

                acc
            },
        );
        body.reserve_exact(1);
        body.spare_capacity_mut()[0].write(Nul as u8);

        // SAFETY: We know there's enough capacity to increment by one.
        unsafe {
            let len = body.len();
            body.set_len(len + 1);
        };

        body
    } else {
        (Box::new([Nul as u8; 1]) as Box<[_]>).into()
    };

    assert!(
        body.len() <= super::MAX_SIZE as usize,
        "spa string is too large",
    );

    body
}

impl<S> FromIterator<S> for Box<SpaStr>
where
    S: AsSpaStr,
{
    #[track_caller]
    fn from_iter<I>(iter: I) -> Self
    where
        I: IntoIterator<Item = S>,
    {
        let buf = collect_spa_str(iter).into_boxed_slice();

        // SAFETY: We know that `buf` does not exceed the maximum SPA POD size,
        //         and is a valid SPA string.
        unsafe { Box::from_raw(Box::into_raw(buf) as *mut SpaStr) }
    }
}

impl FromIterator<NonZero<char>> for Box<SpaStr> {
    #[track_caller]
    fn from_iter<I>(iter: I) -> Self
    where
        I: IntoIterator<Item = NonZero<char>>,
    {
        let string = iter
            .into_iter()
            .map(NonZero::get)
            .chain(Some(char::from(Nul)))
            .collect::<Box<str>>();

        // SAFETY: Do not remove this, we need this for soundness.
        assert!(
            string.len() <= super::MAX_SIZE as usize,
            "invalid spa pod size",
        );

        // SAFETY: We know that `string` is a NUL-terminated UTF-8 string that
        //         won't exceed `MAX_SIZE`.
        unsafe { Box::from_raw(Box::into_raw(string) as *mut SpaStr) }
    }
}

impl FromIterator<NonZero<u8>> for Box<SpaStr> {
    #[track_caller]
    fn from_iter<I>(iter: I) -> Self
    where
        I: IntoIterator<Item = NonZero<u8>>,
    {
        let string = iter
            .into_iter()
            .map(NonZero::get)
            .chain(Some(Nul as u8))
            .collect::<Box<[u8]>>();

        // SAFETY: Do not remove this, we need this for soundness.
        assert!(
            string.len() <= super::MAX_SIZE as usize,
            "invalid spa pod size",
        );

        // SAFETY: We know that `string` is a NUL-terminated string that doesn't exceed `MAX_SIZE`.
        unsafe { Box::from_raw(Box::into_raw(string) as *mut SpaStr) }
    }
}

impl FromIterator<NonZero<i8>> for Box<SpaStr> {
    #[track_caller]
    fn from_iter<I>(iter: I) -> Self
    where
        I: IntoIterator<Item = NonZero<i8>>,
    {
        let string = iter
            .into_iter()
            .map(NonZero::get)
            .chain(Some(Nul as i8))
            .collect::<Box<[i8]>>();

        // SAFETY: Do not remove this, we need this for soundness.
        assert!(
            string.len() <= super::MAX_SIZE as usize,
            "invalid spa pod size",
        );

        // SAFETY: We know that `string` is a NUL-terminated string that doesn't exceed `MAX_SIZE`.
        unsafe { Box::from_raw(Box::into_raw(string) as *mut SpaStr) }
    }
}

impl<'a> FromIterator<&'a SpaStr> for Box<CStr> {
    #[track_caller]
    fn from_iter<I>(iter: I) -> Self
    where
        I: IntoIterator<Item = &'a SpaStr>,
    {
        let mut iter = iter.into_iter();

        if let Some(first) = iter.next() {
            let mut body =
                iter.fold(first.as_bytes().to_vec(), |mut acc, spa_str| {
                    acc.extend_from_slice(spa_str.as_bytes());

                    acc
                });

            body.reserve_exact(1);
            body.spare_capacity_mut()[0].write(Nul as u8);

            // SAFETY: We just initialized this byte.
            unsafe {
                let len = body.len();

                body.set_len(len + 1);
            }

            let body = body.into_boxed_slice();

            // SAFETY: We know this to be a valid C string.
            unsafe { Box::from_raw(Box::into_raw(body) as *mut CStr) }
        } else {
            Box::<CStr>::default()
        }
    }
}

impl<'a> FromIterator<&'a mut SpaStr> for Box<CStr> {
    #[track_caller]
    fn from_iter<I>(iter: I) -> Self
    where
        I: IntoIterator<Item = &'a mut SpaStr>,
    {
        iter.into_iter().map(|s| &*s).collect()
    }
}

impl FromIterator<Box<SpaStr>> for Box<CStr> {
    #[track_caller]
    fn from_iter<I>(iter: I) -> Self
    where
        I: IntoIterator<Item = Box<SpaStr>>,
    {
        let mut iter = iter.into_iter();

        if let Some(first) = iter.next() {
            let mut body = iter.fold(
                {
                    let body = first.as_bytes().to_vec();

                    drop(first);

                    body
                },
                |mut acc, spa_str| {
                    acc.extend_from_slice(spa_str.as_bytes());

                    acc
                },
            );

            body.reserve_exact(1);
            body.spare_capacity_mut()[0].write(Nul as u8);

            // SAFETY: We just initialized this byte.
            unsafe {
                let len = body.len();

                body.set_len(len + 1);
            }

            let body = body.into_boxed_slice();

            // SAFETY: We know this to be a valid C string.
            unsafe { Box::from_raw(Box::into_raw(body) as *mut CStr) }
        } else {
            Box::<CStr>::default()
        }
    }
}

impl<'a> FromIterator<&'a SpaStr> for CString {
    #[track_caller]
    fn from_iter<I>(iter: I) -> Self
    where
        I: IntoIterator<Item = &'a SpaStr>,
    {
        iter.into_iter().collect::<Box<CStr>>().into()
    }
}

impl<'a> FromIterator<&'a mut SpaStr> for CString {
    #[track_caller]
    fn from_iter<I>(iter: I) -> Self
    where
        I: IntoIterator<Item = &'a mut SpaStr>,
    {
        iter.into_iter().collect::<Box<CStr>>().into()
    }
}

impl FromIterator<Box<SpaStr>> for CString {
    #[track_caller]
    fn from_iter<I>(iter: I) -> Self
    where
        I: IntoIterator<Item = Box<SpaStr>>,
    {
        iter.into_iter().collect::<Box<CStr>>().into()
    }
}

/// Just a fancy [`u8`] that has a single, valid in-memory representation, `0x00`, or NUL.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Default)]
#[repr(u8)]
pub enum Nul {
    /// A value that is only ever NUL (`0x00`).
    #[default]
    Nul = 0x00,
}

macro_rules! nul_ints {
    ($($int:ident),+) => {
        $(
            impl From<Nul> for $int {
                #[inline(always)]
                fn from(nul: Nul) -> $int {
                    nul as $int
                }
            }

            impl TryFrom<$int> for Nul {
                type Error = TryFromIntError;

                #[inline(always)]
                fn try_from(i: $int) -> Result<Nul, TryFromIntError> {
                    match i.cmp(&(Nul as _)) {
                        Ordering::Less => Err(u32::try_from(-1_i32).unwrap_err()),
                        Ordering::Equal => Ok(Nul),
                        Ordering::Greater => Err(i32::try_from(u32::MAX).unwrap_err()),
                    }
                }
            }

            impl PartialEq<Nul> for $int {
                #[inline(always)]
                fn eq(&self, nul: &Nul) -> bool {
                    *self == *nul as $int
                }
            }

            impl PartialEq<$int> for Nul {
                #[inline(always)]
                fn eq(&self, i: &$int) -> bool {
                    *self as $int == *i
                }
            }

            impl PartialOrd<Nul> for $int {
                #[inline(always)]
                fn partial_cmp(&self, nul: &Nul) -> Option<Ordering> {
                    Some(self.cmp(&(*nul as $int)))
                }
            }

            impl PartialOrd<$int> for Nul {
                #[inline(always)]
                fn partial_cmp(&self, i: &$int) -> Option<Ordering> {
                    Some((*self as $int).cmp(i))
                }
            }

            impl TryFrom<Nul> for NonZero<$int> {
                type Error = TryFromIntError;

                #[inline(always)]
                fn try_from(nul: Nul) -> Result<NonZero<$int>, TryFromIntError> {
                    // NOTE: We know this will always fail.
                    (nul as $int).try_into()
                }
            }

            impl TryFrom<NonZero<$int>> for Nul {
                type Error = TryFromIntError;

                #[inline(always)]
                fn try_from(i: NonZero<$int>) -> Result<Nul, TryFromIntError> {
                    // NOTE: We know this will always fail.
                    i.get().try_into()
                }
            }


            impl PartialEq<Nul> for NonZero<$int> {
                #[inline(always)]
                fn eq(&self, nul: &Nul) -> bool {
                    self.get() == *nul
                }
            }

            impl PartialEq<NonZero<$int>> for Nul {
                #[inline(always)]
                fn eq(&self, i: &NonZero<$int>) -> bool {
                    *self == i.get()
                }
            }

            impl PartialOrd<Nul> for NonZero<$int> {
                #[inline(always)]
                fn partial_cmp(&self, nul: &Nul) -> Option<Ordering> {
                    self.get().partial_cmp(nul)
                }
            }

            impl PartialOrd<NonZero<$int>> for Nul {
                #[inline(always)]
                fn partial_cmp(&self, i: &NonZero<$int>) -> Option<Ordering> {
                    self.partial_cmp(&i.get())
                }
            }
        )+
    };
}

nul_ints!(u8, u16, u32, u64, u128, usize);
nul_ints!(i8, i16, i32, i64, i128, isize);

impl From<Nul> for char {
    #[inline(always)]
    fn from(nul: Nul) -> Self {
        nul as u8 as char
    }
}

impl hash::Hash for Nul {
    #[inline(always)]
    fn hash<H>(
        &self,
        state: &mut H,
    ) where
        H: hash::Hasher,
    {
        (*self as u8).hash(state);
    }

    #[inline(always)]
    fn hash_slice<H>(
        data: &[Self],
        state: &mut H,
    ) where
        H: hash::Hasher,
    {
        u8::hash_slice(Byte::as_u8_slice(as_bytes(data)), state);
    }
}

impl fmt::Debug for Nul {
    #[inline(always)]
    #[track_caller]
    fn fmt(
        &self,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        (*self as u8).fmt(f)
    }
}

impl fmt::Display for Nul {
    #[inline(always)]
    #[track_caller]
    fn fmt(
        &self,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        (*self as u8).fmt(f)
    }
}

impl fmt::Binary for Nul {
    #[inline(always)]
    #[track_caller]
    fn fmt(
        &self,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        (*self as u8).fmt(f)
    }
}

impl fmt::LowerHex for Nul {
    #[inline(always)]
    #[track_caller]
    fn fmt(
        &self,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        (*self as u8).fmt(f)
    }
}

impl fmt::UpperHex for Nul {
    #[inline(always)]
    #[track_caller]
    fn fmt(
        &self,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        (*self as u8).fmt(f)
    }
}

impl fmt::LowerExp for Nul {
    #[inline(always)]
    #[track_caller]
    fn fmt(
        &self,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        (*self as u8).fmt(f)
    }
}

impl fmt::UpperExp for Nul {
    #[inline(always)]
    #[track_caller]
    fn fmt(
        &self,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        (*self as u8).fmt(f)
    }
}

impl fmt::Octal for Nul {
    #[inline(always)]
    #[track_caller]
    fn fmt(
        &self,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        (*self as u8).fmt(f)
    }
}

impl AsRef<CStr> for Nul {
    #[inline(always)]
    fn as_ref(&self) -> &CStr {
        // SAFETY: `Nul` is just an empty C string.
        let c_str = unsafe {
            (&raw const *slice::from_ref(self) as *const CStr)
                .as_ref_unchecked()
        };

        // SAFETY: We know that the length is zero.
        unsafe { hint::assert_unchecked(c_str.count_bytes() == 0) };

        c_str
    }
}

impl AsRef<SpaStr> for Nul {
    #[inline(always)]
    fn as_ref(&self) -> &SpaStr {
        // SAFETY: `Nul` is just an empty C string.
        let spa_str = unsafe {
            (&raw const *slice::from_ref(self) as *mut SpaStr)
                .as_ref_unchecked()
        };

        // SAFETY: We know this is an empty C string.
        unsafe {
            hint::assert_unchecked(spa_str.has_nul() && spa_str.is_empty())
        };

        spa_str
    }
}

impl AsMut<CStr> for Nul {
    #[inline(always)]
    fn as_mut(&mut self) -> &mut CStr {
        // SAFETY: `Nul` is just an empty C string.
        let c_str = unsafe {
            (&raw mut *slice::from_mut(self) as *mut CStr).as_mut_unchecked()
        };

        // SAFETY: We know that the length is zero.
        unsafe { hint::assert_unchecked(c_str.count_bytes() == 0) };

        c_str
    }
}

impl AsMut<SpaStr> for Nul {
    #[inline(always)]
    fn as_mut(&mut self) -> &mut SpaStr {
        // SAFETY: `Nul` is just an empty C string.
        let spa_str = unsafe {
            (&raw mut *slice::from_mut(self) as *mut SpaStr).as_mut_unchecked()
        };

        // SAFETY: We know this is an empty C string.
        unsafe {
            hint::assert_unchecked(spa_str.has_nul() && spa_str.is_empty())
        };

        spa_str
    }
}

impl Borrow<CStr> for Nul {
    #[inline(always)]
    fn borrow(&self) -> &CStr {
        self.as_ref()
    }
}

impl Borrow<SpaStr> for Nul {
    #[inline(always)]
    fn borrow(&self) -> &SpaStr {
        self.as_ref()
    }
}

impl BorrowMut<CStr> for Nul {
    #[inline(always)]
    fn borrow_mut(&mut self) -> &mut CStr {
        self.as_mut()
    }
}

impl BorrowMut<SpaStr> for Nul {
    #[inline(always)]
    fn borrow_mut(&mut self) -> &mut SpaStr {
        self.as_mut()
    }
}

impl ops::Add for Nul {
    type Output = Nul;

    #[inline(always)]
    fn add(
        self,
        _: Self,
    ) -> Self::Output {
        Nul
    }
}

impl ops::AddAssign for Nul {
    #[inline(always)]
    fn add_assign(
        &mut self,
        _: Self,
    ) {
    }
}

impl ops::Sub for Nul {
    type Output = Nul;

    #[inline(always)]
    fn sub(
        self,
        _: Self,
    ) -> Self::Output {
        Nul
    }
}

impl ops::SubAssign for Nul {
    #[inline(always)]
    fn sub_assign(
        &mut self,
        _: Self,
    ) {
    }
}

impl ops::Mul for Nul {
    type Output = Nul;

    #[inline(always)]
    fn mul(
        self,
        _: Self,
    ) -> Self::Output {
        Nul
    }
}

impl ops::MulAssign for Nul {
    #[inline(always)]
    fn mul_assign(
        &mut self,
        _: Self,
    ) {
    }
}

impl ops::Neg for Nul {
    type Output = Nul;

    #[inline(always)]
    fn neg(self) -> Self::Output {
        Nul
    }
}

impl ops::BitAnd for Nul {
    type Output = Nul;

    #[inline(always)]
    fn bitand(
        self,
        _: Self,
    ) -> Self::Output {
        Nul
    }
}

impl ops::BitAndAssign for Nul {
    #[inline(always)]
    fn bitand_assign(
        &mut self,
        _: Self,
    ) {
    }
}

impl ops::BitOr for Nul {
    type Output = Nul;

    #[inline(always)]
    fn bitor(
        self,
        _: Self,
    ) -> Self::Output {
        Nul
    }
}

impl BitOrAssign for Nul {
    #[inline(always)]
    fn bitor_assign(
        &mut self,
        _: Self,
    ) {
    }
}

impl ops::BitXor for Nul {
    type Output = Nul;

    #[inline(always)]
    fn bitxor(
        self,
        _: Self,
    ) -> Self::Output {
        Nul
    }
}

impl ops::BitXorAssign for Nul {
    #[inline(always)]
    fn bitxor_assign(
        &mut self,
        _: Self,
    ) {
    }
}

impl iter::Product for Nul {
    #[inline(always)]
    #[track_caller]
    fn product<I>(iter: I) -> Self
    where
        I: Iterator<Item = Self>,
    {
        iter.fold(Nul, |a, b| a * b)
    }
}

impl<'a> iter::Product<&'a Nul> for Nul {
    #[inline(always)]
    #[track_caller]
    fn product<I>(iter: I) -> Self
    where
        I: Iterator<Item = &'a Nul>,
    {
        iter.copied().product()
    }
}

impl iter::Sum for Nul {
    #[inline(always)]
    #[track_caller]
    fn sum<I>(iter: I) -> Self
    where
        I: Iterator<Item = Self>,
    {
        iter.fold(Nul, |a, b| a + b)
    }
}

impl<'a> iter::Sum<&'a Nul> for Nul {
    #[inline(always)]
    #[track_caller]
    fn sum<I>(iter: I) -> Self
    where
        I: Iterator<Item = &'a Nul>,
    {
        iter.copied().sum()
    }
}

// SAFETY: This type is just a `u8` that is zero, it's always safe to read it as bytes.
unsafe impl AsBytes for Nul {}

// Just a little namespace hack.
mod nul_namespace {
    #[doc(hidden)]
    pub use super::Nul::Nul;
}

#[doc(hidden)]
pub use nul_namespace::*;

/// An enum representing how strict our NUL terminator search should be.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
#[non_exhaustive]
pub enum NulSetting {
    /// This requires there be no NUL terminator.
    Absent,

    /// This indicates that a NUL terminator is optional, but requires
    /// it to be at the end, if it exists.
    ///
    /// This disallows interior NULs. This is the default.
    #[default]
    Optional,

    /// This indicates that a NUL terminator is optional, not caring where.
    ///
    /// This permits interior NULs.
    OptionalRelaxed,

    /// This requires there be a NUL terminator, but requires it be at the
    /// end.
    ///
    /// This disallows interior NULs.
    Required,

    /// This requires there be a NUL terminator, not caring where.
    ///
    /// This permits interior NULs.
    RequiredRelaxed,
}

impl NulSetting {
    /// Returns whether or not this setting permits interior NULs.
    #[inline(always)]
    #[must_use]
    pub const fn permits_interior_nul(self) -> bool {
        matches!(
            self,
            NulSetting::OptionalRelaxed | NulSetting::RequiredRelaxed
        )
    }

    /// Returns whether or not this setting permits NULs.
    #[inline(always)]
    #[must_use]
    pub const fn permits_nul(self) -> bool {
        matches!(
            self,
            NulSetting::Optional
                | NulSetting::OptionalRelaxed
                | NulSetting::Required
                | NulSetting::RequiredRelaxed
        )
    }

    /// Returns whether or not this setting permits the lack of NULs.
    #[inline(always)]
    #[must_use]
    pub const fn permits_no_nul(self) -> bool {
        matches!(
            self,
            NulSetting::Absent
                | NulSetting::Optional
                | NulSetting::OptionalRelaxed
        )
    }

    /// Validates a [`NulPos`] for this setting.
    #[inline(always)]
    pub const fn validate_pos(
        self,
        nul_pos: &NulPos,
    ) -> Result<(), PodError> {
        match (self, nul_pos) {
            // If we require NULs to be absent, yet there is one, we consider its existence
            // as interior. It may make more sense to create a separate error for it later.
            (
                NulSetting::Absent,
                &NulPos::End { pos } | &NulPos::Interior { pos },
            ) => Err(PodError::InteriorNul { pos }),

            // If there's an interior NUL on non-relaxed settings, that's a problem.
            (
                NulSetting::Optional | NulSetting::Required,
                &NulPos::Interior { pos },
            ) => Err(PodError::InteriorNul { pos }),

            // If we require a NUL, and non was found, that's a problem.
            (
                NulSetting::Required | NulSetting::RequiredRelaxed,
                NulPos::None { .. },
            ) => Err(PodError::NotNulTerminated),

            // If there's no NUL, and we consider it optional or require its nonexistence,
            // we're okay.
            (
                NulSetting::Absent
                | NulSetting::Optional
                | NulSetting::OptionalRelaxed,
                NulPos::None { .. },
            ) => Ok(()),

            // If there's a NUL at the end of the buffer, and we consider it optional, or
            // require it at the end, we're okay.
            (
                NulSetting::Optional
                | NulSetting::OptionalRelaxed
                | NulSetting::Required
                | NulSetting::RequiredRelaxed,
                NulPos::End { .. },
            ) => Ok(()),

            // If there's an interior NUL, and we're using a relaxed setting, we're okay.
            (
                NulSetting::OptionalRelaxed | NulSetting::RequiredRelaxed,
                NulPos::Interior { .. },
            ) => Ok(()),
        }
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
            &NulPos::End { pos } | &NulPos::Interior { pos } => Some(pos),
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
            Some(with_nul) => NonZero::new(with_nul),
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
            &NulPos::None {
                len: len_without_nul,
            }
            | &NulPos::End {
                pos: len_without_nul,
            }
            | &NulPos::Interior {
                pos: len_without_nul,
            } => len_without_nul,
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
        let nul_pos = NulPos::search_impl(bytes);
        let full_len = match nul_pos {
            NulPos::None { len } => len,
            // SAFETY: We know that `pos < bytes.len()`, as this is the position of a NUL, which must
            //         exist within it.
            NulPos::End { pos } | NulPos::Interior { pos } => unsafe {
                pos.unchecked_add(1)
            },
        };

        // SAFETY: We know that the derived positions from above are within the bounds of `bytes`.
        unsafe { hint::assert_unchecked(full_len <= bytes.len()) };

        nul_pos
    }

    #[inline(always)]
    #[must_use]
    #[track_caller]
    const fn search_impl(bytes: &[Byte]) -> NulPos {
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
                                == Nul as u8,
                        )
                    };

                    len_with_nul
                }),
                Err(_) => None,
            };

        match len_with_nul {
            Some(len_with_nul) => {
                // SAFETY: We know that if there's a NUL terminator, that the length of our buffer is nonzero.
                let buffer_len = NonZero::new(bytes.len()).expect(
                    "undefined behavior: the buffer length should be nonzero",
                );

                debug_assert!(
                    len_with_nul.get() <= buffer_len.get(),
                    "undefined behavior: the length including a nul must be within the bounds of the original length",
                );

                let pos = len_with_nul.get().strict_sub(1);

                debug_assert!(
                    bytes[pos].get() == Nul as u8,
                    "undefined behavior: the last byte in the buffer with the nul, must be nul",
                );

                match (
                    len_with_nul.get() < buffer_len.get(),
                    len_with_nul.get() > buffer_len.get(),
                ) {
                    (false, false) => {
                        debug_assert!(
                            len_with_nul.get() == buffer_len.get(),
                            "undefined behavior: the length with nul in this case must be equal to the original buffer length",
                        );

                        NulPos::End { pos }
                    },
                    (true, false) => {
                        debug_assert!(
                            len_with_nul.get() < buffer_len.get(),
                            "undefined behavior: the length with nul in this case must be less than the original buffer length",
                        );

                        NulPos::Interior { pos }
                    },
                    (_, true) => unreachable!(),
                }
            },
            None => NulPos::None { len: bytes.len() },
        }
    }
}
