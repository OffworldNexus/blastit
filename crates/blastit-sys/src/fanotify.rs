//! The one filesystem watcher: an unprivileged Linux `fanotify` file-handle
//! group, marked per directory, reporting `(dir, name, mask)`.
//!
//! `blastit dev` reloads what changed, and this module is the only mechanism
//! that says so (`ADR-0014`). It is deliberately **path-agnostic**: it reports
//! the directory entry that changed and nothing about views, routes or the set
//! of files an application loaded — that policy belongs to
//! `crates/blastit/src/dev_reload.rs`. One group, one event stream, one
//! consumer; there is no poll thread, no background task and no async API. The
//! supervisor owns the descriptor and polls it.
//!
//! # What the kernel gives an unprivileged caller
//!
//! Since Linux 5.13 a normal user may create a group, but only one that
//! identifies objects by **file handle**: `fanotify_init` without a
//! `FAN_REPORT_*FID` flag is `EPERM`, `FAN_CLASS_CONTENT` is `EPERM`, and
//! `FAN_MARK_MOUNT`/`FAN_MARK_FILESYSTEM` are `EPERM`. So the flags are
//! [`INIT_FLAGS`] (`FAN_CLOEXEC|FAN_NONBLOCK|FAN_REPORT_DFID_NAME` = `0x0c03`,
//! `O_RDONLY`) and the marks are inode marks with [`MARK_MASK`]
//! (research `docs/research/2026-11-16-dev-fanotify-reload.md` §1.1, §1.6).
//! The same caller always reads `pid == 0` for events it did not cause, so no
//! decision may depend on `pid`.
//!
//! # Why a handle table
//!
//! Every event carries the **parent directory's** file handle plus the entry's
//! name — never a path, and never a usable descriptor. [`Watcher::mark_dir`]
//! records `name_to_handle_at(AT_FDCWD, dir, …, AT_HANDLE_FID)` under
//! [`HandleKey`] (`fsid`, `handle_type`, handle **bytes**), and
//! [`Watcher::read`] looks an event's handle up to recover the directory it was
//! marked from; the caller joins the name. The handle is byte-identical to the
//! event's (§1.4), so no privilege is needed: `open_by_handle_at` is never
//! called and `CAP_DAC_READ_SEARCH` is never required.
//!
//! Three details are load-bearing:
//!
//! * `AT_HANDLE_FID` (Linux 6.5+) is what makes an overlayfs directory
//!   resolvable; plain `name_to_handle_at` is `EOPNOTSUPP` there. On an older
//!   kernel [`Watcher::probe_flags`] answers `0`, and `mark_dir` retries once
//!   with `flags = 0` after `EINVAL` (§1.4.3).
//! * The key is the handle **bytes** plus `handle_type`: `handle_bytes` is 12
//!   on unrelated filesystems, and the same directory can report type `1`
//!   (tmpfs) or `129` (xfs, overlayfs).
//! * The `fsid` is `statfs(2).f_fsid` verbatim. It is *not* `st_dev` and must
//!   not be derived from it (§1.4.2).
//!
//! # Parsing
//!
//! One `read(2)` fills a 16 KiB [`ReadBuffer`] aligned for the event's
//! `__aligned_u64` mask. Events are a chain (`event_len`), each holding an
//! info-record chain (`metadata_len` to `event_len`) walked by `hdr.len`. The
//! name area of a `DFID_NAME` record is `hdr.len - 20 - handle_bytes`, it
//! includes a trailing NUL and is padded to four bytes, so the name stops at
//! the **first NUL** and never at `len` (§1.2). A malformed record is skipped
//! and counted, never indexed past and never a panic: this parser reads
//! kernel-owned bytes at the edge of a trust boundary.
//!
//! `EAGAIN` (the descriptor is non-blocking) and `EINTR` are an empty,
//! non-overflow [`Readout`] — idle, not an error. `EINVAL` from `read` means
//! the buffer was too small for the next event, which is a bug, so it is
//! returned as `EINVAL` and never retried.
//!
//! # Errors
//!
//! Every fallible function returns [`std::io::Result`], the crate's rule, and
//! the errno *is* the classification (`docs/design/dev-fanotify-reload.md`
//! §4.2). Where an errno is not enough to act on, the detail travels *inside*
//! the `io::Error` as a [`WatcherError`] — the same shape [`crate::Error`] uses
//! — so the policy can print one sentence per cause of the failure ladder
//! (§4.7):
//!
//! ```
//! use blastit_sys::fanotify::{Op, WatcherError};
//!
//! const EPERM: i32 = 1;
//! let denied = std::io::Error::from_raw_os_error(EPERM);
//! assert_eq!(
//!     WatcherError::from_io(Op::Init, &denied),
//!     WatcherError::Unavailable { op: Op::Init, errno: EPERM }
//! );
//! let err = std::io::Error::new(
//!     std::io::ErrorKind::PermissionDenied,
//!     WatcherError::from_io(Op::Init, &denied),
//! );
//! assert_eq!(WatcherError::of(&err).map(WatcherError::errno), Some(EPERM));
//! assert_eq!(
//!     WatcherError::of(&err).map(WatcherError::errno_name),
//!     Some("EPERM".to_string())
//! );
//! ```
//!
//! A filesystem without file handles is not an errno the ladder can describe
//! on its own, so it is the one typed error that names something else:
//! [`WatcherError::NoFileHandle`] carries [`filesystem_type`]'s answer.
//!
//! # The production mask
//!
//! [`MARK_MASK`] is `FAN_CLOSE_WRITE|FAN_MOVED_TO|FAN_MOVED_FROM|FAN_CREATE|
//! FAN_DELETE|FAN_ONDIR|FAN_EVENT_ON_CHILD`. `FAN_MODIFY`, `FAN_ACCESS`,
//! `FAN_RENAME`, `FAN_REPORT_TARGET_FID`, `FAN_REPORT_FD_ERROR` and
//! `FAN_CLASS_CONTENT` are deliberately **not** requested: noise, duplicate
//! events, `EPERM`, or a constant `libc 0.2.191` does not define (§1.6).
//!
//! ```
//! use blastit_sys::fanotify::{MARK_MASK, Watcher};
//!
//! assert_eq!(Watcher::mark_mask(), MARK_MASK);
//! assert_ne!(Watcher::mark_mask() & 0x0000_0008, 0); // FAN_CLOSE_WRITE
//! assert_eq!(Watcher::mark_mask() & 0x0000_0002, 0); // FAN_MODIFY: never asked for
//! ```
//!
//! # Example
//!
//! ```no_run
//! use std::path::Path;
//!
//! use blastit_sys::fanotify::{Event, Watcher};
//!
//! # fn main() -> std::io::Result<()> {
//! let mut watcher = Watcher::new()?;
//! watcher.mark_dir(Path::new("/srv/app"))?;
//!
//! let mut events = Vec::new();
//! let readout = watcher.read(&mut events)?;
//! for Event { mask, dir, name } in &events {
//!     println!("{}: {:?} under {}", mask, name, dir.display());
//! }
//! println!("{} events, overflow: {}", readout.events, readout.overflow);
//! # Ok(())
//! # }
//! ```

use std::collections::HashMap;
use std::ffi::{CStr, CString};
use std::io;
use std::mem::{offset_of, size_of};
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, FromRawFd, OwnedFd};
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

/// The group's creation flags: `FAN_CLOEXEC|FAN_NONBLOCK|FAN_REPORT_DFID_NAME`.
///
/// `FAN_REPORT_DFID_NAME` is `FAN_REPORT_DIR_FID|FAN_REPORT_NAME` (`0x0c00`);
/// the two traps are that `0x400` alone yields name-less records and that
/// `FAN_REPORT_NAME` alone is `EINVAL` (research §1.1).
pub const INIT_FLAGS: u32 = libc::FAN_CLOEXEC | libc::FAN_NONBLOCK | libc::FAN_REPORT_DFID_NAME;

