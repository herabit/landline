use std::{
    borrow::Borrow,
    convert::Infallible,
    error,
    ffi::{CStr, FromBytesUntilNulError, FromBytesWithNulError},
    fmt, hash, hint, io,
    marker::PhantomData,
    mem,
    str::Utf8Error,
};

/// Self explanatory, a [`Result`](std::result::Result) that exists
/// to reduce boilerplate.
pub type Result<T, E = PodError> = std::result::Result<T, E>;

/// An error that can occur when processing a POD.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum PodError {
    /// Some size, such as those found within a SPA POD header, exceeded [`MAX_SIZE`](super::MAX_SIZE).
    InvalidSize,

    /// The kind specified by a SPA POD header was invalid.
    InvalidKind,

    /// There wasn't enough space in a buffer.
    InsufficientSpace,

    /// There was no NUL terminator when attempting to parse a [`SpaStr`].
    ///
    /// This corresponds to [`FromBytesWithNulError::NotNulTerminated`] or
    /// [`FromBytesUntilNulError`] depending upon the context.
    NotNulTerminated,

    /// There was an unexpected NUL terminator.
    ///
    /// This corresponds to [`FromBytesWithNulError::InteriorNul`].
    InteriorNul {
        /// The position of the interior NUL terminator.
        pos: usize,
    },

    /// There was an issue decoding UTF-8 data.
    Utf8 {
        /// The error that occurred whil decoding.
        error: Utf8Error,
    },

    /// Some unknown error occurred.
    Unknown,
}

impl PodError {
    /// Get an error message corresponding to this error.
    #[inline]
    #[must_use]
    pub const fn message(&self) -> &'static str {
        match self {
            PodError::InvalidSize => "an invalid size was specified",
            PodError::InvalidKind => "an invalid kind was specified",
            PodError::InsufficientSpace => "insufficient space for data",
            PodError::NotNulTerminated => "data provided is not nul terminated",

            PodError::InteriorNul { .. } => {
                "data provided contains an interior nul byte"
            },

            PodError::Utf8 { error } => match error.error_len() {
                None => "incomplete utf-8 byte sequence",
                Some(1) => "invalid utf-8 sequence of 1 byte",
                Some(2) => "invalid utf-8 sequence of 2 bytes",
                Some(3) => "invalid utf-8 sequence of 3 bytes",
                // SAFETY: Invalid UTF-8 sequences are at most 3 bytes.
                Some(0 | 4..) => unsafe { hint::unreachable_unchecked() },
            },

            PodError::Unknown => "an unknown error occurred",
        }
    }

    /// Attempt to return a best-guess [`io::ErrorKind`] for this error.
    #[inline(always)]
    #[must_use]
    pub const fn io_error_kind(&self) -> io::ErrorKind {
        use io::ErrorKind;

        match self {
            PodError::InvalidSize => ErrorKind::InvalidData,
            PodError::InvalidKind => ErrorKind::InvalidData,
            PodError::InsufficientSpace => ErrorKind::UnexpectedEof,
            PodError::NotNulTerminated => ErrorKind::InvalidData,
            PodError::InteriorNul { .. } => ErrorKind::InvalidData,
            PodError::Utf8 { .. } => ErrorKind::InvalidData,
            PodError::Unknown => ErrorKind::Other,
        }
    }

    /// Convert this [`PodError`] into an [`io::Error`].
    #[inline(always)]
    #[must_use]
    #[track_caller]
    pub fn into_io_error(self) -> io::Error {
        io::Error::new(self.io_error_kind(), self)
    }
}

