//! Protected critical suite for `SeccompRole::{View, EmbeddedView}` (R14).
//!
//! This is the **public-surface** oracle: `make test-critical` runs it as
//! `cargo test -p blastit-sys --test critical_embedded_seccomp --locked`, with
//! no feature flags, so it must not depend on `feature = "testutil"` (a
//! `#![cfg(feature = …)]` file would silently run zero tests). It forks with
//! `libc::fork` and drives the installed filter with raw syscalls, proving the
//! rules a consumer actually gets through [`blastit_sys::install`].
//!
//! The deeper program-shape oracle — exact pinned instruction counts, the
//! sorted 35-entry `DENIED` table and the `CLONE_NEW*` mask bits — lives in
//! `src/seccomp.rs`'s unit tests, where it can see the private `Program`.
//! Together the two files are R14's "zero widening" proof.

use std::io;

use blastit_sys::SeccompRole;

/// The eight `CLONE_NEW*` flags, as `clone(2)` reads them in `args[0]`.
const CLONE_NEW_FLAGS: [libc::c_ulong; 8] = [
    0x0000_0080, // CLONE_NEWTIME
    0x0002_0000, // CLONE_NEWNS
    0x0200_0000, // CLONE_NEWCGROUP
    0x0400_0000, // CLONE_NEWUTS
    0x0800_0000, // CLONE_NEWIPC
    0x1000_0000, // CLONE_NEWUSER
    0x2000_0000, // CLONE_NEWPID
    0x4000_0000, // CLONE_NEWNET
];

/// The `errno` of the last failed call.
fn errno() -> Option<i32> {
    io::Error::last_os_error().raw_os_error()
}

/// Reap a child the test created; `None` means "the call was permitted".
fn reap(pid: libc::pid_t) -> Option<i32> {
    let mut status = 0;
    loop {
        // SAFETY: `waitpid` writes one status word into the valid storage
        // `status`; only this process's own child is waited for.
        let waited = unsafe { libc::waitpid(pid, &mut status, 0) };
        if waited == pid {
            return None;
        }
        if errno() != Some(libc::EINTR) {
            return Some(libc::EINVAL);
        }
    }
}

/// `socket(domain, SOCK_STREAM, 0)`, closing what it created.
fn try_socket(domain: libc::c_int) -> Option<i32> {
    // SAFETY: `socket` takes three integers and returns a descriptor.
    let fd = unsafe { libc::socket(domain, libc::SOCK_STREAM, 0) };
    if fd == -1 {
        return errno();
    }
    // SAFETY: the descriptor was just created here and nothing else owns it.
    unsafe { libc::close(fd) };
    None
}

/// `clone3(NULL, 0)`: `EFAULT` unfiltered, `ENOSYS` through the filter.
fn try_clone3() -> Option<i32> {
    // SAFETY: `clone3` takes the `args` pointer and its size; a null pointer
    // is rejected with `EFAULT` before it is read.
    let ret = unsafe {
        libc::syscall(
            libc::SYS_clone3,
            std::ptr::null_mut::<libc::c_void>(),
            0_usize,
        )
    };
    if ret == -1 { errno() } else { None }
}

/// `clone(flags, NULL, NULL, NULL, 0)`; the child `_exit`s immediately.
fn try_clone(flags: libc::c_ulong) -> Option<i32> {
    // SAFETY: without `CLONE_VM` the null stack makes this a process fork,
    // which the child body (one `_exit`) is safe for.
    let pid = unsafe {
        libc::syscall(
            libc::SYS_clone,
            flags,
            std::ptr::null_mut::<libc::c_void>(),
            std::ptr::null_mut::<libc::c_void>(),
            std::ptr::null_mut::<libc::c_void>(),
            0 as libc::c_ulong,
        )
    };
    if pid == -1 {
        return errno();
    }
    if pid == 0 {
        // SAFETY: this is the new process; `_exit` never returns and runs no
        // destructors.
        unsafe { libc::_exit(0) }
    }
    reap(pid as libc::pid_t)
}

