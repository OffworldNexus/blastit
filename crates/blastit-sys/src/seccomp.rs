//! seccomp: one hand-written classic-BPF filter per role (ARCHITECTURE.md §7.3).
//!
//! Every process blastit forks restricts its own system calls after it has
//! dropped privileges. The filter needs no capability — `PR_SET_NO_NEW_PRIVS`
//! is what lets an unprivileged process install one, and [`install`] sets it
//! itself so a caller cannot forget — which is what makes this work under
//! DigitalOcean's gVisor, where the spike measured `AF_INET`/`AF_INET6` going
//! to `EPERM` while `AF_UNIX` kept working (§9, `docs/spikes`).
//!
//! Three properties shape the program:
//!
//! * **The default action is allow.** This is a refusal list, not an allowlist:
//!   a syscall blastit did not think about keeps working, so the filter cannot
//!   be what breaks a request. What it refuses, it refuses with `EPERM`, which
//!   every caller reports as "not permitted" rather than as a crash.
//! * **A foreign architecture is fatal.** `seccomp_data.arch` has to be the one
//!   this filter was written for, and an x32 syscall number on x86_64 (`nr &
//!   __X32_SYSCALL_BIT`) is killed the same way: a syscall number from another
//!   ABI means a number-to-syscall mapping unlike the one the filter's list is
//!   written in, so there is nothing to compare it against. Nothing legitimate
//!   reaches either branch.
//! * **The filter is a refusal list of numbers, not of names.** `socket` and
//!   `socketpair` are the syscalls whose *address family* matters, and `clone`
//!   is the one whose *flags* matter: `args[0]` is the address family for the
//!   first two and the `CLONE_*` flags for the last (§7.3).
//!
//! The filter cannot be removed once installed, and it applies to every thread
//! that exists when it is installed as well as every thread created later, so
//! it has to go on while the process is still single-threaded and before the
//! runtime or any request thread exists (§4) — which is why [`install`] refuses
//! a process that has more than one thread.

use std::io;

use crate::error::Error;

/// Which filter a process installs (ARCHITECTURE.md §7.3).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SeccompRole {
    /// A view process. `egress` is the manifest route's `egress` flag.
    View {
        /// Whether the route may create `AF_INET` and `AF_INET6` sockets.
        egress: bool,
    },
    /// A PHP view process (ADR-0010 §1). It has every [`SeccompRole::View`]
    /// rule, and it also may not create a process or a new program image.
    ///
    /// NTS `libphp` serves one request per OS process, and a route's workers
    /// are forked **before** this filter goes on, so a request that reaches
    /// `execve`, `execveat`, `fork`, `vfork` or a process-creating `clone`
    /// is trying to escape the process-wide CPU charge rather than to serve
    /// the request (§7.3, `R14`). Threads stay allowed: PHP extensions and
    /// glibc itself start them.
    EmbeddedView {
        /// Whether the route may create `AF_INET` and `AF_INET6` sockets.
        egress: bool,
    },
    /// The HTTP front end (§2.3): it parses bytes on inherited descriptors and
    /// must not be able to open a socket of its own.
    Http,
    /// The scheduler (§2.4): same as [`SeccompRole::Http`].
    Scheduler,
    /// The database proxy (§2.2): it adopts inherited Unix listeners and
    /// connects to the real PostgreSQL over TCP.
    DbProxy,
}

impl SeccompRole {
    /// The role's name, for logs and messages.
    pub const fn name(self) -> &'static str {
        match self {
            Self::View { .. } => "view",
            Self::EmbeddedView { .. } => "embedded-view",
            Self::Http => "http",
            Self::Scheduler => "scheduler",
            Self::DbProxy => "dbproxy",
        }
    }

    /// Whether this role may create `AF_INET`/`AF_INET6` sockets.
    ///
    /// Only an egress view and dbproxy may: dbproxy is the one component whose
    /// job is to reach the upstream database (§2.2).
    pub const fn egress(self) -> bool {
        match self {
            Self::View { egress } | Self::EmbeddedView { egress } => egress,
            Self::DbProxy => true,
            Self::Http | Self::Scheduler => false,
        }
    }

    /// Whether this role may create sockets at all.
    pub const fn sockets(self) -> bool {
        !matches!(self, Self::Http | Self::Scheduler)
    }

    /// Whether this role refuses to create a process or a new program image.
    ///
    /// Only [`SeccompRole::EmbeddedView`] does; its workers were forked before
    /// the filter went on, so nothing legitimate in a request creates one.
    const fn embedded(self) -> bool {
        matches!(self, Self::EmbeddedView { .. })
    }
}

