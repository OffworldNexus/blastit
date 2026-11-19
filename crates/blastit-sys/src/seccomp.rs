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

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;
    use crate::ExitStatus;
    use crate::error::Error;
    use crate::testutil::{PROBE_VAR, child_status, report_probe, run_probe};

    /// Every role, so the tests cannot forget one.
    const ROLES: &[SeccompRole] = &[
        SeccompRole::View { egress: false },
        SeccompRole::View { egress: true },
        SeccompRole::EmbeddedView { egress: false },
        SeccompRole::EmbeddedView { egress: true },
        SeccompRole::Http,
        SeccompRole::Scheduler,
        SeccompRole::DbProxy,
    ];

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

    // # The async-signal-safe rule for every child of `fork()` in these tests
    //
    // The `fork()` happens inside the **multi-threaded** `libtest` process. At
    // the instant of the fork another thread may hold the allocator's lock, a
    // stdio lock or a lock inside the harness itself, and the child has no
    // other thread to release it: it waits there forever (`futex_wait_queue`),
    // which is how a `make fast-check` run once hung for ten minutes and how a
    // second one was OOM-killed while spawning a thread.
    //
    // So a child body here makes **raw syscalls only**. Inside it and in
    // everything it calls: no `Vec`, `String`, `format!`, `println!`,
    // `assert!`/`assert_eq!`, `unwrap`, `Box`, `panic!`, no `Drop` that frees,
    // and no function that can allocate or take a lock. Everything that needs
    // any of those runs in the parent — before the fork (the resolved
    // [`PreparedFilter`], which `Program::resolve` writes into a fixed stack
    // array) or after `waitpid` (every assertion and every message). The only
    // thing a child reports is an exit code.
    //
    // [`PreparedFilter::install`] is the child-side primitive: `prctl` and
    // `seccomp` and nothing else, with a `Copy` failure value instead of an
    // `io::Error`. The two bodies that *need* to allocate or start a thread run
    // in a fresh process instead ([`run_probe`]).

    /// Close every descriptor at or above 3, with one raw syscall.
    ///
    /// A sibling test deliberately holds a descriptor **without**
    /// `FD_CLOEXEC` — that is `dup_onto`'s whole subject — and a child must not
    /// keep it open. Production closes them the same way in `close_fds_except`;
    /// a child cannot run that function's allocating scan fallback, so a kernel
    /// older than 5.9 keeps them for the microseconds until `_exit`.
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

    /// This thread's `errno`, read without allocating or taking a lock.
    fn raw_errno() -> i32 {
        // SAFETY: `__errno_location` returns this thread's `errno` cell.
        unsafe { *libc::__errno_location() }
    }

    /// `socket(domain, SOCK_STREAM, 0)`, closing what it created.
    ///
    /// `0` when the family is permitted, else the `errno` of the refusal.
    fn raw_socket(domain: libc::c_int) -> i32 {
        // SAFETY: `socket` takes three integers and returns a descriptor.
        let fd = unsafe { libc::socket(domain, libc::SOCK_STREAM, 0) };
        if fd == -1 {
            return raw_errno();
        }
        // SAFETY: the descriptor was just created here and nothing else owns it.
        unsafe { libc::close(fd) };
        0
    }

    /// `socketpair(domain, SOCK_STREAM, 0, sv)`, closing what it created.
    fn raw_socketpair(domain: libc::c_int) -> i32 {
        let mut fds = [0 as libc::c_int; 2];
        // SAFETY: `socketpair` writes two descriptors into the two-element
        // array, which is exactly the storage it needs.
        let created = unsafe { libc::socketpair(domain, libc::SOCK_STREAM, 0, fds.as_mut_ptr()) };
        if created == -1 {
            return raw_errno();
        }
        // SAFETY: both descriptors were just created here.
        unsafe {
            libc::close(fds[0]);
            libc::close(fds[1]);
        }
        0
    }

    /// `ptrace(PTRACE_TRACEME, 0, 0, 0)`, which an unprivileged process may
    /// otherwise call on itself.
    fn raw_ptrace() -> i32 {
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
        if traced == -1 { raw_errno() } else { 0 }
    }

    /// Reap `pid` with raw syscalls: `0` when it was reaped, `1` when `waitpid`
    /// failed for a reason other than `EINTR` — no test accepts that.
    fn raw_wait(pid: libc::pid_t) -> i32 {
        let mut status = 0;
        loop {
            // SAFETY: `waitpid` writes one status word into the valid storage
            // `status`; only this process's own child is waited for.
            let waited = unsafe { libc::waitpid(pid, &mut status, 0) };
            if waited == pid {
                return 0;
            }
            if raw_errno() != libc::EINTR {
                return 1;
            }
        }
    }

    /// `clone(flags, NULL, NULL, NULL, 0)`.
    ///
    /// `0` when a process child was created — it `_exit`s immediately and this
    /// process reaps it — else the `errno` of the refusal.
    fn raw_clone(flags: libc::c_ulong) -> i32 {
        // SAFETY: the five arguments are the flags, three optionally-null
        // pointer arguments and the TLS value; without `CLONE_VM` the null
        // stack makes this a process fork, which is what the child body
        // (one `_exit`) is safe for.
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
            return raw_errno();
        }
        if pid == 0 {
            // SAFETY: this is the new process; `_exit` never returns and runs
            // no destructors.
            unsafe { libc::_exit(0) }
        }
        raw_wait(pid as libc::pid_t)
    }

    /// `clone3(NULL, 0)`: `EFAULT` on a kernel without the filter, `ENOSYS`
    /// once the filter answers it.
    fn raw_clone3() -> i32 {
        // SAFETY: `clone3` takes the `args` pointer and its size; a null
        // pointer is rejected by the kernel with `EFAULT` before it is read.
        let ret = unsafe {
            libc::syscall(
                libc::SYS_clone3,
                std::ptr::null_mut::<libc::c_void>(),
                0_usize,
            )
        };
        if ret == -1 { raw_errno() } else { 0 }
    }

    /// `fork(2)` or `vfork(2)` by syscall number; the child body is one
    /// `_exit`.
    #[cfg(target_arch = "x86_64")]
    fn raw_fork_like(number: libc::c_long) -> i32 {
        // SAFETY: both take no arguments; the child only `_exit`s.
        let pid = unsafe { libc::syscall(number) };
        if pid == -1 {
            return raw_errno();
        }
        if pid == 0 {
            // SAFETY: this is the new process; `_exit` never returns.
            unsafe { libc::_exit(0) }
        }
        raw_wait(pid as libc::pid_t)
    }

    /// `fork(2)`, on the architecture whose syscall table has it.
    #[cfg(target_arch = "x86_64")]
    fn raw_fork() -> i32 {
        raw_fork_like(libc::SYS_fork)
    }

    /// `vfork(2)`, on the architecture whose syscall table has it.
    ///
    /// Only ever called against `EmbeddedView`, where the filter refuses it
    /// before a child exists: a real `vfork` child shares this stack.
    #[cfg(target_arch = "x86_64")]
    fn raw_vfork() -> i32 {
        raw_fork_like(libc::SYS_vfork)
    }

    /// `execve("/nonexistent", ...)`: `ENOENT` when permitted, `EPERM` when
    /// the role refuses it.
    fn raw_execve() -> i32 {
        let path = b"/nonexistent-blastit-seccomp\0";
        let argv = [std::ptr::null::<libc::c_char>()];
        let envp = [std::ptr::null::<libc::c_char>()];
        // SAFETY: `path` is NUL-terminated and both arrays are
        // NULL-terminated; a missing file is `ENOENT` before any image runs.
        let ret = unsafe {
            libc::syscall(
                libc::SYS_execve,
                path.as_ptr().cast::<libc::c_char>(),
                argv.as_ptr(),
                envp.as_ptr(),
            )
        };
        if ret == -1 { raw_errno() } else { 0 }
    }

    /// `execveat(AT_FDCWD, "/nonexistent", ...)`.
    fn raw_execveat() -> i32 {
        let path = b"/nonexistent-blastit-seccomp\0";
        let argv = [std::ptr::null::<libc::c_char>()];
        let envp = [std::ptr::null::<libc::c_char>()];
        // SAFETY: the same NUL- and NULL-terminated arguments as `raw_execve`,
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
        if ret == -1 { raw_errno() } else { 0 }
    }

    /// `setsid()`.
    fn raw_setsid() -> i32 {
        // SAFETY: `setsid` takes no arguments.
        if unsafe { libc::setsid() } == -1 {
            raw_errno()
        } else {
            0
        }
    }

    /// `setpgid(0, 0)`, the calling process into its own group.
    fn raw_setpgid() -> i32 {
        // SAFETY: `setpgid` takes two pids; 0 means the calling process.
        if unsafe { libc::setpgid(0, 0) } == -1 {
            raw_errno()
        } else {
            0
        }
    }

    /// `fork()`, install `filter`, run `body`, and require exit code `0`.
    ///
    /// Every `body` passed here is raw syscalls only — see the rule at the top
    /// of this module's tests.
    fn in_filtered_child(label: &str, filter: PreparedFilter, body: impl FnOnce() -> i32) {
        match filtered_child(filter, body) {
            ExitStatus::Code(0) => {}
            ExitStatus::Code(254) => {
                panic!("{label}: the forked child could not install the filter")
            }
            ExitStatus::Code(code) => panic!("{label}: the forked child reported failure {code}"),
            other => panic!("{label}: the forked child did not exit normally: {other}"),
        }
    }

    /// [`in_filtered_child`] without insisting on a normal exit, for the tests
    /// whose expected answer is a signal death.
    fn filtered_child(filter: PreparedFilter, body: impl FnOnce() -> i32) -> ExitStatus {
        // This is the one `fork()` in this module: raw syscalls only from here
        // to `_exit`, because the harness that called it is multi-threaded.
        child_status(move || {
            close_inherited();
            if filter.install().is_err() {
                return 254;
            }
            body()
        })
    }

    /// `role`'s filter, prepared in the parent where allocating is safe.
    fn prepared(role: SeccompRole) -> PreparedFilter {
        PreparedFilter::for_role(role).unwrap()
    }

    /// `role`'s filter for `arch`, so a test can build one that has to kill.
    fn prepared_for(role: SeccompRole, arch: u32) -> PreparedFilter {
        PreparedFilter::for_arch(role, arch).unwrap()
    }

    /// The test a probe process matches, so only this module's runner starts.
    const PROBE_RUNNER: &str = "seccomp::tests::probe_runner";

    /// The one test a probe process runs; a no-op in an ordinary test run.
    ///
    /// Two bodies cannot run in a child of `fork()`: one starts a thread and the
    /// other must not be single-threaded, and both allocate. They run in a
    /// process of their own instead — see `crate::testutil::run_probe`.
    #[test]
    fn probe_runner() {
        let Ok(name) = std::env::var(PROBE_VAR) else {
            return;
        };
        report_probe(&name, probe(&name).unwrap_or(1));
    }

    /// Run probe `name`, in a process that may allocate and start threads.
    fn probe(name: &str) -> Option<i32> {
        match name {
            "thread-refusal" => Some(probe_thread_refusal()),
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

    /// `install` must refuse a process that already has a second thread, and it
    /// must refuse **before** the kernel sees the filter.
    fn probe_thread_refusal() -> i32 {
        let worker = std::thread::spawn(std::thread::park);
        for _ in 0..100_000 {
            match crate::thread_count() {
                Ok(threads) if threads >= 2 => break,
                _ => std::thread::yield_now(),
            }
        }
        match install(SeccompRole::View { egress: false }) {
            Err(err) => match Error::from_io(&err) {
                Some(Error::NotSingleThreadedSeccomp { threads }) if *threads >= 2 => {}
                _ => return 2,
            },
            Ok(()) => return 1,
        }
        // The refusal happened before the filter could be installed: a socket
        // the filter would have refused still works.
        if raw_socket(libc::AF_INET) != 0 {
            return 3;
        }
        worker.thread().unpark();
        0
    }

    /// `clone3` answers `ENOSYS`, so glibc's `pthread_create` falls back to
    /// `clone` — and a thread really starts afterwards.
    fn probe_thread_after_filter(role: SeccompRole) -> i32 {
        let Ok(filter) = PreparedFilter::for_role(role) else {
            return 1;
        };
        if filter.install().is_err() {
            return 1;
        }
        if raw_clone3() != libc::ENOSYS {
            return 2;
        }
        match std::thread::spawn(|| 7).join() {
            Ok(7) => 0,
            _ => 3,
        }
    }

    /// The syscall numbers the filter's comparison chain tests, in order.
    ///
    /// The `JEQ` on `seccomp_data.arch` is skipped: it compares an
    /// `AUDIT_ARCH_*` value, not a syscall number.
    fn compared_syscalls(role: SeccompRole) -> Vec<u32> {
        let mut program = Program::new();
        build_for(role, AUDIT_ARCH, &mut program).unwrap();
        let end = program.steps[..program.len]
            .iter()
            .position(|step| step.code == JA)
            .unwrap();
        program.steps[..end]
            .iter()
            .filter(|step| step.code == JEQ && step.k != AUDIT_ARCH)
            .map(|step| step.k)
            .collect()
    }

    #[test]
    fn a_view_without_egress_keeps_unix_and_loses_inet() {
        in_filtered_child(
            "view socket",
            prepared(SeccompRole::View { egress: false }),
            || {
                if raw_socket(libc::AF_UNIX) != 0 {
                    return 1;
                }
                if raw_socket(libc::AF_INET) != libc::EPERM {
                    return 2;
                }
                if raw_socket(libc::AF_INET6) != libc::EPERM {
                    return 3;
                }
                if raw_socket(libc::AF_NETLINK) != libc::EPERM {
                    return 4;
                }
                // A pair of descriptors this process owns stays available.
                if raw_socketpair(libc::AF_UNIX) != 0 {
                    return 5;
                }
                0
            },
        );
    }

    #[test]
    fn socketpair_is_unix_only_for_every_role() {
        for role in ROLES {
            in_filtered_child(&format!("{role:?} socketpair"), prepared(*role), || {
                if raw_socketpair(libc::AF_UNIX) != 0 {
                    return 1;
                }
                // `EPERM`, not the `EOPNOTSUPP` the kernel itself returns for a
                // family `socketpair` does not support.
                if raw_socketpair(libc::AF_INET) != libc::EPERM {
                    return 2;
                }
                0
            });
        }
    }

    #[test]
    fn a_view_with_egress_keeps_inet_and_inet6() {
        in_filtered_child(
            "egress view socket",
            prepared(SeccompRole::View { egress: true }),
            || {
                if raw_socket(libc::AF_INET) != 0 {
                    return 1;
                }
                if raw_socket(libc::AF_INET6) != 0 {
                    return 2;
                }
                if raw_socket(libc::AF_UNIX) != 0 {
                    return 3;
                }
                // Egress is a permission, not a requirement: everything else is
                // still refused.
                if raw_socket(libc::AF_NETLINK) != libc::EPERM {
                    return 4;
                }
                0
            },
        );
    }

    #[test]
    fn http_and_the_scheduler_cannot_open_a_socket_at_all() {
        for role in [SeccompRole::Http, SeccompRole::Scheduler] {
            in_filtered_child(&format!("{role:?} socket"), prepared(role), || {
                for domain in [libc::AF_UNIX, libc::AF_INET, libc::AF_INET6] {
                    if raw_socket(domain) != libc::EPERM {
                        return 1;
                    }
                }
                if raw_socketpair(libc::AF_UNIX) != 0 {
                    return 2;
                }
                0
            });
        }
    }

    #[test]
    fn dbproxy_keeps_unix_and_tcp() {
        in_filtered_child("dbproxy socket", prepared(SeccompRole::DbProxy), || {
            for domain in [libc::AF_UNIX, libc::AF_INET, libc::AF_INET6] {
                if raw_socket(domain) != 0 {
                    return 1;
                }
            }
            if raw_socket(libc::AF_NETLINK) != libc::EPERM {
                return 2;
            }
            0
        });
    }

    #[test]
    fn every_role_refuses_ptrace() {
        for role in ROLES {
            in_filtered_child(&format!("{role:?} ptrace"), prepared(*role), || {
                if raw_ptrace() != libc::EPERM {
                    return 1;
                }
                0
            });
        }
    }

    #[test]
    fn the_refusal_list_names_the_documented_syscalls() {
        let names: Vec<&str> = DENIED.iter().map(|(name, _)| *name).collect();
        assert_eq!(DENIED.len(), 35, "the shared refusal list changed size");
        for expected in [
            "ptrace",
            "pidfd_getfd",
            "kcmp",
            "process_vm_readv",
            "process_vm_writev",
            "process_madvise",
            "bpf",
            "keyctl",
            "add_key",
            "request_key",
            "perf_event_open",
            "userfaultfd",
            "io_uring_setup",
            "io_uring_enter",
            "io_uring_register",
            "kexec_load",
            "kexec_file_load",
            "init_module",
            "finit_module",
            "delete_module",
            "mount",
            "umount2",
            "pivot_root",
            "open_tree",
            "move_mount",
            "fsopen",
            "fsconfig",
            "fsmount",
            "fspick",
            "swapon",
            "swapoff",
            "reboot",
            "setns",
            "unshare",
            "open_by_handle_at",
        ] {
            assert!(names.contains(&expected), "{expected} is not refused");
        }
        // No duplicates: two entries for one number would only make the chain
        // longer.
        let mut numbers: Vec<libc::c_long> = DENIED.iter().map(|(_, number)| *number).collect();
        numbers.sort_unstable();
        let unique = numbers.len();
        numbers.dedup();
        assert_eq!(numbers.len(), unique, "the refusal list repeats a syscall");
    }

    #[test]
    fn every_role_fits_the_program_buffer_and_resolves() {
        for role in ROLES {
            let mut program = Program::new();
            build_for(*role, AUDIT_ARCH, &mut program).unwrap();
            assert!(
                program.len <= MAX_PROGRAM,
                "{role:?} needs {} instructions",
                program.len
            );
            let (filter, len) = program.resolve().unwrap();
            assert_eq!(usize::from(len), program.len);
            // The tail is the refusal, then the allow, then the kill: a program
            // that lost an action would either refuse everything or lose the
            // architecture check.
            assert_eq!(
                filter[program.len - 1].k,
                libc::SECCOMP_RET_KILL_PROCESS,
                "{role:?}"
            );
            assert_eq!(
                filter[program.len - 2].k,
                libc::SECCOMP_RET_ERRNO | libc::EPERM as u32,
                "{role:?}"
            );
            assert_eq!(
                filter[program.len - 3].k,
                libc::SECCOMP_RET_ALLOW,
                "{role:?}"
            );
            // The architecture check comes first, and the kill is right behind
            // the branch that fails it.
            assert_eq!(filter[0].k, data::ARCH);
            assert_eq!(filter[2].k, libc::SECCOMP_RET_KILL_PROCESS);
        }
    }

    /// The widest `View` filter, pinned exactly, in instructions.
    ///
    /// The count differs by architecture because x86_64 carries the x32
    /// check; the test below is what keeps this number and `MAX_PROGRAM`'s
    /// doc honest.
    #[cfg(target_arch = "x86_64")]
    const WIDEST_VIEW: usize = 61;
    #[cfg(target_arch = "aarch64")]
    const WIDEST_VIEW: usize = 60;

    /// The widest `EmbeddedView` filter, pinned exactly, in instructions.
    #[cfg(target_arch = "x86_64")]
    const WIDEST_EMBEDDED: usize = 66;
    #[cfg(target_arch = "aarch64")]
    const WIDEST_EMBEDDED: usize = 63;

    /// The length of `build(role)`.
    fn program_len(role: SeccompRole) -> usize {
        let mut program = Program::new();
        build_for(role, AUDIT_ARCH, &mut program).unwrap();
        program.len
    }

    #[test]
    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    fn the_widest_program_is_the_size_the_comment_claims() {
        // `MAX_PROGRAM` and its doc name a number, and a program that grew past
        // that buffer would otherwise only fail inside a forked child. This is
        // the one place that keeps the two honest, and it pins each role's
        // widest configuration so a new rule cannot arrive unmeasured.
        let widest_view = [false, true]
            .into_iter()
            .map(|egress| program_len(SeccompRole::View { egress }))
            .max()
            .unwrap();
        let widest_embedded = [false, true]
            .into_iter()
            .map(|egress| program_len(SeccompRole::EmbeddedView { egress }))
            .max()
            .unwrap();
        assert_eq!(
            widest_view, WIDEST_VIEW,
            "the widest View filter changed size"
        );
        assert_eq!(
            widest_embedded, WIDEST_EMBEDDED,
            "the widest EmbeddedView filter changed size"
        );
        assert!(widest_view <= MAX_PROGRAM);
        assert!(widest_embedded <= MAX_PROGRAM);
    }

    /// The 35 shared refusals as the kernel numbers them, **sorted**, spelled
    /// out here rather than read from `libc`.
    ///
    /// This is what makes the zero-widening oracle independent of the table it
    /// checks: editing a `libc::SYS_*` number in `DENIED` changes the built
    /// filter but not this list, so the test below fails instead of agreeing
    /// with the edit. The numbers are the two architectures this module has an
    /// `AUDIT_ARCH_*` for.
    #[cfg(target_arch = "x86_64")]
    const SHARED_DENIED_NUMBERS: [u32; 35] = [
        101, 155, 165, 166, 167, 168, 169, 175, 176, 246, 248, 249, 250, 272, 298, 304, 308, 310,
        311, 312, 313, 320, 321, 323, 425, 426, 427, 428, 429, 430, 431, 432, 433, 438, 440,
    ];
    #[cfg(target_arch = "aarch64")]
    const SHARED_DENIED_NUMBERS: [u32; 35] = [
        39, 40, 41, 97, 104, 105, 106, 117, 142, 217, 218, 219, 224, 225, 241, 265, 268, 270, 271,
        272, 273, 280, 282, 294, 425, 426, 427, 428, 429, 430, 431, 432, 433, 438, 440,
    ];

    #[test]
    #[cfg(any(target_arch = "x86_64", target_arch = "aarch64"))]
    fn the_shared_refusal_table_pins_the_kernel_numbers() {
        // A copy is sorted for the comparison: the table itself is grouped by
        // theme, not by number.
        let mut declared: Vec<u32> = DENIED.iter().map(|(_, number)| *number as u32).collect();
        declared.sort_unstable();
        assert_eq!(
            declared, SHARED_DENIED_NUMBERS,
            "a shared refusal changed number or disappeared"
        );

        // The installed programs must test exactly those numbers, which is the
        // step that connects this literal list to the filter the kernel runs.
        for role in [
            SeccompRole::View { egress: false },
            SeccompRole::EmbeddedView { egress: false },
        ] {
            let compared: std::collections::BTreeSet<u32> =
                compared_syscalls(role).into_iter().collect();
            for number in SHARED_DENIED_NUMBERS {
                assert!(
                    compared.contains(&number),
                    "{role:?} does not test {number}"
                );
            }
        }
    }

    #[test]
    fn the_view_table_is_unchanged_and_embedded_only_adds_its_documented_calls() {
        use std::collections::BTreeSet;

        let shared: BTreeSet<u32> = DENIED
            .iter()
            .map(|(_, number)| *number as u32)
            .chain([
                libc::SYS_socket as u32,
                libc::SYS_socketpair as u32,
                libc::SYS_clone3 as u32,
                libc::SYS_clone as u32,
                libc::SYS_setsid as u32,
                libc::SYS_setpgid as u32,
            ])
            .collect();

        // The failure this test exists for: a PHP-only call smuggled into the
        // role that also runs WSGI, Node and the JVM.
        let view: BTreeSet<u32> = compared_syscalls(SeccompRole::View { egress: false })
            .into_iter()
            .collect();
        assert_eq!(view, shared, "the View refusal table changed");
        for (name, number) in EMBEDDED_DENIED {
            assert!(
                !view.contains(&(*number as u32)),
                "View must not refuse {name}"
            );
        }

        let mut expected = shared.clone();
        expected.extend(EMBEDDED_DENIED.iter().map(|(_, number)| *number as u32));
        let embedded = compared_syscalls(SeccompRole::EmbeddedView { egress: false })
            .into_iter()
            .collect::<BTreeSet<u32>>();
        assert_eq!(
            embedded, expected,
            "EmbeddedView must add exactly its documented calls"
        );

        // Egress changes the socket handler, never the syscall list.
        assert_eq!(
            compared_syscalls(SeccompRole::View { egress: true }),
            compared_syscalls(SeccompRole::View { egress: false })
        );
        assert_eq!(
            compared_syscalls(SeccompRole::EmbeddedView { egress: true }),
            compared_syscalls(SeccompRole::EmbeddedView { egress: false })
        );
    }

    #[test]
    fn the_clone_handler_carries_the_namespace_mask_and_embedded_the_thread_bit() {
        let bitsets = |role: SeccompRole| {
            let mut program = Program::new();
            build_for(role, AUDIT_ARCH, &mut program).unwrap();
            let masks: Vec<u32> = program.steps[..program.len]
                .iter()
                .filter(|step| step.code == JSET)
                .map(|step| step.k)
                .collect();
            let actions: Vec<u32> = program.steps[..program.len]
                .iter()
                .filter(|step| step.code == RET)
                .map(|step| step.k)
                .collect();
            (masks, actions)
        };

        for role in [
            SeccompRole::View { egress: false },
            SeccompRole::EmbeddedView { egress: false },
        ] {
            let (masks, actions) = bitsets(role);
            assert!(
                masks.contains(&CLONE_NEW_MASK),
                "{role:?} does not test the clone namespace mask"
            );
            assert!(
                actions.contains(&RET_ENOSYS),
                "{role:?} has no ENOSYS action for clone3"
            );
            assert_eq!(
                masks.contains(&CLONE_THREAD),
                matches!(role, SeccompRole::EmbeddedView { .. }),
                "{role:?}: only EmbeddedView tests CLONE_THREAD"
            );
        }
        // The mask is exactly the eight namespace flags the design names.
        let union = CLONE_NEW_FLAGS
            .iter()
            .fold(0_u32, |acc, flag| acc | *flag as u32);
        assert_eq!(union, CLONE_NEW_MASK);
    }

    #[test]
    fn a_foreign_architecture_kills_the_process() {
        // `build_for` with an architecture nothing produces: the filter falls
        // through to the kill on its very first pair of instructions, so the
        // child dies on its first syscall. This is the branch that makes an
        // x32 syscall number fatal too, and this is the only way to watch a
        // `SECCOMP_RET_KILL_PROCESS` happen without an x32 userland.
        let status = filtered_child(prepared_for(SeccompRole::Http, 0xc000_ffff), || {
            // SAFETY: `getpid` takes no arguments and cannot fail. The child
            // dies here, so nothing after this line runs.
            unsafe { libc::getpid() };
            2
        });
        assert_eq!(status.raw_signal(), Some(libc::SIGSYS), "{status}");
    }

    #[test]
    fn an_x32_syscall_number_kills_the_process() {
        // x32 shares x86_64's syscall instruction but not its table, so a
        // number with that bit set is not one of the numbers the refusal list
        // is written in. Refusing it with `EPERM` would mean answering a
        // syscall blastit cannot name, so the process dies, exactly as it does
        // on a foreign architecture (§7.3).
        #[cfg(target_arch = "x86_64")]
        {
            let status = filtered_child(prepared(SeccompRole::View { egress: false }), || {
                // SAFETY: `syscall` takes a number and up to six word
                // arguments; the x32 `getpid` takes none. The child dies here,
                // so nothing after this line runs.
                unsafe { libc::syscall((X32_SYSCALL_BIT | 39) as libc::c_long) };
                2
            });
            assert_eq!(status.raw_signal(), Some(libc::SIGSYS), "{status}");
        }
    }

    #[test]
    fn an_ordinary_syscall_still_works() {
        for role in ROLES {
            in_filtered_child(&format!("{role:?} getpid"), prepared(*role), || {
                // SAFETY: `getpid` takes no arguments and cannot fail. If the
                // filter killed or refused it, the child would not get here.
                if unsafe { libc::getpid() } > 0 { 0 } else { 1 }
            });
        }
    }

    #[test]
    fn install_refuses_a_process_that_is_not_single_threaded() {
        // The check is what turns a wrong fork order into a failed boot instead
        // of a half-sandboxed process, so it has to fire before the kernel sees
        // anything. It needs a process with a second thread, and a child of
        // `fork()` cannot make one — thread creation allocates — so this body
        // runs in a fresh process of its own (`run_probe`).
        assert_eq!(
            run_probe(PROBE_RUNNER, "thread-refusal"),
            0,
            "install did not refuse a multi-threaded process"
        );
    }

    #[test]
    fn clone3_is_enosys_and_a_thread_still_starts_on_both_views() {
        // `ENOSYS` is what makes glibc's `pthread_create` fall back to
        // `clone`, so the two facts belong in one body: the refusal, then a
        // thread that really starts. `std::thread::spawn` allocates, so each
        // role gets a fresh process of its own (`run_probe`).
        for (label, name) in [
            ("view clone3", "thread-after-filter-view"),
            ("embedded view clone3", "thread-after-filter-embedded"),
        ] {
            assert_eq!(
                run_probe(PROBE_RUNNER, name),
                0,
                "{label}: the thread did not start after the filter"
            );
        }
    }

    #[test]
    fn every_new_namespace_clone_flag_is_refused_by_both_views() {
        for role in [
            SeccompRole::View { egress: false },
            SeccompRole::EmbeddedView { egress: false },
        ] {
            in_filtered_child(&format!("{role:?} namespaces"), prepared(role), || {
                for flag in CLONE_NEW_FLAGS {
                    if raw_clone(flag) != libc::EPERM {
                        return 1;
                    }
                }
                0
            });
        }
    }

    #[test]
    fn setsid_and_setpgid_are_refused_by_both_views() {
        for role in [
            SeccompRole::View { egress: false },
            SeccompRole::EmbeddedView { egress: false },
        ] {
            in_filtered_child(&format!("{role:?} session"), prepared(role), || {
                if raw_setsid() != libc::EPERM {
                    return 1;
                }
                if raw_setpgid() != libc::EPERM {
                    return 2;
                }
                // The accepted twin: reading the ids is still allowed.
                // SAFETY: `getpgrp` takes no arguments and cannot fail.
                if unsafe { libc::getpgrp() } < 0 {
                    return 3;
                }
                0
            });
        }
    }

    #[test]
    fn embedded_view_refuses_process_creation_and_exec_while_view_keeps_them() {
        in_filtered_child(
            "embedded view exec",
            prepared(SeccompRole::EmbeddedView { egress: false }),
            || {
                if raw_execve() != libc::EPERM {
                    return 1;
                }
                if raw_execveat() != libc::EPERM {
                    return 2;
                }
                // A `clone` without `CLONE_THREAD` is a new process.
                if raw_clone(libc::SIGCHLD as libc::c_ulong) != libc::EPERM {
                    return 3;
                }
                #[cfg(target_arch = "x86_64")]
                {
                    if raw_fork() != libc::EPERM {
                        return 4;
                    }
                    if raw_vfork() != libc::EPERM {
                        return 5;
                    }
                }
                0
            },
        );

        in_filtered_child(
            "view exec",
            prepared(SeccompRole::View { egress: false }),
            || {
                // The same two calls are permitted: a missing file is `ENOENT`,
                // which only an image lookup can produce.
                if raw_execve() != libc::ENOENT {
                    return 1;
                }
                if raw_execveat() != libc::ENOENT {
                    return 2;
                }
                // And a process-creating `clone` still works.
                if raw_clone(libc::SIGCHLD as libc::c_ulong) != 0 {
                    return 3;
                }
                #[cfg(target_arch = "x86_64")]
                if raw_fork() != 0 {
                    return 4;
                }
                0
            },
        );
    }

    #[test]
    fn embedded_view_egress_reflects_the_flag() {
        in_filtered_child(
            "embedded view no egress",
            prepared(SeccompRole::EmbeddedView { egress: false }),
            || {
                if raw_socket(libc::AF_UNIX) != 0 {
                    return 1;
                }
                if raw_socket(libc::AF_INET) != libc::EPERM {
                    return 2;
                }
                if raw_socket(libc::AF_INET6) != libc::EPERM {
                    return 3;
                }
                if raw_socketpair(libc::AF_UNIX) != 0 {
                    return 4;
                }
                0
            },
        );

        in_filtered_child(
            "embedded view egress",
            prepared(SeccompRole::EmbeddedView { egress: true }),
            || {
                if raw_socket(libc::AF_INET) != 0 {
                    return 1;
                }
                if raw_socket(libc::AF_INET6) != 0 {
                    return 2;
                }
                // Egress is a permission, not a requirement.
                if raw_socket(libc::AF_NETLINK) != libc::EPERM {
                    return 3;
                }
                0
            },
        );
    }

    #[test]
    fn the_roles_describe_themselves() {
        assert_eq!(SeccompRole::View { egress: true }.name(), "view");
        assert_eq!(
            SeccompRole::EmbeddedView { egress: true }.name(),
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
        assert!(!SeccompRole::Http.sockets());
        assert!(!SeccompRole::Scheduler.sockets());
        assert!(SeccompRole::View { egress: false }.sockets());
        assert!(SeccompRole::EmbeddedView { egress: false }.sockets());
        assert!(SeccompRole::DbProxy.sockets());
    }
}