/// The one mark mask: a change is a close, a move, a create or a delete.
///
/// `FAN_EVENT_ON_CHILD` is immediate children only, so the caller marks every
/// directory it cares about (§1.5). `FAN_MODIFY` is not here on purpose.
pub const MARK_MASK: u64 = libc::FAN_CLOSE_WRITE
    | libc::FAN_MOVED_TO
    | libc::FAN_MOVED_FROM
    | libc::FAN_CREATE
    | libc::FAN_DELETE
    | libc::FAN_ONDIR
    | libc::FAN_EVENT_ON_CHILD;

/// The size of the one read buffer, in bytes.
///
/// A `DFID_NAME` event is at most `24 + 12 + 8 + 128 + 256` bytes, so 16 KiB
/// holds several of them; `read(2)` returns `EINVAL` if the buffer is smaller
/// than the next event (§1.2.3).
pub const READ_BUF: usize = 16 * 1024;

/// The largest file handle the kernel writes, from `MAX_HANDLE_SZ`.
const HANDLE_MAX: usize = libc::MAX_HANDLE_SZ as usize;

/// The `read(2)` target: a 16 KiB buffer aligned for the event's `u64` mask.
///
/// `read` into `[u8; N]` would be alignment 1 while `fanotify_event_metadata`'s
/// mask is `__aligned_u64`, so this is the only type `read` may fill (§1.2.4).
#[repr(C, align(8))]
pub struct ReadBuffer([u8; READ_BUF]);

impl ReadBuffer {
    /// A zeroed buffer of [`READ_BUF`] bytes, aligned to 8.
    #[must_use]
    pub const fn new() -> Self {
        Self([0; READ_BUF])
    }

    /// The bytes a `read` filled, for [`parse_records`] and for tests.
    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// The buffer as writable bytes, for a caller that reads into it itself.
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        &mut self.0
    }

    fn as_mut_ptr(&mut self) -> *mut u8 {
        self.0.as_mut_ptr()
    }
}

impl Default for ReadBuffer {
    fn default() -> Self {
        Self::new()
    }
}

/// One filesystem change: the mask the kernel reported, and the entry.
///
/// `dir` is the directory that was marked, `name` is the entry's name as the
/// kernel sent it — **bytes**, not a `String`, because a file name need not be
/// UTF-8 and inlining a lossy conversion here would make two different files
/// look like one. Joining is the caller's decision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Event {
    /// The event mask, tested as **bits**: one create-write-close carries
    /// `FAN_CREATE|FAN_CLOSE_WRITE` (`0x108`), never one bit alone (§1.6).
    pub mask: u64,
    /// The marked directory the entry lives in.
    pub dir: PathBuf,
    /// The entry's name, raw bytes, without the kernel's trailing NUL.
    pub name: Vec<u8>,
}

/// What one [`Watcher::read`] (or [`parse_records`]) call produced.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Readout {
    /// How many [`Event`]s this call appended to the caller's vector.
    pub events: usize,
    /// The kernel dropped events (§1.6): either the `FAN_Q_OVERFLOW`
    /// notification arrived, or an event carried no info record at all (the
    /// other way this group says its queue overflowed).
    pub overflow: bool,
    /// Records this call could not use: malformed, or a handle this watcher
    /// never marked. Counted, never logged per record.
    pub skipped: u32,
}

/// Why a record could not be parsed at all.
///
/// The only parse failure that is an error rather than a skip: the buffer is
/// shorter than one event header, so the whole read is unusable and
/// [`Watcher::read`] reports it as `EINVAL` — a bug to fix, not to retry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ParseError {
    /// The buffer does not hold one complete `fanotify_event_metadata`.
    #[error("the read buffer holds less than one fanotify event header")]
    ShortBuffer,
}

/// The syscall a failure came from, for the one warning `dev` prints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum Op {
    /// [`Watcher::new`]: `fanotify_init`.
    Init,
    /// [`Watcher::mark_dir`]: `fanotify_mark`.
    Mark,
    /// [`Watcher::mark_dir`]: `name_to_handle_at` or `statfs`.
    Probe,
    /// [`Watcher::read`]: `read`.
    Read,
}

impl Op {
    /// The syscall's name, as a failure message spells it.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Op::Init => "fanotify_init",
            Op::Mark => "fanotify_mark",
            Op::Probe => "name_to_handle_at",
            Op::Read => "read",
        }
    }
}

impl std::fmt::Display for Op {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The typed detail behind the `io::Error`s this module returns.
///
/// [`WatcherError::Unavailable`] pairs the failed syscall with the errno the
/// failure ladder is written in (`ENOSYS`, `EPERM`, `EINVAL`, `EOPNOTSUPP`,
/// `ENOSPC`, `EMFILE`), and [`WatcherError::NoFileHandle`] names the filesystem
/// that has no file handles at all (9p, some FUSE) — the two cases `dev` turns
/// into one sentence each (§4.7).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum WatcherError {
    /// A syscall of the group failed.
    #[error("{} failed: {}; {}", op.as_str(), errno_symbol(*errno), fix_for(*errno))]
    Unavailable {
        /// The call that failed.
        op: Op,
        /// The errno it reported.
        errno: i32,
    },
    /// The filesystem cannot name its objects by handle, so no event can ever
    /// be resolved to a path.
    #[error(
        "this filesystem has no file handles ({fs_type}); bind-mount the project from the host"
    )]
    NoFileHandle {
        /// [`filesystem_type`]'s answer, or `"unknown"`.
        fs_type: String,
    },
}

impl WatcherError {
    /// Classify a failed syscall: the errno, and which call produced it.
    #[must_use]
    pub fn from_io(op: Op, err: &io::Error) -> Self {
        WatcherError::Unavailable {
            op,
            errno: err.raw_os_error().unwrap_or(libc::EIO),
        }
    }

    /// The typed detail carried inside an `io::Error` this module returned.
    ///
    /// `None` for an error from somewhere else, which is what keeps a caller
    /// from reading an errno that was never classified.
    #[must_use]
    pub fn of(err: &io::Error) -> Option<&Self> {
        err.get_ref().and_then(|inner| inner.downcast_ref::<Self>())
    }

    /// The errno this failure names.
    ///
    /// [`WatcherError::NoFileHandle`] is `EOPNOTSUPP`: unlike a permission or
    /// mark-count failure, the ladder row for "this filesystem has no file
    /// handles" is exactly that errno (§1.7).
    #[must_use]
    pub fn errno(&self) -> i32 {
        match self {
            WatcherError::Unavailable { errno, .. } => *errno,
            WatcherError::NoFileHandle { .. } => libc::EOPNOTSUPP,
        }
    }

    /// The symbolic errno a log event carries: the ladder's names, and
    /// `E<number>` for anything outside it (§4.7).
    #[must_use]
    pub fn errno_name(&self) -> String {
        errno_symbol(self.errno())
    }
}

/// The symbolic name of the errnos the failure ladder names, else `E?`.
fn errno_name(errno: i32) -> &'static str {
    match errno {
        libc::ENOSYS => "ENOSYS",
        libc::EPERM => "EPERM",
        libc::EINVAL => "EINVAL",
        libc::EOPNOTSUPP => "EOPNOTSUPP",
        libc::ENOSPC => "ENOSPC",
        libc::EMFILE => "EMFILE",
        libc::ENOTDIR => "ENOTDIR",
        libc::EACCES => "EACCES",
        libc::ENOENT => "ENOENT",
        libc::EAGAIN => "EAGAIN",
        libc::EINTR => "EINTR",
        libc::EIO => "EIO",
        libc::EBADF => "EBADF",
        _ => "E?",
    }
}

/// A symbolic errno, with `E<number>` for an errno the table does not name.
fn errno_symbol(errno: i32) -> String {
    let name = errno_name(errno);
    if name == "E?" {
        format!("E{errno}")
    } else {
        name.to_string()
    }
}