/// Installs the filter for `role` on the calling process (ARCHITECTURE.md §7.3).
///
/// What every role refuses: `ptrace`, `pidfd_getfd`, `kcmp`, `process_vm_readv`,
/// `process_vm_writev`, `process_madvise`, `bpf`, `keyctl`, `add_key`,
/// `request_key`, `perf_event_open`, `userfaultfd`, the `io_uring` calls,
/// `kexec_load`, `kexec_file_load`, `init_module`, `finit_module`,
/// `delete_module`, `mount`, `umount2`, `pivot_root`, `open_tree`, `move_mount`,
/// `fsopen`, `fsconfig`, `fsmount`, `fspick`, `swapon`, `swapoff`, `reboot`,
/// `setns`, `unshare` and `open_by_handle_at`. They are the calls with which one
/// process reads or writes another's memory, loads code into the kernel, or
/// reaches for namespaces and mounts; none of them is part of serving a request.
///
/// What every role additionally refuses:
///
/// * `clone3` fails with `ENOSYS` — the one refusal that is not `EPERM`:
///   glibc's `pthread_create` tries `clone3` first and must fall back to
///   `clone`, which is still allowed.
/// * `clone` fails with `EPERM` when `args[0]` sets any `CLONE_NEW*` flag
///   (including `CLONE_NEWTIME`): a process may not reach for a namespace.
/// * `setsid` and `setpgid` fail with `EPERM`: a worker cannot leave init's
///   supervision by starting a new session or process group — init signals it
///   by pid and by process-group id. A view's own `setpgid(0, 0)` happens in
///   child setup *before* this filter is installed, so the call being refused
///   is never the legitimate one.
///
/// [`SeccompRole::EmbeddedView`] additionally refuses process creation and a
/// new program image — `execve`, `execveat`, `fork`, `vfork` and `clone`
/// without `CLONE_THREAD` all fail with `EPERM`; a thread-creating `clone`
/// stays allowed. [`SeccompRole::View`] keeps all of them.
///
/// | Role | `socket()` | `socketpair()` |
/// |---|---|---|
/// | view, no egress | every family but `AF_UNIX` | `AF_UNIX` only |
/// | view, egress | every family but `AF_UNIX`, `AF_INET`, `AF_INET6` | `AF_UNIX` only |
/// | embedded view, no egress | every family but `AF_UNIX` | `AF_UNIX` only |
/// | embedded view, egress | every family but `AF_UNIX`, `AF_INET`, `AF_INET6` | `AF_UNIX` only |
/// | http, scheduler | every `socket()` | `AF_UNIX` only |
/// | dbproxy | every family but `AF_UNIX`, `AF_INET`, `AF_INET6` | `AF_UNIX` only |
///
/// `socketpair()` stays `AF_UNIX`-only everywhere: a pair of descriptors a
/// process already owns is not a new way out of it, and std and tokio use one
/// internally on some platforms.
///
/// # Errors
///
/// [`std::io::Error`] when the process has more than one thread, when
/// `PR_SET_NO_NEW_PRIVS` or the filter installation fails, or on a platform
/// without an `AUDIT_ARCH_*` value. The filter itself can no longer fail once
/// the kernel has accepted it.
pub fn install(role: SeccompRole) -> io::Result<()> {
    if AUDIT_ARCH == 0 {
        return Err(Error::UnsupportedArchitecture {
            arch: std::env::consts::ARCH,
        }
        .into());
    }

    // The filter applies to every thread that already exists as well as every
    // one created later, and it cannot be removed, so installing it from a
    // process that has already started a thread would race with that thread's
    // own syscalls. init forks every role while it is single-threaded for
    // exactly this reason (§4), and this is where a mistake in that order
    // becomes a failed boot instead of a half-sandboxed process.
    match crate::thread_count() {
        Ok(1) => {}
        Ok(threads) => return Err(Error::NotSingleThreadedSeccomp { threads }.into()),
        Err(err) => return Err(step("counting threads", err)),
    }

    match PreparedFilter::for_role(role)?.install() {
        Ok(()) => Ok(()),
        Err(failure) => Err(step(
            failure.stage,
            io::Error::from_raw_os_error(failure.errno),
        )),
    }
}

