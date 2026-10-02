use std::{
    array::TryFromSliceError,
    borrow::{Borrow, BorrowMut, Cow},
    cmp::Ordering,
    collections::VecDeque,
    ffi::{CStr, CString},
    fmt, hash, hint, iter,
    mem::{self, MaybeUninit},
    num::{NonZero, TryFromIntError},
    ops::{self, BitOrAssign, Deref},
    ptr::NonNull,
    rc::Rc,
    slice,
    sync::Arc,
};

use crate::{
    mem::{AsBytes, AsBytesMut, Byte, as_bytes, as_bytes_mut},
    util::is_char_ish,
};

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
pub struct SpaStr {
    whole: [u8],
}

impl SpaStr {
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

        if let Some(Nul) = nul {
            // SAFETY: We know that if there's a NUL terminator, that incrementing the body
            //         size is equivalent to the size of the whole buffer.
            unsafe {
                hint::assert_unchecked(
                    body.len().unchecked_add(1) == self.whole.len(),
                )
            };
        } else {
            // SAFETY: If there's no NUL, then we know they're equivalent in size.
            unsafe { hint::assert_unchecked(body.len() == self.whole.len()) };
        }

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

        if let Some(Nul) = nul {
            // SAFETY: We know that if there's a NUL terminator, that incrementing the body
            //         size is equivalent to the size of the whole buffer.
            unsafe {
                hint::assert_unchecked(body.len().unchecked_add(1) == whole_len)
            };
        } else {
            // SAFETY: If there's no NUL, then we know they're equivalent in size.
            unsafe { hint::assert_unchecked(body.len() == whole_len) };
        }

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
            Some(unsafe { (&raw mut *self as *mut CStr).as_mut_unchecked() })
        } else {
            None
        }
    }

    /// Allocate a box on the heap of this SPA string, including a NUL terminator.
    #[inline(never)]
    #[must_use]
    #[track_caller]
    fn make_box(&self) -> Box<[u8]> {
        let bytes = self.as_bytes();
        let mut alloc = Box::new_uninit_slice(bytes.len().strict_add(1));

        alloc[..bytes.len()].write_copy_of_slice(bytes);
        alloc[bytes.len()].write(Nul as u8);

        // SAFETY: We've successfully initialized the buffer.
        unsafe { alloc.assume_init() }
    }

    /// Allocate a refcounted heap value of this SPA string, including a NUL terminator.
    #[inline(never)]
    #[must_use]
    #[track_caller]
    fn make_rc(&self) -> Rc<[u8]> {
        let bytes = self.as_bytes();
        let mut alloc = Rc::new_uninit_slice(bytes.len().strict_add(1));

        // SAFETY: We know the allocation is unique.
        let buffer = unsafe { Rc::get_mut(&mut alloc).unwrap_unchecked() };

        buffer[..bytes.len()].write_copy_of_slice(bytes);
        buffer[bytes.len()].write(Nul as u8);

        // SAFETY: We know the buffer is initialized.
        unsafe { alloc.assume_init() }
    }

    /// Allocate an atomic refcounted heap value of this SPA string, including a NUL terminator.
    #[inline(never)]
    #[must_use]
    #[track_caller]
    fn make_arc(&self) -> Arc<[u8]> {
        let bytes = self.as_bytes();
        let alloc = Arc::new_uninit_slice(bytes.len().strict_add(1));

        let alloc = {
            // NOTE: I would be really surprised if the provenance for this was fucked up.
            let alloc = Arc::into_raw(alloc).cast_mut();

            // SAFETY: We know the allocation is unique.
            let buffer = unsafe { alloc.as_mut_unchecked() };

            buffer[..bytes.len()].write_copy_of_slice(bytes);
            buffer[bytes.len()].write(Nul as u8);

            // SAFETY: We know `alloc` is a valid `Arc`.
            unsafe { Arc::from_raw(alloc) }
        };

        // SAFETY: We know the buffer is initialized.
        unsafe { alloc.assume_init() }
    }

    /// Allocate some heap allocated thing with a NUL terminator, and add additional hints.
    #[inline(always)]
    #[must_use]
    #[track_caller]
    unsafe fn make_heap<'a, H, A, As, AsI>(
        &'a self,
        alloc: A,
        additional_assertions: As,
    ) -> H
    where
        H: Deref<Target = [u8]>,
        A: FnOnce(&'a SpaStr) -> H,
        As: FnOnce(&mut H) -> AsI,
        AsI: IntoIterator<Item = bool>,
    {
        let mut heap_alloc = alloc(self);

        // SAFETY: We know that they have the same length when including
        //         a NUL.
        unsafe {
            hint::assert_unchecked(
                heap_alloc.len() == self.len_with_nul().get(),
            )
        };

        // SAFETY: We know the heap buffer is not empty.
        unsafe { hint::assert_unchecked(!heap_alloc.is_empty()) };

        // SAFETY: We know the last byte is NUL.
        unsafe {
            hint::assert_unchecked(
                heap_alloc.last().copied().unwrap() == Nul as u8,
            )
        };

        // SAFETY: We know that the `strlen + 1` is equal to the
        //         size of the buffer.
        unsafe {
            hint::assert_unchecked(
                CStr::from_ptr(heap_alloc.as_ptr().cast())
                    .count_bytes()
                    .unchecked_add(1)
                    == heap_alloc.len(),
            )
        };

        // SAFETY: The caller ensures this is fine.
        additional_assertions(&mut heap_alloc)
            .into_iter()
            .for_each(|cond| unsafe { hint::assert_unchecked(cond) });

        heap_alloc
    }
}

