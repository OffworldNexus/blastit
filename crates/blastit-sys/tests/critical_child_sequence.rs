//! Protected critical suite for the view child sequence (R4, `ARCHITECTURE.md` §4, §7.2, §7.3).
//!
//! `make test-critical` runs it as `cargo test -p blastit-sys --test
//! critical_child_sequence --locked`, with **no** feature flags, so it must not
//! depend on `feature = "testutil"` (a `#![cfg(feature = …)]` file would
//! silently run zero tests) and it must not need root: every case is a case an
//! unprivileged caller can run.
//!
//! The five cases are the rows of the design's table (`view-crates-architecture`
//! §7 R4), one per property of [`blastit_sys::child::enter`]:
//!
//! 1. `set_dumpable(false)` ran — the child is not dumpable after `enter`.
//! 2. A `Seccomp:` filter is installed — `/proc/self/status` says so.
//! 3. **The order**: a plan with `pgroup: true` and an `EmbeddedView` filter
//!    prints its pid and pgid, which it can only reach if `setpgid(0, 0)` ran
//!    *before* the filter — `EmbeddedView` answers `setpgid` with `EPERM`, so a
//!    filter installed first would have failed `enter` instead.
//! 4. A symlinked working directory is refused, and a real one is entered.
//! 5. The privilege drop is a distinct failure: a plan whose `DropTo` the
//!    kernel refuses exits non-zero with the drop step's own sentence, which a
//!    descriptor, `setpgid` or filter failure cannot produce.
//!
//! Case 3 is the one the deletion of `own_process_group` depends on: its
//! `setpgid` had to move *inside* `enter`, before the filter, and a "tidier"
//! order that still passed the other cases is exactly what this case refuses.

use std::io::Read;
use std::os::fd::OwnedFd;

use blastit_sys::child::{ChildIo, ChildPlan, Filter, Privileges, child_exit, enter};
use blastit_sys::{Rlimits, SeccompRole};

/// How a forked child ended, with everything it printed.
struct ChildRun {
    /// The child's exit code.
    code: i32,
    /// Everything it wrote to stdout.
    stdout: String,
    /// Everything it wrote to stderr.
    stderr: String,
}

impl ChildRun {
    /// The child's stderr, for a failure message that says what it printed.
    fn transcripts(&self) -> String {
        format!(
            "exit {}\n--- stdout ---\n{}--- stderr ---\n{}",
            self.code, self.stdout, self.stderr
        )
    }
}

/// Runs `body` in a forked child whose stdout and stderr are its own pipes.
///
/// The body is handed the two pipe write ends, so it builds its own
/// [`ChildIo`]; the rlimits, umask and descriptor half of the sequence are the
/// child's to run through [`enter`]. The child is a copy of a multi-threaded
/// test process, so its body is kept short and ends in `_exit` without
/// unwinding — the same contract `src/testutil.rs` keeps.
fn run_child(label: &str, body: impl FnOnce(OwnedFd, OwnedFd) -> i32) -> ChildRun {
    let (stdout_read, stdout_write) =
        blastit_sys::pipe().unwrap_or_else(|err| panic!("{label}: stdout pipe: {err}"));
    let (stderr_read, stderr_write) =
        blastit_sys::pipe().unwrap_or_else(|err| panic!("{label}: stderr pipe: {err}"));

    // SAFETY: the child is a copy of a multi-threaded process, so its body is
    // kept short and mostly syscall-shaped, and it ends in `_exit` without
    // unwinding.
    let pid = unsafe { libc::fork() };
    match pid {
        -1 => panic!("{label}: fork failed: {}", std::io::Error::last_os_error()),
        0 => {
            // The read ends belong to the parent; `enter`'s descriptor step
            // would close them anyway, but a body that never reaches it must
            // not keep the parent's read from ending.
            drop(stdout_read);
            drop(stderr_read);
            let code = body(stdout_write, stderr_write);
            // SAFETY: `_exit` never returns and runs no destructors.
            unsafe { libc::_exit(code) }
        }
        pid => {
            // The parent keeps only the read ends, or reading to end-of-file
            // would never see one.
            drop(stdout_write);
            drop(stderr_write);
            let stdout = read_to_end(stdout_read);
            let stderr = read_to_end(stderr_read);
            let mut status = 0;
            loop {
                // SAFETY: the status word is valid storage for this one child.
                let waited = unsafe { libc::waitpid(pid, &mut status, 0) };
                if waited == pid {
                    break;
                }
                let err = std::io::Error::last_os_error();
                if err.raw_os_error() == Some(libc::EINTR) {
                    continue;
                }
                panic!("{label}: waitpid failed: {err}");
            }
            assert!(
                libc::WIFEXITED(status),
                "{label}: the child died from signal {}; {}\n{}",
                libc::WTERMSIG(status),
                String::from_utf8_lossy(&stdout),
                String::from_utf8_lossy(&stderr),
            );
            ChildRun {
                code: libc::WEXITSTATUS(status),
                stdout: String::from_utf8_lossy(&stdout).into_owned(),
                stderr: String::from_utf8_lossy(&stderr).into_owned(),
            }
        }
    }
}