/// `execve("/nonexistent", …)`: `ENOENT` permitted, `EPERM` refused.
fn try_execve() -> Option<i32> {
    let path = b"/nonexistent-blastit-seccomp\0";
    let argv = [std::ptr::null::<libc::c_char>()];
    let envp = [std::ptr::null::<libc::c_char>()];
    // SAFETY: `path` is NUL-terminated and both arrays are NULL-terminated; a
    // missing file is `ENOENT` before any image runs.
    let ret = unsafe {
        libc::syscall(
            libc::SYS_execve,
            path.as_ptr().cast::<libc::c_char>(),
            argv.as_ptr(),
            envp.as_ptr(),
        )
    };
    if ret == -1 { errno() } else { None }
}

/// `execveat(AT_FDCWD, "/nonexistent", …)`.
fn try_execveat() -> Option<i32> {
    let path = b"/nonexistent-blastit-seccomp\0";
    let argv = [std::ptr::null::<libc::c_char>()];
    let envp = [std::ptr::null::<libc::c_char>()];
    // SAFETY: the same NUL- and NULL-terminated arguments as `try_execve`,
    // resolved from the current directory.
    let ret = unsafe {
        libc::syscall(
            libc::SYS_execveat,
            libc::AT_FDCWD,
            path.as_ptr().cast::<libc::c_char>(),
            argv.as_ptr(),
            envp.as_ptr(),
            0 as libc::c_int,
        )
    };
    if ret == -1 { errno() } else { None }
}

/// `setsid()`.
fn try_setsid() -> Option<i32> {
    // SAFETY: `setsid` takes no arguments.
    if unsafe { libc::setsid() } == -1 {
        errno()
    } else {
        None
    }
}

/// `setpgid(0, 0)`, the calling process into its own group.
fn try_setpgid() -> Option<i32> {
    // SAFETY: `setpgid` takes two pids; 0 means the calling process.
    if unsafe { libc::setpgid(0, 0) } == -1 {
        errno()
    } else {
        None
    }
}

/// `fork(2)` where the architecture's table has it; the child `_exit`s.
#[cfg(target_arch = "x86_64")]
fn try_fork() -> Option<i32> {
    // SAFETY: `fork` takes no arguments; the child only `_exit`s.
    let pid = unsafe { libc::syscall(libc::SYS_fork) };
    if pid == -1 {
        return errno();
    }
    if pid == 0 {
        // SAFETY: this is the new process; `_exit` never returns.
        unsafe { libc::_exit(0) }
    }
    reap(pid as libc::pid_t)
}

/// `ptrace(PTRACE_TRACEME, 0, 0, 0)`, which an unprivileged process may
/// otherwise call on itself.
fn try_ptrace() -> Option<i32> {
    // SAFETY: `ptrace` takes an enum and three word-sized arguments.
    let traced = unsafe {
        libc::ptrace(
            libc::PTRACE_TRACEME,
            0 as libc::pid_t,
            std::ptr::null_mut::<libc::c_void>(),
            std::ptr::null_mut::<libc::c_void>(),
        )
    };
    match traced {
        -1 => errno(),
        _ => None,
    }
}

/// `bpf(0, NULL, 0)`: a representative shared denial.
fn try_bpf() -> Option<i32> {
    // SAFETY: `bpf` takes a command, an attribute pointer and its size; the
    // command is refused before the null pointer is read.
    let ret = unsafe {
        libc::syscall(
            libc::SYS_bpf,
            0 as libc::c_int,
            std::ptr::null_mut::<libc::c_void>(),
            0_usize,
        )
    };
    if ret == -1 { errno() } else { None }
}

/// `mount(NULL, NULL, NULL, 0, NULL)`: a representative shared denial.
fn try_mount() -> Option<i32> {
    // SAFETY: `mount` takes two path pointers, a filesystem-type pointer, a
    // flag word and a data pointer; all are null here.
    let ret = unsafe {
        libc::syscall(
            libc::SYS_mount,
            std::ptr::null::<libc::c_char>(),
            std::ptr::null::<libc::c_char>(),
            std::ptr::null::<libc::c_char>(),
            0 as libc::c_ulong,
            std::ptr::null::<libc::c_void>(),
        )
    };
    if ret == -1 { errno() } else { None }
}