impl hash::Hash for PodError {
    #[inline]
    #[track_caller]
    fn hash<H>(
        &self,
        state: &mut H,
    ) where
        H: hash::Hasher,
    {
        // NOTE: We need to differentiate between our pod values.
        mem::discriminant(self).hash(state);

        match self {
            PodError::InteriorNul { pos } => {
                pos.hash(state);
            },

            // NOTE: `Utf8Error` currently does not implement Hash... No clue why,
            //        but this *technically* runs the risk of not being *correct*,
            //        say, in the case `Utf8Error` adds some other, hidden fields,
            //        or public methods.
            //
            //        Is this likely? No, not at all.
            PodError::Utf8 { error } => {
                error.valid_up_to().hash(state);
                error.error_len().hash(state);
            },

            PodError::InvalidSize
            | PodError::InvalidKind
            | PodError::InsufficientSpace
            | PodError::NotNulTerminated
            | PodError::Unknown => {},
        }
    }
}

impl Default for PodError {
    #[inline(always)]
    fn default() -> Self {
        PodError::Unknown
    }
}

impl fmt::Display for PodError {
    fn fmt(
        &self,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        match self {
            PodError::NotNulTerminated | PodError::InteriorNul { .. } => {
                FromBytesWithNulError::try_from(*self).unwrap().fmt(f)
            },

            PodError::Utf8 { error } => error.fmt(f),

            _ => self.message().fmt(f),
        }
    }
}

impl error::Error for PodError {
    #[inline(always)]
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        match self {
            PodError::Utf8 { error } => Some(error),
            PodError::InvalidSize
            | PodError::InvalidKind
            | PodError::InsufficientSpace
            | PodError::NotNulTerminated
            | PodError::InteriorNul { .. }
            | PodError::Unknown => None,
        }
    }

    #[allow(deprecated)]
    #[inline(always)]
    fn description(&self) -> &str {
        self.message()
    }
}

impl From<Infallible> for PodError {
    #[inline(always)]
    fn from(value: Infallible) -> Self {
        match value {}
    }
}

impl From<Utf8Error> for PodError {
    #[inline(always)]
    fn from(error: Utf8Error) -> Self {
        PodError::Utf8 { error }
    }
}

impl From<PodError> for io::ErrorKind {
    #[inline(always)]
    fn from(pod_error: PodError) -> Self {
        pod_error.io_error_kind()
    }
}

impl From<PodError> for io::Error {
    #[inline(always)]
    #[track_caller]
    fn from(pod_error: PodError) -> Self {
        pod_error.into_io_error()
    }
}

impl From<FromBytesUntilNulError> for PodError {
    #[inline(always)]
    fn from(_: FromBytesUntilNulError) -> Self {
        PodError::NotNulTerminated
    }
}

impl TryFrom<PodError> for FromBytesUntilNulError {
    type Error = PodErrorIntoError<FromBytesUntilNulError>;

    #[inline(always)]
    fn try_from(pod_error: PodError) -> Result<Self, Self::Error> {
        if let PodError::NotNulTerminated = pod_error {
            Ok(const {
                match CStr::from_bytes_until_nul(b"") {
                    Ok(_) => panic!(
                        "we know that the provided string has no nul terminator"
                    ),
                    Err(err) => err,
                }
            })
        } else {
            Err(PodErrorIntoError {
                pod_error,
                _expected: PhantomData,
            })
        }
    }
}

impl From<FromBytesWithNulError> for PodError {
    #[inline(always)]
    fn from(value: FromBytesWithNulError) -> Self {
        match value {
            FromBytesWithNulError::InteriorNul { position } => {
                PodError::InteriorNul { pos: position }
            },
            FromBytesWithNulError::NotNulTerminated => {
                PodError::NotNulTerminated
            },
        }
    }
}

impl TryFrom<PodError> for FromBytesWithNulError {
    type Error = PodErrorIntoError<FromBytesWithNulError>;

    #[inline(always)]
    fn try_from(pod_error: PodError) -> Result<Self, Self::Error> {
        match pod_error {
            PodError::NotNulTerminated => {
                Ok(FromBytesWithNulError::NotNulTerminated)
            },
            PodError::InteriorNul { pos: position } => {
                Ok(FromBytesWithNulError::InteriorNul { position })
            },
            pod_error => Err(PodErrorIntoError {
                pod_error,
                _expected: PhantomData,
            }),
        }
    }
}