/// The fix a failure message names, one clause per errno of the ladder.
///
/// The words are the ones `dev` documents (§1.7, §4.7): a user reading the line
/// is told what to change, and never shown a secret or a kernel-internals
/// lecture.
const fn fix_for(errno: i32) -> &'static str {
    match errno {
        libc::ENOSYS => "this kernel has no fanotify; serving without live reload",
        libc::EPERM => {
            "allow fanotify_init in the devcontainer's profile, or run blastit dev on the host"
        }
        libc::EINVAL => {
            "this kernel does not accept the file-handle group; serving without live reload"
        }
        libc::EOPNOTSUPP => "bind-mount the project from the host; serving without live reload",
        libc::ENOSPC => "no marks are left; the directories already marked keep working",
        libc::EMFILE => "no fanotify group is left; close another one and restart",
        libc::ENOTDIR => "mark a directory, not a file",
        _ => "live reload stops here; restart blastit dev",
    }
}

/// The key a marked directory is filed under, and an event's parent handle is
/// looked up by.
///
/// Handles are opaque: they are compared as bytes and never parsed. The type is
/// part of the key because the same directory reports `handle_type` `1` on
/// tmpfs and `129` on xfs/overlayfs, and the bytes alone can collide across
/// unrelated filesystems (`handle_bytes` is 12 on both). `fsid` is
/// `statfs(2).f_fsid`, **never** `st_dev` (§1.4).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct HandleKey {
    /// `statfs(2).f_fsid`, as the two words the kernel reports.
    pub fsid: [u32; 2],
    /// The filesystem's `handle_type` (`1` tmpfs, `129` xfs and overlayfs).
    pub handle_type: i32,
    /// The handle bytes, exactly as the kernel wrote them.
    pub handle: Vec<u8>,
}

/// The stored directory behind one [`HandleKey`].
type DirectoryTable = HashMap<HandleKey, PathBuf>;

/// One parsed event record, before its parent handle is resolved to a path.
struct RawRecord {
    mask: u64,
    key: HandleKey,
    name: Vec<u8>,
}

/// `name_to_handle_at`'s output storage: a `libc::file_handle` followed by a
/// full `MAX_HANDLE_SZ` handle, in one aligned allocation.
///
/// The three fields are laid out exactly as `file_handle` (`handle_bytes` `u32`
/// @0, `handle_type` `i32` @4, payload @8), so the buffer can be handed to the
/// kernel as a `*mut file_handle`; the payload is a real array so the bytes
/// never need unsafe pointer arithmetic to read back. The layout is asserted
/// against `libc::file_handle` in this module's tests.
#[repr(C, align(8))]
struct HandleStorage {
    handle_bytes: u32,
    handle_type: i32,
    handle: [u8; HANDLE_MAX],
}

impl HandleStorage {
    fn new() -> Self {
        Self {
            handle_bytes: HANDLE_MAX as u32,
            handle_type: 0,
            handle: [0; HANDLE_MAX],
        }
    }

    /// The storage as the kernel's `file_handle` pointer.
    ///
    /// The cast is sound because this type is `#[repr(C, align(8))]` with the
    /// same three fields in `file_handle`'s order and offsets (asserted in the
    /// tests); the pointer is only ever dereferenced by `name_to_handle_at`.
    fn as_handle(&mut self) -> *mut libc::file_handle {
        std::ptr::from_mut(self).cast::<libc::file_handle>()
    }

    /// The handle bytes the kernel wrote, or `None` if it wrote a bogus length.
    fn bytes(&self) -> Option<&[u8]> {
        let len = usize::try_from(self.handle_bytes).ok()?;
        if len == 0 || len > HANDLE_MAX {
            return None;
        }
        self.handle.get(..len)
    }
}

/// The unprivileged `fanotify` group and the directory table it resolves with.
///
/// One of these per boot image, held by the `dev` supervisor — never by a view,
/// never by production. `FAN_CLOEXEC` means the descriptor never survives a
/// reload `execve`, so an old generation's group and marks die with its image.
///
/// ```no_run
/// use std::path::Path;
///
/// use blastit_sys::fanotify::Watcher;
///
/// # fn main() -> std::io::Result<()> {
/// let mut watcher = Watcher::new()?;
/// watcher.mark_dir(Path::new("/srv/app"))?;
/// assert_eq!(Watcher::init_flags(), 0x0c03);
/// # Ok(())
/// # }
/// ```
pub struct Watcher {
    group: OwnedFd,
    dirs: DirectoryTable,
    buffer: ReadBuffer,
}

impl std::fmt::Debug for Watcher {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Watcher")
            .field("group", &self.group)
            .field("marked_dirs", &self.dirs.len())
            .finish()
    }
}

impl Watcher {
    /// Create the group: `fanotify_init(0x0c03, O_RDONLY)`.
    ///
    /// Fails with the failure ladder's errno (`ENOSYS` for a kernel without
    /// `CONFIG_FANOTIFY`, `EPERM` for a container profile or a non-FID group).
    pub fn new() -> io::Result<Self> {
        // SAFETY: `fanotify_init` takes two integers and returns a descriptor
        // (or -1); it reads no memory from this process.
        let fd = unsafe { libc::fanotify_init(INIT_FLAGS, libc::O_RDONLY as libc::c_uint) };
        if fd == -1 {
            return Err(classified(Op::Init, io::Error::last_os_error()));
        }
        // SAFETY: `fd` is a descriptor this call just created and no other
        // owner exists, so `OwnedFd` may close it.
        let group = unsafe { OwnedFd::from_raw_fd(fd) };
        Ok(Self {
            group,
            dirs: HashMap::new(),
            buffer: ReadBuffer::new(),
        })
    }

    /// Mark one directory: inode mark, immediate children only.
    ///
    /// Marks **before** probing the handle, so a path that is not a directory
    /// is `ENOTDIR` from `fanotify_mark` and never enters the table. A
    /// directory that is already marked is marked again harmlessly — the kernel
    /// merges the masks and a duplicate `FAN_MARK_ADD` is a no-op (§1.5). The
    /// one exception to "nothing happens on failure": a mark placed before a
    /// probe that then fails stays in place, which is harmless because the
    /// caller treats a probe failure as "reload unavailable" and stops.
    pub fn mark_dir(&mut self, dir: &Path) -> io::Result<()> {
        let path = c_path(dir)?;
        self.mark(&path).map_err(|err| classified(Op::Mark, err))?;
        let key = self.handle_key(dir, &path)?;
        self.dirs.insert(key, dir.to_path_buf());
        Ok(())
    }

    /// Drain what the kernel has queued, appending every resolved event to
    /// `out`.
    ///
    /// `EAGAIN` and `EINTR` are an empty, non-overflow [`Readout`]: idle, not a
    /// failure. An event whose parent handle this watcher never marked is
    /// counted in [`Readout::skipped`] rather than resolved to a wrong path.
    pub fn read(&mut self, out: &mut Vec<Event>) -> io::Result<Readout> {
        let len = self.read_raw()?;
        if len == 0 {
            return Ok(Readout::default());
        }
        let Some(bytes) = self.buffer.as_bytes().get(..len) else {
            // A `read` cannot return more than the buffer holds; refusing the
            // impossible case keeps this from indexing anything.
            return Err(io::Error::from_raw_os_error(libc::EINVAL));
        };
        let mut records = Vec::new();
        let mut readout = walk(bytes, &mut records).map_err(|_| {
            // The whole read is unusable: the buffer was too small for the next
            // event, which is a bug in the buffer size (research §1.2.3).
            io::Error::from_raw_os_error(libc::EINVAL)
        })?;
        let before = out.len();
        for record in records {
            match self.dirs.get(&record.key) {
                Some(dir) => out.push(Event {
                    mask: record.mask,
                    dir: dir.clone(),
                    name: record.name,
                }),
                None => readout.skipped = readout.skipped.saturating_add(1),
            }
        }
        readout.events = out.len().saturating_sub(before);
        Ok(readout)
    }

