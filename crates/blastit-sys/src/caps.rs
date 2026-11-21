//! Capabilities: the mode init runs in, and how every child drops them all
//! (ARCHITECTURE.md §2.1, §4, §7.2, §9.1; ROADMAP D6).
//!
//! `init` is the only process in blastit with privileges (§2.1). Before D6 it
//! was also the only process that had to be *root*; D6 lets it run as an
//! unprivileged uid holding exactly the capabilities the boot needs. Root and a
//! capability set are not the same thing, and the kernel is what tells them
//! apart:
//!
//! * as root, `setresuid`, `chown` and `chmod` are authorised by uid 0 itself;
//! * as an unprivileged uid they are authorised by the **effective**
//!   capability set, which a process **keeps across `setresuid`**: the kernel
//!   only clears the capability sets when the uid crosses 0, and a rootless
//!   init's uid never was 0. So a rootless child that dropped nothing would
//!   keep `CAP_SETUID` — a way back to any uid in the container. Every child
//!   therefore clears the capability sets explicitly, and
//!   [`crate::drop_privileges`] fails if one is left.
//!
//! Two orderings matter, and both are implemented by [`crate::drop_privileges`]:
//!
//! * **the bounding set is dropped before the uid changes**, because
//!   `PR_CAPBSET_DROP` needs `CAP_SETPCAP` in the effective set, and a root
//!   process loses that set the moment `setresuid` leaves uid 0;
//! * **the ambient set is cleared first**, because the kernel's invariant is
//!   `ambient ⊆ permitted ∩ inheritable`, and a `capset` that broke it would be
//!   refused with `EPERM` — which would leave every set exactly as it was.
//!
//! Nothing here is D6's own invention: `cap_setuid`, `cap_setgid`,
//! `cap_chown`, `cap_fowner` and `cap_setpcap` are what §9.1's App Platform
//! container already hands root, and two more come from what the boot actually
//! does:
//!
//! * `cap_dac_override`, because a view's private directory is `chown`ed to the
//!   view **before** the socket inside it is bound, and binding one opens that
//!   directory;
//! * `cap_kill`, because `kill(2)` only signals another uid with it, and the
//!   supervisor kills view processes whose uid is never its own.

use std::io;

use rustix::process::geteuid;

use crate::error::Error;

/// A Linux capability, by number.
///
/// The numbers are the kernel's (`<linux/capability.h>`), which is what
/// `/proc/self/status` and `PR_CAPBSET_DROP` speak. Only the capabilities
/// blastit names are given constants; every one the kernel has is named in the
/// table behind [`Capability::name`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Capability(u32);

impl Capability {
    /// `CAP_CHOWN`: `chown` a file this process does not own — the view
    /// directories and their sockets, which are chowned to the view's uid
    /// (§2.2, §7.2).
    pub const CHOWN: Self = Self(0);
    /// `CAP_DAC_OVERRIDE`: bypass file read, write and execute permission
    /// checks.
    ///
    /// Needed because §7.2's *ordering* is deliberate: a view's private
    /// directory is created `0700` and **chowned to the view uid** before the
    /// socket inside it is bound, and binding one means opening that directory
    /// ([`crate::bind_unix_listener`]). As root the kernel ignores the mode bits
    /// for it; as an unprivileged uid with this capability it may open the
    /// directory it just gave away. This is also the one capability that makes
    /// the boot able to read anything on the filesystem, exactly as root mode
    /// does.
    pub const DAC_OVERRIDE: Self = Self(1);
    /// `CAP_FOWNER`: `chmod` a file this process does not own — the mode of
    /// every view directory and socket, applied *after* the `chown` that made
    /// them the view's ([`crate::make_private_dir`]).
    pub const FOWNER: Self = Self(3);
    /// `CAP_KILL`: signal a process whose uid is not this process's — the
    /// supervisor's view processes (`RETIRE`, shutdown), and dbproxy.
    pub const KILL: Self = Self(5);
    /// `CAP_SETGID`: `setgroups(0, NULL)` and `setresgid`.
    pub const SETGID: Self = Self(6);
    /// `CAP_SETUID`: `setresuid` to a view's uid, with no `/etc/passwd` entry
    /// (§9.1).
    pub const SETUID: Self = Self(7);
    /// `CAP_SETPCAP`: drop capabilities from the bounding set, so a child
    /// cannot regain one even through a file with file capabilities.
    pub const SETPCAP: Self = Self(8);
    /// `CAP_SYS_RESOURCE`: raise a `setrlimit` hard limit. Not needed: blastit
    /// only ever lowers one.
    pub const SYS_RESOURCE: Self = Self(24);