impl TryFrom<PodError> for Utf8Error {
    type Error = PodErrorIntoError<Utf8Error>;

    #[inline(always)]
    fn try_from(pod_error: PodError) -> Result<Self, Self::Error> {
        match pod_error {
            PodError::Utf8 { error } => Ok(error),
            pod_error => Err(PodErrorIntoError {
                pod_error,
                _expected: PhantomData,
            }),
        }
    }
}

/// An error that occurs when we're unable to convert from a [`PodError`] to some
/// other error type.
#[repr(transparent)]
#[non_exhaustive]
pub struct PodErrorIntoError<E>
where
    E: ?Sized,
{
    /// The error we failed to convert from.
    pub pod_error: PodError,
    /// The expected error type.
    _expected: PhantomData<E>,
}

impl<E> fmt::Debug for PodErrorIntoError<E>
where
    E: ?Sized,
{
    #[inline]
    #[track_caller]
    fn fmt(
        &self,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        #[track_caller]
        fn inner(
            error: &PodError,
            f: &mut fmt::Formatter<'_>,
        ) -> fmt::Result {
            f.debug_struct("PodErrorIntoError")
                .field("pod_error", error)
                .finish_non_exhaustive()
        }

        inner(&self.pod_error, f)
    }
}

impl<E> fmt::Display for PodErrorIntoError<E>
where
    E: ?Sized,
{
    #[inline]
    #[track_caller]
    fn fmt(
        &self,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        #[track_caller]
        fn inner(
            error: &PodError,
            type_name: &str,
            f: &mut fmt::Formatter<'_>,
        ) -> fmt::Result {
            // FIXME: Write a better error message.
            write!(f, "failed to create a `{type_name}`, instead: {error:?}")
        }

        inner(&self.pod_error, std::any::type_name::<E>(), f)
    }
}

impl<E> error::Error for PodErrorIntoError<E>
where
    E: ?Sized,
{
    #[inline(always)]
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        Some(&self.pod_error)
    }

    #[inline(always)]
    #[allow(deprecated)]
    fn description(&self) -> &str {
        "failed to create some error from a spa pod error"
    }
}

impl<E> From<PodErrorIntoError<E>> for PodError
where
    E: ?Sized,
{
    #[inline(always)]
    fn from(value: PodErrorIntoError<E>) -> Self {
        value.pod_error
    }
}

impl<E> From<&PodErrorIntoError<E>> for PodError
where
    E: ?Sized,
{
    #[inline(always)]
    fn from(value: &PodErrorIntoError<E>) -> Self {
        value.pod_error
    }
}

impl<E> From<&mut PodErrorIntoError<E>> for PodError
where
    E: ?Sized,
{
    #[inline(always)]
    fn from(value: &mut PodErrorIntoError<E>) -> Self {
        value.pod_error
    }
}

impl<E> From<Infallible> for PodErrorIntoError<E> {
    #[inline(always)]
    fn from(value: Infallible) -> Self {
        match value {}
    }
}

/// A [`PodError`] occurred with some associated data.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub struct WithPodError<T>
where
    T: ?Sized,
{
    /// The actual [`PodError`] that occurred.
    pub pod_error: PodError,
    /// The associated value.
    pub value: T,
}

impl<T> From<WithPodError<T>> for PodError {
    #[inline(always)]
    #[track_caller]
    fn from(error: WithPodError<T>) -> Self {
        error.pod_error
    }
}

impl<T> From<&WithPodError<T>> for PodError
where
    T: ?Sized,
{
    #[inline(always)]
    fn from(error: &WithPodError<T>) -> Self {
        error.pod_error
    }
}

impl<T> From<&mut WithPodError<T>> for PodError
where
    T: ?Sized,
{
    #[inline(always)]
    fn from(error: &mut WithPodError<T>) -> Self {
        error.pod_error
    }
}

