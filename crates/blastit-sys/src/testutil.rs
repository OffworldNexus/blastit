//! Helpers shared by this crate's tests.
//!
//! The interesting tests here change process-wide state (working directory,
//! umask, rlimits, dumpable, ids) or take ownership of children (`waitpid(-1)`,
//! `signalfd`). The test harness runs tests as threads of one process, so both
//! kinds have to happen in a **forked child** instead: the child runs the body,
//! reports one small exit code, and `_exit`s without unwinding.
//!
//! Forking from the harness means the child is a copy of a multi-threaded
//! process. The child bodies are deliberately short and mostly syscall-shaped,
//! which is what keeps that sound in practice — see the safety contract of
//! [`crate::fork_unchecked`].

use std::io::Read;
use std::os::fd::{AsRawFd, BorrowedFd, OwnedFd};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};

use crate::pid::{ExitStatus, Pid};
use crate::{Fork, Signal, fork_unchecked};

/// Fail the test with a message when a `Result` is an `Err`.
pub fn must<T, E: std::fmt::Debug>(result: Result<T, E>) -> T {
    match result {
        Ok(value) => value,
        Err(err) => panic!("unexpected error: {err:?}"),
    }
}

/// Fail the test with a message when an `Option` is `None`.
pub fn must_some<T>(value: Option<T>) -> T {
    match value {
        Some(value) => value,
        None => panic!("expected a value"),
    }
}

/// Fail the test with a message when a `Result` is an `Ok`.
pub fn must_err<T: std::fmt::Debug, E>(result: Result<T, E>) -> E {
    match result {
        Ok(value) => panic!("expected an error, got {value:?}"),
        Err(err) => err,
    }
}

/// Whether the tests are running as root.
pub fn is_root() -> bool {
    rustix::process::geteuid().is_root()
}

/// The effective uid of the test process.
pub fn euid() -> u32 {
    rustix::process::geteuid().as_raw()
}

/// True when a root-only test can run; prints the skip line when it cannot.
pub fn root_only() -> bool {
    if is_root() {
        true
    } else {
        println!("skipped: needs root");
        false
    }
}

/// Run `body` in a forked child and return the child's exit code.
///
/// The child reports by exiting with the code `body` returns; it never unwinds,
/// so a panic in a child body is a `SIGABRT` the parent sees as a failed
/// assertion rather than a test.
pub fn in_child(body: impl FnOnce() -> i32) -> i32 {
    match child_status(body) {
        ExitStatus::Code(code) => code,
        other => panic!("the forked child did not exit normally: {other}"),
    }
}

/// Run `body` in a forked child and return **how it ended**.
///
/// [`in_child`] insists on a normal exit, so it turns a child that died from a
/// signal into a failed test. This is the raw form, for the tests where dying
/// from a signal is the answer: a seccomp kill is `SIGSYS`.
pub fn child_status(body: impl FnOnce() -> i32) -> ExitStatus {
    // SAFETY: the harness is multi-threaded, so the checked `fork()` would
    // refuse. The child body ends in `_exit`, and it **must make
    // async-signal-safe calls only**: at the instant of the fork another thread
    // may hold the allocator's lock, a stdio lock or a lock inside the harness,
    // and the child has no other thread to release it. A body that allocates can
    // therefore wait on that lock forever; `seccomp.rs`'s tests pass raw-syscall
    // bodies for exactly that reason.
    let fork = must(unsafe { fork_unchecked() });
    match fork {
        Fork::Child => {
            let code = body();
            // SAFETY: `_exit` never returns and runs neither atexit handlers
            // nor destructors, so nothing in the child is finalised twice.
            unsafe { libc::_exit(code) }
        }
        Fork::Parent(pid) => wait_for(pid),
    }
}

/// Assert that a forked child body succeeds.
pub fn child_ok(label: &str, body: impl FnOnce() -> i32) {
    let code = in_child(body);
    assert_eq!(code, 0, "{label}: the forked child reported failure {code}");
}