    /// The capabilities an unprivileged `init` needs, in ascending order —
    /// which is also the order [`Capability::SETCAP`] and its log lines spell
    /// them.
    ///
    /// These six are what *must* be there for the boot to work, as root and as
    /// an unprivileged uid alike (see [`decide`]): the uid layout
    /// (`setuid`/`setgid`), the private directories (`chown`/`fowner`), opening
    /// one of them (`dac_override`) and signalling a view (`kill`).
    /// `cap_setpcap` is not in the list; [`Capability::WANTED`] adds it, and a
    /// boot without it still runs (D6/M1).
    pub const NEEDED: [Self; 6] = [
        Self::CHOWN,
        Self::DAC_OVERRIDE,
        Self::FOWNER,
        Self::KILL,
        Self::SETGID,
        Self::SETUID,
    ];

    /// Everything a deployment should give `init`: [`Capability::NEEDED`] plus
    /// `cap_setpcap`, which lets it **empty its children's bounding set**.
    ///
    /// Without `cap_setpcap` a child still holds nothing, and
    /// `PR_SET_NO_NEW_PRIVS` — set by every child — still stops an `execve` from
    /// granting what the bounding set allows, so its absence is a warning at
    /// boot and a `"bounding_set": "kept"` in the `boot` event rather than a
    /// refusal: a container that cannot give it (a `--cap-drop` from before, a
    /// runtime without `PR_CAPBSET_DROP`) must keep working.
    pub const WANTED: [Self; 7] = [
        Self::CHOWN,
        Self::DAC_OVERRIDE,
        Self::FOWNER,
        Self::KILL,
        Self::SETGID,
        Self::SETUID,
        Self::SETPCAP,
    ];

    /// The whole set as `setcap`/`getcap` spell it, for the error that says how
    /// to give blastit what it is missing.
    ///
    /// ```console
    /// $ sudo setcap 'cap_chown,cap_dac_override,cap_fowner,cap_kill,cap_setgid,cap_setuid,cap_setpcap+ep' /path/to/blastit-init
    /// ```
    pub const SETCAP: &'static str =
        "cap_chown,cap_dac_override,cap_fowner,cap_kill,cap_setgid,cap_setuid,cap_setpcap";

    /// The `setcap`/`capsh` names, by capability number (`0..=40`, the range
    /// Linux 6.x defines).
    const NAMES: [&'static str; 41] = [
        "cap_chown",
        "cap_dac_override",
        "cap_dac_read_search",
        "cap_fowner",
        "cap_fsetid",
        "cap_kill",
        "cap_setgid",
        "cap_setuid",
        "cap_setpcap",
        "cap_linux_immutable",
        "cap_net_bind_service",
        "cap_net_broadcast",
        "cap_net_admin",
        "cap_net_raw",
        "cap_ipc_lock",
        "cap_ipc_owner",
        "cap_sys_module",
        "cap_sys_rawio",
        "cap_sys_chroot",
        "cap_sys_ptrace",
        "cap_sys_pacct",
        "cap_sys_admin",
        "cap_sys_boot",
        "cap_sys_nice",
        "cap_sys_resource",
        "cap_sys_time",
        "cap_sys_tty_config",
        "cap_mknod",
        "cap_lease",
        "cap_audit_write",
        "cap_audit_control",
        "cap_setfcap",
        "cap_mac_override",
        "cap_mac_admin",
        "cap_syslog",
        "cap_wake_alarm",
        "cap_block_suspend",
        "cap_audit_read",
        "cap_perfmon",
        "cap_bpf",
        "cap_checkpoint_restore",
    ];

    /// A capability from its kernel number.
    pub const fn from_raw(raw: u32) -> Self {
        Self(raw)
    }