unsafe impl AsBytes for SpaStr {}

impl<const N: usize> TryFrom<&SpaStr> for [u8; N] {
    type Error = TryFromSliceError;

    #[inline(always)]
    #[track_caller]
    fn try_from(value: &SpaStr) -> Result<Self, Self::Error> {
        let bytes = value.as_bytes();

        if N == bytes.len().strict_add(1) {
            let mut output = [MaybeUninit::uninit(); N];

            output[..bytes.len()].write_copy_of_slice(bytes);
            output[bytes.len()].write(Nul as u8);

            // SAFETY: We initialized the entire array.
            Ok(unsafe { MaybeUninit::from(output).assume_init() })
        } else {
            // SAFETY: We're conjuring a ZST slice, this is always fine.
            let slice = unsafe {
                NonNull::slice_from_raw_parts(
                    NonNull::<()>::dangling(),
                    bytes.len().strict_add(1),
                )
                .as_ref()
            };
            // NOTE: We're essentially reconstructing the error that we would expect.
            let array: Result<[(); N], _> = slice.try_into();

            // NOTE: This should never panic.
            Err(array.unwrap_err())
        }
    }
}

impl<const N: usize> TryFrom<&SpaStr> for [i8; N] {
    type Error = TryFromSliceError;

    #[inline(always)]
    #[track_caller]
    #[allow(clippy::toplevel_ref_arg)]
    fn try_from(value: &SpaStr) -> Result<Self, Self::Error> {
        let ref array: [u8; N] = value.try_into()?;

        // SAFETY: `i8` and `u8` are POD.
        Ok(unsafe { mem::transmute_copy(array) })
    }
}

impl<const N: usize> TryFrom<&SpaStr> for [Byte; N] {
    type Error = TryFromSliceError;

    #[inline(always)]
    #[track_caller]
    #[allow(clippy::toplevel_ref_arg)]
    fn try_from(value: &SpaStr) -> Result<Self, Self::Error> {
        let ref array: [u8; N] = value.try_into()?;

        // SAFETY: `i8` and `u8` are POD.
        Ok(unsafe { mem::transmute_copy(array) })
    }
}

#[allow(clippy::nonnull_unchecked_on_box_ptr, clippy::missing_safety_doc)]
trait HeapFamily {
    type New<Target: ?Sized>: Sized + Deref<Target = Target>;

    #[must_use]
    #[track_caller]
    fn new<T>(val: T) -> Self::New<T>;

