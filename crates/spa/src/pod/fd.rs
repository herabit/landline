use std::{
    fs::File,
    io::{PipeReader, PipeWriter},
    net::{TcpListener, TcpStream, UdpSocket},
    num::TryFromIntError,
    os::{
        fd::{AsRawFd, BorrowedFd, IntoRawFd, OwnedFd},
        unix::net::{UnixDatagram, UnixStream},
    },
    process::{ChildStderr, ChildStdin, ChildStdout},
    rc::{self, Rc},
    sync::{self, Arc},
};

use crate::{
    mem::{AsBytes, AsBytesMut, Byte},
    pod::{PrimPod, kind::SpaKind, sealed},
};

/// A SPA file descriptor (that in some contexts is an index to a file descriptor). It is 64-bit, despite practically all
/// unixes defining file descriptors to be 32-bit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(C, packed)]
pub struct SpaFd(pub i64);

impl SpaFd {
    /// An invalid file descriptor (`-1`).
    pub const INVALID: SpaFd = SpaFd(-1);
}

impl From<i32> for SpaFd {
    #[inline(always)]
    fn from(value: i32) -> Self {
        SpaFd(value.into())
    }
}

impl TryFrom<SpaFd> for i32 {
    type Error = TryFromIntError;

    #[inline(always)]
    fn try_from(value: SpaFd) -> Result<Self, Self::Error> {
        value.0.try_into()
    }
}

impl From<u32> for SpaFd {
    #[inline(always)]
    fn from(value: u32) -> Self {
        SpaFd(value.into())
    }
}

impl TryFrom<SpaFd> for u32 {
    type Error = TryFromIntError;

    #[inline(always)]
    fn try_from(value: SpaFd) -> Result<Self, Self::Error> {
        value.0.try_into()
    }
}

impl From<i64> for SpaFd {
    #[inline(always)]
    fn from(value: i64) -> Self {
        SpaFd(value)
    }
}

impl From<SpaFd> for i64 {
    #[inline(always)]
    fn from(value: SpaFd) -> Self {
        value.0
    }
}

impl TryFrom<u64> for SpaFd {
    type Error = TryFromIntError;

    #[inline(always)]
    fn try_from(value: u64) -> Result<Self, Self::Error> {
        value.try_into().map(SpaFd)
    }
}

impl TryFrom<SpaFd> for u64 {
    type Error = TryFromIntError;

    #[inline(always)]
    fn try_from(value: SpaFd) -> Result<Self, Self::Error> {
        value.0.try_into()
    }
}

impl TryFrom<isize> for SpaFd {
    type Error = TryFromIntError;

    #[inline(always)]
    fn try_from(value: isize) -> Result<Self, Self::Error> {
        value.try_into().map(SpaFd)
    }
}

impl TryFrom<SpaFd> for isize {
    type Error = TryFromIntError;

    #[inline(always)]
    fn try_from(value: SpaFd) -> Result<Self, Self::Error> {
        value.0.try_into()
    }
}

impl TryFrom<usize> for SpaFd {
    type Error = TryFromIntError;

    #[inline(always)]
    fn try_from(value: usize) -> Result<Self, Self::Error> {
        value.try_into().map(SpaFd)
    }
}

impl TryFrom<SpaFd> for usize {
    type Error = TryFromIntError;

    #[inline(always)]
    fn try_from(value: SpaFd) -> Result<Self, Self::Error> {
        value.0.try_into()
    }
}

#[inline(always)]
fn into_spa_fd<F>(fd: F) -> SpaFd
where
    F: TryInto<i64, Error: std::fmt::Debug>,
{
    fd.try_into()
        .map(SpaFd)
        .expect("somehow we have a file descriptor that does not fit within 64 bits")
}

impl From<OwnedFd> for SpaFd {
    #[inline(always)]
    #[track_caller]
    fn from(value: OwnedFd) -> Self {
        into_spa_fd(value.into_raw_fd())
    }
}

impl From<BorrowedFd<'_>> for SpaFd {
    #[inline(always)]
    #[track_caller]
    fn from(value: BorrowedFd<'_>) -> Self {
        into_spa_fd(value.as_raw_fd())
    }
}

impl From<ChildStderr> for SpaFd {
    #[inline(always)]
    #[track_caller]
    fn from(value: ChildStderr) -> Self {
        into_spa_fd(value.into_raw_fd())
    }
}

impl From<ChildStdin> for SpaFd {
    #[inline(always)]
    #[track_caller]
    fn from(value: ChildStdin) -> Self {
        into_spa_fd(value.into_raw_fd())
    }
}

impl From<ChildStdout> for SpaFd {
    #[inline(always)]
    #[track_caller]
    fn from(value: ChildStdout) -> Self {
        into_spa_fd(value.into_raw_fd())
    }
}