/// Reads a pipe to end-of-file.
fn read_to_end(fd: OwnedFd) -> Vec<u8> {
    let mut file = std::fs::File::from(fd);
    let mut out = Vec::new();
    file.read_to_end(&mut out)
        .unwrap_or_else(|err| panic!("reading a child's pipe: {err}"));
    out
}

/// A [`ChildIo`] whose stdio is the two pipes and which keeps nothing else.
fn child_io(stdout: OwnedFd, stderr: OwnedFd) -> ChildIo {
    ChildIo {
        stdout: Some(stdout),
        stderr: Some(stderr),
        keep: Vec::new(),
        limits: Rlimits::default(),
        working_dir: None,
    }
}

/// Case 1: `enter` leaves the child not dumpable.
#[test]
fn a_child_that_entered_is_not_dumpable() {
    let run = run_child("dumpable", |stdout, stderr| {
        match enter(ChildPlan::for_test(child_io(stdout, stderr))) {
            Ok(()) => {}
            Err(message) => child_exit(&message, 1),
        }
        // `/proc/<pid>/{environ,mem}` must not be readable by another uid
        // (§4); the call is best effort, so the assertion is the state.
        match blastit_sys::dumpable() {
            Ok(false) => 0,
            Ok(true) => 2,
            Err(_) => 3,
        }
    });
    assert_eq!(run.code, 0, "the child reported failure 2=still dumpable");
}

/// Case 2: `enter` installs the role's seccomp filter.
#[test]
fn a_child_that_entered_carries_a_seccomp_filter() {
    let run = run_child("seccomp", |stdout, stderr| {
        let plan = ChildPlan {
            name: "test view".to_string(),
            io: child_io(stdout, stderr),
            privileges: Privileges::Shared,
            filter: Filter::Seccomp(SeccompRole::View { egress: false }),
            pgroup: false,
            mask: None,
        };
        match enter(plan) {
            Ok(()) => {}
            Err(message) => child_exit(&message, 1),
        }
        // `Seccomp: 0` is "no filter"; 1 is strict mode and 2 is filter mode.
        // Only the kernel's own report is evidence: the filter cannot be
        // removed, so nothing else in this child could fake it.
        let status = match std::fs::read_to_string("/proc/self/status") {
            Ok(status) => status,
            Err(_) => return 2,
        };
        for line in status.lines() {
            if let Some(value) = line.strip_prefix("Seccomp:") {
                return match value.trim() {
                    "0" | "" => 3,
                    _ => 0,
                };
            }
        }
        4
    });
    assert_eq!(
        run.code, 0,
        "the child reported failure 2=no /proc/self/status, 3=no filter, 4=no Seccomp line"
    );
}

/// Case 3: `setpgid(0, 0)` runs **before** the filter.
///
/// `EmbeddedView` answers `setpgid` with `EPERM` (`seccomp.rs`), so a sequence
/// that installed the filter first could not have produced a group leader: it
/// would have failed `enter` with that refusal instead of printing a pid equal
/// to its pgid. This is the property the deleted `own_process_group` moved
/// into `enter` for, and the one the PHP probe's `kill(-pgid, SIGKILL)`
/// containment depends on.
#[test]
fn the_child_is_a_group_leader_before_the_embedded_view_filter() {
    let run = run_child("pgroup", |stdout, stderr| {
        let plan = ChildPlan {
            name: "test probe".to_string(),
            io: child_io(stdout, stderr),
            // `Shared`: case 5 is the drop's, and this case is the order's — a
            // `DropTo` the caller may not perform would fail `enter` before
            // the point under test.
            privileges: Privileges::Shared,
            filter: Filter::Seccomp(SeccompRole::EmbeddedView { egress: false }),
            pgroup: true,
            mask: None,
        };
        match enter(plan) {
            Ok(()) => {}
            Err(message) => child_exit(&message, 1),
        }
        // After the filter, `setpgid` is refused — so reaching this line at
        // all means the leader was made first.
        if blastit_sys::child::set_process_group().is_ok() {
            return 2;
        }
        // SAFETY: `getpid` and `getpgid` take no arguments and cannot fail for
        // the calling process.
        let (pid, pgid) = unsafe { (libc::getpid(), libc::getpgid(0)) };
        // One write to descriptor 1, not `println!`: libtest captures the
        // macro's output in a thread-local sink, so a forked child's `println!`
        // reaches the harness's buffer instead of its own pipe.
        let line = format!("{pid} {pgid}\n");
        // SAFETY: `line` is a valid byte slice that lives for the call, and
        // descriptor 1 is the child's stdout pipe after `enter`.
        unsafe { libc::write(1, line.as_ptr().cast::<libc::c_void>(), line.len()) };
        0
    });
    assert_eq!(
        run.code,
        0,
        "the child reported failure 2=setpgid still permitted after the filter; {}",
        run.transcripts()
    );
    let mut numbers = run.stdout.split_whitespace().map(str::parse::<i32>);
    let pid = numbers
        .next()
        .and_then(Result::ok)
        .unwrap_or_else(|| panic!("the child printed no pid: {}", run.transcripts()));
    let pgid = numbers
        .next()
        .and_then(Result::ok)
        .unwrap_or_else(|| panic!("the child printed no pgid: {}", run.transcripts()));
    assert_ne!(pid, 0, "{}", run.transcripts());
    assert_eq!(pid, pgid, "the child is not its own group leader");
}

