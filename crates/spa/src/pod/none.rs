// use std::{hint, mem};

use std::{convert::Infallible, ffi::c_void, fmt, hash, marker::PhantomData, mem};

use crate::{
    mem::{AsBytes, AsBytesMut, Byte},
    pod::{BasicPod, kind::SpaKind, sealed},
};

/// A SPA POD `None` value, which indicates the absence of something.
#[repr(C, packed)]
pub struct SpaNone<T = c_void>
where
    T: ?Sized,
{
    /// This is semantically a `Option<T>` that is always `None`.
    ///
    /// This exists to indicate that it is *never* inhabited.
    value: Option<Infallible>,

    /// We want to be covariant over `T`.
    _marker: PhantomData<T>,
}

impl<T> SpaNone<T>
where
    T: ?Sized,
{
    /// Create a [`SpaNone`].
    #[inline(always)]
    #[must_use]
    pub const fn new() -> SpaNone<T> {
        SpaNone {
            value: None,
            _marker: PhantomData,
        }
    }

    /// Create a new [`SpaNone`] from an [`Option`].
    #[inline(always)]
    #[allow(clippy::unnecessary_unwrap)]
    pub const fn from_option(option: Option<T>) -> Result<SpaNone<T>, T>
    where
        T: Sized,
    {
        if option.is_some() {
            Err(option.unwrap())
        } else {
            // NOTE: It's `None`, there's no destructor to run.
            mem::forget(option);

            Ok(SpaNone::new())
        }
    }

    /// Consume this value and turn it into an [`Option`].
    ///
    /// # Returns
    ///
    /// This always returns [`None`].
    #[inline(always)]
    #[must_use]
    #[allow(unreachable_code)]
    pub const fn get(self) -> Option<T>
    where
        T: Sized,
    {
        match self.value {
            Some(value) => Some(match value {}),
            None => None,
        }
    }

    /// Get a reference to the underlying value.
    ///
    /// # Returns
    ///
    ///  This always returns [`None`].
    #[inline(always)]
    #[must_use]
    #[allow(unreachable_code)]
    pub const fn as_ref(&self) -> Option<&T> {
        match &self.value {
            &Some(value) => Some(match value {}),
            None => None,
        }
    }

    /// Get a mutable reference to the underlying value.
    ///
    /// # Returns
    ///
    /// This always return [`None`].
    #[inline(always)]
    #[must_use]
    #[allow(unreachable_code)]
    pub const fn as_mut(&mut self) -> Option<&mut T> {
        match &mut self.value {
            &mut Some(value) => Some(match value {}),
            None => None,
        }
    }

    /// Cast this [`SpaNone`] to another kind of [`SpaNone`].
    #[inline(always)]
    #[must_use]
    pub const fn cast<U>(self) -> SpaNone<U>
    where
        U: ?Sized,
    {
        SpaNone::new()
    }
}

impl<T> Unpin for SpaNone<T> where T: ?Sized {}

impl<T> Clone for SpaNone<T>
where
    T: ?Sized,
{
    #[inline(always)]
    fn clone(&self) -> Self {
        *self
    }
}

impl<T> Copy for SpaNone<T> where T: ?Sized {}

impl<T> Default for SpaNone<T>
where
    T: ?Sized,
{
    #[inline(always)]
    fn default() -> Self {
        SpaNone::new()
    }
}

impl<T> fmt::Debug for SpaNone<T>
where
    T: ?Sized,
{
    fn fmt(
        &self,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        self.value.fmt(f)
    }
}

impl<T> PartialEq for SpaNone<T>
where
    T: ?Sized,
{
    #[inline(always)]
    fn eq(
        &self,
        other: &Self,
    ) -> bool {
        self.value == other.value
    }
}

impl<T> Eq for SpaNone<T> where T: ?Sized {}

impl<T> PartialOrd for SpaNone<T>
where
    T: ?Sized,
{
    #[inline(always)]
    fn partial_cmp(
        &self,
        other: &Self,
    ) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl<T> Ord for SpaNone<T>
where
    T: ?Sized,
{
    #[inline(always)]
    fn cmp(
        &self,
        other: &Self,
    ) -> std::cmp::Ordering {
        self.value.cmp(&other.value)
    }
}

impl<T> hash::Hash for SpaNone<T>
where
    T: ?Sized,
{
    #[inline(always)]
    fn hash<H>(
        &self,
        state: &mut H,
    ) where
        H: hash::Hasher,
    {
        self.value.hash(state);
    }
}

impl<T> PartialEq<Option<T>> for SpaNone<T>
where
    T: PartialEq,
{
    #[inline(always)]
    fn eq(
        &self,
        other: &Option<T>,
    ) -> bool {
        self.get().eq(other)
    }
}

impl<T> PartialOrd<Option<T>> for SpaNone<T>
where
    T: PartialOrd,
{
    #[inline(always)]
    fn partial_cmp(
        &self,
        other: &Option<T>,
    ) -> Option<std::cmp::Ordering> {
        self.get().partial_cmp(other)
    }
}

impl<T> PartialEq<SpaNone<T>> for Option<T>
where
    T: PartialEq,
{
    #[inline(always)]
    fn eq(
        &self,
        other: &SpaNone<T>,
    ) -> bool {
        self.eq(&other.get())
    }
}

impl<T> PartialOrd<SpaNone<T>> for Option<T>
where
    T: PartialOrd,
{
    #[inline(always)]
    fn partial_cmp(
        &self,
        other: &SpaNone<T>,
    ) -> Option<std::cmp::Ordering> {
        self.partial_cmp(&other.get())
    }
}

impl<T> TryFrom<Option<T>> for SpaNone<T> {
    type Error = T;

    #[inline(always)]
    fn try_from(value: Option<T>) -> Result<Self, Self::Error> {
        SpaNone::from_option(value)
    }
}

impl<T> From<SpaNone<T>> for Option<T> {
    #[inline(always)]
    fn from(value: SpaNone<T>) -> Self {
        value.get()
    }
}

// NOTE: All `SpaNone` types have the same layout, the generic type
//       is only used in a `PhantomData`.
const _: () = assert!(size_of::<SpaNone>() == 0);
const _: () = assert!(align_of::<SpaNone>() == 1);

impl<T> sealed::BasicPod for SpaNone<T> where T: ?Sized {}

unsafe impl<T> BasicPod for SpaNone<T>
where
    T: 'static + ?Sized,
{
    type Padding = [Byte; 0];

    const DEFAULT: Self = SpaNone::new();
    const SPA_KIND: SpaKind = SpaKind::None;
}

unsafe impl<T> AsBytes for SpaNone<T> where T: ?Sized {}
unsafe impl<T> AsBytesMut for SpaNone<T> where T: ?Sized {}