/// A filter built and resolved ahead of the fork that installs it.
///
/// The program is a fixed stack array, so preparing a filter allocates nothing
/// and [`PreparedFilter::install`] is a pair of syscalls. That pair is exactly
/// what a **child of `fork()`** may run, which is where blastit installs its
/// filters — the filter has to go on while the process is still single-threaded
/// (§4, §7.3), and allocating in such a child can wait forever on a lock another
/// thread held at the instant of the fork.
///
/// [`install`] is the checked entry point init uses. This type exists for a
/// caller that prepares the program somewhere else, or that has to install one
/// in a process which cannot satisfy [`install`]'s single-threaded precondition
/// (a test process whose harness has threads of its own) — the filter's rules
/// are then still the ones this module builds.
pub struct PreparedFilter {
    filter: [libc::sock_filter; MAX_PROGRAM],
    len: u16,
}

impl PreparedFilter {
    /// Builds `role`'s filter, resolved and ready to install.
    ///
    /// # Errors
    ///
    /// [`std::io::Error`] when the program does not fit its buffer or a branch
    /// does not resolve — a bug in this module, never a reaction to input.
    pub fn for_role(role: SeccompRole) -> io::Result<Self> {
        Self::for_arch(role, AUDIT_ARCH)
    }

    /// [`for_role`] for the architecture the caller names, so a test can build
    /// a filter that has to kill.
    fn for_arch(role: SeccompRole, arch: u32) -> io::Result<Self> {
        let mut program = Program::new();
        build_for(role, arch, &mut program)?;
        let (filter, len) = program.resolve()?;
        Ok(Self { filter, len })
    }

    /// Hands the filter to the kernel: `PR_SET_NO_NEW_PRIVS` first — §7.3: that,
    /// and nothing else, is what allows an unprivileged process to install a
    /// filter, and it also means no child of this one may gain privileges — then
    /// `seccomp(SECCOMP_SET_MODE_FILTER)`, with the older `prctl(PR_SET_SECCOMP)`
    /// spelling as the fallback.
    ///
    /// Makes syscalls and nothing else: no allocation, no lock, no destructor,
    /// and a [`Copy`] failure value instead of an `io::Error`. A child of
    /// `fork()` may therefore call it. The filter cannot be removed afterwards.
    pub fn install(&self) -> Result<(), InstallFailure> {
        // The kernel's `sock_fprog` wants `*mut`; this stack copy keeps
        // `install` callable on a shared reference, and copying a fixed array
        // allocates nothing.
        let mut filter = self.filter;
        let mut descriptor = libc::sock_fprog {
            len: self.len,
            filter: filter.as_mut_ptr(),
        };

        // SAFETY: `PR_SET_NO_NEW_PRIVS` takes one integer, 1 to enable it, and
        // ignores the rest. `descriptor` describes `self.len` instructions in
        // `filter`, which lives on this frame across both calls; the kernel
        // copies the program before returning.
        unsafe {
            if libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) == -1 {
                return Err(InstallFailure {
                    stage: "PR_SET_NO_NEW_PRIVS",
                    errno: current_errno(),
                });
            }

            let installed = libc::syscall(
                libc::SYS_seccomp,
                libc::SECCOMP_SET_MODE_FILTER as libc::c_long,
                0 as libc::c_long,
                &mut descriptor as *mut libc::sock_fprog,
            );
            if installed == 0 {
                return Ok(());
            }
            // A kernel without `seccomp(2)` (before 3.17) still has the `prctl`
            // spelling, which takes the same program.
            if libc::prctl(
                libc::PR_SET_SECCOMP,
                libc::SECCOMP_MODE_FILTER as libc::c_ulong,
                &mut descriptor as *mut libc::sock_fprog,
            ) == 0
            {
                return Ok(());
            }
        }
        Err(InstallFailure {
            stage: "installing the filter",
            errno: current_errno(),
        })
    }
}

