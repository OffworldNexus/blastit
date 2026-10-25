//! Sockets, pipes, descriptor passing and the runtime directories they live in.

use std::ffi::{CString, OsStr};
use std::io;
use std::mem::MaybeUninit;
use std::net::{SocketAddr, TcpListener};
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, FromRawFd, OwnedFd};
use std::os::unix::net::UnixListener;
use std::path::Path;
use std::ptr;

use rustix::fs::{AtFlags, FileType, Gid, Mode, OFlags, Uid};
use rustix::io::Errno;
use rustix::net::sockopt::{set_socket_passcred, set_socket_reuseaddr};
use rustix::net::{
    AddressFamily, SocketAddrUnix, SocketFlags, SocketType, bind, listen, socket_with,
};
use std::os::unix::ffi::OsStrExt;

use crate::Error;

/// The backlog used by [`bind_unix_listener`].
///
/// A view's pg listeners see one connection per request at most, so the queue is
/// never deep. The TCP listener takes its backlog as an argument instead,
/// because `BLASTIT_BACKLOG` is part of the configuration (§3).
pub const UNIX_BACKLOG: i32 = 128;

/// A connected `AF_UNIX`/`SOCK_STREAM` socketpair, both ends close-on-exec.
///
/// Socketpairs are how every internal channel starts (`http.chan`, `ctl.chan`,
/// `view.chan`, §3): both ends are known at fork time, so nothing can connect
/// to them later.
pub fn socketpair_stream() -> io::Result<(OwnedFd, OwnedFd)> {
    rustix::net::socketpair(
        AddressFamily::UNIX,
        SocketType::STREAM,
        SocketFlags::CLOEXEC,
        None,
    )
    .map_err(Into::into)
}

/// A pipe whose two ends are close-on-exec.
///
/// `pipe2(O_CLOEXEC)`: log pipes and `secrets.pipe` themselves are inherited by
/// an explicit child, never by accident.
pub fn pipe() -> io::Result<(OwnedFd, OwnedFd)> {
    let mut fds = MaybeUninit::<[libc::c_int; 2]>::uninit();
    // SAFETY: `pipe2` writes two descriptors into the buffer, which is exactly
    // two `c_int`s of properly aligned storage.
    let ret = unsafe { libc::pipe2(fds.as_mut_ptr().cast::<libc::c_int>(), libc::O_CLOEXEC) };
    if ret == -1 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: the call above succeeded and initialised both descriptors. They
    // are owned from here on and closed by the returned `OwnedFd`s.
    let [read_end, write_end] = unsafe { fds.assume_init() };
    // SAFETY: `pipe2` returned two open descriptors that no other code owns.
    Ok(unsafe {
        (
            OwnedFd::from_raw_fd(read_end),
            OwnedFd::from_raw_fd(write_end),
        )
    })
}

/// Send `bytes` over `sock`, attaching exactly one descriptor with `SCM_RIGHTS`.
///
/// The descriptor is duplicated into the receiving process, so the sender keeps
/// its own copy and closes it when its own use is over. On Linux the receiver
/// gets close-on-exec behaviour by asking for it (`MSG_CMSG_CLOEXEC` in
/// [`recv_with_fd`]).
///
/// `bytes` must not be empty: attaching a control message to a zero-length
/// write is a corner of the socket API where a stream socket may report success
/// without delivering the descriptor at all, so an empty payload is an
/// [`Error::EmptyPayload`]. Every message on `ctl.chan` carries a frame, so
/// nothing in blastit has an empty one.
///
/// Returns how many bytes were written; a short write is possible, so a caller
/// that must deliver all of `bytes` should check.
pub fn send_with_fd(sock: BorrowedFd<'_>, bytes: &[u8], fd: BorrowedFd<'_>) -> io::Result<usize> {
    if bytes.is_empty() {
        return Err(Error::EmptyPayload.into());
    }
    let mut iov = libc::iovec {
        iov_base: bytes.as_ptr().cast::<libc::c_void>().cast_mut(),
        iov_len: bytes.len(),
    };
    let mut buffer = CmsgBuffer::for_one_fd();
    // SAFETY: `msghdr` is a plain C struct of integers and pointers; all zero
    // is a valid empty header, which the fields below fill in.
    let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
    msg.msg_iov = &mut iov;
    msg.msg_iovlen = 1;
    msg.msg_control = buffer.0.as_mut_ptr().cast::<libc::c_void>();
    msg.msg_controllen = buffer.0.len();

    // SAFETY: `msg_control` points at `buffer`, which is at least
    // `CMSG_SPACE(size_of::<c_int>())` bytes of aligned storage, and
    // `msg_controllen` says so; the first header therefore fits.
    let cmsg = unsafe { libc::CMSG_FIRSTHDR(&msg) };
    if cmsg.is_null() {
        return Err(io::Error::other("SCM_RIGHTS control buffer is too small"));
    }
    // SAFETY: `cmsg` points into `buffer`, which outlives this block, and the
    // header has room for one `c_int` of data.
    unsafe {
        (*cmsg).cmsg_level = libc::SOL_SOCKET;
        (*cmsg).cmsg_type = libc::SCM_RIGHTS;
        (*cmsg).cmsg_len =
            libc::CMSG_LEN(std::mem::size_of::<libc::c_int>() as u32) as libc::size_t;
        ptr::write_unaligned(libc::CMSG_DATA(cmsg).cast::<libc::c_int>(), fd.as_raw_fd());
    }
    msg.msg_controllen =
        unsafe { libc::CMSG_SPACE(std::mem::size_of::<libc::c_int>() as u32) } as usize;

    // SAFETY: `msg` describes one valid iovec and one valid control message,
    // both alive for the duration of the call.
    let written = unsafe { libc::sendmsg(sock.as_raw_fd(), &msg, 0) };
    if written == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(written as usize)
}

