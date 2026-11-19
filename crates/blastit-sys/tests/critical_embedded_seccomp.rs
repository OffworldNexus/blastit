//! Protected critical suite for `SeccompRole::{View, EmbeddedView}` (R14).
//!
//! This is the **public-surface** oracle: `make test-critical` runs it as
//! `cargo test -p blastit-sys --test critical_embedded_seccomp --locked`, with
//! no feature flags, so it must not depend on `feature = "testutil"` (a
//! `#![cfg(feature = …)]` file would silently run zero tests). It forks with
//! `libc::fork` and drives the installed filter with raw syscalls, proving the
//! rules a consumer actually gets through [`blastit_sys::install`].
//!
//! **Every forked child here makes async-signal-safe calls only.** The fork
//! happens inside the multi-threaded `libtest` process, where another thread may
//! hold the allocator's lock — or a stdio lock, or a lock inside the harness —
//! at the instant of the fork, and the child has no other thread to release it:
//! a child that allocates waits there forever (that is how `make fast-check`
//! once hung for ten minutes, and how a second run was OOM-killed spawning a
//! thread). So between `fork()` and `_exit` a child runs `prctl`, `seccomp` and
//! raw syscalls and nothing else: no `Vec`, `String`, `format!`, `println!`,
//! `assert!`, `unwrap`, `Box`, `panic!`, no `Drop` that frees, and nothing that
//! can allocate or take a lock. `blastit_sys::install` is allowed there because
//! it resolves its program into a fixed stack array and reads
//! `/proc/self/status` into a stack buffer — on a supported kernel it allocates
//! nothing, and its error path (a kernel refusing a filter built for it) is the
//! one branch a child cannot reach on a healthy host.
//!
//! The single body that must allocate — `std::thread::spawn`, to prove glibc's
//! `pthread_create` still works behind the filter — runs in a **fresh process**
//! instead, spawned with [`std::process::Command`]: `execve` gives it a private,
//! unshared address space, where allocating and starting threads are safe. See
//! [`run_probe`].
//!
//! The deeper program-shape oracle — exact pinned instruction counts, the
//! sorted 35-entry `DENIED` table and the `CLONE_NEW*` mask bits — lives in
//! `src/seccomp.rs`'s unit tests, where it can see the private `Program`.
//! Together the two files are R14's "zero widening" proof.

use std::io;

use blastit_sys::{PreparedFilter, SeccompRole};

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

// Every helper below reports "permitted" as `0` and a refusal as its `errno`,
// so a forked child's answer is a plain integer: `errno` is never zero, and
// building an `io::Error` in the child is exactly the allocation the module
// docs forbid.

/// The `errno` of the last failed call.
fn errno() -> i32 {
    // SAFETY: `__errno_location` returns this thread's `errno` cell.
    unsafe { *libc::__errno_location() }
}

/// Close every descriptor at or above 3, with one raw syscall.
///
/// A descriptor without `FD_CLOEXEC` — `dup_onto`'s whole subject in the crate's
/// unit tests — must not be held open by a child of this process. Production
/// closes them the same way in `close_fds_except`; a child cannot run that
/// function's allocating scan fallback, so a kernel older than 5.9 keeps them
/// for the microseconds until `_exit`.
fn close_inherited() {
    // SAFETY: `close_range` takes three integers and no pointers.
    unsafe {
        libc::syscall(
            libc::SYS_close_range,
            3 as libc::c_uint,
            u32::MAX,
            0 as libc::c_uint,
        )
    };
}

/// Reap a child the test created; `0` means it was reaped.
fn reap(pid: libc::pid_t) -> i32 {
    let mut status = 0;
    loop {
        // SAFETY: `waitpid` writes one status word into the valid storage
        // `status`; only this process's own child is waited for.
        let waited = unsafe { libc::waitpid(pid, &mut status, 0) };
        if waited == pid {
            return 0;
        }
        if errno() != libc::EINTR {
            return libc::EINVAL;
        }
    }
}

/// `socket(domain, SOCK_STREAM, 0)`, closing what it created.
fn try_socket(domain: libc::c_int) -> i32 {
    // SAFETY: `socket` takes three integers and returns a descriptor.
    let fd = unsafe { libc::socket(domain, libc::SOCK_STREAM, 0) };
    if fd == -1 {
        return errno();
    }
    // SAFETY: the descriptor was just created here and nothing else owns it.
    unsafe { libc::close(fd) };
    0
}