/// Which step of [`PreparedFilter::install`] failed, and the `errno` there.
///
/// [`Copy`] and heap-free, so a child of `fork()` can hold one without
/// allocating: [`install`] is what turns it into the `io::Error` a caller
/// reports.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct InstallFailure {
    /// The step, named as the boot message names it.
    pub stage: &'static str,
    /// The `errno` that step returned.
    pub errno: i32,
}

/// The `errno` of the last failed call, read without building an `io::Error`.
fn current_errno() -> i32 {
    io::Error::last_os_error()
        .raw_os_error()
        .unwrap_or(libc::EINVAL)
}

/// The `AUDIT_ARCH_*` value `seccomp_data.arch` must carry.
///
/// libc does not define `AUDIT_ARCH_*`, so it is spelled out: `EM_X86_64` or
/// `EM_AARCH64` with `__AUDIT_ARCH_64BIT | __AUDIT_ARCH_LE` (`0xc000_0000`).
/// The filter kills a process whose `arch` is something else, so a platform
/// with no value here gets [`Error::UnsupportedArchitecture`] from [`install`]
/// instead of a filter that would refuse every syscall.
#[cfg(target_arch = "x86_64")]
const AUDIT_ARCH: u32 = 0xc000_003e;
#[cfg(target_arch = "aarch64")]
const AUDIT_ARCH: u32 = 0xc000_00b7;
#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
const AUDIT_ARCH: u32 = 0;

/// `__X32_SYSCALL_BIT`: an x32 syscall number on x86_64, which shares the
/// syscall instruction with 64-bit code but not its table (§7.3).
#[cfg(target_arch = "x86_64")]
const X32_SYSCALL_BIT: u32 = 0x4000_0000;

/// `kexec_file_load`'s syscall number.
///
/// The number is per architecture, and it is not one of the unified ones:
/// `kexec_file_load` arrived in 3.17, before new syscalls were numbered alike
/// everywhere, so it is **320** on x86_64 and **294** on aarch64's asm-generic
/// table. `libc` 0.2.191 publishes `SYS_kexec_file_load` for x86_64 in both
/// libcs and for aarch64 in glibc, but not for `aarch64-unknown-linux-musl`,
/// which is what makes that target fail to compile. The number is therefore
/// spelled out for the two architectures this module can filter — the same two
/// values `SHARED_DENIED_NUMBERS` pins below — and read from `libc` elsewhere,
/// where [`install`] refuses with [`Error::UnsupportedArchitecture`] before any
/// table is compared.
#[cfg(target_arch = "x86_64")]
const SYS_KEXEC_FILE_LOAD: libc::c_long = 320;
#[cfg(target_arch = "aarch64")]
const SYS_KEXEC_FILE_LOAD: libc::c_long = 294;
#[cfg(not(any(target_arch = "x86_64", target_arch = "aarch64")))]
const SYS_KEXEC_FILE_LOAD: libc::c_long = libc::SYS_kexec_file_load;