/// Receive bytes and at most one descriptor from `sock`.
///
/// The kernel applies `MSG_CMSG_CLOEXEC`, so the received descriptor is
/// close-on-exec without a second call. If the message carries more than one
/// descriptor, all of them are closed and [`Error::TooManyFds`] is returned: a
/// caller that funnels every `view.chan` end through one channel never wants to
/// guess which one is which (§3).
pub fn recv_with_fd(sock: BorrowedFd<'_>, buf: &mut [u8]) -> io::Result<(usize, Option<OwnedFd>)> {
    let mut iov = libc::iovec {
        iov_base: buf.as_mut_ptr().cast::<libc::c_void>(),
        iov_len: buf.len(),
    };
    let mut buffer = CmsgBuffer::for_one_fd();
    // SAFETY: `msghdr` is a plain C struct of integers and pointers; all zero
    // is a valid empty header, which the fields below fill in.
    let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
    msg.msg_iov = &mut iov;
    msg.msg_iovlen = 1;
    msg.msg_control = buffer.0.as_mut_ptr().cast::<libc::c_void>();
    msg.msg_controllen = buffer.0.len();

    // SAFETY: `msg` describes one valid iovec and one valid control buffer,
    // both alive for the duration of the call.
    let read = unsafe { libc::recvmsg(sock.as_raw_fd(), &mut msg, libc::MSG_CMSG_CLOEXEC) };
    if read == -1 {
        return Err(io::Error::last_os_error());
    }

    if msg.msg_flags & libc::MSG_CTRUNC != 0 {
        // The kernel may have installed some descriptors before it gave up;
        // close whatever fits in the buffer and refuse the message.
        for fd in unsafe { collect_fds(&msg) } {
            drop(fd);
        }
        return Err(Error::ControlMessageTruncated.into());
    }

    // SAFETY: the call above succeeded, and `msg` still describes the same
    // control buffer, which is valid and has not been moved.
    let fds = unsafe { collect_fds(&msg) };
    let mut received = None;
    let mut extra = 0_usize;
    for fd in fds {
        if received.is_none() && extra == 0 {
            received = Some(fd);
        } else {
            extra += 1;
            drop(fd);
        }
    }
    if extra > 0 {
        drop(received.take());
        return Err(Error::TooManyFds { count: extra + 1 }.into());
    }
    Ok((read as usize, received))
}

/// Take ownership of every `SCM_RIGHTS` descriptor in a received message.
///
/// The count is clamped to what the control buffer really holds: `cmsg_len`
/// comes from the peer, and a bogus length must not make this read — or take
/// ownership of — memory past the end of the buffer the kernel filled in.
///
/// # Safety
///
/// `msg` must be the message header of a successful `recvmsg` call, whose
/// control buffer is still alive; every descriptor in it must have been
/// installed by the kernel and not yet closed.
unsafe fn collect_fds(msg: &libc::msghdr) -> Vec<OwnedFd> {
    let mut out = Vec::new();
    // The buffer the kernel was allowed to write into, as bounds to check the
    // peer's `cmsg_len` against.
    let buffer_start = msg.msg_control as usize;
    let buffer_end = buffer_start.saturating_add(msg.msg_controllen);
    // SAFETY: the caller guarantees `msg` describes a valid control buffer
    // that outlives this function.
    let mut cmsg = unsafe { libc::CMSG_FIRSTHDR(msg) };
    while !cmsg.is_null() {
        // SAFETY: `cmsg` points inside the control buffer `msg` describes.
        let (level, kind, len) =
            unsafe { ((*cmsg).cmsg_level, (*cmsg).cmsg_type, (*cmsg).cmsg_len) };
        if level == libc::SOL_SOCKET && kind == libc::SCM_RIGHTS {
            // SAFETY: `CMSG_LEN(0)` is the header length of an empty control
            // message; what is left after the header is the payload.
            let header_len = unsafe { libc::CMSG_LEN(0) } as usize;
            let payload = len.saturating_sub(header_len);
            // SAFETY: the header is inside the buffer, so its data pointer is
            // inside it too.
            let data_start = unsafe { libc::CMSG_DATA(cmsg) } as usize;
            let room = buffer_end.saturating_sub(data_start) / std::mem::size_of::<libc::c_int>();
            let count = (payload / std::mem::size_of::<libc::c_int>()).min(room);
            // SAFETY: `cmsg` points into `msg`'s control buffer.
            let data = unsafe { libc::CMSG_DATA(cmsg).cast::<libc::c_int>() };
            for index in 0..count {
                // SAFETY: `count` was clamped to the bytes really in the
                // buffer, so every read is inside it.
                let raw = unsafe { ptr::read_unaligned(data.add(index)) };
                // SAFETY: the kernel installed this descriptor in this process
                // during `recvmsg`; nothing else owns it.
                out.push(unsafe { OwnedFd::from_raw_fd(raw) });
            }
        }
        // SAFETY: `cmsg` points into the buffer and `msg` bounds it.
        cmsg = unsafe { libc::CMSG_NXTHDR(msg, cmsg) };
    }
    out
}

/// Control-message buffer.
///
/// Sized for one descriptor **plus** an `SCM_CREDENTIALS` message: if the
/// receiving socket has `SO_PASSCRED` set (§2.2's listeners do), the kernel adds
/// credentials to every message, and a buffer sized for a descriptor alone would
/// truncate. The channel sockets of §3 must not have `SO_PASSCRED` — internal
/// traffic carries no credentials — but a receiver that has it must not lose fds
/// either, so the room is always there.
#[repr(align(8))]
struct CmsgBuffer([u8; Self::LEN]);

impl CmsgBuffer {
    /// `CMSG_SPACE` for one `c_int` of descriptor payload plus one `ucred`.
    const LEN: usize = unsafe { libc::CMSG_SPACE(std::mem::size_of::<libc::c_int>() as u32) }
        as usize
        + unsafe { libc::CMSG_SPACE(std::mem::size_of::<libc::ucred>() as u32) } as usize;