/// `clone3(NULL, 0)`: `EFAULT` unfiltered, `ENOSYS` through the filter.
fn try_clone3() -> i32 {
    // SAFETY: `clone3` takes the `args` pointer and its size; a null pointer
    // is rejected with `EFAULT` before it is read.
    let ret = unsafe {
        libc::syscall(
            libc::SYS_clone3,
            std::ptr::null_mut::<libc::c_void>(),
            0_usize,
        )
    };
    if ret == -1 { errno() } else { 0 }
}

/// `clone(flags, NULL, NULL, NULL, 0)`; the child `_exit`s immediately.
fn try_clone(flags: libc::c_ulong) -> i32 {
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
fn try_execve() -> i32 {
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
    if ret == -1 { errno() } else { 0 }
}

/// `execveat(AT_FDCWD, "/nonexistent", …)`.
fn try_execveat() -> i32 {
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
    if ret == -1 { errno() } else { 0 }
}

/// `setsid()`.
fn try_setsid() -> i32 {
    // SAFETY: `setsid` takes no arguments.
    if unsafe { libc::setsid() } == -1 {
        errno()
    } else {
        0
    }
}

/// `setpgid(0, 0)`, the calling process into its own group.
fn try_setpgid() -> i32 {
    // SAFETY: `setpgid` takes two pids; 0 means the calling process.
    if unsafe { libc::setpgid(0, 0) } == -1 {
        errno()
    } else {
        0
    }
}

/// `fork(2)` where the architecture's table has it; the child `_exit`s.
#[cfg(target_arch = "x86_64")]
fn try_fork() -> i32 {
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
fn try_ptrace() -> i32 {
    // SAFETY: `ptrace` takes an enum and three word-sized arguments, none of
    // which `PTRACE_TRACEME` reads.
    let traced = unsafe {
        libc::ptrace(
            libc::PTRACE_TRACEME,
            0 as libc::pid_t,
            std::ptr::null_mut::<libc::c_void>(),
            std::ptr::null_mut::<libc::c_void>(),
        )
    };
    if traced == -1 { errno() } else { 0 }
}

/// `bpf(0, NULL, 0)`: a representative shared denial.
fn try_bpf() -> i32 {
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
    if ret == -1 { errno() } else { 0 }
}

/// `mount(NULL, NULL, NULL, 0, NULL)`: a representative shared denial.
fn try_mount() -> i32 {
    // SAFETY: `mount` takes two path pointers, a filesystem-type pointer, a
    // flag word and a data pointer; all are null here.
    let ret = unsafe {
        libc::syscall(
            libc::SYS_mount,
            std::ptr::null::<libc::c_char>(),
            std::ptr::null::<libc::c_char>(),
            std::ptr::null::<libc::c_char>(),
            0 as libc::c_ulong,
            std::ptr::null_mut::<libc::c_void>(),
        )
    };
    if ret == -1 { errno() } else { 0 }
}

/// `unshare(0)`: a successful no-op unfiltered, `EPERM` through the filter.
fn try_unshare() -> i32 {
    // SAFETY: `unshare` takes one flag word; zero creates nothing.
    let ret = unsafe { libc::syscall(libc::SYS_unshare, 0 as libc::c_int) };
    if ret == -1 { errno() } else { 0 }
}

/// Install `role`'s filter in a forked child, run `body` there, and require a
/// clean exit.
///
/// # The async-signal-safe rule
///
/// The child is a copy of the multi-threaded harness, so between `fork()` and
/// `_exit` it must make async-signal-safe calls only — see the module docs.
/// `body` therefore returns one exit code and nothing else: every assertion and
/// every message belong to the parent, after `waitpid`.
///
/// The child reports `254` when the filter could not be installed, which the
/// parent turns into a message of its own; a body's own codes stay below it.
fn in_child(label: &str, role: SeccompRole, body: impl FnOnce() -> i32) {
    // SAFETY: the child body is async-signal-safe by construction: it installs
    // the filter, closes inherited descriptors and makes raw syscalls, then
    // `_exit`s without unwinding. Nothing in it allocates or takes a lock.
    let pid = unsafe { libc::fork() };
    match pid {
        -1 => panic!("{label}: fork failed: {}", io::Error::last_os_error()),
        0 => {
            // Raw syscalls only from here to `_exit`: see the module docs.
            close_inherited();
            let code = if blastit_sys::install(role).is_err() {
                254
            } else {
                body()
            };
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
            let code = libc::WEXITSTATUS(status);
            assert_ne!(
                code, 254,
                "{label}: the forked child could not install the filter"
            );
            assert_eq!(code, 0, "{label}: the child reported failure {code}");
        }
    }
}

/// The environment variable naming the probe a spawned test process runs.
const PROBE_VAR: &str = "BLASTIT_SYS_SECCOMP_PROBE";

/// The prefix of the line a probe process writes to its stdout.
const PROBE_MARKER: &str = "blastit-sys-probe:";

/// The test that doubles as this suite's probe entry point.
///
/// It is one test in both modes, so the suite keeps its test count: the parent
/// runs it (no `PROBE_VAR`) to spawn the probes, and the spawned copy (filtered
/// to this one name, `PROBE_VAR` set) runs one body and exits.
const PROBE_RUNNER: &str = "both_views_refuse_clone3_and_still_start_a_thread";

/// Run probe `name` in a **fresh process** of this same test binary and return
/// the code it reported.
///
/// The body needs to allocate and start a thread, which a child of `fork()`
/// inside the multi-threaded harness must not do; `execve` gives the spawned
/// copy a private, unshared address space where both are safe. The marker line
/// proves the probe really ran, rather than a filter that matched nothing
/// exiting `0` with nothing done.
fn run_probe(name: &str) -> i32 {
    let exe = match std::env::current_exe() {
        Ok(exe) => exe,
        Err(err) => panic!("the test binary has a path: {err}"),
    };
    let spawned = std::process::Command::new(exe)
        .args(["--test-threads=1", "--nocapture", PROBE_RUNNER])
        .env(PROBE_VAR, name)
        .output();
    let output = match spawned {
        Ok(output) => output,
        Err(err) => panic!("the test binary can be run again: {err}"),
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    let reported = stdout
        .split_whitespace()
        .find_map(|token| token.strip_prefix(PROBE_MARKER))
        .and_then(|rest| rest.rsplit_once(':'))
        .and_then(|(seen, code)| Some((seen, code.parse::<i32>().ok()?)));
    let Some((seen, code)) = reported else {
        panic!(
            "probe {name} never ran: exited {:?}\nstdout:\n{stdout}\nstderr:\n{}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr)
        );
    };
    assert_eq!(seen, name, "the test binary ran another probe");
    assert_eq!(
        output.status.code(),
        Some(code),
        "probe {name} reported {code} but exited with {:?}",
        output.status.code()
    );
    code
}

/// Report `code` for probe `name` on stdout and exit with it. Never returns.
fn report_probe(name: &str, code: i32) -> ! {
    let line = format!("{PROBE_MARKER}{name}:{code}\n");
    // SAFETY: `line` is a live buffer of `line.len()` bytes and 1 is this
    // process's stdout. A raw write, so the report does not depend on a
    // buffered writer being flushed by `process::exit` below.
    unsafe { libc::write(1, line.as_ptr().cast::<libc::c_void>(), line.len()) };
    std::process::exit(code);
}

/// The probe bodies: a process that may allocate and start threads.
fn probe(name: &str) -> Option<i32> {
    match name {
        "thread-after-filter-view" => Some(probe_thread_after_filter(SeccompRole::View {
            egress: false,
        })),
        "thread-after-filter-embedded" => {
            Some(probe_thread_after_filter(SeccompRole::EmbeddedView {
                egress: false,
            }))
        }
        _ => None,
    }
}

/// Install `role`'s filter, then prove a thread still starts behind it.
///
/// [`blastit_sys::PreparedFilter`] rather than `install`: this process is a
/// `libtest` harness, so it has a thread of its own and `install`'s
/// single-threaded precondition is what must not apply here — the probe is not
/// a boot. The filter is the same one `install` builds.
fn probe_thread_after_filter(role: SeccompRole) -> i32 {
    let Ok(filter) = PreparedFilter::for_role(role) else {
        return 1;
    };
    if filter.install().is_err() {
        return 1;
    }
    // glibc's `pthread_create` needs this `ENOSYS` to fall back to `clone`;
    // `EPERM` here would break every threaded runtime.
    if try_clone3() != libc::ENOSYS {
        return 2;
    }
    match std::thread::spawn(|| 7).join() {
        Ok(7) => 0,
        _ => 3,
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
    in_child(
        "embedded view",
        SeccompRole::EmbeddedView { egress: false },
        || {
            if try_execve() != libc::EPERM {
                return 2;
            }
            if try_execveat() != libc::EPERM {
                return 3;
            }
            // A `clone` without `CLONE_THREAD` is a new process.
            if try_clone(libc::SIGCHLD as libc::c_ulong) != libc::EPERM {
                return 4;
            }
            for flag in CLONE_NEW_FLAGS {
                if try_clone(flag) != libc::EPERM {
                    return 5;
                }
            }
            if try_setsid() != libc::EPERM {
                return 6;
            }
            if try_setpgid() != libc::EPERM {
                return 7;
            }
            #[cfg(target_arch = "x86_64")]
            if try_fork() != libc::EPERM {
                return 8;
            }
            0
        },
    );
}

#[test]
fn view_keeps_exec_and_process_creation_but_shares_the_hardening() {
    in_child("view", SeccompRole::View { egress: false }, || {
        // The PHP-only refusals must not reach the WSGI/Node/JVM role: a
        // missing file is `ENOENT`, which only an image lookup produces.
        if try_execve() != libc::ENOENT {
            return 2;
        }
        if try_execveat() != libc::ENOENT {
            return 3;
        }
        if try_clone(libc::SIGCHLD as libc::c_ulong) != 0 {
            return 4;
        }
        #[cfg(target_arch = "x86_64")]
        if try_fork() != 0 {
            return 5;
        }
        // The shared hardening is still there.
        if try_setsid() != libc::EPERM {
            return 6;
        }
        if try_setpgid() != libc::EPERM {
            return 7;
        }
        for flag in CLONE_NEW_FLAGS {
            if try_clone(flag) != libc::EPERM {
                return 8;
            }
        }
        0
    });
}

#[test]
fn both_views_refuse_clone3_and_still_start_a_thread() {
    // A probe process runs one body here and exits; the harness itself spawns
    // them. Both are the same test, so the suite's test count does not change.
    if let Ok(name) = std::env::var(PROBE_VAR) {
        report_probe(&name, probe(&name).unwrap_or(1));
    }
    for (label, name) in [
        ("view clone3", "thread-after-filter-view"),
        ("embedded view clone3", "thread-after-filter-embedded"),
    ] {
        assert_eq!(
            run_probe(name),
            0,
            "{label}: the thread did not start after the filter"
        );
    }
}

#[test]
fn both_views_refuse_representative_shared_calls() {
    // These four are the shared `DENIED` table as a running process sees it:
    // `ptrace` and `unshare(0)` succeed unfiltered, and the filter is the only
    // thing that turns all four into `EPERM` for an unprivileged worker. This
    // is the behavioural half of the zero-widening oracle (the literal numbers
    // are pinned in `src/seccomp.rs`).
    for role in [
        SeccompRole::View { egress: false },
        SeccompRole::EmbeddedView { egress: false },
    ] {
        in_child(&format!("{role:?} shared calls"), role, || {
            if try_ptrace() != libc::EPERM {
                return 2;
            }
            if try_bpf() != libc::EPERM {
                return 3;
            }
            if try_mount() != libc::EPERM {
                return 4;
            }
            if try_unshare() != libc::EPERM {
                return 5;
            }
            0
        });
    }
}

#[test]
fn embedded_view_egress_reflects_the_flag() {
    in_child(
        "embedded view no egress",
        SeccompRole::EmbeddedView { egress: false },
        || {
            if try_socket(libc::AF_UNIX) != 0 {
                return 2;
            }
            if try_socket(libc::AF_INET) != libc::EPERM {
                return 3;
            }
            0
        },
    );

    in_child(
        "embedded view egress",
        SeccompRole::EmbeddedView { egress: true },
        || {
            if try_socket(libc::AF_INET) != 0 {
                return 2;
            }
            // Egress is a permission, not a requirement.
            if try_socket(libc::AF_NETLINK) != libc::EPERM {
                return 3;
            }
            0
        },
    );
}