    #[must_use]
    #[track_caller]
    fn into_raw<T>(ptr: Self::New<T>) -> NonNull<T>
    where
        T: ?Sized;

    #[must_use]
    #[track_caller]
    unsafe fn from_raw<T>(ptr: NonNull<T>) -> Self::New<T>
    where
        T: ?Sized;
}

struct BoxHeap;

#[allow(clippy::nonnull_unchecked_on_box_ptr, clippy::missing_safety_doc)]
impl HeapFamily for BoxHeap {
    type New<Target: ?Sized> = Box<Target>;

    #[inline(always)]
    fn new<T>(val: T) -> Self::New<T> {
        Box::new(val)
    }
    #[inline(always)]
    fn into_raw<T>(ptr: Self::New<T>) -> NonNull<T>
    where
        T: ?Sized,
    {
        unsafe { NonNull::new_unchecked(Box::into_raw(ptr)) }
    }

    #[inline(always)]
    unsafe fn from_raw<T>(ptr: NonNull<T>) -> Self::New<T>
    where
        T: ?Sized,
    {
        unsafe { Box::from_raw(ptr.as_ptr()) }
    }
}

struct RcHeap;

#[allow(clippy::nonnull_unchecked_on_box_ptr, clippy::missing_safety_doc)]
impl HeapFamily for RcHeap {
    type New<Target: ?Sized> = Rc<Target>;

    #[inline(always)]
    fn new<T>(val: T) -> Self::New<T> {
        Rc::new(val)
    }
    #[inline(always)]
    fn into_raw<T>(ptr: Self::New<T>) -> NonNull<T>
    where
        T: ?Sized,
    {
        unsafe { NonNull::new_unchecked(Rc::into_raw(ptr).cast_mut()) }
    }

    #[inline(always)]
    unsafe fn from_raw<T>(ptr: NonNull<T>) -> Self::New<T>
    where
        T: ?Sized,
    {
        unsafe { Rc::from_raw(ptr.as_ptr()) }
    }
}

struct ArcHeap;

#[allow(clippy::nonnull_unchecked_on_box_ptr, clippy::missing_safety_doc)]
impl HeapFamily for ArcHeap {
    type New<Target: ?Sized> = Arc<Target>;

    #[inline(always)]
    fn new<T>(val: T) -> Self::New<T> {
        Arc::new(val)
    }
    #[inline(always)]
    fn into_raw<T>(ptr: Self::New<T>) -> NonNull<T>
    where
        T: ?Sized,
    {
        unsafe { NonNull::new_unchecked(Arc::into_raw(ptr).cast_mut()) }
    }

    #[inline(always)]
    unsafe fn from_raw<T>(ptr: NonNull<T>) -> Self::New<T>
    where
        T: ?Sized,
    {
        unsafe { Arc::from_raw(ptr.as_ptr()) }
    }
}

#[inline(always)]
#[track_caller]
#[allow(clippy::type_complexity)]
fn into_heap_array<'a, const N: usize, T, H>(
    spa_str: &'a SpaStr
) -> Result<H::New<[T; N]>, <&'a SpaStr as TryInto<[T; N]>>::Error>
where
    H: HeapFamily,
    &'a SpaStr: TryInto<[T; N]> + Into<H::New<[T]>>,
{
    if is_char_ish::<T>() {
        spa_str.try_into().map(|_: [T; N]| ())?;

        let alloc: H::New<[T]> = spa_str.into();

        // SAFETY: We trust `T`, thus we know the allocation to have a length of `N`.
        unsafe { hint::assert_unchecked(alloc.len() == N) };

        // SAFETY: We know it's in bounds.
        Ok(unsafe { H::from_raw(H::into_raw(alloc).cast()) })
    } else {
        spa_str.try_into().map(H::new)
    }
}

#[unsafe(no_mangle)]
pub fn hello(spa_str: &SpaStr) -> Box<[u8; 256]> {
    spa_str.try_into().unwrap()
}