/// The syscalls every role refuses, with the name of each for documentation.
///
/// The names are not part of the filter — a filter compares numbers — but they
/// are what makes this list readable and testable. Every name is spelled by
/// libc for `x86_64` and `aarch64`, the two architectures this module has an
/// `AUDIT_ARCH_*` for.
const DENIED: &[(&str, libc::c_long)] = &[
    ("ptrace", libc::SYS_ptrace),
    ("pidfd_getfd", libc::SYS_pidfd_getfd),
    ("kcmp", libc::SYS_kcmp),
    ("process_vm_readv", libc::SYS_process_vm_readv),
    ("process_vm_writev", libc::SYS_process_vm_writev),
    ("process_madvise", libc::SYS_process_madvise),
    ("bpf", libc::SYS_bpf),
    ("keyctl", libc::SYS_keyctl),
    ("add_key", libc::SYS_add_key),
    ("request_key", libc::SYS_request_key),
    ("perf_event_open", libc::SYS_perf_event_open),
    ("userfaultfd", libc::SYS_userfaultfd),
    ("io_uring_setup", libc::SYS_io_uring_setup),
    ("io_uring_enter", libc::SYS_io_uring_enter),
    ("io_uring_register", libc::SYS_io_uring_register),
    ("kexec_load", libc::SYS_kexec_load),
    ("kexec_file_load", SYS_KEXEC_FILE_LOAD),
    ("init_module", libc::SYS_init_module),
    ("finit_module", libc::SYS_finit_module),
    ("delete_module", libc::SYS_delete_module),
    ("mount", libc::SYS_mount),
    ("umount2", libc::SYS_umount2),
    ("pivot_root", libc::SYS_pivot_root),
    ("open_tree", libc::SYS_open_tree),
    ("move_mount", libc::SYS_move_mount),
    ("fsopen", libc::SYS_fsopen),
    ("fsconfig", libc::SYS_fsconfig),
    ("fsmount", libc::SYS_fsmount),
    ("fspick", libc::SYS_fspick),
    ("swapon", libc::SYS_swapon),
    ("swapoff", libc::SYS_swapoff),
    ("reboot", libc::SYS_reboot),
    ("setns", libc::SYS_setns),
    ("unshare", libc::SYS_unshare),
    ("open_by_handle_at", libc::SYS_open_by_handle_at),
];

/// The extra syscalls [`SeccompRole::EmbeddedView`] refuses beyond
/// [`DENIED`], with each name for documentation.
///
/// `fork` and `vfork` have no syscall number on an architecture whose table
/// does not define one (`aarch64`): there glibc's `fork`/`vfork` compile to
/// `clone`, which the role's `CLONE_THREAD` check already refuses.
#[cfg(target_arch = "x86_64")]
const EMBEDDED_DENIED: &[(&str, libc::c_long)] = &[
    ("execve", libc::SYS_execve),
    ("execveat", libc::SYS_execveat),
    ("fork", libc::SYS_fork),
    ("vfork", libc::SYS_vfork),
];
#[cfg(not(target_arch = "x86_64"))]
const EMBEDDED_DENIED: &[(&str, libc::c_long)] = &[
    ("execve", libc::SYS_execve),
    ("execveat", libc::SYS_execveat),
];

/// The offsets of the fields this filter reads, inside `struct seccomp_data`.
///
/// ```text
/// struct seccomp_data { int nr; __u32 arch; __u64 ip; __u64 args[6]; };
/// ```
///
/// Both supported architectures are little-endian, so the low 32 bits of
/// `args[0]` are the first four bytes of the field — and an address family fits
/// in 32 bits with room to spare.
mod data {
    /// The syscall number.
    pub const NR: u32 = 0;
    /// The architecture the syscall was made from.
    pub const ARCH: u32 = 4;
    /// `args[0]`: the address family of `socket`/`socketpair`, or the
    /// `CLONE_*` flags of `clone`.
    pub const ARG0: u32 = 16;
}

/// `BPF_LD | BPF_W | BPF_ABS`: load a 32-bit word from the data at `k`.
const LD_ABS: u16 = (libc::BPF_LD | libc::BPF_W | libc::BPF_ABS) as u16;
/// `BPF_JMP | BPF_JEQ | BPF_K`: compare A with `k` and branch.
const JEQ: u16 = (libc::BPF_JMP | libc::BPF_JEQ | libc::BPF_K) as u16;
/// `BPF_JMP | BPF_JSET | BPF_K`: branch when any bit of `k` is set in A.
const JSET: u16 = (libc::BPF_JMP | libc::BPF_JSET | libc::BPF_K) as u16;
/// `BPF_JMP | BPF_JA`: an unconditional branch, with its distance in `k`.
const JA: u16 = (libc::BPF_JMP | libc::BPF_JA) as u16;
/// `BPF_RET | BPF_K`: return the action in `k`.
const RET: u16 = (libc::BPF_RET | libc::BPF_K) as u16;

/// The longest program [`build`] builds, in instructions.
///
/// `install` runs in a freshly forked child, where allocating is not safe, so
/// the program lives on the stack; the current worst case is **66**
/// instructions (`EmbeddedView { egress: true }` on `x86_64`, 63 on
/// `aarch64`) and there is room for the list to grow.
const MAX_PROGRAM: usize = 96;