    fn for_one_fd() -> Self {
        Self([0_u8; Self::LEN])
    }
}

/// Bind `addr` with `SO_REUSEADDR`, close-on-exec, and listen.
///
/// This is the one listener blastit takes from the platform
/// (`0.0.0.0:$PORT`), handed to http (§3). The returned listener is blocking:
/// http puts it into its own runtime after it has been forked.
pub fn bind_tcp(addr: SocketAddr, backlog: i32) -> io::Result<TcpListener> {
    let family = match addr {
        SocketAddr::V4(_) => AddressFamily::INET,
        SocketAddr::V6(_) => AddressFamily::INET6,
    };
    let fd = socket_with(family, SocketType::STREAM, SocketFlags::CLOEXEC, None)?;
    set_socket_reuseaddr(&fd, true)?;
    bind(&fd, &addr)?;
    listen(&fd, backlog)?;
    Ok(TcpListener::from(fd))
}

/// Bind a named Unix socket at `path`, owned by `owner_uid`, mode `mode`, with
/// `SO_PASSCRED` set, and listen.
///
/// These are the only named sockets in blastit: one per (route, `postgresql`
/// proxy) that a view has a grant on, inside that view's private directory
/// (§2.2). Identity comes from the listener a connection arrived on plus the
/// kernel-attached `SCM_CREDENTIALS` uid, which is why `SO_PASSCRED` is on
/// before the socket can accept anything: `SO_PEERCRED` lies under gVisor
/// (§3, §9.1).
///
/// # Why the socket is never usable with wider permissions
///
/// The leaf lives in a directory a view owns, so nothing here follows a path
/// after the fact: the parent directory is opened once
/// (`O_DIRECTORY|O_NOFOLLOW`) and every step after that — the unlink of a stale
/// socket, the ownership change, the mode — is relative to that descriptor.
/// `chown` is `fchownat(dirfd, name, AT_SYMLINK_NOFOLLOW)`; the mode is
/// `fchmodat2(dirfd, name, AT_SYMLINK_NOFOLLOW)` where the kernel has it, and
/// otherwise a `chmod` of the inode through an `O_PATH|O_NOFOLLOW` descriptor's
/// `/proc/self/fd/<n>` link, which resolves to the inode and not to a path.
///
/// Between the `bind` and that `fchmod`, the new file has whatever mode the
/// umask allows. Nothing can use it in that window:
///
/// * a connection needs an accept queue, and the queue only exists once
///   `listen` has run, which is the last step — before that, `connect` fails
///   with `ECONNREFUSED` whatever the file's mode is;
/// * the directory is `0700` and belongs to `owner_uid`, so no other uid can
///   even unlink or replace the socket in that window.
///
/// If the mode cannot be applied this fails *before* `listen`, so nothing is
/// ever served with the wrong permissions — a half-started sandbox is worse
/// than a crash loop (§4).
///
/// A file already at `path` is removed first — after a restart `/run` may still
/// hold the previous socket — but only if it *is* a socket. A directory, a
/// symlink or a regular file there is somebody else's, and is refused with
/// [`Error::NotASocket`] rather than unlinked.
pub fn bind_unix_listener(path: &Path, owner_uid: u32, mode: u32) -> io::Result<UnixListener> {
    let parent = path.parent().ok_or_else(|| Error::NotADirectory {
        path: path.to_path_buf(),
    })?;
    let name = path.file_name().ok_or_else(|| Error::NotADirectory {
        path: path.to_path_buf(),
    })?;
    // Hold the directory itself: everything below is done relative to this
    // descriptor, so a symlink swapped in under `path` cannot make init touch
    // something else, and the socket mode is never a path-based `chmod`.
    let dir = rustix::fs::open(
        parent,
        OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )?;

    match rustix::fs::statat(&dir, name, AtFlags::SYMLINK_NOFOLLOW) {
        Ok(stat) => match FileType::from_raw_mode(stat.st_mode) {
            FileType::Socket => rustix::fs::unlinkat(&dir, name, AtFlags::empty())?,
            _ => {
                return Err(Error::NotASocket {
                    path: path.to_path_buf(),
                }
                .into());
            }
        },
        Err(err) if err == Errno::NOENT => {}
        Err(err) => return Err(err.into()),
    }

    let addr = SocketAddrUnix::new(path)?;
    let fd = socket_with(
        AddressFamily::UNIX,
        SocketType::STREAM,
        SocketFlags::CLOEXEC,
        None,
    )?;
    bind(&fd, &addr)?;
    // The socket must be where the directory descriptor says it is: if a path
    // component had been swapped while we bound, this notices.
    let bound_stat = rustix::fs::statat(&dir, name, AtFlags::SYMLINK_NOFOLLOW)?;
    if FileType::from_raw_mode(bound_stat.st_mode) != FileType::Socket {
        return Err(Error::NotASocket {
            path: path.to_path_buf(),
        }
        .into());
    }
    set_socket_passcred(&fd, true)?;
    rustix::fs::chownat(
        &dir,
        name,
        Some(Uid::from_raw(owner_uid)),
        Some(Gid::from_raw(owner_uid)),
        AtFlags::SYMLINK_NOFOLLOW,
    )?;
    chmod_socket(dir.as_fd(), name, path, mode)?;
    // Only now is there an accept queue to connect to.
    listen(&fd, UNIX_BACKLOG)?;
    Ok(UnixListener::from(fd))
}

/// Set the mode of the socket file `name` inside `dir`, without following a
/// symlink at the last component.
fn chmod_socket(dir: BorrowedFd<'_>, name: &OsStr, path: &Path, mode: u32) -> io::Result<()> {
    match fchmodat2(dir, name, mode) {
        Ok(()) => Ok(()),
        // No `fchmodat2`: Linux before 6.5, and gVisor today.
        Err(err) if matches!(err.raw_os_error(), Some(libc::ENOSYS) | Some(libc::EINVAL)) => {
            chmod_socket_via_proc(dir, name, path, mode)
        }
        Err(err) => Err(err),
    }
}