    /// The group's descriptor, for a supervisor that owns the poll loop.
    #[must_use]
    pub fn fd(&self) -> BorrowedFd<'_> {
        self.group.as_fd()
    }

    /// The flags [`Watcher::new`] uses: `0x0c03`.
    #[must_use]
    pub const fn init_flags() -> u32 {
        INIT_FLAGS
    }

    /// The mask every mark carries: [`MARK_MASK`].
    #[must_use]
    pub const fn mark_mask() -> u64 {
        MARK_MASK
    }

    /// The `name_to_handle_at` flags this kernel understands:
    /// `AT_HANDLE_FID` on Linux 6.5 and later, `0` below it.
    ///
    /// `AT_HANDLE_FID` is the only flag that resolves an overlayfs directory;
    /// on an older kernel `mark_dir` falls back to `flags = 0`, which works on
    /// xfs/ext4/btrfs/tmpfs but not on overlayfs (§1.4.3). A kernel below 5.13
    /// cannot create the group at all, so [`Watcher::new`] has already failed
    /// by the time this matters.
    #[must_use]
    pub fn probe_flags() -> u32 {
        if kernel_at_least(6, 5) {
            libc::AT_HANDLE_FID as u32
        } else {
            0
        }
    }

    /// `read(2)` once; `0` for "nothing queued" (`EAGAIN`/`EINTR`).
    fn read_raw(&mut self) -> io::Result<usize> {
        // SAFETY: the buffer is `READ_BUF` writable bytes for the whole call and
        // the kernel writes at most `count` of them; the descriptor is open and
        // owned by `self`.
        let read = unsafe {
            libc::read(
                self.group.as_raw_fd(),
                self.buffer.as_mut_ptr().cast(),
                READ_BUF,
            )
        };
        if read < 0 {
            let err = io::Error::last_os_error();
            return match err.raw_os_error() {
                Some(libc::EAGAIN) | Some(libc::EINTR) => Ok(0),
                _ => Err(classified(Op::Read, err)),
            };
        }
        Ok(usize::try_from(read).unwrap_or(0))
    }

    /// The one `fanotify_mark` call, hiding the gnu/musl signature split.
    fn mark(&self, path: &CStr) -> io::Result<()> {
        let flags = libc::FAN_MARK_ADD | libc::FAN_MARK_ONLYDIR;
        // SAFETY: the group descriptor is open for `self`'s lifetime, `path` is
        // a valid NUL-terminated string for the whole call, and the kernel
        // copies it rather than retaining the pointer.
        let marked = unsafe {
            libc::fanotify_mark(
                self.group.as_raw_fd(),
                flags,
                MARK_MASK as _,
                libc::AT_FDCWD,
                path.as_ptr(),
            )
        };
        if marked == -1 {
            return Err(io::Error::last_os_error());
        }
        Ok(())
    }

    /// The directory's handle key: probe with [`Watcher::probe_flags`], retry
    /// once with `flags = 0` on `EINVAL`, and statfs for the `fsid`.
    fn handle_key(&self, dir: &Path, path: &CStr) -> io::Result<HandleKey> {
        let flags = Self::probe_flags();
        match self.probe(dir, path, flags as libc::c_int) {
            Ok(key) => Ok(key),
            Err(err) if flags != 0 && err.raw_os_error() == Some(libc::EINVAL) => self
                .probe(dir, path, 0)
                .map_err(|err| classified_watcher(probe_error(dir, err))),
            Err(err) => Err(classified_watcher(probe_error(dir, err))),
        }
    }

    /// One `name_to_handle_at` call plus the `fsid` the event will carry.
    fn probe(&self, dir: &Path, path: &CStr, flags: libc::c_int) -> io::Result<HandleKey> {
        let mut storage = HandleStorage::new();
        let mut mount_id: libc::c_int = 0;
        // SAFETY: `storage` is aligned for `file_handle` and holds its header
        // plus `MAX_HANDLE_SZ` payload bytes (asserted in the tests); `path` is
        // a valid NUL-terminated string; the kernel writes only into the two
        // pointers given, both of which outlive the call.
        let probed = unsafe {
            libc::name_to_handle_at(
                libc::AT_FDCWD,
                path.as_ptr(),
                storage.as_handle(),
                &mut mount_id,
                flags,
            )
        };
        if probed == -1 {
            return Err(io::Error::last_os_error());
        }
        let handle = storage.bytes().ok_or_else(bogus_handle).map(Vec::from)?;
        let fsid = fsid_of(dir).ok_or_else(io::Error::last_os_error)?;
        Ok(HandleKey {
            fsid,
            handle_type: storage.handle_type,
            handle,
        })
    }
}

/// The error for a handle length the kernel could not legitimately write.
fn bogus_handle() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "name_to_handle_at reported a handle outside 1..=MAX_HANDLE_SZ",
    )
}

/// Classify a `name_to_handle_at`/`statfs` failure: a filesystem without
/// handles gets its own typed error and names itself.
fn probe_error(dir: &Path, err: io::Error) -> WatcherError {
    match err.raw_os_error() {
        Some(libc::EOPNOTSUPP) => WatcherError::NoFileHandle {
            fs_type: filesystem_type(dir).unwrap_or("unknown").to_string(),
        },
        _ => WatcherError::from_io(Op::Probe, &err),
    }
}

/// Wrap a syscall error as an `io::Error` carrying its classification.
///
/// The `kind` is kept, because `ErrorKind` is what callers check first; the
/// errno the ladder is written in travels in the [`WatcherError`].
fn classified(op: Op, err: io::Error) -> io::Error {
    classified_watcher(WatcherError::from_io(op, &err))
}

/// Wrap an already-classified failure as an `io::Error`.
fn classified_watcher(watcher: WatcherError) -> io::Error {
    let kind = io::Error::from_raw_os_error(watcher.errno()).kind();
    io::Error::new(kind, watcher)
}

/// A path as the NUL-terminated string the syscalls take.
fn c_path(path: &Path) -> io::Result<CString> {
    CString::new(path.as_os_str().as_bytes()).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{}: contains a NUL byte", path.display()),
        )
    })
}

/// The `statfs(2)` `f_fsid` of `path`, as the two words an event carries.
fn fsid_of(path: &Path) -> Option<[u32; 2]> {
    statfs(path).map(|stat| fsid_words(&stat))
}

/// The two `f_fsid` words of one `statfs(2)` result.
///
/// `libc`'s `fsid_t` keeps its payload in a private `__val` field, so the eight
/// bytes are read through a pointer cast. That is sound because `fsid_t` is
/// `#[repr(C)]` over exactly `[c_int; 2]` — same size, same alignment, same two
/// words — which this module's tests assert. The words are re-read as `u32`
/// because the event reports the identical bits and only the comparison
/// matters.
fn fsid_words(stat: &libc::statfs) -> [u32; 2] {
    // SAFETY: `stat.f_fsid` is a live `fsid_t` (`#[repr(C)]`, 8 bytes, align 4)
    // and `[u32; 2]` has the same size and alignment; `read_unaligned` copies
    // it without assuming an alignment the source does not have.
    unsafe { std::ptr::read_unaligned(std::ptr::addr_of!(stat.f_fsid).cast::<[u32; 2]>()) }
}

/// One `statfs(2)`, or `None` if the call failed.
fn statfs(path: &Path) -> Option<libc::statfs> {
    let c = CString::new(path.as_os_str().as_bytes()).ok()?;
    // SAFETY: `statfs` is plain old data (integers), so an all-zero value is
    // valid, and the kernel fills it before it is read.
    let mut stat: libc::statfs = unsafe { std::mem::zeroed() };
    // SAFETY: `c` is a valid NUL-terminated string and `stat` is valid writable
    // storage for exactly the struct the call expects.
    if unsafe { libc::statfs(c.as_ptr(), &mut stat) } == -1 {
        return None;
    }
    Some(stat)
}