    /// The kernel number, as `/proc/self/status` and `PR_CAPBSET_DROP` use it.
    pub const fn raw(self) -> u32 {
        self.0
    }

    /// The `setcap`/`capsh` name of this capability.
    ///
    /// A number no Linux release names — a capability a future kernel added —
    /// is reported as `cap_unknown`, which is honest rather than a number the
    /// reader would have to look up.
    pub fn name(self) -> &'static str {
        Self::NAMES
            .get(self.0 as usize)
            .copied()
            .unwrap_or("cap_unknown")
    }
}

/// One of the five per-process capability sets `<linux/capability.h>` defines.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CapSet {
    /// `CapInh`: what a child inherits through `execve`.
    Inheritable,
    /// `CapPrm`: what the process may raise into its effective set.
    Permitted,
    /// `CapEff`: what authorises a syscall.
    Effective,
    /// `CapBnd`: what may ever be added to the inheritable set again.
    Bounding,
    /// `CapAmb`: what survives an `execve` of a file without file
    /// capabilities.
    Ambient,
}

impl CapSet {
    /// The `Cap*` field of `/proc/self/status` this set is read from.
    pub const fn status_field(self) -> &'static str {
        match self {
            Self::Inheritable => "CapInh",
            Self::Permitted => "CapPrm",
            Self::Effective => "CapEff",
            Self::Bounding => "CapBnd",
            Self::Ambient => "CapAmb",
        }
    }

    /// Every set, in the order a log line lists them.
    pub const ALL: [Self; 5] = [
        Self::Inheritable,
        Self::Permitted,
        Self::Effective,
        Self::Bounding,
        Self::Ambient,
    ];

    /// The sets every kernel's `/proc/self/status` holds.
    const MANDATORY: [Self; 4] = [
        Self::Inheritable,
        Self::Permitted,
        Self::Effective,
        Self::Bounding,
    ];
}

/// The five capability sets, as `/proc/self/status` reports them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Capabilities {
    inheritable: u64,
    permitted: u64,
    effective: u64,
    bounding: u64,
    ambient: u64,
}

impl Capabilities {
    /// Every set is empty.
    ///
    /// This is the invariant [`crate::drop_privileges`] leaves behind and
    /// checks: a child that failed to drop a capability is worse than one that
    /// refused to start (§4).
    pub fn is_clear(self) -> bool {
        self == Self::default()
    }

    /// The process holds no capability: permitted, effective, inheritable and
    /// ambient are empty.
    ///
    /// The **bounding** set is deliberately not part of this (M1): it is not
    /// something a process holds but what a later `execve` could still be
    /// granted, and `PR_SET_NO_NEW_PRIVS` is what closes that door. A process
    /// that checks this one and then sets the flag holds nothing that a
    /// capability check could use.
    pub fn holds_none(self) -> bool {
        self.inheritable == 0 && self.permitted == 0 && self.effective == 0 && self.ambient == 0
    }

    /// One set, as the bitmask the kernel reports.
    pub fn get(self, set: CapSet) -> u64 {
        match set {
            CapSet::Inheritable => self.inheritable,
            CapSet::Permitted => self.permitted,
            CapSet::Effective => self.effective,
            CapSet::Bounding => self.bounding,
            CapSet::Ambient => self.ambient,
        }
    }

    /// Whether `cap` is in `set`.
    pub fn holds(self, set: CapSet, cap: Capability) -> bool {
        self.get(set) & (1_u64 << cap.raw()) != 0
    }

    /// The capabilities of `caps` that are not in `set`, in the order given.
    pub fn missing(self, set: CapSet, caps: &[Capability]) -> Vec<Capability> {
        caps.iter()
            .copied()
            .filter(|cap| !self.holds(set, *cap))
            .collect()
    }

