use std::{
    convert::Infallible,
    error,
    ffi::{CStr, FromBytesUntilNulError, FromBytesWithNulError},
    fmt,
    marker::PhantomData,
};

/// Self explanatory, a [`Result`](std::result::Result) that exists
/// to reduce boilerplate.
pub type Result<T, E = PodError> = std::result::Result<T, E>;

/// An error that can occur when processing a POD.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum PodError {
    /// The size specified by the POD was invalid, such as it exceeding [`MAX_SIZE`],
    /// or it otherwise didn't meet our expectations.
    InvalidSize,

    /// The kind specified by the POD was invalid, or it otherwise didn't meet our expectations,
    /// such as it specifying an element type of string when we're parsing an array.
    InvalidKind,

    /// There wasn't enough space in the buffer to parse the POD.
    InsufficientSpace,

    /// There was no NUL terminator when attempting to parse a [`SpaStr`].
    ///
    /// This corresponds to [`FromBytesWithNulError::NotNulTerminated`] or
    /// [`FromBytesUntilNulError`] depending upon the context.
    NotNulTerminated,

    /// There was an unexpected NUL terminator.
    #[non_exhaustive]
    InteriorNul {
        /// The position of the interior NUL terminator.
        position: usize,
    },

    /// An error that doesn't yet have a name.
    Other,
}

impl PodError {
    /// Get an error message corresponding to this error.
    ///
    /// This may not necessarily reflect what's displayed by [`fmt::Display`], as that may contain more information, however,
    /// it will still provide as accurate of an error message as possible.
    #[inline]
    #[must_use]
    pub const fn message(&self) -> &'static str {
        match self {
            PodError::InvalidSize => {
                "an invalid size was specified for the provided SPA POD"
            },
            PodError::InvalidKind => {
                "an invalid kind was specified for the provided SPA POD"
            },
            PodError::InsufficientSpace => {
                "insufficient space for the provided SPA POD"
            },
            PodError::NotNulTerminated => {
                "no NUL terminator found within the provided SPA POD"
            },
            PodError::InteriorNul { .. } => {
                "an interior NUL terminator was found within the provided SPA POD"
            },
            PodError::Other => {
                "an unknown error occurred when handling a SPA POD"
            },
        }
    }
}

impl fmt::Display for PodError {
    #[track_caller]
    fn fmt(
        &self,
        f: &mut fmt::Formatter<'_>,
    ) -> fmt::Result {
        match self {
            PodError::InteriorNul { position } => write!(
                f,
                "an interior NUL terminator was found within the provided SPA POD at byte position {position}"
            ),
            error => write!(f, "{}", error.message()),
        }
    }
}

impl error::Error for PodError {
    #[allow(deprecated)]
    #[inline]
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

impl From<FromBytesUntilNulError> for PodError {
    #[inline(always)]
    fn from(_: FromBytesUntilNulError) -> Self {
        PodError::NotNulTerminated
    }
}

impl TryFrom<PodError> for FromBytesUntilNulError {
    type Error = FromPodErrorError<FromBytesUntilNulError>;

    #[inline(always)]
    fn try_from(pod_error: PodError) -> Result<Self, Self::Error> {
        if let PodError::NotNulTerminated = pod_error {
            Ok(const {
                match CStr::from_bytes_until_nul(b"") {
                    Ok(_) => panic!(
                        "we know that the provided string has no NUL terimator"
                    ),
                    Err(err) => err,
                }
            })
        } else {
            Err(FromPodErrorError {
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
                PodError::InteriorNul { position }
            },
            FromBytesWithNulError::NotNulTerminated => {
                PodError::NotNulTerminated
            },
        }
    }
}

impl TryFrom<PodError> for FromBytesWithNulError {
    type Error = FromPodErrorError<FromBytesWithNulError>;

    #[inline(always)]
    fn try_from(pod_error: PodError) -> Result<Self, Self::Error> {
        match pod_error {
            PodError::NotNulTerminated => {
                Ok(FromBytesWithNulError::NotNulTerminated)
            },
            PodError::InteriorNul { position } => {
                Ok(FromBytesWithNulError::InteriorNul { position })
            },
            pod_error => Err(FromPodErrorError {
                pod_error,
                _expected: PhantomData,
            }),
        }
    }
}

/// An error that occures when we're unable to convert from a [`PodError`] to some
/// other error type.
#[repr(transparent)]
#[non_exhaustive]
pub struct FromPodErrorError<E>
where
    E: ?Sized,
{
    /// The error we failed to convert from.
    pub pod_error: PodError,
    /// The expected error type.
    _expected: PhantomData<E>,
}

impl<E> fmt::Debug for FromPodErrorError<E>
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
            f.debug_struct("FromPodErrorError")
                .field("pod_error", error)
                .finish_non_exhaustive()
        }

        inner(&self.pod_error, f)
    }
}

impl<E> fmt::Display for FromPodErrorError<E>
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
            write!(f, "failed to create a `{type_name}`, instead: {error}")
        }

        inner(&self.pod_error, std::any::type_name::<E>(), f)
    }
}

impl<E> error::Error for FromPodErrorError<E>
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
        "failed to create an `FromPodErrorError` value from some other error type"
    }
}

impl<E> From<FromPodErrorError<E>> for PodError
where
    E: ?Sized,
{
    #[inline(always)]
    fn from(value: FromPodErrorError<E>) -> Self {
        value.pod_error
    }
}