/// Case 4: a symlinked working directory is refused, and a real one is entered.
///
/// `ChildIo::prepare` opens the directory `O_NOFOLLOW` and enters it with
/// `fchdir`, so the path the caller named and the directory the child runs in
/// are the same object. Both halves are asserted: the allowed path (a real
/// directory is entered) and the denied path (a symlink is not followed).
#[test]
fn a_symlinked_working_directory_is_refused_and_a_real_one_is_entered() {
    let root = std::env::temp_dir().join(format!(
        "blastit-child-sequence-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|since| since.as_nanos())
            .unwrap_or(0)
    ));
    let real = root.join("real");
    if let Err(err) = std::fs::create_dir_all(&real) {
        panic!("the fixture's real directory: {err}");
    }
    let link = root.join("link");
    if let Err(err) = std::os::unix::fs::symlink(&real, &link) {
        let _ = std::fs::remove_dir_all(&root);
        panic!("the fixture's symlink: {err}");
    }

    let accepted = run_child("cwd real", |stdout, stderr| {
        let io = ChildIo {
            working_dir: Some(real.clone()),
            ..child_io(stdout, stderr)
        };
        match enter(ChildPlan::for_test(io)) {
            Ok(()) => {}
            Err(message) => child_exit(&message, 1),
        }
        // `fchdir`, not `chdir(path)`: the child is in the directory the
        // descriptor named.
        match std::env::current_dir() {
            Ok(dir) if dir == real => 0,
            _ => 2,
        }
    });
    assert_eq!(
        accepted.code,
        0,
        "a real directory must be entered: {}",
        accepted.transcripts()
    );

    let refused = run_child("cwd symlink", |stdout, stderr| {
        let io = ChildIo {
            working_dir: Some(link.clone()),
            ..child_io(stdout, stderr)
        };
        match enter(ChildPlan::for_test(io)) {
            // A followed symlink is the defect: the child is then in `real`
            // while the caller named `link`.
            Ok(()) => 2,
            Err(message) => child_exit(&message, 0),
        }
    });
    assert_eq!(
        refused.code,
        0,
        "the symlink was followed: {}",
        refused.transcripts()
    );
    // Linux answers `O_DIRECTORY|O_NOFOLLOW` on a symlink with `ENOTDIR` (the
    // directory check fires first) and a plain `O_NOFOLLOW` with `ELOOP`; both
    // are the same refusal, and neither is a `prepare`-step failure.
    assert!(
        refused.stderr.contains("Too many levels of symbolic links")
            || refused.stderr.contains("Not a directory"),
        "the refusal must be the O_NOFOLLOW open, not another step: {}",
        refused.transcripts()
    );

    let _ = std::fs::remove_dir_all(&root);
}

/// Case 5: the privilege drop is its own failure, with its own sentence.
///
/// A `DropTo` the caller may not perform must fail **at the drop step**: the
/// child reaches `enter` (so `prepare` succeeded), the plan asks for no group
/// leader and no filter (so neither can be the failure), and the message on the
/// child's stderr is the crate's own drop sentence. A `prepare`, `setpgid` or
/// filter failure cannot produce those words.
#[test]
fn a_refused_privilege_drop_fails_at_the_drop_step() {
    // SAFETY: `geteuid` takes no arguments.
    let euid = unsafe { libc::geteuid() };
    // An unprivileged caller cannot `setgroups(0, NULL)`, so a drop it is not
    // allowed to perform fails in the drop. A root caller may drop to any uid,
    // so the drop it may not perform is uid 0, which `drop_privileges` refuses
    // outright — the same step, its own sentence, and the only one available
    // to root.
    let (destination, expected) = if euid == 0 {
        (0, "refusing to drop privileges to uid 0")
    } else {
        (euid.saturating_add(4242), "dropping privileges failed at `")
    };

    let run = run_child("privilege drop", |stdout, stderr| {
        let plan = ChildPlan {
            name: "test view".to_string(),
            io: child_io(stdout, stderr),
            privileges: Privileges::DropTo(destination),
            filter: Filter::Off,
            pgroup: false,
            mask: None,
        };
        match enter(plan) {
            Ok(()) => 2,
            Err(message) => child_exit(&message, 1),
        }
    });
    assert_eq!(
        run.code,
        1,
        "the drop was not refused (2 means `enter` succeeded): {}",
        run.transcripts()
    );
    assert!(
        run.stderr.contains(expected),
        "the child's sentence must be the drop step's ({expected:?}): {}",
        run.transcripts()
    );
    assert!(
        run.stderr.contains("test view:"),
        "the child's line must name the plan: {}",
        run.transcripts()
    );
}
