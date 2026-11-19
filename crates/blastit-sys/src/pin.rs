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

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use std::io::Write;
    use std::os::unix::fs::MetadataExt;

    use super::*;

    /// A directory under the workspace's `target/tmp`, removed when dropped.
    struct Fixture {
        dir: PathBuf,
    }

    impl Fixture {
        fn new(tag: &str) -> Self {
            let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/tmp/pin");
            let dir = root.join(format!("{}-{tag}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("the fixture directory");
            Self { dir }
        }

        fn file(&self, name: &str, contents: &[u8]) -> PathBuf {
            let path = self.dir.join(name);
            std::fs::write(&path, contents).expect("the fixture file");
            path
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.dir);
        }
    }

    fn identity(path: &Path) -> (u64, u64) {
        let metadata = std::fs::metadata(path).expect("metadata");
        (metadata.dev(), metadata.ino())
    }

    #[test]
    fn an_accepted_object_is_opened_and_named_through_proc() {
        let fixture = Fixture::new("accept");
        let path = fixture.file("libpython3.14.so.1.0", b"a shared object");

        let object = SharedObject::open(&path, Some(identity(&path))).expect("the recorded file");
        assert_eq!(
            object.path(),
            PathBuf::from(format!("/proc/self/fd/{}", object.as_raw_fd()))
        );
        assert_eq!(object.source(), path);
        assert_eq!(object.identity().expect("live identity"), identity(&path));
        assert_eq!(
            std::fs::read(object.path()).expect("the descriptor"),
            b"a shared object"
        );
    }

    #[test]
    fn a_replaced_object_is_refused_with_both_identities() {
        let fixture = Fixture::new("swap");
        let path = fixture.file("libpython3.14.so.1.0", b"the inspected object");
        let recorded = identity(&path);

        // A side file written while the recorded inode is still allocated, then
        // renamed over it: an unlinked inode's number can be handed straight
        // back, so `remove_file` + `write` would not prove a different file.
        let side = fixture.file("replacement", b"something else");
        std::fs::rename(&side, &path).expect("renaming the replacement over the object");
        assert_ne!(identity(&path), recorded, "the fixture replaced the file");

        let refusal = open_pinned(&path, Some(recorded)).expect_err("a moved inode is refused");
        assert!(
            matches!(refusal, PinError::IdentityChanged { expected_ino, found_ino, .. } if expected_ino == recorded.1 && found_ino == identity(&path).1),
            "{refusal}"
        );
        let text = refusal.to_string();
        assert!(text.contains(&path.display().to_string()), "{text}");
        assert!(text.contains("restart blastit"), "{text}");
        assert!(
            text.contains("was replaced after blastit inspected it"),
            "a user-facing refusal never names an internal stage: {text}"
        );
        assert!(!text.contains("stage 0"), "{text}");
        assert!(
            text.contains(&format!(
                "inode {} is now {}",
                recorded.1,
                identity(&path).1
            )),
            "the sentence names both inodes: {text}"
        );
    }

    #[test]
    fn a_symlink_is_refused_by_o_nofollow() {
        let fixture = Fixture::new("symlink");
        let target = fixture.file("real.so", b"a shared object");
        let link = fixture.dir.join("libpython3.14.so");
        std::os::unix::fs::symlink(&target, &link).expect("the symlink");

        let refusal = open_pinned(&link, None).expect_err("O_NOFOLLOW refuses a symlink");
        assert!(matches!(refusal, PinError::Symlink { .. }), "{refusal}");
        let text = refusal.to_string();
        assert!(text.contains(&link.display().to_string()), "{text}");
        assert!(text.contains("is a symbolic link"), "{text}");
    }

    #[test]
    fn a_directory_or_fifo_is_refused_as_non_regular() {
        let fixture = Fixture::new("not-regular");
        let directory = fixture.dir.join("a-directory");
        std::fs::create_dir(&directory).expect("the directory");
        assert!(matches!(
            open_pinned(&directory, None).expect_err("a directory is not a shared object"),
            PinError::NotRegular { .. }
        ));

        // A FIFO without a writer would block a plain read-only `open(2)`: the
        // non-blocking open is what makes this a refusal instead of a hang.
        let fifo = fixture.dir.join("a-fifo");
        let name = std::ffi::CString::new(fifo.as_os_str().as_encoded_bytes()).expect("a path");
        // SAFETY: `name` is a live NUL-terminated path and `mkfifo` touches no
        // other memory.
        let created = unsafe { libc::mkfifo(name.as_ptr(), 0o600) };
        assert_eq!(created, 0, "mkfifo: {}", io::Error::last_os_error());
        assert!(matches!(
            open_pinned(&fifo, None).expect_err("a FIFO is not a shared object"),
            PinError::NotRegular { .. }
        ));
    }

    #[test]
    fn the_descriptor_still_names_the_object_after_its_path_is_unlinked() {
        let fixture = Fixture::new("unlinked");
        let path = fixture.file("libpython3.14.so.1.0", b"a pinned payload");
        let object = SharedObject::open(&path, None).expect("the pinned object");
        let fd = object.as_raw_fd();

        std::fs::remove_file(&path).expect("unlinking the source path");
        assert!(!path.exists());
        assert_eq!(
            std::fs::read(format!("/proc/self/fd/{fd}")).expect("the descriptor survives"),
            b"a pinned payload"
        );

        // `into_fd` hands the same descriptor on, and it is still usable.
        let mut file: std::fs::File = object.into_fd().into();
        let mut contents = Vec::new();
        std::io::Read::read_to_end(&mut file, &mut contents).expect("read the handed-on fd");
        assert_eq!(contents, b"a pinned payload");
        (&file).write_all(b"").expect("the fd is still open");
    }
}
