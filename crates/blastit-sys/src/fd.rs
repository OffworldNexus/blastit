//! File-descriptor hygiene: `FD_CLOEXEC` and closing everything a child must
//! not inherit.

use std::io;
use std::os::fd::{AsRawFd, BorrowedFd, OwnedFd, RawFd};
use std::path::Path;

/// Whether `FD_CLOEXEC` is set on `fd`.
pub fn fd_is_cloexec(fd: BorrowedFd<'_>) -> io::Result<bool> {
    // SAFETY: `F_GETFD` takes no pointer and only reads descriptor flags.
    let flags = unsafe { libc::fcntl(fd.as_raw_fd(), libc::F_GETFD) };
    if flags == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(flags & libc::FD_CLOEXEC != 0)
}

/// Set or clear `FD_CLOEXEC` on `fd`, returning whether it was set before.
///
/// Every descriptor this crate creates is already close-on-exec. This is for
/// the deliberate exceptions: `secrets.pipe` has to survive dbproxy's `execve`
/// (§4), so init clears the flag on the one end that must be inherited.
pub fn set_cloexec(fd: BorrowedFd<'_>, cloexec: bool) -> io::Result<bool> {
    let raw = fd.as_raw_fd();
    // SAFETY: `F_GETFD` takes no pointer and only reads descriptor flags.
    let flags = unsafe { libc::fcntl(raw, libc::F_GETFD) };
    if flags == -1 {
        return Err(io::Error::last_os_error());
    }
    let mut new_flags = flags & !libc::FD_CLOEXEC;
    if cloexec {
        new_flags |= libc::FD_CLOEXEC;
    }
    // SAFETY: `F_SETFD` sets the descriptor flags of an open descriptor; the
    // only bit we ever pass is `FD_CLOEXEC`.
    if unsafe { libc::fcntl(raw, libc::F_SETFD, new_flags) } == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(flags & libc::FD_CLOEXEC != 0)
}

/// Open `path` as a directory descriptor: read-only, no symlink followed, close
/// on exec.
///
/// Everything that acts on what is *inside* the directory then does it relative
/// to this descriptor (`openat`), so a symlink swapped in under `path` between
/// the open and the use cannot make the caller touch something else.
pub fn open_dir_nofollow(path: &Path) -> io::Result<OwnedFd> {
    rustix::fs::open(
        path,
        rustix::fs::OFlags::RDONLY
            | rustix::fs::OFlags::DIRECTORY
            | rustix::fs::OFlags::CLOEXEC
            | rustix::fs::OFlags::NOFOLLOW,
        rustix::fs::Mode::empty(),
    )
    .map_err(io::Error::from)
}

/// `dup2(fd, target)`: put a copy of `fd` on descriptor number `target`,
/// **without** `FD_CLOEXEC`.
///
/// This is the one place in blastit where a descriptor deliberately does not
/// carry `FD_CLOEXEC`, and it exists for descriptors 0, 1 and 2 only: a child's
/// stdin, stdout and stderr are the log pipes (§8.1) and, for dbproxy, the
/// secrets pipe, and every one of those has to survive the child's own
/// `execve`. Handing them over with `dup_onto(pipe_write_end, 1)` puts them
/// where a program expects to find them and leaves the flag clear, so the
/// `execve` does not take them away.
///
/// Whatever was on `target` is closed first — that is how a child replaces the
/// stdin it inherited from init. `target` must be a valid descriptor number;
/// `fd` and `target` may be the same number, in which case nothing happens.
///
/// Nothing else in the child should keep a descriptor without `FD_CLOEXEC`:
/// every other descriptor a child owns is either inherited deliberately by
/// number or closed with [`close_fds_except`].
pub fn dup_onto(fd: BorrowedFd<'_>, target: RawFd) -> io::Result<()> {
    if target < 0 {
        return Err(io::Error::from_raw_os_error(libc::EBADF));
    }
    // SAFETY: `dup2` takes two descriptor numbers. It closes `target` first if
    // it happens to be open, and returns it; no memory is touched.
    if unsafe { libc::dup2(fd.as_raw_fd(), target) } == -1 {
        return Err(io::Error::last_os_error());
    }
    // `dup2` clears `FD_CLOEXEC` on the new descriptor, and clearing it again is
    // free; the point is that the caller can rely on the flag being absent.
    // SAFETY: `dup2` just returned `target`, so it is a live descriptor.
    let target = unsafe { BorrowedFd::borrow_raw(target) };
    set_cloexec(target, false)?;
    if fd_is_cloexec(target)? {
        return Err(io::Error::other(
            "dup_onto left FD_CLOEXEC set on the target descriptor",
        ));
    }
    Ok(())
}