impl<'a, const N: usize, T> TryFrom<&'a SpaStr> for Rc<[T; N]>
where
    &'a SpaStr: TryInto<[T; N]> + Into<Rc<[T]>>,
{
    type Error = <&'a SpaStr as TryInto<[T; N]>>::Error;

    #[inline(always)]
    #[track_caller]
    fn try_from(value: &'a SpaStr) -> Result<Self, Self::Error> {
        into_heap_array::<N, T, RcHeap>(value)
    }
}

impl<'a, const N: usize, T> TryFrom<&'a SpaStr> for Arc<[T; N]>
where
    &'a SpaStr: TryInto<[T; N]> + Into<Arc<[T]>>,
{
    type Error = <&'a SpaStr as TryInto<[T; N]>>::Error;

    #[inline(always)]
    #[track_caller]
    fn try_from(value: &'a SpaStr) -> Result<Self, Self::Error> {
        into_heap_array::<N, T, ArcHeap>(value)
    }
}

impl<'a, const N: usize, T> TryFrom<&'a SpaStr> for Box<[T; N]>
where
    &'a SpaStr: TryInto<[T; N]> + Into<Box<[T]>>,
{
    type Error = <&'a SpaStr as TryInto<[T; N]>>::Error;

    #[inline(always)]
    #[track_caller]
    fn try_from(value: &'a SpaStr) -> Result<Self, Self::Error> {
        into_heap_array::<N, T, BoxHeap>(value)
    }
}

impl From<&SpaStr> for Box<[u8]> {
    #[inline(always)]
    #[track_caller]
    fn from(spa_str: &SpaStr) -> Self {
        // SAFETY: We know this is valid.
        unsafe { spa_str.make_heap(SpaStr::make_box, |_| []) }
    }
}

impl From<&SpaStr> for Rc<[u8]> {
    #[inline(always)]
    #[track_caller]
    fn from(spa_str: &SpaStr) -> Self {
        // SAFETY: We know this is valid.
        unsafe {
            spa_str.make_heap(SpaStr::make_rc, |rc| {
                [Rc::weak_count(rc) == 0, Rc::strong_count(rc) == 1]
            })
        }
    }
}

impl From<&SpaStr> for Arc<[u8]> {
    #[inline(always)]
    #[track_caller]
    fn from(spa_str: &SpaStr) -> Self {
        // SAFETY: We know this is valid.
        unsafe { spa_str.make_heap(SpaStr::make_arc, |_arc| []) }
    }
}

impl From<&SpaStr> for Box<[i8]> {
    #[inline(always)]
    #[track_caller]
    fn from(value: &SpaStr) -> Self {
        let bytes: Box<[u8]> = value.into();

        // SAFETY: `i8` and `u8` are POD.
        unsafe { Box::from_raw(Box::into_raw(bytes) as *mut [i8]) }
    }
}

impl From<&SpaStr> for Arc<[i8]> {
    #[inline(always)]
    fn from(value: &SpaStr) -> Self {
        let bytes: Arc<[u8]> = value.into();

        // SAFETY: `i8` and `u8` are POD.
        unsafe { Arc::from_raw(Arc::into_raw(bytes) as *const [i8]) }
    }
}

impl From<&SpaStr> for Rc<[i8]> {
    #[inline(always)]
    fn from(value: &SpaStr) -> Self {
        let bytes: Rc<[u8]> = value.into();

        // SAFETY: `i8` and `u8` are POD.
        unsafe { Rc::from_raw(Rc::into_raw(bytes) as *const [i8]) }
    }
}

impl From<&SpaStr> for Box<[Byte]> {
    #[inline(always)]
    #[track_caller]
    fn from(value: &SpaStr) -> Self {
        let bytes: Box<[u8]> = value.into();

        // SAFETY: `Byte` and `u8` are POD.
        unsafe { Box::from_raw(Box::into_raw(bytes) as *mut [Byte]) }
    }
}

impl From<&SpaStr> for Arc<[Byte]> {
    #[inline(always)]
    fn from(value: &SpaStr) -> Self {
        let bytes: Arc<[u8]> = value.into();

        // SAFETY: `Byte` and `u8` are POD.
        unsafe { Arc::from_raw(Arc::into_raw(bytes) as *const [Byte]) }
    }
}