/// The filesystem type behind a `statfs` magic, or `None` when it is not one
/// this table names.
fn fs_name(magic: u64) -> Option<&'static str> {
    Some(match magic {
        0x5846_5342 => "xfs",
        0x794c_7630 => "overlayfs",
        0x0102_1994 => "tmpfs",
        0x0102_1997 => "9p",
        0x0000_ef53 => "ext4",
        0x9123_683e => "btrfs",
        0x0000_6969 => "nfs",
        0x6573_5546 => "fuse",
        0xfe53_4d42 => "smb",
        0xff53_4d42 => "cifs",
        0xf2f5_2010 => "f2fs",
        0x2fc1_2fc1 => "zfs",
        _ => return None,
    })
}

/// The filesystem type of `path`, for the one warning `dev` prints.
///
/// `None` when the path cannot be statfs'd or its magic is not a type this
/// table names; the caller renders `"unknown"` (§4.5).
///
/// ```
/// use std::path::Path;
///
/// use blastit_sys::fanotify::filesystem_type;
///
/// assert_eq!(filesystem_type(Path::new("/this/path/does/not/exist")), None);
/// ```
#[must_use]
pub fn filesystem_type(path: &Path) -> Option<&'static str> {
    let stat = statfs(path)?;
    fs_name(stat.f_type as u64)
}

/// Whether the running kernel is at least `major.minor`.
///
/// Read from `uname(2)`'s release rather than guessed, because the answer
/// decides whether `AT_HANDLE_FID` exists (Linux 6.5+).
fn kernel_at_least(major: u32, minor: u32) -> bool {
    // SAFETY: `utsname` is plain old data, so all-zero is a valid value, and
    // `uname` fills it before it is read.
    let mut uts: libc::utsname = unsafe { std::mem::zeroed() };
    // SAFETY: `uname` takes one writable pointer to a `utsname`.
    if unsafe { libc::uname(&mut uts) } == -1 {
        return false;
    }
    // SAFETY: `uname` filled `release` with a NUL-terminated string, and the
    // borrow ends before `uts` is dropped.
    let release = unsafe { CStr::from_ptr(uts.release.as_ptr()) };
    let release = release.to_string_lossy();
    let mut numbers = release.split('.').filter_map(|part| {
        let digits: String = part.chars().take_while(char::is_ascii_digit).collect();
        digits.parse::<u32>().ok()
    });
    match (numbers.next(), numbers.next()) {
        (Some(found_major), Some(found_minor)) => (found_major, found_minor) >= (major, minor),
        _ => false,
    }
}

/// Read the native-endian bytes of one fixed-width integer at `off`.
fn word_at<const N: usize>(buf: &[u8], off: usize) -> Option<[u8; N]> {
    let bytes = buf.get(off..off.checked_add(N)?)?;
    let mut word = [0u8; N];
    word.copy_from_slice(bytes);
    Some(word)
}

/// Read one native-endian `u16` at `off`, or `None` past the end.
fn u16_at(buf: &[u8], off: usize) -> Option<u16> {
    word_at::<2>(buf, off).map(u16::from_ne_bytes)
}

/// Read one native-endian `u32` at `off`, or `None` past the end.
fn u32_at(buf: &[u8], off: usize) -> Option<u32> {
    word_at::<4>(buf, off).map(u32::from_ne_bytes)
}

/// Read one native-endian `i32` at `off`, or `None` past the end.
fn i32_at(buf: &[u8], off: usize) -> Option<i32> {
    word_at::<4>(buf, off).map(i32::from_ne_bytes)
}

/// Read one native-endian `u64` at `off`, or `None` past the end.
fn u64_at(buf: &[u8], off: usize) -> Option<u64> {
    word_at::<8>(buf, off).map(u64::from_ne_bytes)
}

/// The pure parse seam: walk one `read(2)` result into events.
///
/// No descriptor, no kernel and no directory table: this is the oracle a test
/// can drive with synthetic bytes, and it is exactly what [`Watcher::read`]
/// walks. Every record is bounded by `event_len` before it is sliced, a
/// malformed record bumps [`Readout::skipped`] and the walk continues at the
/// next event, and a record whose handle cannot be resolved here (there is no
/// table) is still returned — its [`Event::dir`] is **empty**, and
/// `Watcher::read` is what fills it in.
///
/// Only a buffer shorter than one event header is [`ParseError::ShortBuffer`].
///
/// ```
/// use blastit_sys::fanotify::{parse_records, Readout};
///
/// let mut events = Vec::new();
/// assert_eq!(parse_records(&[], &mut events), Ok(Readout::default()));
/// assert!(events.is_empty());
/// ```
pub fn parse_records(buf: &[u8], out: &mut Vec<Event>) -> Result<Readout, ParseError> {
    let mut records = Vec::new();
    let mut readout = walk(buf, &mut records)?;
    let before = out.len();
    out.extend(records.into_iter().map(|record| Event {
        mask: record.mask,
        dir: PathBuf::new(),
        name: record.name,
    }));
    readout.events = out.len().saturating_sub(before);
    Ok(readout)
}

/// Walk the metadata chain, collecting one raw record per resolvable event.
fn walk(buf: &[u8], records: &mut Vec<RawRecord>) -> Result<Readout, ParseError> {
    let event_len_at = offset_of!(libc::fanotify_event_metadata, event_len);
    let metadata_len_at = offset_of!(libc::fanotify_event_metadata, metadata_len);
    let mask_at = offset_of!(libc::fanotify_event_metadata, mask);
    let mut readout = Readout::default();
    let mut offset = 0usize;
    while let Some(rest) = buf.get(offset..) {
        if rest.is_empty() {
            break;
        }
        if rest.len() < size_of::<libc::fanotify_event_metadata>() {
            return Err(ParseError::ShortBuffer);
        }
        let Some(event_len) = u32_at(rest, event_len_at) else {
            return Err(ParseError::ShortBuffer);
        };
        let event_len = event_len as usize;
        let Some(metadata_len) = u16_at(rest, metadata_len_at) else {
            return Err(ParseError::ShortBuffer);
        };
        let metadata_len = usize::from(metadata_len);
        let Some(mask) = u64_at(rest, mask_at) else {
            return Err(ParseError::ShortBuffer);
        };
        if metadata_len < size_of::<libc::fanotify_event_metadata>()
            || event_len < metadata_len
            || event_len > rest.len()
        {
            // The chain cannot be advanced from here: count this event as
            // unusable and stop rather than guess an offset.
            readout.skipped = readout.skipped.saturating_add(1);
            break;
        }
        // Two signals mean "events were dropped" (§1.6): the `FAN_Q_OVERFLOW`
        // notification, and an event with **no info record at all**
        // (`metadata_len == event_len`, which the kernel sends with
        // `fd == FAN_NOFD`). A `FAN_REPORT_FID` group attaches a fid record to
        // every event it reports, so a record with no info area is the kernel
        // saying the queue overflowed — not a name-less event to skip.
        if mask & libc::FAN_Q_OVERFLOW != 0 || metadata_len == event_len {
            readout.overflow = true;
            offset += event_len;
            continue;
        }
        match walk_info(rest, metadata_len, event_len, mask) {
            Some(record) => records.push(record),
            None => readout.skipped = readout.skipped.saturating_add(1),
        }
        offset += event_len;
    }
    readout.events = records.len();
    Ok(readout)
}

