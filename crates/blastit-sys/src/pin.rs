//! Open a shared object once and prove it is still the file that was inspected.
//!
//! `libphp`, `libpython` and the per-minor C shims are loaded **once**, through
//! `/proc/self/fd/<fd>`, and `(st_dev, st_ino)` is their whole record: the
//! descriptor keeps the inode allocated and nothing re-opens the file by path
//! afterwards (design `universal-distribution.md` §1.1).
//!
//! The PHP entry pin is deliberately **not** here: `SCRIPT_FILENAME` is
//! re-resolved by path per request, so `EntryPin` (`st_size` + `st_ctime` plus
//! an `O_PATH` descriptor and `repin_entry`) stays in
//! `blastit-view-php/src/pinning.rs`. A Python shim only `dlopen`s a descriptor.

use std::fmt;
use std::io;
use std::os::fd::{AsRawFd, OwnedFd, RawFd};
use std::path::{Path, PathBuf};

use rustix::fs::{FileType, Mode, OFlags};
use rustix::io::Errno;

/// Why an object was not pinned. Every case is a refusal a boot prints.
#[derive(Debug)]
#[non_exhaustive]
pub enum PinError {
    /// The path is a symbolic link (`O_NOFOLLOW` -> `ELOOP`).
    Symlink { path: PathBuf },
    /// `open(2)` failed for any other reason.
    Open { path: PathBuf, detail: String },
    /// The object is not a regular file (a directory, a FIFO, a socket, …).
    NotRegular { path: PathBuf },
    /// The file is not the object that was inspected: `(st_dev, st_ino)` moved.
    IdentityChanged {
        path: PathBuf,
        expected_ino: u64,
        found_ino: u64,
    },
    /// `fstat(2)` on the freshly opened descriptor failed.
    Io { path: PathBuf, detail: String },
}

impl fmt::Display for PinError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Symlink { path } => write!(
                f,
                "{}: is a symbolic link, and a shared object this build loads is never a link; \
                 restore it and restart blastit",
                path.display()
            ),
            Self::Open { path, detail } => write!(
                f,
                "{}: cannot be opened read-only (O_NOFOLLOW): {detail}",
                path.display()
            ),
            Self::NotRegular { path } => write!(f, "{} is not a regular file", path.display()),
            Self::IdentityChanged {
                path,
                expected_ino,
                found_ino,
            } => write!(
                f,
                "{} was replaced after blastit inspected it (inode {expected_ino} is now \
                 {found_ino}); restore the file and restart blastit",
                path.display()
            ),
            Self::Io { path, detail } => {
                write!(f, "{}: it cannot be inspected ({detail})", path.display())
            }
        }
    }
}

impl std::error::Error for PinError {}

/// Opens `path` as a shared object and refuses a file whose identity moved.
///
/// `O_RDONLY | O_CLOEXEC | O_NOFOLLOW | O_NONBLOCK`, and it must be a regular
/// file. `O_NONBLOCK` matters: a FIFO left at a library's path makes a plain
/// read-only `open(2)` wait for a writer, and a boot must refuse that path
/// rather than hang before its listener binds.
///
/// # Errors
///
/// [`PinError`] for a symlink, an open failure, a non-file, a moved identity,
/// or a failed `fstat(2)`.
pub fn open_pinned(path: &Path, expected: Option<(u64, u64)>) -> Result<OwnedFd, PinError> {
    let descriptor = rustix::fs::open(
        path,
        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NOFOLLOW | OFlags::NONBLOCK,
        Mode::empty(),
    )
    .map_err(|err| match err {
        Errno::LOOP => PinError::Symlink {
            path: path.to_path_buf(),
        },
        other => PinError::Open {
            path: path.to_path_buf(),
            detail: other.to_string(),
        },
    })?;
    let stat = rustix::fs::fstat(&descriptor).map_err(|err| PinError::Io {
        path: path.to_path_buf(),
        detail: err.to_string(),
    })?;
    if !FileType::from_raw_mode(stat.st_mode).is_file() {
        return Err(PinError::NotRegular {
            path: path.to_path_buf(),
        });
    }
    if let Some((dev, ino)) = expected
        && (stat.st_dev != dev || stat.st_ino != ino)
    {
        return Err(PinError::IdentityChanged {
            path: path.to_path_buf(),
            expected_ino: ino,
            found_ino: stat.st_ino,
        });
    }
    Ok(descriptor)
}

/// A pinned shared object: the descriptor, its `/proc/self/fd/<fd>` spelling,
/// and the path it was opened from.
#[derive(Debug)]
pub struct SharedObject {
    fd: OwnedFd,
    source: PathBuf,
}

impl SharedObject {
    /// [`open_pinned`]'s descriptor, kept as the object it names.
    pub fn open(path: &Path, expected: Option<(u64, u64)>) -> Result<Self, PinError> {
        Ok(Self {
            fd: open_pinned(path, expected)?,
            source: path.to_path_buf(),
        })
    }

    /// `/proc/self/fd/<fd>`, the only name a load uses.
    ///
    /// Both languages' loaders `dlopen` **this**, so the directory around the
    /// original path cannot be swapped between verification and the load.
    #[must_use]
    pub fn path(&self) -> PathBuf {
        PathBuf::from(format!("/proc/self/fd/{}", self.fd.as_raw_fd()))
    }

    /// The path this object was opened from — what a refusal names, never the
    /// `/proc/self/fd/<fd>` spelling.
    #[must_use]
    pub fn source(&self) -> &Path {
        &self.source
    }

    /// The live `(st_dev, st_ino)`, re-read from the descriptor itself.
    pub fn identity(&self) -> io::Result<(u64, u64)> {
        let stat = rustix::fs::fstat(&self.fd).map_err(io::Error::from)?;
        Ok((stat.st_dev, stat.st_ino))
    }

    /// The raw descriptor, for a caller that keeps it across an `execve`.
    #[must_use]
    pub fn as_raw_fd(&self) -> RawFd {
        self.fd.as_raw_fd()
    }

    /// The descriptor itself, for the stage-1 hand-off.
    #[must_use]
    pub fn into_fd(self) -> OwnedFd {
        self.fd
    }
}