/// `fchmodat2(dirfd, name, mode, AT_SYMLINK_NOFOLLOW)`.
fn fchmodat2(dir: BorrowedFd<'_>, name: &OsStr, mode: u32) -> io::Result<()> {
    let name = CString::new(name.as_bytes())
        .map_err(|_| io::Error::other("a file name cannot contain a NUL byte"))?;
    // SAFETY: `fchmodat2` takes a descriptor, a NUL-terminated path, a mode and
    // a flag word; it reads no other memory. `AT_SYMLINK_NOFOLLOW` is the flag
    // that makes it refuse a symlink at the last component.
    let ret = unsafe {
        libc::syscall(
            libc::SYS_fchmodat2,
            dir.as_raw_fd(),
            name.as_ptr(),
            mode as libc::mode_t,
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if ret == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// The portable half of [`chmod_socket`]: `chmod` the inode itself.
///
/// `O_PATH|O_NOFOLLOW` opens the last component without following a symlink,
/// `fstat` proves it is a socket, and `chmod("/proc/self/fd/<n>")` changes the
/// mode of *that* inode: the `/proc` entry is a magic link to the open file,
/// not a path that could be redirected afterwards. A symlink at `name` is
/// therefore either rejected by `fstat` (it is not a socket) or reached only
/// through its own inode.
///
/// Exposed because kernels without `fchmodat2` — and the tests, which cannot
/// choose their kernel — need it directly.
pub(crate) fn chmod_socket_via_proc(
    dir: BorrowedFd<'_>,
    name: &OsStr,
    path: &Path,
    mode: u32,
) -> io::Result<()> {
    let socket = rustix::fs::openat(
        dir,
        name,
        OFlags::PATH | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )?;
    let stat = rustix::fs::fstat(&socket)?;
    if FileType::from_raw_mode(stat.st_mode) != FileType::Socket {
        return Err(Error::NotASocket {
            path: path.to_path_buf(),
        }
        .into());
    }
    let link = CString::new(format!("/proc/self/fd/{}", socket.as_raw_fd()))
        .map_err(|_| io::Error::other("the /proc/self/fd path is NUL-free by construction"))?;
    // SAFETY: `chmod` takes a NUL-terminated path; the path is one we built, and
    // the descriptor stays open for the duration of the call.
    if unsafe { libc::chmod(link.as_ptr(), mode as libc::mode_t) } == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Create `path` as a directory owned by `uid` with exactly `mode`, creating
/// missing parents as root-owned `0755`.
///
/// This is `/run/blastit/views/<uid>/` (§2.2, §7.2): root creates the tree, the
/// leaf belongs to the view and is `0700`, and `/run/blastit` itself stays
/// root-owned so a view cannot add directories of its own next to it.
///
/// The leaf is never followed through a symlink — `lstat` and then an
/// `O_NOFOLLOW` open decide that — and an existing leaf that is not a plain
/// directory is an error. An existing plain directory is accepted and brought
/// to `mode` and `uid`, so a restart over a warm `/run` does not fail.
///
/// The mode and the owner are applied through that open descriptor
/// (`fchmod`/`fchown`), never by path: the directory a view owns is exactly the
/// place where a path-based `chown` as root would be worth attacking, and a
/// descriptor cannot be redirected after the fact.
pub fn make_private_dir(path: &Path, uid: u32, mode: u32) -> io::Result<()> {
    create_parents(path)?;

    match std::fs::symlink_metadata(path) {
        Ok(meta) => {
            if !meta.file_type().is_dir() {
                return Err(Error::NotADirectory {
                    path: path.to_path_buf(),
                }
                .into());
            }
        }
        Err(err) if err.kind() == io::ErrorKind::NotFound => {
            rustix::fs::mkdir(path, Mode::from_bits_truncate(mode))?;
        }
        Err(err) => return Err(err),
    }

    // Opening with `O_NOFOLLOW` is the real check, and the descriptor is also
    // how the mode and the owner are set: a path-based `chmod`/`chown` in a
    // directory a view uid owns would follow whatever is at `path` *now*.
    let leaf = rustix::fs::open(
        path,
        OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )?;
    rustix::fs::fchmod(&leaf, Mode::from_bits_truncate(mode))?;
    rustix::fs::fchown(&leaf, Some(Uid::from_raw(uid)), Some(Gid::from_raw(uid)))?;
    drop(leaf);
    Ok(())
}

/// Create every missing parent of `path`, as a root-owned `0755` directory.
fn create_parents(path: &Path) -> io::Result<()> {
    let Some(parent) = path.parent() else {
        return Ok(());
    };
    let mut ancestors: Vec<&Path> = parent.ancestors().collect();
    ancestors.reverse();
    for dir in ancestors {
        if dir.as_os_str().is_empty() {
            continue;
        }
        match std::fs::symlink_metadata(dir) {
            Ok(meta) if meta.file_type().is_dir() => {}
            Ok(_) => {
                return Err(Error::NotADirectory {
                    path: dir.to_path_buf(),
                }
                .into());
            }
            Err(err) if err.kind() == io::ErrorKind::NotFound => {
                // `mkdir` is masked by the umask, so the mode is set again.
                rustix::fs::mkdir(dir, Mode::from_bits_truncate(0o755))?;
                rustix::fs::chmod(dir, Mode::from_bits_truncate(0o755))?;
            }
            Err(err) => return Err(err),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::Write;

    use std::os::fd::{AsFd, AsRawFd};
    use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};

    use rustix::net::sockopt::{set_socket_passcred, socket_passcred, socket_reuseaddr};

    use super::*;
    use crate::Error;
    use crate::drop_privileges;
    use crate::fd::fd_is_cloexec;
    use crate::testutil::{
        TempDir, euid, in_child, is_root, must, must_err, must_some, open_fds, read_to_end,
        root_only, write_all,
    };

    #[test]
    fn socketpair_stream_is_connected_and_cloexec() {
        let (left, right) = must(socketpair_stream());
        assert!(must(fd_is_cloexec(left.as_fd())));
        assert!(must(fd_is_cloexec(right.as_fd())));
        write_all(left.as_fd(), b"ping");
        let mut buf = [0_u8; 8];
        // SAFETY: `read` writes at most `buf.len()` bytes into a valid buffer.
        let read = unsafe { libc::read(right.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len()) };
        assert_eq!(read, 4);
        assert_eq!(&buf[..4], b"ping");
    }

    #[test]
    fn pipe_ends_are_cloexec() {
        let (read_end, write_end) = must(pipe());
        assert!(must(fd_is_cloexec(read_end.as_fd())));
        assert!(must(fd_is_cloexec(write_end.as_fd())));
        write_all(write_end.as_fd(), b"logs");
        drop(write_end);
        assert_eq!(read_to_end(read_end), b"logs");
    }

    #[test]
    fn send_with_fd_passes_a_descriptor_that_works() {
        let (sender, receiver) = must(socketpair_stream());
        let (log_read, log_write) = must(pipe());
        assert_eq!(
            must(send_with_fd(sender.as_fd(), b"payload", log_write.as_fd())),
            7
        );

        let mut buf = [0_u8; 16];
        let (len, fd) = must(recv_with_fd(receiver.as_fd(), &mut buf));
        assert_eq!(&buf[..len], b"payload");
        let received = must_some(fd);
        assert!(
            must(fd_is_cloexec(received.as_fd())),
            "MSG_CMSG_CLOEXEC must apply to the received descriptor"
        );

        // The received descriptor is a duplicate of the write end: writing
        // through it has to reach the far end of the pipe.
        write_all(received.as_fd(), b"through the received fd");
        drop(received);
        drop(log_write);
        assert_eq!(read_to_end(log_read), b"through the received fd");
    }

    #[test]
    fn recv_with_fd_returns_none_when_no_descriptor_arrives() {
        let (sender, receiver) = must(socketpair_stream());
        write_all(sender.as_fd(), b"plain");
        let mut buf = [0_u8; 16];
        let (len, fd) = must(recv_with_fd(receiver.as_fd(), &mut buf));
        assert_eq!(&buf[..len], b"plain");
        assert!(fd.is_none());
    }

    #[test]
    fn recv_with_fd_closes_extra_descriptors_and_refuses_the_message() {
        // In a forked child, so that "how many descriptors are open" only
        // counts this test's descriptors: the harness's other tests allocate
        // and close descriptors while this one runs.
        let code = in_child(|| {
            let (sender, receiver) = match socketpair_stream() {
                Ok(pair) => pair,
                Err(_) => return 1,
            };
            let (first_read, first_write) = match pipe() {
                Ok(pair) => pair,
                Err(_) => return 2,
            };
            let (second_read, second_write) = match pipe() {
                Ok(pair) => pair,
                Err(_) => return 3,
            };
            let before = open_fds().len();

            send_many_fds(
                sender.as_fd(),
                b"two",
                &[first_write.as_raw_fd(), second_write.as_raw_fd()],
            );

            let mut buf = [0_u8; 16];
            match recv_with_fd(receiver.as_fd(), &mut buf) {
                Err(err) => match Error::from_io(&err) {
                    Some(Error::TooManyFds { count }) if *count == 2 => {}
                    _ => return 4,
                },
                Ok(_) => return 5,
            }
            if &buf[..3] != b"two" {
                return 6;
            }
            if open_fds().len() != before {
                return 7;
            }
            // Both ends we sent are still ours and still usable.
            drop((first_read, first_write, second_read, second_write, sender));
            0
        });
        assert_eq!(code, 0, "child failure code {code}");
    }

    /// Send several descriptors in one message — something the crate's own API
    /// deliberately refuses to do, so the receiver has to clean up.
    fn send_many_fds(sock: BorrowedFd<'_>, bytes: &[u8], fds: &[libc::c_int]) {
        #[repr(align(8))]
        struct Buffer([u8; 128]);
        let mut buffer = Buffer([0_u8; 128]);
        let mut iov = libc::iovec {
            iov_base: bytes.as_ptr().cast::<libc::c_void>().cast_mut(),
            iov_len: bytes.len(),
        };
        // SAFETY: `msghdr` is a plain C struct; all zero is a valid empty header.
        let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
        msg.msg_iov = &mut iov;
        msg.msg_iovlen = 1;
        msg.msg_control = buffer.0.as_mut_ptr().cast::<libc::c_void>();
        msg.msg_controllen = buffer.0.len();
        // SAFETY: the control buffer is large enough for the descriptors we
        // write into it, and it outlives the call below.
        unsafe {
            let cmsg = libc::CMSG_FIRSTHDR(&msg);
            assert!(!cmsg.is_null());
            (*cmsg).cmsg_level = libc::SOL_SOCKET;
            (*cmsg).cmsg_type = libc::SCM_RIGHTS;
            let payload = std::mem::size_of_val(fds);
            (*cmsg).cmsg_len = libc::CMSG_LEN(payload as u32) as libc::size_t;
            let data = libc::CMSG_DATA(cmsg).cast::<libc::c_int>();
            for (index, fd) in fds.iter().enumerate() {
                ptr::write_unaligned(data.add(index), *fd);
            }
            msg.msg_controllen = libc::CMSG_SPACE(payload as u32) as usize;
            let written = libc::sendmsg(sock.as_raw_fd(), &msg, 0);
            assert_eq!(written as usize, bytes.len());
        }
    }

    #[test]
    fn send_with_fd_refuses_an_empty_payload() {
        let (sender, _receiver) = must(socketpair_stream());
        let (_read, write_end) = must(pipe());
        let err = must_err(send_with_fd(sender.as_fd(), b"", write_end.as_fd()));
        assert!(
            matches!(Error::from_io(&err), Some(Error::EmptyPayload)),
            "got {err}"
        );
    }

    #[test]
    fn recv_with_fd_is_not_confused_by_credentials() {
        // A socket with SO_PASSCRED also receives SCM_CREDENTIALS; the control
        // buffer has room for it and the descriptor still arrives.
        let (sender, receiver) = must(socketpair_stream());
        must(set_socket_passcred(receiver.as_fd(), true));
        let (_read, write_end) = must(pipe());
        assert_eq!(
            must(send_with_fd(sender.as_fd(), b"x", write_end.as_fd())),
            1
        );

        let mut buf = [0_u8; 8];
        let (len, fd) = must(recv_with_fd(receiver.as_fd(), &mut buf));
        assert_eq!(&buf[..len], b"x");
        assert!(fd.is_some(), "SCM_CREDENTIALS must not hide the descriptor");
    }

    #[test]
    fn collect_fds_never_reads_past_the_control_buffer() {
        // A peer (or a future kernel) claiming more descriptors than the
        // control buffer can hold must not make the receiver take ownership of
        // anything past the end of the buffer.
        let (_read, write_end) = must(pipe());
        // SAFETY: `dup` takes an open descriptor and returns a new one.
        let first = unsafe { libc::dup(write_end.as_raw_fd()) };
        // SAFETY: same.
        let second = unsafe { libc::dup(write_end.as_raw_fd()) };
        assert!(first > 0 && second > 0, "dup failed");

        #[repr(align(8))]
        struct Buffer([u8; 64]);
        let mut buffer = Buffer([0_u8; 64]);
        let mut iov = libc::iovec {
            iov_base: std::ptr::null_mut(),
            iov_len: 0,
        };
        // SAFETY: `msghdr` is a plain C struct; all zero is a valid empty header.
        let mut msg: libc::msghdr = unsafe { std::mem::zeroed() };
        msg.msg_iov = &mut iov;
        msg.msg_control = buffer.0.as_mut_ptr().cast::<libc::c_void>();
        msg.msg_controllen = buffer.0.len();
        // SAFETY: the control buffer is 64 bytes, and the header plus one
        // descriptor fits in the first 20 bytes the kernel would have filled.
        unsafe {
            let cmsg = libc::CMSG_FIRSTHDR(&msg);
            assert!(!cmsg.is_null());
            (*cmsg).cmsg_level = libc::SOL_SOCKET;
            (*cmsg).cmsg_type = libc::SCM_RIGHTS;
            // Claims two descriptors...
            (*cmsg).cmsg_len =
                libc::CMSG_LEN((2 * std::mem::size_of::<libc::c_int>()) as u32) as libc::size_t;
            let data = libc::CMSG_DATA(cmsg).cast::<libc::c_int>();
            ptr::write_unaligned(data, first);
            ptr::write_unaligned(data.add(1), second);
        }
        // ... but the buffer that was filled in only has room for one.
        msg.msg_controllen =
            unsafe { libc::CMSG_LEN(std::mem::size_of::<libc::c_int>() as u32) } as usize;

        // SAFETY: the header describes a control buffer that is alive for the
        // duration of the call and holds descriptors this process owns.
        let fds = unsafe { collect_fds(&msg) };
        assert_eq!(fds.len(), 1, "the count must be clamped to the buffer");
        drop(fds);

        // The second descriptor was never taken, so it is still open and still
        // ours to close.
        // SAFETY: `second` came from `dup` and nothing else has closed it.
        assert_eq!(unsafe { libc::fcntl(second, libc::F_GETFD) }, 0);
        // SAFETY: `second` is ours.
        assert_eq!(unsafe { libc::close(second) }, 0);
    }

    #[test]
    fn bind_tcp_sets_reuseaddr_and_cloexec() {
        let addr = must("127.0.0.1:0".parse::<SocketAddr>());
        let listener = must(bind_tcp(addr, 16));
        assert!(must(fd_is_cloexec(listener.as_fd())));
        assert!(must(socket_reuseaddr(listener.as_fd())));
        assert_ne!(must(listener.local_addr()).port(), 0);
    }

    #[test]
    fn bind_tcp_accepts_a_connection() {
        let addr = must("127.0.0.1:0".parse::<SocketAddr>());
        let listener = must(bind_tcp(addr, 16));
        let local = must(listener.local_addr());
        let client = std::thread::spawn(move || {
            let mut stream = must(std::net::TcpStream::connect(local));
            must(stream.write_all(b"hello"));
        });
        let (mut accepted, _peer) = must(listener.accept());
        let mut buf = [0_u8; 8];
        let mut filled = 0;
        while filled < 5 {
            let read = must(std::io::Read::read(&mut accepted, &mut buf[filled..]));
            assert_ne!(read, 0);
            filled += read;
        }
        assert_eq!(&buf[..5], b"hello");
        must(client.join());
    }

    #[test]
    fn bind_unix_listener_sets_mode_passcred_and_owner() {
        let dir = TempDir::new("unix-listen");
        must(std::fs::create_dir_all(dir.path()));
        let owner = if is_root() { 20_500 } else { euid() };
        must(make_private_dir(dir.path(), owner, 0o700));
        let socket = dir.path().join(".s.PGSQL.5432");

        let listener = must(bind_unix_listener(&socket, owner, 0o600));
        let meta = must(std::fs::symlink_metadata(&socket));
        assert!(meta.file_type().is_socket(), "the path holds a socket");
        assert_eq!(
            meta.permissions().mode() & 0o777,
            0o600,
            "a view's pg socket is 0600"
        );
        assert_eq!(meta.uid(), owner, "the socket belongs to the view uid");
        assert_eq!(meta.gid(), owner, "gid == uid");
        assert!(
            must(socket_passcred(listener.as_fd())),
            "SO_PASSCRED must be on before anything can connect"
        );
        assert!(must(fd_is_cloexec(listener.as_fd())));

        let client = must(std::os::unix::net::UnixStream::connect(&socket));
        let (accepted, _addr) = must(listener.accept());
        assert!(
            must(socket_passcred(accepted.as_fd())),
            "the accepted socket inherits SO_PASSCRED"
        );
        write_all(accepted.as_fd(), b"R");
        drop(client);
    }

    #[test]
    fn bind_unix_listener_sets_the_exact_mode() {
        let dir = TempDir::new("unix-mode");
        must(std::fs::create_dir_all(dir.path()));
        let uid = euid();
        must(make_private_dir(dir.path(), uid, 0o700));
        for mode in [0o600_u32, 0o640, 0o666, 0o400] {
            let socket = dir.path().join(format!("s{mode:o}"));
            let _listener = must(bind_unix_listener(&socket, uid, mode));
            let meta = must(std::fs::symlink_metadata(&socket));
            assert_eq!(
                meta.permissions().mode() & 0o777,
                mode,
                "the socket must have exactly the mode it was asked for"
            );
        }
    }

    #[test]
    fn chmod_socket_via_proc_sets_the_socket_mode() {
        // The route a kernel without `fchmodat2` takes (gVisor today), exercised
        // directly because the kernel under test cannot be chosen.
        let dir = TempDir::new("unix-proc");
        must(std::fs::create_dir_all(dir.path()));
        let uid = euid();
        must(make_private_dir(dir.path(), uid, 0o700));
        let socket = dir.path().join(".s.PGSQL.5432");
        let _listener = must(bind_unix_listener(&socket, uid, 0o644));

        let directory = must(rustix::fs::open(
            dir.path(),
            OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        ));
        let name = must_some(socket.file_name());
        must(chmod_socket_via_proc(
            directory.as_fd(),
            name,
            &socket,
            0o640,
        ));
        assert_eq!(
            must(std::fs::symlink_metadata(&socket))
                .permissions()
                .mode()
                & 0o777,
            0o640
        );
    }

    #[test]
    fn chmod_socket_via_proc_refuses_a_symlink() {
        let dir = TempDir::new("unix-proc-link");
        must(std::fs::create_dir_all(dir.path()));
        let target = dir.path().join("target");
        must(std::fs::write(&target, b"x"));
        let link = dir.path().join("link");
        must(std::os::unix::fs::symlink(&target, &link));

        let directory = must(rustix::fs::open(
            dir.path(),
            OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        ));
        let name = must_some(link.file_name());
        let err = must_err(chmod_socket_via_proc(directory.as_fd(), name, &link, 0o777));
        assert!(
            matches!(Error::from_io(&err), Some(Error::NotASocket { .. })),
            "got {err}"
        );
    }

    #[test]
    fn a_bound_but_unlistened_socket_refuses_connections() {
        // Why the mode may lag the `bind` by a few microseconds: until
        // `listen` runs there is nothing to connect to, whatever the mode is.
        let dir = TempDir::new("unix-unlistened");
        must(std::fs::create_dir_all(dir.path()));
        let socket = dir.path().join(".s.PGSQL.5432");
        let addr = must(SocketAddrUnix::new(&socket));
        let fd = must(socket_with(
            AddressFamily::UNIX,
            SocketType::STREAM,
            SocketFlags::CLOEXEC,
            None,
        ));
        must(bind(&fd, &addr));
        // Bound, not listening: no accept queue exists yet.
        let err = must_err(std::os::unix::net::UnixStream::connect(&socket));
        assert_eq!(err.kind(), io::ErrorKind::ConnectionRefused, "got {err:?}");
        // And with a queue it works, so the assertion above is about `listen`.
        must(listen(&fd, UNIX_BACKLOG));
        let _client = must(std::os::unix::net::UnixStream::connect(&socket));
    }

    #[test]
    fn bind_unix_listener_replaces_a_stale_socket() {
        let dir = TempDir::new("unix-stale");
        must(std::fs::create_dir_all(dir.path()));
        let owner = euid();
        let socket = dir.path().join(".s.PGSQL.5432");

        let first = must(bind_unix_listener(&socket, owner, 0o600));
        // Dropping the listener closes the descriptor but leaves the file
        // behind, which is what a crash looks like from the next start.
        drop(first);
        assert!(
            must(std::fs::symlink_metadata(&socket))
                .file_type()
                .is_socket()
        );

        let _second = must(bind_unix_listener(&socket, owner, 0o600));
        assert!(
            must(std::fs::symlink_metadata(&socket))
                .file_type()
                .is_socket()
        );
    }

    #[test]
    fn bind_unix_listener_refuses_a_regular_file() {
        let dir = TempDir::new("unix-file");
        must(std::fs::create_dir_all(dir.path()));
        let socket = dir.path().join(".s.PGSQL.5432");
        must(std::fs::write(&socket, b"not a socket"));

        let err = must_err(bind_unix_listener(&socket, euid(), 0o600));
        assert!(
            matches!(Error::from_io(&err), Some(Error::NotASocket { .. })),
            "got {err}"
        );
        assert!(socket.exists(), "somebody else's file is left alone");
    }

    #[test]
    fn bind_unix_listener_refuses_a_symlink() {
        let dir = TempDir::new("unix-link");
        must(std::fs::create_dir_all(dir.path()));
        let target = dir.path().join("target");
        must(std::fs::write(&target, b"x"));
        let socket = dir.path().join(".s.PGSQL.5432");
        must(std::os::unix::fs::symlink(&target, &socket));

        let err = must_err(bind_unix_listener(&socket, euid(), 0o600));
        assert!(
            matches!(Error::from_io(&err), Some(Error::NotASocket { .. })),
            "got {err}"
        );
        assert!(
            must(std::fs::symlink_metadata(&socket))
                .file_type()
                .is_symlink(),
            "the symlink is not unlinked"
        );
        assert!(target.exists(), "the target is not touched");
    }

    #[test]
    fn bind_unix_listener_refuses_a_directory() {
        let dir = TempDir::new("unix-dir");
        must(std::fs::create_dir_all(dir.path()));
        let socket = dir.path().join(".s.PGSQL.5432");
        must(std::fs::create_dir(&socket));

        let err = must_err(bind_unix_listener(&socket, euid(), 0o600));
        assert!(
            matches!(Error::from_io(&err), Some(Error::NotASocket { .. })),
            "got {err}"
        );
        assert!(
            must(std::fs::symlink_metadata(&socket))
                .file_type()
                .is_dir()
        );
    }

    #[test]
    fn bind_unix_listener_is_closed_to_another_uid() {
        if !root_only() {
            return;
        }
        let dir = TempDir::new("unix-uid");
        must(std::fs::create_dir_all(dir.path()));
        let owner = 20_500_u32;
        let other = 20_501_u32;
        must(make_private_dir(dir.path(), owner, 0o700));
        let socket = dir.path().join(".s.PGSQL.5432");
        let _listener = must(bind_unix_listener(&socket, owner, 0o600));

        // The view uid can connect: 0700 directory, 0600 socket, both its own.
        let owner_socket = socket.clone();
        let code = in_child(move || {
            if drop_privileges(owner).is_err() {
                return 1;
            }
            match std::os::unix::net::UnixStream::connect(&owner_socket) {
                Ok(_) => 0,
                Err(_) => 2,
            }
        });
        assert_eq!(code, 0, "the view uid must connect (child code {code})");

        // Any other uid gets EACCES, which is the identity check of §2.2.
        let other_socket = socket.clone();
        let code = in_child(move || {
            if drop_privileges(other).is_err() {
                return 1;
            }
            match std::os::unix::net::UnixStream::connect(&other_socket) {
                Ok(_) => 3,
                Err(err) if err.kind() == io::ErrorKind::PermissionDenied => 0,
                Err(_) => 4,
            }
        });
        assert_eq!(code, 0, "another uid must get EACCES (child code {code})");
    }

    #[test]
    fn make_private_dir_creates_parents_with_separate_modes() {
        let root = TempDir::new("mkpriv");
        must(std::fs::create_dir_all(root.path()));
        let owner = if is_root() { 20_500 } else { euid() };
        let leaf = root.path().join("views").join("20500");
        must(make_private_dir(&leaf, owner, 0o700));

        let leaf_meta = must(std::fs::symlink_metadata(&leaf));
        assert!(leaf_meta.file_type().is_dir());
        assert_eq!(leaf_meta.permissions().mode() & 0o777, 0o700);
        assert_eq!(leaf_meta.uid(), owner, "the leaf belongs to the view uid");
        assert_eq!(leaf_meta.gid(), owner, "gid == uid");

        let parent = must(std::fs::symlink_metadata(root.path().join("views")));
        assert_eq!(parent.permissions().mode() & 0o777, 0o755);
        if is_root() {
            assert_eq!(parent.uid(), 0, "parents stay root-owned");
        }
    }

    #[test]
    fn make_private_dir_accepts_an_existing_directory_and_fixes_its_mode() {
        let root = TempDir::new("mkpriv-again");
        let leaf = root.path().join("views").join("20500");
        must(std::fs::create_dir_all(&leaf));
        must(std::fs::set_permissions(
            &leaf,
            std::fs::Permissions::from_mode(0o777),
        ));
        must(make_private_dir(&leaf, euid(), 0o700));
        // A restart over a warm /run must not fail, and must end up strict.
        must(make_private_dir(&leaf, euid(), 0o700));
        assert_eq!(
            must(std::fs::symlink_metadata(&leaf)).permissions().mode() & 0o777,
            0o700
        );
    }

    #[test]
    fn make_private_dir_refuses_a_symlink() {
        let root = TempDir::new("mkpriv-link");
        let target = root.path().join("target");
        must(std::fs::create_dir_all(&target));
        let before = must(std::fs::symlink_metadata(&target))
            .permissions()
            .mode()
            & 0o777;
        let link = root.path().join("link");
        must(std::os::unix::fs::symlink(&target, &link));

        let err = must_err(make_private_dir(&link, euid(), 0o700));
        match Error::from_io(&err) {
            Some(Error::NotADirectory { path }) => assert_eq!(path, &link),
            other => panic!("expected NotADirectory, got {other:?} ({err})"),
        }
        assert_eq!(
            must(std::fs::symlink_metadata(&target))
                .permissions()
                .mode()
                & 0o777,
            before,
            "the symlink target must not be touched"
        );
        assert!(
            must(std::fs::symlink_metadata(&link))
                .file_type()
                .is_symlink(),
            "the symlink must still be a symlink"
        );
    }

    #[test]
    fn make_private_dir_refuses_a_regular_file() {
        let root = TempDir::new("mkpriv-file");
        must(std::fs::create_dir_all(root.path()));
        let leaf = root.path().join("not-a-dir");
        must(std::fs::write(&leaf, b"x"));
        let err = must_err(make_private_dir(&leaf, euid(), 0o700));
        assert!(
            matches!(Error::from_io(&err), Some(Error::NotADirectory { .. })),
            "got {err}"
        );
    }

    #[test]
    fn make_private_dir_refuses_a_parent_that_is_a_file() {
        let root = TempDir::new("mkpriv-parent");
        must(std::fs::create_dir_all(root.path()));
        let file = root.path().join("file");
        must(std::fs::write(&file, b"x"));
        let err = must_err(make_private_dir(&file.join("leaf"), euid(), 0o700));
        assert!(
            matches!(Error::from_io(&err), Some(Error::NotADirectory { .. })),
            "got {err}"
        );
    }
}
