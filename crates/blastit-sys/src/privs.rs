//! Privilege dropping, hardening and resource limits.

use std::io;

use rustix::process::{DumpableBehavior, Resource, Rlimit, set_dumpable_behavior, setrlimit};

use crate::Signal;
use crate::error::Error;

/// Process-wide identity: drop to `uid`, with `gid == uid` and no supplementary
/// groups (ARCHITECTURE.md §2.1, §7.2).
///
/// Root can `setuid()` to any uid without an `/etc/passwd` entry and without
/// breaking COW (§9.1). The order is the one that cannot be reversed:
/// `setgroups(0, NULL)` first, then `setresgid`, then `setresuid`. Dropping the
/// uid last is what makes the earlier calls possible, and dropping all three
/// saved ids is what makes them irreversible.
///
/// The result is checked rather than assumed: the real, effective and saved
/// uids and gids must all be `uid`, and `setuid(0)` must now fail. Any other
/// outcome is an error, because a sandbox that kept a way back to root is worse
/// than one that failed to start (§4).
///
/// This is process-wide, not thread-local: glibc's `setresuid` applies the
/// change to every thread, which is one more reason init must be
/// single-threaded when it calls it. `uid` must not be 0.
pub fn drop_privileges(uid: u32) -> io::Result<()> {
    if uid == 0 {
        return Err(Error::PrivilegeCheck {
            stage: "drop_privileges(0)",
            detail: "refusing to drop privileges to uid 0",
        }
        .into());
    }

    // SAFETY: `setgroups` with a count of 0 and a null pointer clears the
    // supplementary group list; it reads no memory.
    if unsafe { libc::setgroups(0, std::ptr::null()) } == -1 {
        return Err(priv_drop("setgroups(0, NULL)"));
    }

    // SAFETY: `setresgid` takes three gids by value.
    if unsafe { libc::setresgid(uid, uid, uid) } == -1 {
        return Err(priv_drop("setresgid"));
    }

    // SAFETY: `setresuid` takes three uids by value.
    if unsafe { libc::setresuid(uid, uid, uid) } == -1 {
        return Err(priv_drop("setresuid"));
    }

    let mut rgid = 0;
    let mut egid = 0;
    let mut sgid = 0;
    // SAFETY: the three pointers are valid, aligned, initialised `gid_t`s.
    if unsafe { libc::getresgid(&mut rgid, &mut egid, &mut sgid) } == -1 {
        return Err(priv_drop("getresgid"));
    }
    if (rgid, egid, sgid) != (uid, uid, uid) {
        return Err(Error::PrivilegeCheck {
            stage: "setresgid",
            detail: "the real, effective and saved gids are not all the target uid",
        }
        .into());
    }

    let mut ruid = 0;
    let mut euid = 0;
    let mut suid = 0;
    // SAFETY: the three pointers are valid, aligned, initialised `uid_t`s.
    if unsafe { libc::getresuid(&mut ruid, &mut euid, &mut suid) } == -1 {
        return Err(priv_drop("getresuid"));
    }
    if (ruid, euid, suid) != (uid, uid, uid) {
        return Err(Error::PrivilegeCheck {
            stage: "setresuid",
            detail: "the real, effective and saved uids are not all the target uid",
        }
        .into());
    }

    // SAFETY: `setuid(0)` takes one uid by value. It must fail here: with no
    // capabilities and no root in any of the saved ids, there is nothing to
    // set the uid back to 0 with.
    if unsafe { libc::setuid(0) } == 0 {
        return Err(Error::PrivilegeCheck {
            stage: "setuid(0)",
            detail: "setuid(0) succeeded after dropping privileges",
        }
        .into());
    }

    Ok(())
}

/// A failing privilege-drop step, with the `errno` read immediately.
fn priv_drop(stage: &'static str) -> io::Error {
    Error::PrivilegeDrop {
        stage,
        source: io::Error::last_os_error(),
    }
    .into()
}