/// Walk one event's info-record chain for its `DFID_NAME` record.
fn walk_info(buf: &[u8], metadata_len: usize, event_len: usize, mask: u64) -> Option<RawRecord> {
    let info_type_at = offset_of!(libc::fanotify_event_info_header, info_type);
    let info_len_at = offset_of!(libc::fanotify_event_info_header, len);
    let header_size = size_of::<libc::fanotify_event_info_header>();
    let mut info = metadata_len;
    let mut found: Option<(HandleKey, Vec<u8>)> = None;
    loop {
        if event_len.checked_sub(info)? < header_size {
            break;
        }
        let header_len = usize::from(u16_at(buf, info.checked_add(info_len_at)?)?);
        if header_len < header_size {
            break;
        }
        let info_end = info.checked_add(header_len)?;
        if info_end > event_len {
            break;
        }
        let info_type = *buf.get(info.checked_add(info_type_at)?)?;
        if info_type == libc::FAN_EVENT_INFO_TYPE_DFID_NAME && found.is_none() {
            found = parse_fid(buf, info, header_len);
        }
        info = info_end;
    }
    let (key, name) = found?;
    Some(RawRecord { mask, key, name })
}

/// Parse one `DFID_NAME` info record: the parent handle and the entry's name.
///
/// The name area is `hdr.len - 20 - handle_bytes` bytes (the `20` is the fid
/// header's 12 bytes plus `file_handle`'s 8) and it must hold a NUL; the name
/// stops at the **first** NUL and never at `hdr.len` (research §1.2.1).
fn parse_fid(buf: &[u8], info: usize, header_len: usize) -> Option<(HandleKey, Vec<u8>)> {
    let fid_end = info.checked_add(header_len)?;
    let fid = buf.get(info..fid_end)?;
    let fsid_at = offset_of!(libc::fanotify_event_info_fid, fsid);
    let handle_at = offset_of!(libc::fanotify_event_info_fid, handle);
    let bytes_at = handle_at.checked_add(offset_of!(libc::file_handle, handle_bytes))?;
    let type_at = handle_at.checked_add(offset_of!(libc::file_handle, handle_type))?;
    let payload_at = handle_at.checked_add(offset_of!(libc::file_handle, f_handle))?;
    if header_len < payload_at {
        return None;
    }
    let fsid = [u32_at(fid, fsid_at)?, u32_at(fid, fsid_at.checked_add(4)?)?];
    let handle_len = usize::try_from(u32_at(fid, bytes_at)?).ok()?;
    let handle_type = i32_at(fid, type_at)?;
    if handle_len == 0 || handle_len > HANDLE_MAX {
        return None;
    }
    let name_at = payload_at.checked_add(handle_len)?;
    if name_at > header_len {
        // The name area would start past the record: a short name area.
        return None;
    }
    let handle = fid.get(payload_at..name_at)?.to_vec();
    let name_area = fid.get(name_at..header_len)?;
    let nul = name_area.iter().position(|byte| *byte == 0)?;
    let name = name_area.get(..nul)?.to_vec();
    if name.is_empty() {
        return None;
    }
    Some((
        HandleKey {
            fsid,
            handle_type,
            handle,
        },
        name,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Write `bytes` at `at`, reporting whether the range was in bounds.
    fn poke(buf: &mut [u8], at: usize, bytes: &[u8]) -> bool {
        let Some(end) = at.checked_add(bytes.len()) else {
            return false;
        };
        match buf.get_mut(at..end) {
            Some(slot) => {
                slot.copy_from_slice(bytes);
                true
            }
            None => false,
        }
    }

    /// The offsets the parser assumes, checked against `libc`'s own structs.
    #[test]
    fn the_measured_layout_is_libcs_layout() {
        assert_eq!(size_of::<libc::fanotify_event_metadata>(), 24);
        assert_eq!(offset_of!(libc::fanotify_event_metadata, event_len), 0);
        assert_eq!(offset_of!(libc::fanotify_event_metadata, metadata_len), 6);
        assert_eq!(offset_of!(libc::fanotify_event_metadata, mask), 8);
        assert_eq!(offset_of!(libc::fanotify_event_metadata, fd), 16);
        assert_eq!(offset_of!(libc::fanotify_event_metadata, pid), 20);

        assert_eq!(size_of::<libc::fanotify_event_info_header>(), 4);
        assert_eq!(offset_of!(libc::fanotify_event_info_header, info_type), 0);
        assert_eq!(offset_of!(libc::fanotify_event_info_header, len), 2);

        assert_eq!(size_of::<libc::fanotify_event_info_fid>(), 12);
        assert_eq!(offset_of!(libc::fanotify_event_info_fid, fsid), 4);
        assert_eq!(offset_of!(libc::fanotify_event_info_fid, handle), 12);

        assert_eq!(size_of::<libc::file_handle>(), 8);
        assert_eq!(offset_of!(libc::file_handle, handle_bytes), 0);
        assert_eq!(offset_of!(libc::file_handle, handle_type), 4);
        assert_eq!(offset_of!(libc::file_handle, f_handle), 8);

        // `fsid_words` reads `f_fsid` as two words: the types must agree.
        assert_eq!(size_of::<libc::fsid_t>(), size_of::<[u32; 2]>());
        assert_eq!(
            std::mem::align_of::<libc::fsid_t>(),
            std::mem::align_of::<[u32; 2]>()
        );

        // The storage handed to `name_to_handle_at` is a `file_handle` plus a
        // full payload, so the two field offsets must agree.
        assert_eq!(size_of::<HandleStorage>(), 8 + HANDLE_MAX);
        assert_eq!(
            offset_of!(HandleStorage, handle_bytes),
            offset_of!(libc::file_handle, handle_bytes)
        );
        assert_eq!(
            offset_of!(HandleStorage, handle_type),
            offset_of!(libc::file_handle, handle_type)
        );
        assert_eq!(
            offset_of!(HandleStorage, handle),
            offset_of!(libc::file_handle, f_handle)
        );
        assert_eq!(std::mem::align_of::<HandleStorage>(), 8);
    }

    #[test]
    fn read_buffer_is_aligned_and_sixteen_kib() {
        assert_eq!(size_of::<ReadBuffer>(), 16 * 1024);
        assert_eq!(std::mem::align_of::<ReadBuffer>(), 8);
        assert_eq!(ReadBuffer::new().as_bytes().len(), READ_BUF);
    }

    #[test]
    fn init_flags_and_mark_mask_are_the_measured_values() {
        assert_eq!(INIT_FLAGS, 0x0c03);
        assert_eq!(
            INIT_FLAGS,
            libc::FAN_CLOEXEC | libc::FAN_NONBLOCK | libc::FAN_REPORT_DFID_NAME
        );
        assert_eq!(Watcher::init_flags(), INIT_FLAGS);
        assert_eq!(Watcher::mark_mask(), MARK_MASK);

        for bit in [
            libc::FAN_CLOSE_WRITE,
            libc::FAN_MOVED_TO,
            libc::FAN_MOVED_FROM,
            libc::FAN_CREATE,
            libc::FAN_DELETE,
            libc::FAN_ONDIR,
            libc::FAN_EVENT_ON_CHILD,
        ] {
            assert_ne!(MARK_MASK & bit, 0, "MARK_MASK is missing {bit:#x}");
        }
        // Never requested: noise, duplicates, or a bit that needs
        // `FAN_CLASS_CONTENT` (which an unprivileged caller cannot have).
        for bit in [
            libc::FAN_MODIFY,
            libc::FAN_ACCESS,
            libc::FAN_OPEN,
            libc::FAN_CLOSE_NOWRITE,
            libc::FAN_Q_OVERFLOW,
            libc::FAN_RENAME,
        ] {
            assert_eq!(MARK_MASK & bit, 0, "MARK_MASK must not ask for {bit:#x}");
        }
    }

    #[test]
    fn the_failure_ladder_maps_every_errno_to_its_op() {
        for (errno, op) in [
            (libc::ENOSYS, Op::Init),
            (libc::EPERM, Op::Init),
            (libc::EINVAL, Op::Mark),
            (libc::EOPNOTSUPP, Op::Probe),
            (libc::ENOSPC, Op::Mark),
            (libc::EMFILE, Op::Init),
            (libc::ENOTDIR, Op::Mark),
        ] {
            let denied = io::Error::from_raw_os_error(errno);
            assert_eq!(
                WatcherError::from_io(op, &denied),
                WatcherError::Unavailable { op, errno },
                "errno {errno} did not classify as {op}"
            );
        }
        // An error without an errno is `EIO`, never a panic.
        assert_eq!(
            WatcherError::from_io(Op::Read, &io::Error::other("no errno")),
            WatcherError::Unavailable {
                op: Op::Read,
                errno: libc::EIO
            }
        );
    }

    #[test]
    fn a_classified_error_keeps_its_errno_and_kind() {
        for errno in [
            libc::ENOSYS,
            libc::EPERM,
            libc::EINVAL,
            libc::EOPNOTSUPP,
            libc::ENOSPC,
        ] {
            let raw = io::Error::from_raw_os_error(errno);
            let expected = raw.kind();
            let wrapped = classified(Op::Init, raw);
            assert_eq!(
                WatcherError::of(&wrapped).map(WatcherError::errno),
                Some(errno)
            );
            assert_eq!(
                wrapped.kind(),
                expected,
                "the ErrorKind callers check must survive classification"
            );
            assert_eq!(
                WatcherError::of(&wrapped).map(WatcherError::errno_name),
                Some(errno_symbol(errno))
            );
            assert_eq!(
                WatcherError::of(&wrapped).map(ToString::to_string),
                Some(format!(
                    "fanotify_init failed: {}; {}",
                    errno_symbol(errno),
                    fix_for(errno)
                ))
            );
        }
        // A foreign error carries no classification, so no caller can read an
        // errno that was never one.
        assert!(WatcherError::of(&io::Error::other("elsewhere")).is_none());
        // Anything outside the ladder renders `E<number>`.
        let oddball = classified(Op::Read, io::Error::from_raw_os_error(999));
        assert_eq!(
            WatcherError::of(&oddball).map(WatcherError::errno_name),
            Some("E999".to_string())
        );
    }

    #[test]
    fn a_filesystem_without_handles_names_its_type() {
        let dir = std::env::temp_dir();
        let named = probe_error(&dir, io::Error::from_raw_os_error(libc::EOPNOTSUPP));
        match &named {
            WatcherError::NoFileHandle { fs_type } => {
                assert_eq!(
                    Some(fs_type.as_str()),
                    filesystem_type(&dir),
                    "the type must be the filesystem's own answer"
                );
                assert!(!fs_type.is_empty());
            }
            other => panic!("EOPNOTSUPP from the probe must be NoFileHandle, got {other:?}"),
        }
        assert_eq!(named.errno(), libc::EOPNOTSUPP);
        assert_eq!(named.errno_name(), "EOPNOTSUPP");
        // The message names the filesystem and the fix, so `dev` can print it.
        let message = named.to_string();
        assert!(message.contains("no file handles"), "{message}");
        let fs_type = filesystem_type(&dir).unwrap_or("unknown");
        assert!(message.contains(fs_type), "{message} must name {fs_type}");
        // A missing syscall is not "no handles": it is the errno.
        assert_eq!(
            probe_error(&dir, io::Error::from_raw_os_error(libc::ENOSYS)),
            WatcherError::Unavailable {
                op: Op::Probe,
                errno: libc::ENOSYS
            }
        );
        // Nor is a permission failure.
        assert_eq!(
            probe_error(&dir, io::Error::from_raw_os_error(libc::EACCES)),
            WatcherError::Unavailable {
                op: Op::Probe,
                errno: libc::EACCES
            }
        );
    }

    #[test]
    fn filesystem_magics_are_named_and_unknown_ones_are_not() {
        assert_eq!(fs_name(0x0102_1994), Some("tmpfs"));
        assert_eq!(fs_name(0x794c_7630), Some("overlayfs"));
        assert_eq!(fs_name(0x5846_5342), Some("xfs"));
        assert_eq!(fs_name(0x0102_1997), Some("9p"));
        assert_eq!(fs_name(0x6573_5546), Some("fuse"));
        assert_eq!(fs_name(0x0000_ef53), Some("ext4"));
        assert_eq!(fs_name(0xdead_beef), None);
        assert_eq!(filesystem_type(Path::new("/no/such/path/at/all")), None);
    }

    #[test]
    fn handle_keys_compare_bytes_and_type() {
        let base = HandleKey {
            fsid: [0xd37f_9e70, 0x54b3_e9ce],
            handle_type: 1,
            handle: vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12],
        };
        let same = base.clone();
        let other_type = HandleKey {
            handle_type: 129,
            ..base.clone()
        };
        let other_bytes = HandleKey {
            handle: vec![12, 11, 10, 9, 8, 7, 6, 5, 4, 3, 2, 1],
            ..base.clone()
        };
        let other_fsid = HandleKey {
            fsid: [0, 0],
            ..base.clone()
        };
        assert_eq!(base, same);
        assert_ne!(base, other_type);
        assert_ne!(base, other_bytes);
        assert_ne!(base, other_fsid);
        // Equal `handle_bytes` never collide: the keys are the bytes.
        assert_ne!(other_type, other_bytes);

        let mut set = std::collections::HashSet::new();
        assert!(set.insert(base));
        assert!(!set.insert(same));
        assert!(set.insert(other_type));
        assert!(set.insert(other_bytes));
        assert_eq!(set.len(), 3);
    }

    /// Build one `DFID_NAME` event exactly as the kernel lays it out.
    fn event(
        mask: u64,
        fsid: [u32; 2],
        handle_type: i32,
        handle: &[u8],
        name: &[u8],
        with_nul: bool,
    ) -> Vec<u8> {
        let payload = offset_of!(libc::file_handle, f_handle);
        let name_bytes = if with_nul {
            let mut padded = name.to_vec();
            padded.push(0);
            while !padded.len().is_multiple_of(4) {
                padded.push(0);
            }
            padded
        } else {
            name.to_vec()
        };
        let header_len = 12 + payload + handle.len() + name_bytes.len();
        let event_len = 24 + header_len;
        let mut buf = Vec::new();
        buf.extend_from_slice(&(event_len as u32).to_ne_bytes());
        buf.push(libc::FANOTIFY_METADATA_VERSION);
        buf.push(0);
        buf.extend_from_slice(&24u16.to_ne_bytes());
        buf.extend_from_slice(&mask.to_ne_bytes());
        buf.extend_from_slice(&libc::FAN_NOFD.to_ne_bytes());
        buf.extend_from_slice(&0i32.to_ne_bytes());
        assert_eq!(buf.len(), 24);
        buf.push(libc::FAN_EVENT_INFO_TYPE_DFID_NAME);
        buf.push(0);
        buf.extend_from_slice(&(header_len as u16).to_ne_bytes());
        for word in fsid {
            buf.extend_from_slice(&word.to_ne_bytes());
        }
        buf.extend_from_slice(&(handle.len() as u32).to_ne_bytes());
        buf.extend_from_slice(&handle_type.to_ne_bytes());
        buf.extend_from_slice(handle);
        buf.extend_from_slice(&name_bytes);
        assert_eq!(buf.len(), event_len);
        buf
    }

    /// One well-formed event with a 12-byte handle.
    fn one(mask: u64, name: &[u8]) -> Vec<u8> {
        event(
            mask,
            [1, 2],
            1,
            &[9, 8, 7, 6, 5, 4, 3, 2, 1, 0, 1, 2],
            name,
            true,
        )
    }

    #[test]
    fn parse_records_reads_the_name_to_the_first_nul_and_stops_at_padding() {
        let mut out = Vec::new();
        let readout = parse_records(&one(0x108, b"x.php"), &mut out);
        assert_eq!(
            readout,
            Ok(Readout {
                events: 1,
                overflow: false,
                skipped: 0
            })
        );
        assert_eq!(out.len(), 1);
        if let Some(event) = out.first() {
            assert_eq!(event.mask & libc::FAN_CLOSE_WRITE, libc::FAN_CLOSE_WRITE);
            assert_eq!(event.mask & libc::FAN_CREATE, libc::FAN_CREATE);
            assert_eq!(event.name, b"x.php");
            assert_eq!(event.dir, PathBuf::new());
        }
    }

    #[test]
    fn parse_records_keeps_a_name_whose_padding_collapses() {
        // `a.py` is four bytes plus a NUL: no padding at all, and the name must
        // still stop at its own NUL rather than at `hdr.len`.
        let mut out = Vec::new();
        let readout = parse_records(&one(0x8, b"a.py"), &mut out);
        assert_eq!(readout.map(|readout| readout.events), Ok(1));
        assert_eq!(
            out.first().map(|event| event.name.as_slice()),
            Some(&b"a.py"[..])
        );
    }

    #[test]
    fn parse_records_skips_malformed_records_and_keeps_the_next_event() {
        let good = one(0x8, b"good.py");
        let header_len_at = 24 + offset_of!(libc::fanotify_event_info_header, len);

        // `hdr.len` beyond `event_len`.
        let mut too_long = good.clone();
        assert!(poke(&mut too_long, header_len_at, &u16::MAX.to_ne_bytes()));
        assert_malformed_then_good(&too_long, &good);

        // A short name area: `hdr.len` smaller than 20 + handle bytes.
        let mut short = good.clone();
        assert!(poke(&mut short, header_len_at, &20u16.to_ne_bytes()));
        assert_malformed_then_good(&short, &good);

        // A missing NUL: the name area runs to `hdr.len` with no terminator.
        let no_nul = event(0x8, [1, 2], 1, &[0; 12], b"no-nul", false);
        assert_malformed_then_good(&no_nul, &good);

        // No `DFID_NAME` record at all: a name-less record cannot be placed.
        let mut unnamed = good.clone();
        let info_type_at = 24 + offset_of!(libc::fanotify_event_info_header, info_type);
        assert!(poke(
            &mut unnamed,
            info_type_at,
            &[libc::FAN_EVENT_INFO_TYPE_DFID]
        ));
        assert_malformed_then_good(&unnamed, &good);

        // A handle length outside 1..=MAX_HANDLE_SZ.
        let mut bogus = good.clone();
        let handle_bytes_at = 24 + 12 + offset_of!(libc::file_handle, handle_bytes);
        assert!(poke(&mut bogus, handle_bytes_at, &u32::MAX.to_ne_bytes()));
        assert_malformed_then_good(&bogus, &good);

        // `metadata_len` claiming an info chain longer than its own event: the
        // walk cannot advance, so the event is skipped and parsing stops.
        let mut bad_meta = good.clone();
        assert!(poke(&mut bad_meta, 6, &4096u16.to_ne_bytes()));
        let mut out = Vec::new();
        let readout = parse_records(&bad_meta, &mut out);
        assert_eq!(
            readout,
            Ok(Readout {
                events: 0,
                overflow: false,
                skipped: 1
            })
        );
        assert!(out.is_empty());

        // A chain that ends before its own next event.
        let mut truncated = good.clone();
        truncated.truncate(good.len() - 4);
        let mut out = Vec::new();
        let readout = parse_records(&truncated, &mut out);
        assert_eq!(readout.map(|readout| readout.skipped), Ok(1));
        assert!(out.is_empty());
    }

    /// `buf` (one malformed event) followed by `good` must skip the first and
    /// parse the second.
    fn assert_malformed_then_good(buf: &[u8], good: &[u8]) {
        let mut both = buf.to_vec();
        both.extend_from_slice(good);
        let mut out = Vec::new();
        let readout = parse_records(&both, &mut out);
        assert_eq!(
            readout.map(|readout| (readout.events, readout.skipped)),
            Ok((1, 1)),
            "the malformed event must be skipped and the valid one parsed"
        );
        assert_eq!(
            out.first().map(|event| event.name.as_slice()),
            Some(&b"good.py"[..])
        );
    }

    #[test]
    fn parse_records_reports_overflow_and_a_short_buffer() {
        // `FAN_Q_OVERFLOW` carries no info record: events were dropped.
        let mut overflow = one(0x8, b"x.py");
        overflow.truncate(24);
        assert!(poke(&mut overflow, 0, &24u32.to_ne_bytes()));
        assert!(poke(&mut overflow, 6, &24u16.to_ne_bytes()));
        assert!(poke(&mut overflow, 8, &libc::FAN_Q_OVERFLOW.to_ne_bytes()));
        let mut out = Vec::new();
        let readout = parse_records(&overflow, &mut out);
        assert_eq!(
            readout,
            Ok(Readout {
                events: 0,
                overflow: true,
                skipped: 0
            })
        );
        assert!(out.is_empty());

        // Fewer bytes than one header: the whole read is unusable.
        assert_eq!(
            parse_records(&[0u8; 8], &mut Vec::new()),
            Err(ParseError::ShortBuffer)
        );
        // A complete event followed by a partial header is unusable too.
        let mut trailing = one(0x8, b"x.py");
        trailing.extend_from_slice(&[0u8; 8]);
        assert_eq!(
            parse_records(&trailing, &mut Vec::new()),
            Err(ParseError::ShortBuffer)
        );
        // Empty is simply idle.
        assert_eq!(parse_records(&[], &mut Vec::new()), Ok(Readout::default()));
    }

    #[test]
    fn parse_records_counts_a_record_without_an_info_area_as_overflow() {
        // §1.6's second signal: the kernel reports a queue overflow as an
        // event with **no** info record at all (`metadata_len == event_len`,
        // `fd == FAN_NOFD`) and no `FAN_Q_OVERFLOW` bit. That is dropped
        // events, not a name-less record to skip.
        let mut nameless = one(0x8, b"x.py");
        nameless.truncate(24);
        assert!(poke(&mut nameless, 0, &24u32.to_ne_bytes()));
        assert!(poke(&mut nameless, 6, &24u16.to_ne_bytes()));
        // `one` already writes `FAN_NOFD` into `fd`; only the info area is gone.
        let mut out = Vec::new();
        assert_eq!(
            parse_records(&nameless, &mut out),
            Ok(Readout {
                events: 0,
                overflow: true,
                skipped: 0
            }),
            "a record with no info area is the kernel saying events were dropped"
        );
        assert!(out.is_empty());

        // Both signals in one read: the `FAN_Q_OVERFLOW` notification and the
        // name-less record each mark the readout, and neither is `skipped`.
        let mut both = one(0x8, b"x.py");
        both.truncate(24);
        assert!(poke(&mut both, 0, &24u32.to_ne_bytes()));
        assert!(poke(&mut both, 6, &24u16.to_ne_bytes()));
        assert!(poke(&mut both, 8, &libc::FAN_Q_OVERFLOW.to_ne_bytes()));
        both.extend_from_slice(&nameless);
        let mut out = Vec::new();
        assert_eq!(
            parse_records(&both, &mut out),
            Ok(Readout {
                events: 0,
                overflow: true,
                skipped: 0
            })
        );
        assert!(out.is_empty());
    }

    #[test]
    fn parse_records_keeps_a_non_utf8_name_as_bytes() {
        let raw = one(0x8, b"caf\xe9.py");
        let mut out = Vec::new();
        assert_eq!(
            parse_records(&raw, &mut out).map(|readout| readout.events),
            Ok(1)
        );
        assert_eq!(
            out.first().map(|event| event.name.clone()),
            Some(b"caf\xe9.py".to_vec())
        );
    }

    #[test]
    fn kernel_version_parsing_matches_this_kernel() {
        // Whatever this host runs, `probe_flags` must agree with its release.
        let flags = Watcher::probe_flags();
        assert!(
            flags == libc::AT_HANDLE_FID as u32 || flags == 0,
            "probe_flags answered {flags:#x}"
        );
        assert_eq!(
            flags == libc::AT_HANDLE_FID as u32,
            kernel_at_least(6, 5),
            "probe_flags and the kernel release must agree"
        );
    }
}