    /// The names of every capability in `set`, in ascending order.
    ///
    /// Deterministic, so a log line built from it is stable: the sets come out
    /// of the kernel as a bitmask, and the order is the capability number.
    pub fn names(self, set: CapSet) -> Vec<&'static str> {
        let bits = self.get(set);
        (0..64)
            .filter(|bit| bits & (1_u64 << bit) != 0)
            .map(|bit| Capability::from_raw(bit as u32).name())
            .collect()
    }

    /// The five sets, as `/proc/self/status` prints them.
    ///
    /// Pure, so the parsing is tested on the exact text the kernel writes
    /// without a process to read it from. `CapInh`, `CapPrm`, `CapEff` and
    /// `CapBnd` are always there; `CapAmb` is absent on a kernel without
    /// ambient capability support (before 4.3), and reads as empty.
    ///
    /// # Errors
    ///
    /// [`Error::CapabilityStatus`] for a missing or unparseable field, which is
    /// a `/proc` that is not the kernel's.
    pub fn parse_status(status: &str) -> io::Result<Self> {
        let mut sets = Self::default();
        for set in CapSet::MANDATORY {
            let value = optional_field(status, set.status_field())?.ok_or_else(|| {
                io::Error::from(Error::CapabilityStatus {
                    reason: format!("/proc/self/status has no {} line", set.status_field()),
                })
            })?;
            *slot(&mut sets, set) = value;
        }
        // Optional: a kernel without ambient capabilities has no such line, and
        // then there are none to read.
        if let Some(value) = optional_field(status, CapSet::Ambient.status_field())? {
            sets.ambient = value;
        }
        Ok(sets)
    }

    /// The capability sets of the calling process (`/proc/self/status`).
    ///
    /// # Errors
    ///
    /// [`std::io::Error`] when `/proc/self/status` cannot be read, and
    /// [`Error::CapabilityStatus`] when it does not hold the fields
    /// [`Capabilities::parse_status`] documents.
    pub fn read() -> io::Result<Self> {
        let status = std::fs::read_to_string("/proc/self/status")
            .map_err(|err| io::Error::new(err.kind(), format!("/proc/self/status: {err}")))?;
        Self::parse_status(&status)
    }
}

/// The field of a [`Capabilities`] value that holds `set`.
fn slot(sets: &mut Capabilities, set: CapSet) -> &mut u64 {
    match set {
        CapSet::Inheritable => &mut sets.inheritable,
        CapSet::Permitted => &mut sets.permitted,
        CapSet::Effective => &mut sets.effective,
        CapSet::Bounding => &mut sets.bounding,
        CapSet::Ambient => &mut sets.ambient,
    }
}

/// A `Cap*` field of `/proc/self/status`, or `None` when the line is absent.
///
/// # Errors
///
/// [`Error::CapabilityStatus`] when the line is there but is not hexadecimal.
fn optional_field(status: &str, name: &'static str) -> io::Result<Option<u64>> {
    for line in status.lines() {
        let Some(rest) = line.strip_prefix(name) else {
            continue;
        };
        let Some(value) = rest.strip_prefix(':') else {
            continue;
        };
        let value = value.trim();
        return u64::from_str_radix(value, 16).map(Some).map_err(|_| {
            io::Error::from(Error::CapabilityStatus {
                reason: format!("/proc/self/status: {name} is not hexadecimal"),
            })
        });
    }
    Ok(None)
}

/// What a process that cannot run init is missing (D6).
///
/// A value rather than a sentence: the caller is the one that knows *how*
/// blastit was installed — a wheel's interpreter, a standalone binary — and
/// therefore which file it has to tell the operator to `setcap`, and which
/// capability-free file the children must use instead.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Missing {
    /// The uid init is running as.
    pub uid: u32,
    /// The capabilities missing from the effective set, in
    /// [`Capability::NEEDED`] order.
    pub capabilities: Vec<Capability>,
}

impl Missing {
    /// The missing capabilities as `cap_...` names, comma-separated.
    pub fn names(&self) -> String {
        self.capabilities
            .iter()
            .map(|cap| cap.name())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// The plain-est failure there is: a [`Missing`] turned into an `io::Error`.
///
/// The message is deliberately bare — the *caller* adds the deployment-specific
/// advice, because only it knows which file holds the capabilities and which one
/// the children must use instead (D6/B1, B2).
impl From<Missing> for io::Error {
    fn from(missing: Missing) -> Self {
        io::Error::other(Error::MissingCapabilities {
            uid: missing.uid,
            missing: missing.names(),
        })
    }
}

/// The privilege mode `init` runs in (D6).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PrivilegeMode {
    /// uid 0: every `setuid`/`chown` is authorised by the uid itself.
    Root,
    /// An unprivileged uid holding [`Capability::NEEDED`].
    Capabilities,
}

impl PrivilegeMode {
    /// The spelling the `boot` event reports (D6).
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Root => "root",
            Self::Capabilities => "capabilities",
        }
    }
}