/// The limits a child runs under (§7.2).
///
/// `RLIMIT_NPROC` is the backstop under the scheduler's thread budget: the
/// kernel counts threads as well as processes, summed over every process of the
/// uid, on Linux and on gVisor. gVisor does not count a process's main thread,
/// so a caller leaves one unit of margin. `RLIMIT_CORE=0` keeps a crashing view
/// from writing a core file into its scratch directory.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Rlimits {
    /// `RLIMIT_NPROC` for this child's uid, if it should be limited.
    pub nproc: Option<u64>,
    /// `RLIMIT_NOFILE`, if it should be limited.
    pub nofile: Option<u64>,
    /// Set `RLIMIT_CORE` to 0.
    pub core_zero: bool,
}

/// Apply [`Rlimits`] to the calling process.
///
/// Both the soft and the hard limit are set to the requested value: a child
/// that could raise its own limit again would not be limited. Limiting is
/// therefore one-way — lowering a hard limit is allowed, raising it needs
/// privileges — which is exactly what a sandbox wants.
pub fn set_rlimits(limits: Rlimits) -> io::Result<()> {
    if let Some(nproc) = limits.nproc {
        set_one(Resource::Nproc, nproc)?;
    }
    if let Some(nofile) = limits.nofile {
        set_one(Resource::Nofile, nofile)?;
    }
    if limits.core_zero {
        set_one(Resource::Core, 0)?;
    }
    Ok(())
}

/// One `setrlimit` with soft and hard at the same value.
fn set_one(resource: Resource, value: u64) -> io::Result<()> {
    setrlimit(
        resource,
        Rlimit {
            current: Some(value),
            maximum: Some(value),
        },
    )
    .map_err(Into::into)
}

/// Set the process umask (a child gets `0o077`), returning the previous mask.
///
/// The umask is process-wide: every file a view creates is `0600` unless it
/// asks otherwise, and its scratch directory is `0700` already (§7.2).
pub fn set_umask(mask: u32) -> u32 {
    // SAFETY: `umask` takes and returns a mode; it cannot fail.
    unsafe { libc::umask(mask as libc::mode_t) as u32 }
}

/// The hardening a child does before it runs anything of its own (ARCHITECTURE.md
/// §4, §7.2): `PR_SET_PDEATHSIG(pdeathsig)` and `PR_SET_NO_NEW_PRIVS`.
///
/// `PR_SET_PDEATHSIG` guarantees that no child outlives init. `SIGKILL` is the
/// usual choice: a child that ignores a polite signal must not be able to
/// survive its parent. `PR_SET_NO_NEW_PRIVS` is also what allows a seccomp
/// filter to be installed without any capability (§7.3).
///
/// The death signal is not inherited across `fork`, so this runs in each child,
/// after the fork. Calling it with a signal that is blocked in the child is
/// fine — the kernel delivers it anyway.
pub fn harden_child(pdeathsig: Signal) -> io::Result<()> {
    // SAFETY: `PR_SET_PDEATHSIG` takes one integer, the signal number.
    if unsafe { libc::prctl(libc::PR_SET_PDEATHSIG, pdeathsig.as_raw()) } == -1 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `PR_SET_NO_NEW_PRIVS` takes one integer, 1 to enable it, and
    // ignores the remaining arguments.
    if unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) } == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Set `PR_SET_DUMPABLE`.
///
/// On real Linux this guards `/proc/<pid>/{environ,mem}`. **Under gVisor it does
/// not**: any uid can still read any process's `/proc/<pid>/environ`, `cmdline`,
/// `maps` and fd list, dumped or not, and the file keeps showing the process's
/// *initial* environment block — which only `execve` replaces (§4, §9.1). That
/// is why the exec barrier is mandatory and no secret is ever put in `environ`
/// or `argv`, and why this call is defence in depth rather than a mechanism.
pub fn set_dumpable(dumpable: bool) -> io::Result<()> {
    let behavior = if dumpable {
        DumpableBehavior::Dumpable
    } else {
        DumpableBehavior::NotDumpable
    };
    set_dumpable_behavior(behavior).map_err(Into::into)
}