/// `CLONE_NEW*` in `clone`'s `flags`: every flag that creates a namespace.
///
/// `CLONE_NEWTIME` (`0x80`), `CLONE_NEWNS` (`0x0002_0000`), `CLONE_NEWCGROUP`
/// (`0x0200_0000`), `CLONE_NEWUTS` (`0x0400_0000`), `CLONE_NEWIPC`
/// (`0x0800_0000`), `CLONE_NEWUSER` (`0x1000_0000`), `CLONE_NEWPID`
/// (`0x2000_0000`) and `CLONE_NEWNET` (`0x4000_0000`).
const CLONE_NEW_MASK: u32 = 0x7E02_0080;

/// `CLONE_THREAD`: the child is a thread in this process, not a new process.
const CLONE_THREAD: u32 = 0x0001_0000;

/// `SECCOMP_RET_ERRNO | ENOSYS`: "this kernel does not have that syscall".
///
/// libc's `pthread_create` probes `clone3` and falls back to `clone` on
/// `ENOSYS`, which is the only reason [`SeccompRole`] refuses it that way
/// instead of with `EPERM`.
const RET_ENOSYS: u32 = libc::SECCOMP_RET_ERRNO | libc::ENOSYS as u32;

/// Where a branch goes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Target {
    /// The instruction right after this one.
    Next,
    /// The instruction carrying this label.
    Label(Label),
}

/// A branch target, resolved to an instruction index once the whole program is
/// known: classic BPF jumps are relative and forward-only.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Label {
    /// The first instruction after the architecture check.
    AfterArch,
    /// The `socket()` family checks.
    Socket,
    /// The `socketpair()` family check.
    SocketPair,
    /// `SECCOMP_RET_ALLOW`.
    Allow,
    /// `SECCOMP_RET_ERRNO(EPERM)`.
    Deny,
    /// `SECCOMP_RET_ERRNO(ENOSYS)`.
    Enosys,
    /// The `clone()` checks on `args[0]`.
    Clone,
    /// `SECCOMP_RET_KILL_PROCESS`.
    Kill,
}

/// One instruction, before its label is turned into an offset.
#[derive(Clone, Copy)]
struct Step {
    code: u16,
    k: u32,
    label: Option<Label>,
    on_true: Target,
    on_false: Target,
}

/// A step that is neither a branch nor a label, for the array's filler.
const EMPTY: Step = Step {
    code: 0,
    k: 0,
    label: None,
    on_true: Target::Next,
    on_false: Target::Next,
};

/// A program under construction: a fixed stack buffer and a length.
struct Program {
    steps: [Step; MAX_PROGRAM],
    len: usize,
}

impl Program {
    /// An empty program.
    const fn new() -> Self {
        Self {
            steps: [EMPTY; MAX_PROGRAM],
            len: 0,
        }
    }

    /// Appends one instruction; an overflow is a bug in this module, not in the
    /// input, so it is reported rather than panicked on.
    fn push(&mut self, step: Step) -> io::Result<()> {
        if self.len >= MAX_PROGRAM {
            return Err(bad_program("the filter is longer than its buffer"));
        }
        self.steps[self.len] = step;
        self.len += 1;
        Ok(())
    }

    /// The index of the instruction carrying `label`.
    fn label(&self, label: Label) -> io::Result<usize> {
        self.steps[..self.len]
            .iter()
            .position(|step| step.label == Some(label))
            .ok_or_else(|| bad_program("a branch target is missing"))
    }