impl From<&SpaStr> for Rc<[Byte]> {
    #[inline(always)]
    fn from(value: &SpaStr) -> Self {
        let bytes: Rc<[u8]> = value.into();

        // SAFETY: `Byte` and `u8` are POD.
        unsafe { Rc::from_raw(Rc::into_raw(bytes) as *const [Byte]) }
    }
}

impl<'a, T> From<&'a SpaStr> for Vec<T>
where
    &'a SpaStr: Into<Box<[T]>>,
{
    #[inline(always)]
    #[track_caller]
    fn from(value: &'a SpaStr) -> Self {
        Vec::from(value.into())
    }
}

impl<'a, T> From<&'a SpaStr> for VecDeque<T>
where
    &'a SpaStr: Into<Vec<T>>,
{
    #[inline(always)]
    #[track_caller]
    fn from(value: &'a SpaStr) -> Self {
        VecDeque::from(value.into())
    }
}

impl From<&SpaStr> for Box<CStr> {
    #[inline(always)]
    #[track_caller]
    fn from(value: &SpaStr) -> Self {
        let bytes: Box<[u8]> = value.into();

        // SAFETY: We know that the above allocation is a valid C string.
        unsafe { Box::from_raw(Box::into_raw(bytes) as *mut CStr) }
    }
}

impl From<&SpaStr> for Rc<CStr> {
    #[inline(always)]
    #[track_caller]
    fn from(value: &SpaStr) -> Self {
        let bytes: Rc<[u8]> = value.into();

        // SAFETY: We know that the above allocation is a valid C string.
        unsafe { Rc::from_raw(Rc::into_raw(bytes) as *const CStr) }
    }
}

impl From<&SpaStr> for Arc<CStr> {
    #[inline(always)]
    #[track_caller]
    fn from(value: &SpaStr) -> Self {
        let bytes: Arc<[u8]> = value.into();

        // SAFETY: We know that the above allocation is a valid C string.
        unsafe { Arc::from_raw(Arc::into_raw(bytes) as *const CStr) }
    }
}

impl From<&SpaStr> for CString {
    #[inline(always)]
    #[track_caller]
    fn from(value: &SpaStr) -> Self {
        let c_str: Box<CStr> = value.into();

        c_str.into()
    }
}

impl<'a> From<&'a SpaStr> for Cow<'a, CStr> {
    #[inline(always)]
    #[track_caller]
    fn from(value: &'a SpaStr) -> Self {
        match value.as_c_str() {
            Some(c_str) => Cow::Borrowed(c_str),
            None => Cow::Owned(value.into()),
        }
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
                    match i.cmp(&0x00) {
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
        unsafe {
            (&raw const *slice::from_ref(self) as *const CStr)
                .as_ref_unchecked()
        }
    }
}

impl AsMut<CStr> for Nul {
    #[inline(always)]
    fn as_mut(&mut self) -> &mut CStr {
        // SAFETY: `Nul` is just an empty C string.
        unsafe {
            (&raw mut *slice::from_mut(self) as *mut CStr).as_mut_unchecked()
        }
    }
}

impl Borrow<CStr> for Nul {
    #[inline(always)]
    fn borrow(&self) -> &CStr {
        self.as_ref()
    }
}

impl BorrowMut<CStr> for Nul {
    #[inline(always)]
    fn borrow_mut(&mut self) -> &mut CStr {
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

// impl iter::Product for Nul {
//     #[inline(always)]
//     fn product<I>(iter: I) -> Self where I: Iterator<Item = Self> {

//     }
// }

// SAFETY: This type is just a `u8` that is zero, it's always safe to read it as bytes.
unsafe impl AsBytes for Nul {}

// Just a little namespace hack.
mod nul_namespace {
    #[doc(hidden)]
    pub use super::Nul::Nul;
}

#[doc(hidden)]
pub use nul_namespace::*;

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