/// Whether the calling process is currently dumpable.
pub fn dumpable() -> io::Result<bool> {
    Ok(matches!(
        rustix::process::dumpable_behavior()?,
        DumpableBehavior::Dumpable
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::Error;
    use crate::testutil::{child_ok, euid, in_child, is_root, must, must_err, root_only};

    #[test]
    fn set_umask_sets_and_returns_the_previous_mask() {
        child_ok("umask", || {
            let previous = set_umask(0o077);
            if set_umask(0o022) != 0o077 {
                return 1;
            }
            if set_umask(previous) != 0o022 {
                return 2;
            }
            0
        });
    }

    #[test]
    fn set_rlimits_lowers_nproc_and_nofile_and_zeroes_core() {
        child_ok("rlimits", || {
            let limits = Rlimits {
                nproc: Some(16),
                nofile: Some(32),
                core_zero: true,
            };
            if set_rlimits(limits).is_err() {
                return 1;
            }
            let nproc = rustix::process::getrlimit(Resource::Nproc);
            if nproc.current != Some(16) || nproc.maximum != Some(16) {
                return 2;
            }
            let nofile = rustix::process::getrlimit(Resource::Nofile);
            if nofile.current != Some(32) || nofile.maximum != Some(32) {
                return 3;
            }
            let core = rustix::process::getrlimit(Resource::Core);
            if core.current != Some(0) || core.maximum != Some(0) {
                return 4;
            }
            0
        });
    }

    #[test]
    fn set_rlimits_with_everything_unset_changes_nothing() {
        must(set_rlimits(Rlimits::default()));
    }

    #[test]
    fn harden_child_sets_pdeathsig_and_no_new_privs() {
        child_ok("harden_child", || {
            if harden_child(Signal::KILL).is_err() {
                return 1;
            }
            let mut pdeathsig = 0;
            // SAFETY: `PR_GET_PDEATHSIG` writes one `c_int` into the valid
            // storage `pdeathsig` points at.
            if unsafe { libc::prctl(libc::PR_GET_PDEATHSIG, &mut pdeathsig) } == -1 {
                return 2;
            }
            if pdeathsig != Signal::KILL.as_raw() {
                return 3;
            }
            // SAFETY: `PR_GET_NO_NEW_PRIVS` takes no pointer and returns 1 when
            // the flag is set.
            if unsafe { libc::prctl(libc::PR_GET_NO_NEW_PRIVS, 0, 0, 0, 0) } != 1 {
                return 4;
            }
            0
        });
    }

    #[test]
    fn set_dumpable_round_trips() {
        child_ok("dumpable", || {
            if set_dumpable(false).is_err() {
                return 1;
            }
            match dumpable() {
                Ok(false) => {}
                _ => return 2,
            }
            if set_dumpable(true).is_err() {
                return 3;
            }
            match dumpable() {
                Ok(true) => {}
                _ => return 4,
            }
            0
        });
    }

    #[test]
    fn drop_privileges_to_uid_zero_is_refused() {
        let err = must_err(drop_privileges(0));
        assert!(
            matches!(Error::from_io(&err), Some(Error::PrivilegeCheck { .. })),
            "got {err}"
        );
    }

    #[test]
    fn drop_privileges_without_root_fails_at_setgroups() {
        if is_root() {
            println!("skipped: needs a non-root user");
            return;
        }
        let uid = euid();
        let code = in_child(move || match drop_privileges(uid) {
            Ok(()) => 1,
            Err(err) => match Error::from_io(&err) {
                Some(Error::PrivilegeDrop { stage, .. }) if *stage == "setgroups(0, NULL)" => 0,
                _ => 2,
            },
        });
        assert_eq!(code, 0, "child failure code {code}");
    }

    #[test]
    fn drop_privileges_to_a_view_uid_cannot_regain_root() {
        if !root_only() {
            return;
        }
        let uid = 20_500;
        let code = in_child(move || {
            if drop_privileges(uid).is_err() {
                return 1;
            }
            // SAFETY: the four getters take no arguments and cannot fail.
            unsafe {
                if libc::getuid() != uid
                    || libc::geteuid() != uid
                    || libc::getgid() != uid
                    || libc::getegid() != uid
                {
                    return 2;
                }
                // No supplementary groups are left over.
                if libc::getgroups(0, std::ptr::null_mut()) != 0 {
                    return 3;
                }
                // The way back to root is gone.
                if libc::setuid(0) == 0 {
                    return 4;
                }
            }
            0
        });
        assert_eq!(code, 0, "child failure code {code}");
    }
}