/// The environment variable naming the probe a spawned test process runs.
///
/// A probe is the escape hatch for a body that cannot run in a child of
/// `fork()`: it allocates, starts a thread, or drives a function that allocates
/// on its success path (`drop_privileges` reads the capability sets from
/// `/proc/self/status`). See [`run_probe`].
pub const PROBE_VAR: &str = "BLASTIT_SYS_PROBE";

/// The prefix of the line a probe process writes to its stdout.
pub const PROBE_MARKER: &str = "blastit-sys-probe:";

/// Run probe `name` in a **fresh process** of the test binary and return the
/// code it reported.
///
/// `runner` is the full test name of the module's `probe_runner` (`"proc::tests::probe_runner"`),
/// so a binary that has one per module starts only the one that knows `name`.
///
/// The process is spawned with [`std::process::Command`], i.e. `execve`: it gets
/// a private, unshared address space, where allocating and starting threads are
/// safe — unlike a child of `fork()` inside the multi-threaded harness, which
/// must make async-signal-safe calls only (see [`child_status`]).
///
/// The probe reports its code on a marker line, so a spawned process that exits
/// `0` because its filter matched nothing is a failure, never a silent pass.
pub fn run_probe(runner: &str, name: &str) -> i32 {
    let exe = must(std::env::current_exe());
    let output = must(
        std::process::Command::new(exe)
            .args(["--test-threads=1", "--nocapture", runner])
            .env(PROBE_VAR, name)
            .output(),
    );
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
///
/// Called by a module's probe entry point once it has run its body.
pub fn report_probe(name: &str, code: i32) -> ! {
    let line = format!("{PROBE_MARKER}{name}:{code}\n");
    // SAFETY: `line` is a live buffer of `line.len()` bytes and 1 is this
    // process's stdout. A raw write, so the report does not depend on a
    // buffered writer being flushed by `process::exit` below.
    unsafe { libc::write(1, line.as_ptr().cast::<libc::c_void>(), line.len()) };
    std::process::exit(code);
}

/// Block until `pid` has been reaped, and return how it ended.
pub fn wait_for(pid: Pid) -> ExitStatus {
    loop {
        let mut status = 0;
        // SAFETY: `waitpid` writes one status word into the valid storage
        // `status`; we pass the one pid we are waiting for.
        let waited = unsafe { libc::waitpid(pid.as_raw(), &mut status, 0) };
        if waited == pid.as_raw() {
            return must_some(ExitStatus::from_raw(status));
        }
        let err = std::io::Error::last_os_error();
        if err.raw_os_error() == Some(libc::EINTR) {
            continue;
        }
        panic!("waitpid({pid}) failed: {err}");
    }
}

/// Fork a child that exits with `code` right away.
pub fn spawn_exiting(code: i32) -> Pid {
    // SAFETY: the child immediately `_exit`s with no further work.
    let fork = must(unsafe { fork_unchecked() });
    match fork {
        Fork::Child => {
            // SAFETY: `_exit` never returns.
            unsafe { libc::_exit(code) }
        }
        Fork::Parent(pid) => pid,
    }
}

/// Fork a child that dies from `signal`.
pub fn spawn_dying(signal: Signal) -> Pid {
    // SAFETY: the child raises a fatal signal immediately; nothing else runs.
    let fork = must(unsafe { fork_unchecked() });
    match fork {
        Fork::Child => {
            // SAFETY: `getpid` always succeeds, `raise` sends a signal to the
            // calling process and does not return for a fatal one.
            unsafe { libc::raise(signal.as_raw()) };
            // SAFETY: `_exit` never returns.
            unsafe { libc::_exit(97) }
        }
        Fork::Parent(pid) => pid,
    }
}

/// Fork a child that waits for a signal, for as long as it takes.
pub fn spawn_waiting() -> Pid {
    // SAFETY: the child only calls `pause`, which is async-signal-safe.
    let fork = must(unsafe { fork_unchecked() });
    match fork {
        Fork::Child => loop {
            // SAFETY: `pause` suspends the process until a signal arrives; it
            // takes no arguments.
            unsafe { libc::pause() };
        },
        Fork::Parent(pid) => pid,
    }
}

/// Wait until `pid` has been reaped, never longer than a second or so.
///
/// Used where the signal that announces the exit is itself the thing under
/// test; a bounded failure beats a hung CI job.
pub fn reap_until(pid: Pid) -> ExitStatus {
    for _ in 0..100_000 {
        let reaped = must(crate::reap());
        for (child, status) in reaped {
            if child == pid {
                return status;
            }
        }
        std::thread::yield_now();
    }
    panic!("{pid} never showed up in reap()");
}

/// Whether `/proc/self/fd/<fd>` resolves, which is how "still open" is checked
/// without allocating in a forked child.
pub fn proc_fd_open(fd: i32) -> bool {
    let mut path = [0_u8; 32];
    let prefix = b"/proc/self/fd/";
    path[..prefix.len()].copy_from_slice(prefix);
    let mut cursor = prefix.len();
    let mut digits = [0_u8; 10];
    let mut value = fd.unsigned_abs();
    let mut count = 0;
    loop {
        digits[count] = b'0' + (value % 10) as u8;
        value /= 10;
        count += 1;
        if value == 0 {
            break;
        }
    }
    for index in (0..count).rev() {
        path[cursor] = digits[index];
        cursor += 1;
    }
    path[cursor] = 0;
    let mut target = [0_u8; 1];
    // SAFETY: `path` is a NUL-terminated string built above, and `target` is a
    // valid one-byte buffer that `readlink` never overruns.
    unsafe {
        libc::readlink(
            path.as_ptr().cast::<libc::c_char>(),
            target.as_mut_ptr().cast(),
            1,
        ) != -1
    }
}

/// How many descriptors at or above 3 the calling process has open.
///
/// Reads `/proc/self/fd`, so it only sees descriptors in this process — the
/// point is that it is allocation-free for the forked children that use it.
pub fn count_open_fds() -> usize {
    let mut count = 0;
    for fd in 3..=4096 {
        if proc_fd_open(fd) {
            count += 1;
        }
    }
    count
}

/// The entries of `/proc/self/fd`, for the tests that are allowed to allocate.
pub fn open_fds() -> Vec<i32> {
    let mut fds = Vec::new();
    for entry in must(std::fs::read_dir("/proc/self/fd")) {
        let entry = must(entry);
        if let Ok(fd) = entry.file_name().to_string_lossy().parse::<i32>() {
            fds.push(fd);
        }
    }
    fds
}

/// Read a descriptor to end-of-file.
pub fn read_to_end(fd: OwnedFd) -> Vec<u8> {
    let mut file = std::fs::File::from(fd);
    let mut out = Vec::new();
    must(file.read_to_end(&mut out));
    out
}

/// Write all of `bytes` to a descriptor.
pub fn write_all(fd: BorrowedFd<'_>, bytes: &[u8]) {
    let mut offset = 0;
    while offset < bytes.len() {
        // SAFETY: `write` reads `bytes.len() - offset` bytes from a valid
        // pointer into `bytes` and writes them to an open descriptor.
        let written = unsafe {
            libc::write(
                fd.as_raw_fd(),
                bytes[offset..].as_ptr().cast::<libc::c_void>(),
                bytes.len() - offset,
            )
        };
        assert!(written > 0, "write failed");
        offset += written as usize;
    }
}

/// A directory under the system temporary directory, removed when dropped.
pub struct TempDir {
    path: PathBuf,
}

static TEMP_COUNTER: AtomicU32 = AtomicU32::new(0);

impl TempDir {
    /// Reserve a unique path; the directory itself is not created.
    pub fn new(tag: &str) -> Self {
        let unique = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path =
            std::env::temp_dir().join(format!("blastit-sys-{tag}-{}-{unique}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        Self { path }
    }

    /// The reserved path.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}
