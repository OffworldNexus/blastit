//! Sockets, pipes, descriptor passing and the runtime directories they live in.

use std::io;
use std::mem::MaybeUninit;
use std::net::{SocketAddr, TcpListener};
use std::os::fd::{AsRawFd, BorrowedFd, FromRawFd, OwnedFd};
use std::os::unix::net::UnixListener;
use std::path::Path;
use std::ptr;

use rustix::fs::{Gid, Mode, OFlags, Uid};
use rustix::net::sockopt::{set_socket_passcred, set_socket_reuseaddr};
use rustix::net::{
    AddressFamily, SocketAddrUnix, SocketFlags, SocketType, bind, listen, socket_with,
};

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
/// Returns how many bytes were written; a short write is possible, so a caller
/// that must deliver all of `bytes` should check.
pub fn send_with_fd(sock: BorrowedFd<'_>, bytes: &[u8], fd: BorrowedFd<'_>) -> io::Result<usize> {
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
/// # Safety
///
/// `msg` must be the message header of a successful `recvmsg` call, whose
/// control buffer is still alive; every descriptor in it must have been
/// installed by the kernel and not yet closed.
unsafe fn collect_fds(msg: &libc::msghdr) -> Vec<OwnedFd> {
    let mut out = Vec::new();
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
            let count = payload / std::mem::size_of::<libc::c_int>();
            let data = unsafe { libc::CMSG_DATA(cmsg).cast::<libc::c_int>() };
            for index in 0..count {
                // SAFETY: the control message carries `count` descriptors.
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

/// Control-message buffer, aligned the way `cmsghdr` needs it to be.
#[repr(align(8))]
struct CmsgBuffer([u8; Self::LEN]);

impl CmsgBuffer {
    /// `CMSG_SPACE` for one `c_int` of payload.
    const LEN: usize =
        unsafe { libc::CMSG_SPACE(std::mem::size_of::<libc::c_int>() as u32) } as usize;

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
/// A file already at `path` is removed first: after a restart `/run` may still
/// hold the previous socket.
pub fn bind_unix_listener(path: &Path, owner_uid: u32, mode: u32) -> io::Result<UnixListener> {
    match std::fs::remove_file(path) {
        Ok(()) => {}
        Err(err) if err.kind() == io::ErrorKind::NotFound => {}
        Err(err) => return Err(err),
    }
    let addr = SocketAddrUnix::new(path)?;
    let fd = socket_with(
        AddressFamily::UNIX,
        SocketType::STREAM,
        SocketFlags::CLOEXEC,
        None,
    )?;
    bind(&fd, &addr)?;
    set_socket_passcred(&fd, true)?;
    // Permissions before the socket can accept: until `listen` runs, no client
    // can reach it, and the file mode is what keeps other uids out (§9.1).
    rustix::fs::chown(
        path,
        Some(Uid::from_raw(owner_uid)),
        Some(Gid::from_raw(owner_uid)),
    )?;
    rustix::fs::chmod(path, Mode::from_bits_truncate(mode))?;
    listen(&fd, UNIX_BACKLOG)?;
    Ok(UnixListener::from(fd))
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

    // Opening with `O_NOFOLLOW` is the real check: a symlink named `path` fails
    // here even if something raced between the `lstat` and the `mkdir`.
    drop(rustix::fs::open(
        path,
        OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::empty(),
    )?);
    rustix::fs::chmod(path, Mode::from_bits_truncate(mode))?;
    rustix::fs::chown(path, Some(Uid::from_raw(uid)), Some(Gid::from_raw(uid)))?;
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

    use rustix::net::sockopt::{socket_passcred, socket_reuseaddr};

    use super::*;
    use crate::Error;
    use crate::fd::fd_is_cloexec;
    use crate::testutil::{
        TempDir, euid, in_child, is_root, must, must_err, must_some, open_fds, read_to_end,
        write_all,
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

        let client = std::os::unix::net::UnixStream::connect(&socket);
        let _client = match client {
            Ok(client) => client,
            // A non-root test cannot connect to a socket owned by 20500, which
            // is exactly the property being relied on.
            Err(_) => {
                assert!(is_root(), "the test owner and the socket owner differ");
                return;
            }
        };
        let (accepted, _addr) = must(listener.accept());
        assert!(
            must(socket_passcred(accepted.as_fd())),
            "the accepted socket inherits SO_PASSCRED"
        );
        write_all(accepted.as_fd(), b"R");
    }

    #[test]
    fn bind_unix_listener_replaces_a_stale_socket_file() {
        let dir = TempDir::new("unix-stale");
        must(std::fs::create_dir_all(dir.path()));
        let owner = euid();
        let socket = dir.path().join(".s.PGSQL.5432");
        must(std::fs::write(&socket, b"stale"));
        assert!(socket.exists());

        let listener = must(bind_unix_listener(&socket, owner, 0o600));
        assert!(
            must(std::fs::symlink_metadata(&socket))
                .file_type()
                .is_socket()
        );
        drop(listener);
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
