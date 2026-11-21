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
use crate::fd::open_dir_nofollow;

/// A control-message length, as `usize`.
///
/// `struct msghdr::msg_controllen` and `struct cmsghdr::cmsg_len` are `size_t`
/// in glibc's headers and `socklen_t` (a `u32`) in musl's; a 32-bit target
/// narrows `size_t` to those same 32 bits. No code here may name one of those
/// typedefs, because the same source compiles against either libc: every length
/// is *read* through this function and *written* through [`set_control_len`] or
/// [`set_cmsg_len`], which resolve to whatever type the header in front of them
/// declares. A value that cannot be a `usize` saturates, and the caller's
/// buffer bound then clamps it — the same refusal a forged length already
/// gets, one step earlier.
#[inline]
fn control_len<T>(value: T) -> usize
where
    usize: TryFrom<T>,
{
    usize::try_from(value).unwrap_or(usize::MAX)
}

/// Write `msg_controllen` for a control buffer of `bytes` bytes.
///
/// `as _` writes the field in the type the header declares (`size_t` in glibc,
/// `socklen_t` in musl) instead of in the type one libc happens to call it, so
/// there is no `cfg` and no second copy of the send path. The buffer is
/// [`CmsgBuffer::LEN`] bytes and that length is asserted below to fit a
/// `socklen_t`, so the narrowing cannot lose anything on any target.
#[inline]
fn set_control_len(msg: &mut libc::msghdr, bytes: usize) {
    msg.msg_controllen = bytes as _;
}