/// What `init` found out about its own privileges, for the `boot` event (D6).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Held {
    /// The mode the boot will run in.
    pub mode: PrivilegeMode,
    /// The capabilities `init` holds in its permitted set, `cap_...` names.
    ///
    /// Empty in [`PrivilegeMode::Root`]: the interesting set is then the
    /// container's, which §10 documents, so [`detect`] does not guess it from
    /// the process's own — a root process's permitted set is the container's
    /// bounding set intersected with what the file it started from had.
    pub capabilities: Vec<&'static str>,
    /// Whether init can **empty its children's bounding set**
    /// ([`Capability::SETPCAP`]).
    ///
    /// `false` is not a failure: every other set is still cleared and
    /// `PR_SET_NO_NEW_PRIVS` still stops an `execve` from granting what the
    /// bounding set allows, so the boot warns and reports
    /// `"bounding_set": "kept"` (D6/M1).
    pub bounding_set_dropped: bool,
}

/// Decides the mode `init` runs in from its uid and capability sets (D6).
///
/// Pure, so every combination is a unit test: this is the decision that turns
/// "start me as root or with the right `setcap` line" into a boot or into the
/// list of capabilities that are missing.
///
/// The effective set is what authorises the calls the boot makes — `setgroups`,
/// `setresuid`, `chown`, `fchmod`, the `open` of a view's directory and `kill` —
/// so that is the set the decision reads, in both modes. **Root is not
/// special**: a container that dropped one of those from uid 0's effective set
/// cannot build its sandboxes either, and saying so at boot beats an `EPERM` at
/// the first view. `cap_setpcap` is deliberately not in [`Capability::NEEDED`]:
/// without it the boot still runs, and says so (M1).
///
/// # Errors
///
/// [`Missing`], with the capabilities the effective set lacks.
pub fn decide(uid: u32, caps: &Capabilities) -> Result<PrivilegeMode, Missing> {
    let missing = caps.missing(CapSet::Effective, &Capability::NEEDED);
    if !missing.is_empty() {
        return Err(Missing {
            uid,
            capabilities: missing,
        });
    }
    Ok(if uid == 0 {
        PrivilegeMode::Root
    } else {
        PrivilegeMode::Capabilities
    })
}

/// Reads the calling process's own privilege state and decides (D6).
///
/// # Errors
///
/// [`std::io::Error`] when `/proc/self/status` cannot be read or parsed, and
/// [`Missing`] — through [`decide`] — when the process can run neither as root
/// nor as a holder of the needed capabilities.
pub fn detect() -> io::Result<Held> {
    let caps = Capabilities::read()?;
    let mode = decide(geteuid().as_raw(), &caps).map_err(io::Error::from)?;
    let capabilities = match mode {
        // A rootless init is exactly what its permitted set says.
        PrivilegeMode::Capabilities => caps.names(CapSet::Permitted),
        // Root does not need the capabilities listed in its own event: they are
        // the container's, and §10 is where they are documented.
        PrivilegeMode::Root => Vec::new(),
    };
    Ok(Held {
        mode,
        capabilities,
        bounding_set_dropped: caps.holds(CapSet::Effective, Capability::SETPCAP),
    })
}