    /// Resolves every label into the `jt`/`jf` offsets the kernel wants.
    ///
    /// Returns the instruction array and how much of it is used. Classic BPF
    /// offsets are unsigned bytes, so a jump is at most 255 instructions long —
    /// the longest one here is about 50 — and can only go forward.
    fn resolve(&self) -> io::Result<([libc::sock_filter; MAX_PROGRAM], u16)> {
        let mut filter = [libc::sock_filter {
            code: 0,
            jt: 0,
            jf: 0,
            k: 0,
        }; MAX_PROGRAM];
        for (index, step) in self.steps[..self.len].iter().enumerate() {
            // `BPF_JA` carries its distance in `k`, the conditional jumps carry
            // theirs in `jt` and `jf`.
            let (jt, jf, k) = match step.code {
                JA => (0, 0, u32::from(self.distance(index, step.on_true)?)),
                JEQ | JSET => (
                    self.distance(index, step.on_true)?,
                    self.distance(index, step.on_false)?,
                    step.k,
                ),
                _ => (0, 0, step.k),
            };
            filter[index] = libc::sock_filter {
                code: step.code,
                jt,
                jf,
                k,
            };
        }
        Ok((filter, self.len as u16))
    }

    /// How far a branch from `index` has to jump.
    fn distance(&self, index: usize, target: Target) -> io::Result<u8> {
        let to = match target {
            Target::Next => index + 1,
            Target::Label(label) => self.label(label)?,
        };
        let distance = to
            .checked_sub(index + 1)
            .ok_or_else(|| bad_program("a branch points backwards"))?;
        u8::try_from(distance).map_err(|_| bad_program("a branch is longer than 255 instructions"))
    }
}

/// `ld [k]`: load the word at offset `k` of `seccomp_data`.
fn load(k: u32) -> Step {
    Step {
        code: LD_ABS,
        k,
        ..EMPTY
    }
}

/// `if A == k then on_true else on_false`.
fn compare(code: u16, k: u32, on_true: Target, on_false: Target) -> Step {
    Step {
        code,
        k,
        label: None,
        on_true,
        on_false,
    }
}

/// An unconditional branch.
fn branch(to: Target) -> Step {
    Step {
        code: JA,
        k: 0,
        label: None,
        on_true: to,
        on_false: Target::Next,
    }
}

/// A `SECCOMP_RET_*` action.
fn action(value: u32) -> Step {
    Step {
        code: RET,
        k: value,
        ..EMPTY
    }
}

/// Marks `step` as the target of `label`.
fn labelled(label: Label, step: Step) -> Step {
    Step {
        label: Some(label),
        ..step
    }
}