/// Write `cmsg_len` for a control message that carries `payload` bytes.
///
/// # Safety
///
/// `header` must point at a `cmsghdr` that lies inside a control buffer this
/// process owns and that is alive for the write.
unsafe fn set_cmsg_len(header: *mut libc::cmsghdr, payload: usize) {
    // `CMSG_LEN` takes a `c_uint` and returns one on every Linux libc.
    let payload = u32::try_from(payload).unwrap_or(u32::MAX);
    // SAFETY: `CMSG_LEN` is arithmetic from libc's headers; it reads no memory
    // and its argument is a length, not a pointer.
    let len = unsafe { libc::CMSG_LEN(payload) };
    // SAFETY: the caller guarantees `header` points at a live `cmsghdr`. The
    // write stays a raw-pointer write — the same buffer is also reached through
    // `CMSG_DATA` — so no `&mut` is created that would claim the whole header.
    unsafe { (*header).cmsg_len = len as _ };
}

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
    set_control_len(&mut msg, buffer.0.len());

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
        // SAFETY: `cmsg` points into `buffer`, which is alive here.
        set_cmsg_len(cmsg, std::mem::size_of::<libc::c_int>());
        ptr::write_unaligned(libc::CMSG_DATA(cmsg).cast::<libc::c_int>(), fd.as_raw_fd());
    }
    // SAFETY: `CMSG_SPACE` is arithmetic from libc's headers; it reads no
    // memory.
    let space = unsafe { libc::CMSG_SPACE(std::mem::size_of::<libc::c_int>() as u32) };
    set_control_len(&mut msg, control_len(space));

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
    set_control_len(&mut msg, buffer.0.len());

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
    let buffer_end = buffer_start.saturating_add(control_len(msg.msg_controllen));
    // SAFETY: the caller guarantees `msg` describes a valid control buffer
    // that outlives this function.
    let mut cmsg = unsafe { libc::CMSG_FIRSTHDR(msg) };
    while !cmsg.is_null() {
        // SAFETY: `cmsg` points inside the control buffer `msg` describes.
        let (level, kind, len) = unsafe {
            (
                (*cmsg).cmsg_level,
                (*cmsg).cmsg_type,
                control_len((*cmsg).cmsg_len),
            )
        };
        if level == libc::SOL_SOCKET && kind == libc::SCM_RIGHTS {
            // SAFETY: `CMSG_LEN(0)` is the header length of an empty control
            // message; what is left after the header is the payload.
            let header_len = control_len(unsafe { libc::CMSG_LEN(0) });
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

/// The control buffer must fit the narrowest length field any Linux libc
/// declares for it (`socklen_t`, `u32`, in musl), or [`set_control_len`] would
/// truncate the length it writes. Two headers and a descriptor are a few dozen
/// bytes, so this holds with room to spare on every target — and now it is
/// checked at compile time instead of being a comment.
const _: () = assert!(CmsgBuffer::LEN <= libc::socklen_t::MAX as usize);

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
    let dir = open_dir_nofollow(parent)?;

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
///
/// # Kernels without `fchmodat2`
///
/// `fchmodat2` is Linux 6.5 and newer, and gVisor does not have it. The
/// decision here is to **degrade, not refuse**: `ENOSYS`/`EINVAL` falls back to
/// [`chmod_socket_via_proc`], which reaches the same inode through an
/// `O_PATH|O_NOFOLLOW` descriptor and a `/proc/self/fd/<n>` magic link, and so
/// keeps the no-symlink-follow guarantee this function exists for. Refusing
/// would make every pre-6.5 kernel (and every gVisor host) unable to start a
/// pg proxy at all, for a guarantee the fallback also provides.
///
/// The fallback failing is still fatal — the caller returns before `listen`, so
/// a socket never becomes reachable with the umask's mode. On a host with no
/// `/proc` both routes are unavailable and that error is what the caller sees;
/// it names a requirement we cannot meet rather than serving a too-open socket.
fn chmod_socket(dir: BorrowedFd<'_>, name: &OsStr, path: &Path, mode: u32) -> io::Result<()> {
    match fchmodat2(dir, name, mode) {
        Ok(()) => Ok(()),
        // No `fchmodat2`: an architecture whose syscall number this crate
        // cannot trust, a kernel before 6.5, gVisor, or a seccomp filter that
        // answers `EINVAL`.
        Err(err) if matches!(err.raw_os_error(), Some(libc::ENOSYS) | Some(libc::EINVAL)) => {
            chmod_socket_via_proc(dir, name, path, mode)
        }
        Err(err) => Err(err),
    }
}

/// `fchmodat2`'s syscall number, where this crate can state it.
///
/// `fchmodat2` was added to Linux's *unified* syscall table, where one number
/// serves every architecture: 452. `libc` 0.2.191 only publishes
/// `SYS_fchmodat2` for some of those architectures (x86, x86_64, m68k, s390x,
/// loongarch64) — aarch64, arm and riscv are missing it, which is what broke
/// the aarch64 build — so the number is written here.
///
/// The list is deliberately the architectures that share the unified table.
/// alpha, mips, parisc and sparc keep their own numbering, where 452 is some
/// *other* syscall: guessing there would not compile-fail, it would call
/// something unrelated. On those the constant is `None`, [`fchmodat2`] reports
/// `ENOSYS` without making a call, and [`chmod_socket`] takes the `/proc`
/// route it already takes on pre-6.5 kernels.
#[cfg(any(
    target_arch = "x86",
    target_arch = "x86_64",
    target_arch = "aarch64",
    target_arch = "arm",
    target_arch = "riscv32",
    target_arch = "riscv64",
    target_arch = "s390x",
    target_arch = "loongarch64",
))]
const FCHMODAT2_SYSCALL: Option<libc::c_long> = Some(452);

/// See the unified-table constant above.
#[cfg(not(any(
    target_arch = "x86",
    target_arch = "x86_64",
    target_arch = "aarch64",
    target_arch = "arm",
    target_arch = "riscv32",
    target_arch = "riscv64",
    target_arch = "s390x",
    target_arch = "loongarch64",
)))]
const FCHMODAT2_SYSCALL: Option<libc::c_long> = None;

/// `fchmodat2(dirfd, name, mode, AT_SYMLINK_NOFOLLOW)`.
fn fchmodat2(dir: BorrowedFd<'_>, name: &OsStr, mode: u32) -> io::Result<()> {
    let Some(number) = FCHMODAT2_SYSCALL else {
        // Not available as a trustworthy number on this architecture; the
        // caller reads `ENOSYS` as "no `fchmodat2` here" and falls back.
        return Err(io::Error::from_raw_os_error(libc::ENOSYS));
    };
    let name = CString::new(name.as_bytes())
        .map_err(|_| io::Error::other("a file name cannot contain a NUL byte"))?;
    // SAFETY: `fchmodat2` takes a descriptor, a NUL-terminated path, a mode and
    // a flag word; it reads no other memory. `AT_SYMLINK_NOFOLLOW` is the flag
    // that makes it refuse a symlink at the last component.
    let ret = unsafe {
        libc::syscall(
            number,
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
