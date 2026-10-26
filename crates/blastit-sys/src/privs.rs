//! Privilege dropping, hardening and resource limits.

use std::io;

use rustix::process::{DumpableBehavior, Resource, Rlimit, set_dumpable_behavior, setrlimit};

use crate::Signal;
use crate::caps;
use crate::error::Error;

/// Process-wide identity: drop to `uid`, with `gid == uid`, no supplementary
/// groups and **no capabilities at all** (ARCHITECTURE.md §2.1, §7.2; ROADMAP
/// D6).
///
/// Root can `setuid()` to any uid without an `/etc/passwd` entry and without
/// breaking COW (§9.1); an unprivileged `init` starting with the capabilities of
/// [`crate::caps`] can do the same, which is what D6's rootless mode is. The
/// order is the one that cannot be reversed, and it is also the order the
/// capability sets have to be dropped in:
///
/// 1. the ambient, inheritable and bounding sets, while `CAP_SETPCAP` is still
///    there — a root process loses its effective set the moment `setresuid`
///    leaves uid 0, and dropping a bounding capability needs it;
/// 2. `setgroups(0, NULL)`, then `setresgid`, then `setresuid` — dropping the
///    uid last is what makes the earlier calls possible, and dropping all three
///    saved ids is what makes them irreversible;
/// 3. the permitted and effective sets. A **rootless** process still holds
///    them: the kernel only clears the capability sets when *all* of a
///    process's uids cross 0, and a rootless `init` was never 0. Without this
///    step a view would keep `CAP_SETUID` and could become any uid in the
///    container.
///
/// The result is checked rather than assumed: the real, effective and saved uids
/// and gids must all be `uid`, no supplementary group may be left, every one of
/// the five capability sets must be empty, and `setuid(0)` must now fail. Any
/// other outcome is an error, because a sandbox that kept a way back to root is
/// worse than one that failed to start (§4).
///
/// This is process-wide, not thread-local: glibc's `setresuid` applies the
/// change to every thread, which is one more reason init must be
/// single-threaded when it calls it. `uid` must not be 0.
///
/// Note for callers: `setresgid` and `setresuid` **clear
/// `PR_SET_PDEATHSIG`**, so [`harden_child`] has to run *after* this, not
/// before. See [`harden_child`] for the full story.
pub fn drop_privileges(uid: u32) -> io::Result<()> {
    if uid == 0 {
        return Err(Error::PrivilegeCheck {
            stage: "drop_privileges(0)",
            detail: "refusing to drop privileges to uid 0",
        }
        .into());
    }

    // ARCHITECTURE §7.2 / ROADMAP D6: before the uid change, while the
    // capability that can drop a bounding capability is still effective.
    caps::clear_before_uid_change()?;

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

    // ROADMAP D6: a rootless process keeps permitted and effective across the
    // uid change, so they are cleared here — and checked, because a view that
    // kept `CAP_SETUID` is the whole point of this step.
    caps::clear_after_uid_change()?;

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
    // capability and no root in any of the saved ids, there is nothing to set
    // the uid back to 0 with.
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
/// **Call this after [`drop_privileges`], never before.** `prctl(2)` lists
/// `set*gid` and `set*uid` — so `setresgid` and `setresuid` — among the calls
/// that *clear* the parent-death signal, and `drop_privileges` makes all of
/// them. A child hardened first and dropped afterwards comes back up with its
/// death signal silently gone. The order in the crate-level sequence is the one
/// that holds: drop privileges, then harden.
///
/// The death signal is also not inherited across `fork`, and `execve` clears it
/// too, so this runs in each child (again, in dbproxy's case, *after* its
/// `execve` if the child execs). Calling it with a signal that is blocked in
/// the child is fine — the kernel delivers it anyway.
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
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::caps::{CapSet, Capabilities};
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
            if pdeathsig() != Signal::KILL.as_raw() {
                return 2;
            }
            // SAFETY: `PR_GET_NO_NEW_PRIVS` takes no pointer and returns 1 when
            // the flag is set.
            if unsafe { libc::prctl(libc::PR_GET_NO_NEW_PRIVS, 0, 0, 0, 0) } != 1 {
                return 3;
            }
            0
        });
    }

    /// `PR_SET_PDEATHSIG`, read back.
    fn pdeathsig() -> i32 {
        let mut signal = 0;
        // SAFETY: `PR_GET_PDEATHSIG` writes one `c_int` into the valid storage
        // `signal` points at.
        if unsafe { libc::prctl(libc::PR_GET_PDEATHSIG, &mut signal) } == -1 {
            return -1;
        }
        signal
    }

    #[test]
    fn harden_child_after_drop_privileges_keeps_the_death_signal() {
        if !root_only() {
            return;
        }
        // The order the crate documents: drop first, harden second.
        let uid = 20_500;
        let code = in_child(move || {
            if drop_privileges(uid).is_err() {
                return 1;
            }
            if harden_child(Signal::KILL).is_err() {
                return 2;
            }
            match pdeathsig() == Signal::KILL.as_raw() {
                true => 0,
                false => 3,
            }
        });
        assert_eq!(code, 0, "child failure code {code}");
    }

    #[test]
    fn dropping_privileges_after_hardening_loses_the_death_signal() {
        if !root_only() {
            return;
        }
        // `setresgid` and `setresuid` clear `PR_SET_PDEATHSIG` (prctl(2) lists
        // `set*gid`/`set*uid`), which is why the sequence in the crate docs
        // hardens *after* the drop. This is that hazard, on purpose.
        let uid = 20_500;
        let code = in_child(move || {
            if harden_child(Signal::KILL).is_err() {
                return 1;
            }
            if pdeathsig() != Signal::KILL.as_raw() {
                return 2;
            }
            if drop_privileges(uid).is_err() {
                return 3;
            }
            match pdeathsig() {
                0 => 0,
                _ => 4,
            }
        });
        assert_eq!(code, 0, "child failure code {code}");
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
    fn drop_privileges_without_root_fails_before_it_can_drop_anything() {
        if is_root() {
            println!("skipped: needs a non-root user");
            return;
        }
        // A process with no capabilities cannot even clear the bounding set:
        // `PR_CAPBSET_DROP` needs `CAP_SETPCAP`, and D6 clears the sets before
        // the uid change. The boot refuses long before this, in
        // `caps::decide`, with the list of capabilities that are missing —
        // this is the same failure one layer down, so it names its own step.
        let uid = euid();
        let code = in_child(move || match drop_privileges(uid) {
            Ok(()) => 1,
            Err(err) => match Error::from_io(&err) {
                Some(Error::CapabilityDrop { stage, .. }) if *stage == "PR_CAPBSET_DROP" => 0,
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

    /// ROADMAP D6: a drop that *starts* with the capabilities of a rootless
    /// init — the ambient caps `setpriv`/a container can hand an unprivileged
    /// process — leaves every capability set empty.
    ///
    /// This is the shape a rootless `init` is in: a uid that never was 0, a
    /// permitted and effective set the kernel keeps across `setresuid`, and an
    /// ambient set that survives `execve`. Without the explicit clear in
    /// [`drop_privileges`], the child would keep `CAP_SETUID` and could become
    /// any uid in the container.
    #[test]
    fn drop_privileges_clears_a_rootless_capability_set() {
        if !root_only() {
            return;
        }
        let uid = 20_500;
        let code = in_child(move || {
            let needs = crate::caps::Capability::NEEDED
                .iter()
                .fold(0_u64, |mask, cap| mask | (1_u64 << cap.raw()));
            // The capabilities a rootless init is started with: permitted and
            // effective set, plus the inheritable/ambient pair a launcher can
            // hand over. As root we have to raise them ourselves.
            if crate::caps::capset(needs, needs, needs).is_err() {
                return 1;
            }
            if !Capabilities::read()
                .unwrap()
                .holds(CapSet::Permitted, crate::caps::Capability::SETUID)
            {
                return 2;
            }
            if drop_privileges(uid).is_err() {
                return 3;
            }
            let left = Capabilities::read().unwrap();
            if !left.is_clear() {
                return 4;
            }
            // The identity checks ran, so the capabilities are gone for good.
            0
        });
        assert_eq!(code, 0, "child failure code {code}");
    }
}