impl<T> From<Infallible> for WithPodError<T> {
    #[inline(always)]
    fn from(value: Infallible) -> Self {
        match value {}
    }
}

impl<T> PartialEq<WithPodError<T>> for PodError
where
    T: ?Sized,
{
    #[inline(always)]
    fn eq(
        &self,
        other: &WithPodError<T>,
    ) -> bool {
        *self == other.pod_error
    }
}

impl<T> PartialEq<PodError> for WithPodError<T>
where
    T: ?Sized,
{
    #[inline(always)]
    fn eq(
        &self,
        other: &PodError,
    ) -> bool {
        self.pod_error == *other
    }
}

impl<T> fmt::Debug for WithPodError<T>
where
    T: ?Sized + fmt::Debug,
{
    #[track_caller]
    fn fmt(
        &self,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        f.debug_struct("WithPodError")
            .field("pod_error", &self.pod_error)
            .field("value", &&self.value)
            .finish()
    }
}
impl<T> fmt::Display for WithPodError<T>
where
    T: ?Sized,
{
    #[inline(always)]
    #[track_caller]
    fn fmt(
        &self,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        self.pod_error.fmt(f)
    }
}

impl<T> error::Error for WithPodError<T>
where
    T: ?Sized + fmt::Debug,
{
    #[inline(always)]
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        Some(&self.pod_error)
    }

    #[allow(deprecated)]
    #[inline(always)]
    fn description(&self) -> &str {
        self.pod_error.description()
    }
}

/// Error that can occur when attempting to convert *to* UTF-8.
#[derive(Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub struct WithUtf8Error<T>
where
    T: ?Sized,
{
    /// The [`Utf8Error`] that occurred.
    pub error: Utf8Error,
    /// The associated value we failed to convert from.
    pub value: T,
}

impl<T> From<WithUtf8Error<T>> for Utf8Error {
    #[inline(always)]
    #[track_caller]
    fn from(value: WithUtf8Error<T>) -> Self {
        value.error
    }
}

impl<T> From<&WithUtf8Error<T>> for Utf8Error
where
    T: ?Sized,
{
    #[inline(always)]
    fn from(value: &WithUtf8Error<T>) -> Self {
        value.error
    }
}

impl<T> From<&mut WithUtf8Error<T>> for Utf8Error
where
    T: ?Sized,
{
    #[inline(always)]
    fn from(value: &mut WithUtf8Error<T>) -> Self {
        value.error
    }
}

impl<T> From<Infallible> for WithUtf8Error<T> {
    #[inline(always)]
    fn from(value: Infallible) -> Self {
        match value {}
    }
}

impl<T> From<WithUtf8Error<T>> for PodError {
    #[inline(always)]
    #[track_caller]
    fn from(value: WithUtf8Error<T>) -> Self {
        value.error.into()
    }
}

impl<T> fmt::Debug for WithUtf8Error<T>
where
    T: ?Sized + fmt::Debug,
{
    #[inline(always)]
    fn fmt(
        &self,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        #[track_caller]
        fn inner(
            error: &Utf8Error,
            value: &dyn fmt::Debug,
            f: &mut fmt::Formatter<'_>,
        ) -> fmt::Result {
            f.debug_struct("WithUtf8Error")
                .field("error", error)
                .field("value", value)
                .finish()
        }

        inner(&self.error, &&self.value, f)
    }
}

impl<T> fmt::Display for WithUtf8Error<T>
where
    T: ?Sized,
{
    #[inline(always)]
    fn fmt(
        &self,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        self.error.fmt(f)
    }
}

impl<T> error::Error for WithUtf8Error<T>
where
    T: ?Sized + fmt::Debug,
{
    #[inline(always)]
    fn source(&self) -> Option<&(dyn error::Error + 'static)> {
        Some(&self.error)
    }

    #[allow(deprecated)]
    #[inline(always)]
    fn description(&self) -> &str {
        self.error.description()
    }
}