/// Builds the filter for `role` on the architecture `arch`, in the order the
/// kernel walks it.
///
/// The shape is: check the architecture, then the syscall number against the
/// refusal list, then the address family of the two socket calls and the flags
/// of `clone`, and finally the shared actions. Every branch is forward, which
/// is all classic BPF has.
///
/// `arch` is spelled out — [`PreparedFilter::for_role`] passes the running one —
/// so a test can build a filter that has to kill.
fn build_for(role: SeccompRole, arch: u32, program: &mut Program) -> io::Result<()> {
    // A filter written for one architecture's syscall table means nothing on
    // another, so a foreign `arch` kills the process (§7.3). Nothing
    // legitimate gets here: blastit's children are all this same image.
    program.push(load(data::ARCH))?;
    program.push(compare(
        JEQ,
        arch,
        Target::Label(Label::AfterArch),
        Target::Next,
    ))?;
    program.push(action(libc::SECCOMP_RET_KILL_PROCESS))?;

    // From here on A holds the syscall number.
    program.push(labelled(Label::AfterArch, load(data::NR)))?;
    // An x32 syscall number is not one of the numbers below: x32 shares the
    // syscall instruction with x86_64 but not its table, so the same number
    // means something else there. Nothing legitimate makes one, and the
    // process dies exactly as it does on a foreign architecture (§7.3).
    #[cfg(target_arch = "x86_64")]
    program.push(compare(
        JSET,
        X32_SYSCALL_BIT,
        Target::Label(Label::Kill),
        Target::Next,
    ))?;

    for (_name, number) in DENIED {
        program.push(compare(
            JEQ,
            *number as u32,
            Target::Label(Label::Deny),
            Target::Next,
        ))?;
    }

    // `EmbeddedView` adds its own refusals on top of the shared list; `View`
    // never sees one of these numbers, so it cannot widen by accident.
    if role.embedded() {
        for (_name, number) in EMBEDDED_DENIED {
            program.push(compare(
                JEQ,
                *number as u32,
                Target::Label(Label::Deny),
                Target::Next,
            ))?;
        }
    }

    // `socket()` is the only way to reach the network, and the role decides
    // whether there is one at all.
    let socket = if role.sockets() {
        Target::Label(Label::Socket)
    } else {
        Target::Label(Label::Deny)
    };
    program.push(compare(JEQ, libc::SYS_socket as u32, socket, Target::Next))?;
    program.push(compare(
        JEQ,
        libc::SYS_socketpair as u32,
        Target::Label(Label::SocketPair),
        Target::Next,
    ))?;

    // `clone3` is answered with `ENOSYS` so glibc's `pthread_create` falls
    // back to `clone`; `clone`, `setsid` and `setpgid` fail with `EPERM` for
    // every view (§7.3, `R14`).
    program.push(compare(
        JEQ,
        libc::SYS_clone3 as u32,
        Target::Label(Label::Enosys),
        Target::Next,
    ))?;
    program.push(compare(
        JEQ,
        libc::SYS_clone as u32,
        Target::Label(Label::Clone),
        Target::Next,
    ))?;
    program.push(compare(
        JEQ,
        libc::SYS_setsid as u32,
        Target::Label(Label::Deny),
        Target::Next,
    ))?;
    program.push(compare(
        JEQ,
        libc::SYS_setpgid as u32,
        Target::Label(Label::Deny),
        Target::Next,
    ))?;
    // Anything else is allowed: the default action of the filter.
    program.push(branch(Target::Label(Label::Allow)))?;

    program.push(labelled(Label::Socket, load(data::ARG0)))?;
    program.push(compare(
        JEQ,
        libc::AF_UNIX as u32,
        Target::Label(Label::Allow),
        Target::Next,
    ))?;
    if role.egress() {
        program.push(compare(
            JEQ,
            libc::AF_INET as u32,
            Target::Label(Label::Allow),
            Target::Next,
        ))?;
        program.push(compare(
            JEQ,
            libc::AF_INET6 as u32,
            Target::Label(Label::Allow),
            Target::Next,
        ))?;
    }
    program.push(branch(Target::Label(Label::Deny)))?;

    program.push(labelled(Label::SocketPair, load(data::ARG0)))?;
    program.push(compare(
        JEQ,
        libc::AF_UNIX as u32,
        Target::Label(Label::Allow),
        Target::Next,
    ))?;
    program.push(branch(Target::Label(Label::Deny)))?;

    // `clone` may create a thread; it may not reach for a namespace. A role
    // that may not create a process must also see `CLONE_THREAD`, so the one
    // `clone` shape it keeps is a thread.
    program.push(labelled(Label::Clone, load(data::ARG0)))?;
    let after_namespace = if role.embedded() {
        Target::Next
    } else {
        Target::Label(Label::Allow)
    };
    program.push(compare(
        JSET,
        CLONE_NEW_MASK,
        Target::Label(Label::Deny),
        after_namespace,
    ))?;
    if role.embedded() {
        program.push(compare(
            JSET,
            CLONE_THREAD,
            Target::Label(Label::Allow),
            Target::Label(Label::Deny),
        ))?;
    }

    program.push(labelled(Label::Enosys, action(RET_ENOSYS)))?;
    program.push(labelled(Label::Allow, action(libc::SECCOMP_RET_ALLOW)))?;
    program.push(labelled(
        Label::Deny,
        // The refused syscall fails with `EPERM`, which every caller in the
        // process reports as "not permitted" instead of dying.
        action(libc::SECCOMP_RET_ERRNO | libc::EPERM as u32),
    ))?;
    // A syscall number from another ABI is not something a refusal can answer:
    // the process that made one is not running the code blastit thinks it is.
    program.push(labelled(
        Label::Kill,
        action(libc::SECCOMP_RET_KILL_PROCESS),
    ))?;
    Ok(())
}

/// A failed step of [`install`], with the `errno` read immediately.
fn step(what: &'static str, source: io::Error) -> io::Error {
    Error::Seccomp {
        stage: what,
        source,
    }
    .into()
}

/// A program this module built wrong: a bug here, not a reaction to input.
fn bad_program(detail: &'static str) -> io::Error {
    step(
        "building the filter",
        io::Error::new(io::ErrorKind::InvalidData, detail),
    )
}