/// `unshare(0)`: a successful no-op unfiltered, `EPERM` through the filter.
fn try_unshare() -> Option<i32> {
    // SAFETY: `unshare` takes one flag word; zero creates nothing.
    let ret = unsafe { libc::syscall(libc::SYS_unshare, 0 as libc::c_int) };
    if ret == -1 { errno() } else { None }
}

/// Run `body` in a forked child and require a normal exit code 0.
///
/// Forking from the harness means the child is a copy of a multi-threaded
/// process. The child bodies are deliberately short and mostly syscall-shaped,
/// which is what keeps that sound in practice — the same contract
/// `crates/blastit-sys/src/testutil.rs` keeps.
fn in_child(label: &str, body: impl FnOnce() -> i32) {
    // SAFETY: the child is a copy of a multi-threaded process, so its body is
    // kept short and mostly syscall-shaped — it installs a filter and makes raw
    // syscalls, with no intentional lock-holding and no thread beyond the one
    // under test — and it ends in `_exit` without unwinding.
    let pid = unsafe { libc::fork() };
    match pid {
        -1 => panic!("{label}: fork failed: {}", io::Error::last_os_error()),
        0 => {
            let code = body();
            // SAFETY: `_exit` never returns and runs no destructors.
            unsafe { libc::_exit(code) }
        }
        pid => {
            let mut status = 0;
            loop {
                // SAFETY: the status word is valid storage for the one child.
                let waited = unsafe { libc::waitpid(pid, &mut status, 0) };
                if waited == pid {
                    break;
                }
                let err = io::Error::last_os_error();
                if err.raw_os_error() == Some(libc::EINTR) {
                    continue;
                }
                panic!("{label}: waitpid failed: {err}");
            }
            assert!(
                libc::WIFEXITED(status),
                "{label}: the child died from signal {}",
                libc::WTERMSIG(status)
            );
            assert_eq!(
                libc::WEXITSTATUS(status),
                0,
                "{label}: the child reported failure"
            );
        }
    }
}

#[test]
fn the_roles_describe_themselves() {
    assert_eq!(SeccompRole::View { egress: false }.name(), "view");
    assert_eq!(
        SeccompRole::EmbeddedView { egress: false }.name(),
        "embedded-view"
    );
    assert_eq!(SeccompRole::Http.name(), "http");
    assert_eq!(SeccompRole::Scheduler.name(), "scheduler");
    assert_eq!(SeccompRole::DbProxy.name(), "dbproxy");

    assert!(!SeccompRole::View { egress: false }.egress());
    assert!(SeccompRole::View { egress: true }.egress());
    assert!(!SeccompRole::EmbeddedView { egress: false }.egress());
    assert!(SeccompRole::EmbeddedView { egress: true }.egress());
    assert!(SeccompRole::DbProxy.egress());
    assert!(!SeccompRole::Http.egress());
    assert!(!SeccompRole::Scheduler.egress());

    // PHP needs `AF_UNIX` for dbproxy, so the embedded view keeps sockets;
    // only http and the scheduler have none at all.
    assert!(SeccompRole::EmbeddedView { egress: false }.sockets());
    assert!(!SeccompRole::Http.sockets());
    assert!(!SeccompRole::Scheduler.sockets());
}

#[test]
fn embedded_view_refuses_exec_process_creation_and_new_namespaces() {
    in_child("embedded view", || {
        if blastit_sys::install(SeccompRole::EmbeddedView { egress: false }).is_err() {
            return 1;
        }
        match try_execve() {
            Some(libc::EPERM) => {}
            _ => return 2,
        }
        match try_execveat() {
            Some(libc::EPERM) => {}
            _ => return 3,
        }
        // A `clone` without `CLONE_THREAD` is a new process.
        match try_clone(libc::SIGCHLD as libc::c_ulong) {
            Some(libc::EPERM) => {}
            _ => return 4,
        }
        for flag in CLONE_NEW_FLAGS {
            match try_clone(flag) {
                Some(libc::EPERM) => {}
                _ => return 5,
            }
        }
        match try_setsid() {
            Some(libc::EPERM) => {}
            _ => return 6,
        }
        match try_setpgid() {
            Some(libc::EPERM) => {}
            _ => return 7,
        }
        #[cfg(target_arch = "x86_64")]
        match try_fork() {
            Some(libc::EPERM) => {}
            _ => return 8,
        }
        0
    });
}