impl From<File> for SpaFd {
    #[inline(always)]
    #[track_caller]
    fn from(value: File) -> Self {
        into_spa_fd(value.into_raw_fd())
    }
}

impl From<PipeReader> for SpaFd {
    #[inline(always)]
    #[track_caller]
    fn from(value: PipeReader) -> Self {
        into_spa_fd(value.into_raw_fd())
    }
}

impl From<PipeWriter> for SpaFd {
    #[inline(always)]
    #[track_caller]
    fn from(value: PipeWriter) -> Self {
        into_spa_fd(value.into_raw_fd())
    }
}

impl From<TcpListener> for SpaFd {
    #[inline(always)]
    #[track_caller]
    fn from(value: TcpListener) -> Self {
        into_spa_fd(value.into_raw_fd())
    }
}

impl From<TcpStream> for SpaFd {
    #[inline(always)]
    #[track_caller]
    fn from(value: TcpStream) -> Self {
        into_spa_fd(value.into_raw_fd())
    }
}

impl From<UdpSocket> for SpaFd {
    #[inline(always)]
    #[track_caller]
    fn from(value: UdpSocket) -> Self {
        into_spa_fd(value.into_raw_fd())
    }
}

impl From<UnixDatagram> for SpaFd {
    #[inline(always)]
    #[track_caller]
    fn from(value: UnixDatagram) -> Self {
        into_spa_fd(value.into_raw_fd())
    }
}

impl From<UnixStream> for SpaFd {
    #[inline(always)]
    #[track_caller]
    fn from(value: UnixStream) -> Self {
        into_spa_fd(value.into_raw_fd())
    }
}

impl<F> From<&F> for SpaFd
where
    F: AsRawFd + ?Sized,
{
    #[inline(always)]
    #[track_caller]
    fn from(value: &F) -> Self {
        into_spa_fd(value.as_raw_fd())
    }
}

impl<F> From<&mut F> for SpaFd
where
    F: AsRawFd + ?Sized,
{
    #[inline(always)]
    #[track_caller]
    fn from(value: &mut F) -> Self {
        into_spa_fd(value.as_raw_fd())
    }
}

impl<F> TryFrom<Rc<F>> for SpaFd
where
    F: IntoRawFd,
{
    type Error = Rc<F>;

    #[inline(always)]
    #[track_caller]
    fn try_from(value: Rc<F>) -> Result<Self, Self::Error> {
        Rc::try_unwrap(value).map(|f| into_spa_fd(f.into_raw_fd()))
    }
}

impl<F> TryFrom<rc::Weak<F>> for SpaFd
where
    F: IntoRawFd,
{
    type Error = rc::Weak<F>;

    #[inline(always)]
    #[track_caller]
    fn try_from(weak: rc::Weak<F>) -> Result<Self, Self::Error> {
        if let Some(strong) = weak.upgrade()
            && let Ok(value) = Rc::try_unwrap(strong)
        {
            Ok(into_spa_fd(value.into_raw_fd()))
        } else {
            Err(weak)
        }
    }
}

impl<F> TryFrom<Arc<F>> for SpaFd
where
    F: IntoRawFd,
{
    type Error = Arc<F>;

    #[inline(always)]
    #[track_caller]
    fn try_from(value: Arc<F>) -> Result<Self, Self::Error> {
        Arc::try_unwrap(value).map(|f| into_spa_fd(f.into_raw_fd()))
    }
}

impl<F> TryFrom<sync::Weak<F>> for SpaFd
where
    F: IntoRawFd,
{
    type Error = sync::Weak<F>;

    #[inline(always)]
    #[track_caller]
    fn try_from(weak: sync::Weak<F>) -> Result<Self, Self::Error> {
        if let Some(strong) = weak.upgrade()
            && let Ok(value) = Arc::try_unwrap(strong)
        {
            Ok(into_spa_fd(value.into_raw_fd()))
        } else {
            Err(weak)
        }
    }
}

impl<F> From<Box<F>> for SpaFd
where
    F: IntoRawFd,
{
    #[inline(always)]
    #[track_caller]
    fn from(value: Box<F>) -> Self {
        into_spa_fd(value.into_raw_fd())
    }
}

impl Default for SpaFd {
    #[inline(always)]
    fn default() -> Self {
        SpaFd::INVALID
    }
}

impl sealed::PrimPod for SpaFd {}

unsafe impl PrimPod for SpaFd {
    type Padding = [Byte; 0];

    const DEFAULT: Self = SpaFd::INVALID;
    const SPA_KIND: SpaKind = SpaKind::Fd;
}

unsafe impl AsBytes for SpaFd {}
unsafe impl AsBytesMut for SpaFd {}