/// Close every descriptor at or above 3 that is not listed in `keep`.
///
/// Newly forked children must close everything they do not own — a view
/// inherits its own `view.chan` end and its log pipes, and nothing else, so it
/// never gets the listening socket, `http.chan`, `ctl.chan` or another view's
/// channel (§3).
///
/// `close_range(2)` is used when the kernel has it (Linux 5.9, and gVisor);
/// otherwise the descriptor list is read from `/proc/self/fd` and the
/// descriptors are closed one by one, after collecting them all. Descriptors
/// 0, 1 and 2 are never touched.
pub fn close_fds_except(keep: &[RawFd]) -> io::Result<()> {
    match close_range_except(keep) {
        Ok(()) => Ok(()),
        // ENOSYS: no `close_range` at all. EINVAL: `close_range` exists but
        // rejected an argument, which leaves the job half done — either way the
        // portable path finishes it, and closing an already-closed descriptor
        // is not an error.
        Err(err) if matches!(err.raw_os_error(), Some(libc::ENOSYS) | Some(libc::EINVAL)) => {
            close_fds_except_scan(keep)
        }
        Err(err) => Err(err),
    }
}

/// The portable half of [`close_fds_except`]: collect `/proc/self/fd`, then close.
///
/// Exposed because kernels without `close_range` (and the tests) need it
/// directly; [`close_fds_except`] picks it on its own when it has to.
pub fn close_fds_except_scan(keep: &[RawFd]) -> io::Result<()> {
    let mut to_close = Vec::new();
    for entry in std::fs::read_dir("/proc/self/fd")? {
        let entry = entry?;
        // The descriptor that is reading the directory is itself in the list;
        // it is collected like any other and closed after the scan, when the
        // directory handle has been dropped.
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        let Ok(fd) = name.parse::<RawFd>() else {
            continue;
        };
        if fd >= 3 && !keep.contains(&fd) {
            to_close.push(fd);
        }
    }
    for fd in to_close {
        // SAFETY: `close` takes a descriptor number. A descriptor that is
        // already closed reports `EBADF`, which is not an error here: the
        // postcondition is "not open", and it holds either way.
        if unsafe { libc::close(fd) } == -1 {
            let err = io::Error::last_os_error();
            if err.raw_os_error() != Some(libc::EBADF) {
                return Err(err);
            }
        }
    }
    Ok(())
}

/// `close_range` over the gaps between the kept descriptors.
///
/// Walks the kept descriptors in increasing order (without allocating: init
/// calls this in a freshly forked child) and closes the range before each one,
/// then everything above the last one.
fn close_range_except(keep: &[RawFd]) -> io::Result<()> {
    let mut low: u32 = 3;
    loop {
        // The smallest kept descriptor at or above `low`; `u32::MAX` is the
        // "none left" sentinel, and no real descriptor can reach it because a
        // `RawFd` is an `i32`.
        let mut next = u32::MAX;
        for &fd in keep {
            if fd >= 0 {
                let fd = fd as u32;
                if fd >= low && fd < next {
                    next = fd;
                }
            }
        }
        if next == u32::MAX {
            return close_range(low, u32::MAX);
        }
        if next > low {
            close_range(low, next - 1)?;
        }
        low = next + 1;
    }
}

/// `close_range(first, last, 0)` — closes `first..=last`, kernel permitting.
fn close_range(first: u32, last: u32) -> io::Result<()> {
    // SAFETY: `close_range` takes three integers and no pointers. The third
    // argument is the flags word, and 0 means "close the range, nothing else".
    let ret = unsafe {
        libc::syscall(
            libc::SYS_close_range,
            first as libc::c_uint,
            last as libc::c_uint,
            0 as libc::c_uint,
        )
    };
    if ret == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