/// Clears the ambient and inheritable sets, and the bounding set **when it
/// can**, keeping the permitted and effective ones.
///
/// This is the half of [`clear`] that has to happen **before** a uid change:
/// `PR_CAPBSET_DROP` needs `CAP_SETPCAP` in the effective set, and leaving uid 0
/// is what clears that set. Both halves are what [`crate::drop_privileges`]
/// runs, in that order.
///
/// A process without `CAP_SETPCAP` — a container started with
/// `--cap-drop SETPCAP`, a root process whose effective set was cut down —
/// cannot empty the bounding set at all. That is not a reason to refuse to
/// start (M1): the ambient and inheritable sets are still cleared here, the
/// permitted and effective ones by [`clear_after_uid_change`], and the
/// `PR_SET_NO_NEW_PRIVS` every child sets is what stops an `execve` from
/// granting what the bounding set still allows. The boot says which of the two
/// happened, in its `bounding_set` field.
///
/// # Errors
///
/// [`Error::CapabilityDrop`] naming the step that failed, and
/// [`Error::CapabilitiesRemaining`] when a set that was cleared is not empty
/// afterwards.
pub fn clear_before_uid_change() -> io::Result<()> {
    let caps = Capabilities::read()?;
    clear_ambient()?;
    clear_inheritable()?;
    if caps.holds(CapSet::Effective, Capability::SETPCAP) {
        clear_bounding()?;
        return verify(&[CapSet::Ambient, CapSet::Inheritable, CapSet::Bounding]);
    }
    verify(&[CapSet::Ambient, CapSet::Inheritable])
}

/// Clears the permitted and effective sets, and checks that nothing is held.
///
/// The second half of [`clear`]: after this, the process holds no capability at
/// all, whatever it was started with. The **bounding** set is not part of the
/// check — it is a restriction rather than something held, and emptying it is
/// [`clear_before_uid_change`]'s job, when the process can (M1).
///
/// # Errors
///
/// [`Error::CapabilityDrop`] when the kernel refuses the `capset`, and
/// [`Error::CapabilitiesRemaining`] when a set is not empty afterwards.
pub fn clear_after_uid_change() -> io::Result<()> {
    capset(0, 0, 0)?;
    verify(&[
        CapSet::Inheritable,
        CapSet::Permitted,
        CapSet::Effective,
        CapSet::Ambient,
    ])
}

/// Clears every capability set the calling process can clear: ambient,
/// inheritable and bounding, then permitted and effective.
///
/// A process that has already changed uid calls [`clear_after_uid_change`]
/// instead, because dropping the bounding set needs the `CAP_SETPCAP` that the
/// uid change takes away.
///
/// # Errors
///
/// As [`clear_before_uid_change`] and [`clear_after_uid_change`].
pub fn clear() -> io::Result<()> {
    clear_before_uid_change()?;
    clear_after_uid_change()
}

/// Makes sure the calling process holds **no** capability at all.
///
/// [`clear`] when there is anything to clear — which needs `CAP_SETPCAP` for
/// the bounding set, and skips it when the process does not have that — and
/// nothing but a check when there is not. A process that already holds none is
/// what a wheel install's dbproxy starts as (nothing grants *it*
/// capabilities), and refusing to start there would be wrong: what matters is
/// that it holds none, not that it dropped some.
///
/// The check is [`Capabilities::holds_none`]: permitted, effective, inheritable
/// and ambient. The bounding set is not something this process *holds* — it is
/// what a later `execve` could still be granted — and the `no_new_privs` every
/// child of init sets is what closes that door (M1).
///
/// # Errors
///
/// [`Capabilities::read`]'s errors when `/proc/self/status` cannot be read, and
/// [`Error::CapabilitiesRemaining`] — through [`clear`] — when a capability
/// survives the attempt. Both are errors rather than "nothing to do": a process
/// that cannot prove it holds nothing must not serve.
pub fn hold_none() -> io::Result<()> {
    if Capabilities::read()?.holds_none() {
        return Ok(());
    }
    clear()
}

/// Checks that `sets` are empty in the calling process.
///
/// # Errors
///
/// [`Error::CapabilitiesRemaining`] naming the first set that is not empty and
/// the capabilities it still holds, and the same errors as
/// [`Capabilities::read`].
fn verify(sets: &[CapSet]) -> io::Result<()> {
    let held = Capabilities::read()?;
    for set in sets {
        if held.get(*set) != 0 {
            return Err(Error::CapabilitiesRemaining {
                set: set.status_field(),
                held: held.names(*set).join(", "),
            }
            .into());
        }
    }
    Ok(())
}

