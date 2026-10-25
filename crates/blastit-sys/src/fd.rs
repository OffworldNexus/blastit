//! File-descriptor hygiene: `FD_CLOEXEC` and closing everything a child must
//! not inherit.

use std::io;
use std::os::fd::{AsRawFd, BorrowedFd, RawFd};

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

#[cfg(test)]
mod tests {
    use std::os::fd::{AsFd, FromRawFd, OwnedFd};

    use super::*;
    use crate::testutil::{count_open_fds, in_child, proc_fd_open};
    use crate::testutil::{must, must_some};

    #[test]
    fn set_cloexec_round_trip() {
        let (read_end, _write_end) = must(crate::pipe());
        assert!(must(fd_is_cloexec(read_end.as_fd())));
        assert!(must(set_cloexec(read_end.as_fd(), false)));
        assert!(!must(fd_is_cloexec(read_end.as_fd())));
        assert!(!must(set_cloexec(read_end.as_fd(), true)));
        assert!(must(fd_is_cloexec(read_end.as_fd())));
    }

    #[test]
    fn set_cloexec_works_on_a_descriptor_dup_cleared() {
        let (read_end, _write_end) = must(crate::pipe());
        // `dup` does not copy `FD_CLOEXEC`, so this descriptor starts without
        // it — the state `secrets.pipe` needs to be in.
        // SAFETY: `dup` takes an open descriptor and returns a new one.
        let raw = unsafe { libc::dup(read_end.as_raw_fd()) };
        assert_ne!(raw, -1, "dup failed: {}", std::io::Error::last_os_error());
        // SAFETY: `dup` returned a new open descriptor that nothing else owns.
        let duped = unsafe { OwnedFd::from_raw_fd(raw) };
        assert!(!must(fd_is_cloexec(duped.as_fd())));
        assert!(!must(set_cloexec(duped.as_fd(), true)));
        assert!(must(fd_is_cloexec(duped.as_fd())));
        assert!(must(set_cloexec(duped.as_fd(), false)));
        assert!(!must(fd_is_cloexec(duped.as_fd())));
    }

    #[test]
    fn close_fds_except_keeps_only_the_listed_descriptors() {
        let (first_read, _first_write) = must(crate::pipe());
        let (second_read, _second_write) = must(crate::pipe());
        let first = first_read.as_raw_fd();
        let second = second_read.as_raw_fd();
        // The test process has plenty of descriptors open; the child must end
        // up with exactly the two the caller kept.
        assert!(count_open_fds() > 2);
        let code = in_child(move || {
            if close_fds_except(&[first, second]).is_err() {
                return 1;
            }
            if !proc_fd_open(first) || !proc_fd_open(second) {
                return 2;
            }
            // 0, 1 and 2 are never touched.
            if !proc_fd_open(0) || !proc_fd_open(1) || !proc_fd_open(2) {
                return 3;
            }
            if crate::testutil::count_open_fds() != 2 {
                return 4;
            }
            0
        });
        assert_eq!(code, 0, "child failure code {code}");
    }

    #[test]
    fn close_fds_except_tolerates_unsorted_and_duplicate_keep() {
        let (first_read, first_write) = must(crate::pipe());
        let (second_read, second_write) = must(crate::pipe());
        let a = first_read.as_raw_fd();
        let b = first_write.as_raw_fd();
        let c = second_read.as_raw_fd();
        let d = second_write.as_raw_fd();
        let code = in_child(move || {
            // Unordered, repeated, mixing in descriptors below 3 and two that
            // were never opened: only the four live ones may survive.
            if close_fds_except(&[c, a, 100_000, b, b, 0, 1, 2, 100_001, d]).is_err() {
                return 1;
            }
            if !proc_fd_open(a) || !proc_fd_open(b) || !proc_fd_open(c) || !proc_fd_open(d) {
                return 2;
            }
            if crate::testutil::count_open_fds() != 4 {
                return 3;
            }
            0
        });
        assert_eq!(code, 0, "child failure code {code}");
    }

    #[test]
    fn close_fds_except_with_an_empty_keep_closes_everything() {
        let (_read, _write) = must(crate::pipe());
        let code = in_child(|| {
            if close_fds_except(&[]).is_err() {
                return 1;
            }
            if !proc_fd_open(0) || !proc_fd_open(1) || !proc_fd_open(2) {
                return 2;
            }
            if crate::testutil::count_open_fds() != 0 {
                return 3;
            }
            0
        });
        assert_eq!(code, 0, "child failure code {code}");
    }

    #[test]
    fn close_fds_except_scan_keeps_only_the_listed_descriptors() {
        let (first_read, _first_write) = must(crate::pipe());
        let (second_read, _second_write) = must(crate::pipe());
        let first = first_read.as_raw_fd();
        let second = second_read.as_raw_fd();
        let code = in_child(move || {
            // The portable path, exercised directly: it is what runs on a
            // kernel without `close_range`.
            if close_fds_except_scan(&[first, second]).is_err() {
                return 1;
            }
            if !proc_fd_open(first) || !proc_fd_open(second) {
                return 2;
            }
            if crate::testutil::count_open_fds() != 2 {
                return 3;
            }
            0
        });
        assert_eq!(code, 0, "child failure code {code}");
    }

    #[test]
    fn close_fds_except_scan_closes_descriptors_above_the_kept_ones() {
        let (low_read, low_write) = must(crate::pipe());
        let low = low_read.as_raw_fd().min(low_write.as_raw_fd());
        let mut higher = Vec::new();
        for _ in 0..4 {
            let (read_end, write_end) = must(crate::pipe());
            higher.push(read_end);
            higher.push(write_end);
        }
        let highest = must_some(higher.iter().map(|fd| fd.as_raw_fd()).max());
        assert!(highest > low);
        let code = in_child(move || {
            if close_fds_except_scan(&[low]).is_err() {
                return 1;
            }
            if !proc_fd_open(low) {
                return 2;
            }
            if crate::testutil::count_open_fds() != 1 {
                return 3;
            }
            0
        });
        assert_eq!(code, 0, "child failure code {code}");
    }
}