#[test]
fn view_keeps_exec_and_process_creation_but_shares_the_hardening() {
    in_child("view", || {
        if blastit_sys::install(SeccompRole::View { egress: false }).is_err() {
            return 1;
        }
        // The PHP-only refusals must not reach the WSGI/Node/JVM role: a
        // missing file is `ENOENT`, which only an image lookup produces.
        match try_execve() {
            Some(libc::ENOENT) => {}
            _ => return 2,
        }
        match try_execveat() {
            Some(libc::ENOENT) => {}
            _ => return 3,
        }
        if try_clone(libc::SIGCHLD as libc::c_ulong).is_some() {
            return 4;
        }
        #[cfg(target_arch = "x86_64")]
        if try_fork().is_some() {
            return 5;
        }
        // The shared hardening is still there.
        match try_setsid() {
            Some(libc::EPERM) => {}
            _ => return 6,
        }
        match try_setpgid() {
            Some(libc::EPERM) => {}
            _ => return 7,
        }
        for flag in CLONE_NEW_FLAGS {
            match try_clone(flag) {
                Some(libc::EPERM) => {}
                _ => return 8,
            }
        }
        0
    });
}

#[test]
fn both_views_refuse_clone3_and_still_start_a_thread() {
    for (label, role) in [
        ("view clone3", SeccompRole::View { egress: false }),
        (
            "embedded view clone3",
            SeccompRole::EmbeddedView { egress: false },
        ),
    ] {
        in_child(label, move || {
            if blastit_sys::install(role).is_err() {
                return 1;
            }
            // glibc's `pthread_create` needs this `ENOSYS` to fall back to
            // `clone`; `EPERM` here would break every threaded runtime.
            match try_clone3() {
                Some(libc::ENOSYS) => {}
                _ => return 2,
            }
            match std::thread::spawn(|| 7).join() {
                Ok(7) => 0,
                _ => 3,
            }
        });
    }
}

#[test]
fn both_views_refuse_representative_shared_calls() {
    // These four are the shared `DENIED` table as a running process sees it:
    // `ptrace` and `unshare(0)` succeed unfiltered, and the filter is the only
    // thing that turns all four into `EPERM` for an unprivileged worker. This
    // is the behavioural half of the zero-widening oracle (the literal numbers
    // are pinned in `src/seccomp.rs`).
    for (label, role) in [
        ("view shared calls", SeccompRole::View { egress: false }),
        (
            "embedded view shared calls",
            SeccompRole::EmbeddedView { egress: false },
        ),
    ] {
        in_child(label, move || {
            if blastit_sys::install(role).is_err() {
                return 1;
            }
            for (code, result) in [
                (2, try_ptrace()),
                (3, try_bpf()),
                (4, try_mount()),
                (5, try_unshare()),
            ] {
                match result {
                    Some(libc::EPERM) => {}
                    _ => return code,
                }
            }
            0
        });
    }
}

#[test]
fn embedded_view_egress_reflects_the_flag() {
    in_child("embedded view no egress", || {
        if blastit_sys::install(SeccompRole::EmbeddedView { egress: false }).is_err() {
            return 1;
        }
        if try_socket(libc::AF_UNIX).is_some() {
            return 2;
        }
        match try_socket(libc::AF_INET) {
            Some(libc::EPERM) => {}
            _ => return 3,
        }
        0
    });

    in_child("embedded view egress", || {
        if blastit_sys::install(SeccompRole::EmbeddedView { egress: true }).is_err() {
            return 1;
        }
        if try_socket(libc::AF_INET).is_some() {
            return 2;
        }
        // Egress is a permission, not a requirement.
        match try_socket(libc::AF_NETLINK) {
            Some(libc::EPERM) => 0,
            _ => 3,
        }
    });
}