/// `PR_CAP_AMBIENT(PR_CAP_AMBIENT_CLEAR_ALL)`.
///
/// `EINVAL` is the answer on a kernel without ambient capabilities (before
/// 4.3), where there are none to clear.
fn clear_ambient() -> io::Result<()> {
    // SAFETY: `PR_CAP_AMBIENT` takes an integer sub-command and no pointer.
    if unsafe {
        libc::prctl(
            libc::PR_CAP_AMBIENT,
            libc::PR_CAP_AMBIENT_CLEAR_ALL,
            0,
            0,
            0,
        )
    } == -1
    {
        let err = io::Error::last_os_error();
        if err.raw_os_error() == Some(libc::EINVAL) {
            return Ok(());
        }
        return Err(Error::CapabilityDrop {
            stage: "PR_CAP_AMBIENT_CLEAR_ALL",
            source: err,
        }
        .into());
    }
    Ok(())
}

/// `capset` with an empty inheritable set and the permitted/effective sets left
/// alone.
fn clear_inheritable() -> io::Result<()> {
    let current = Capabilities::read()?;
    capset(0, current.permitted, current.effective).map_err(|err| {
        io::Error::from(Error::CapabilityDrop {
            stage: "capset(inheritable)",
            source: err,
        })
    })
}

/// `PR_CAPBSET_DROP` for every capability the kernel knows.
///
/// The list is bounded by the kernel, not by this constant: a number above the
/// last capability comes back `EINVAL`, which is the end of the loop and not a
/// failure.
fn clear_bounding() -> io::Result<()> {
    for raw in 0..=u32::from(last_cap()) {
        // SAFETY: `PR_CAPBSET_DROP` takes one integer, the capability number.
        if unsafe { libc::prctl(libc::PR_CAPBSET_DROP, raw as libc::c_ulong, 0, 0, 0) } == -1 {
            let err = io::Error::last_os_error();
            if err.raw_os_error() == Some(libc::EINVAL) {
                continue;
            }
            return Err(Error::CapabilityDrop {
                stage: "PR_CAPBSET_DROP",
                source: err,
            }
            .into());
        }
    }
    Ok(())
}

/// The highest capability number this kernel has (`/proc/sys/kernel/cap_last_cap`).
///
/// The file is the kernel's own answer and has been there since 4.3; a kernel
/// without it is bounded by the 64-bit mask every capability set is stored in.
fn last_cap() -> u8 {
    std::fs::read_to_string("/proc/sys/kernel/cap_last_cap")
        .ok()
        .and_then(|text| text.trim().parse().ok())
        .unwrap_or(63)
}

/// `_LINUX_CAPABILITY_VERSION_3`: two `__u32` words per set, 64 bits.
pub(crate) const CAPABILITY_VERSION_3: u32 = 0x2008_0522;

/// `struct __user_cap_header_struct`.
#[repr(C)]
pub(crate) struct CapHeader {
    pub(crate) version: u32,
    pub(crate) pid: i32,
}

/// `struct __user_cap_data_struct`.
#[repr(C)]
#[derive(Clone, Copy, Default)]
pub(crate) struct CapData {
    pub(crate) effective: u32,
    pub(crate) permitted: u32,
    pub(crate) inheritable: u32,
}

/// The `capset(2)` syscall, spelled out here because glibc dropped its wrapper.
///
/// The high 32 bits of each set are the second `CapData` word, so 64-bit
/// capability numbers survive the round trip; every field of both words is
/// written, which is what makes the call *set* the sets rather than merge them.
pub(crate) fn capset(inheritable: u64, permitted: u64, effective: u64) -> io::Result<()> {
    let mut header = CapHeader {
        version: CAPABILITY_VERSION_3,
        pid: 0,
    };
    let data = [
        CapData {
            effective: effective as u32,
            permitted: permitted as u32,
            inheritable: inheritable as u32,
        },
        CapData {
            effective: (effective >> 32) as u32,
            permitted: (permitted >> 32) as u32,
            inheritable: (inheritable >> 32) as u32,
        },
    ];
    // SAFETY: `capset` reads the two structures this call owns: a header of one
    // version word and one pid, and an array of two data words, which is the
    // shape `_LINUX_CAPABILITY_VERSION_3` documents.
    let ret = unsafe {
        libc::syscall(
            libc::SYS_capset,
            &mut header as *mut CapHeader,
            data.as_ptr(),
        )
    };
    if ret == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
