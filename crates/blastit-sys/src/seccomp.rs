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
//!   this filter was written for, and on x86_64 an x32 syscall number (`nr &
//!   __X32_SYSCALL_BIT`) is refused too: a syscall number from another ABI
//!   means a number-to-syscall mapping unlike the one the filter's list is
//!   written in, so the only safe answer is to kill the process. Nothing
//!   legitimate reaches either branch.
//! * **The filter is a refusal list of numbers, not of names.** `socket` and
//!   `socketpair` are the only syscalls whose arguments matter: `args[0]` is
//!   the address family, and the family is what allows egress (§7.3).
//!
//! The filter cannot be removed once installed, and it applies to every thread
//! that exists when it is installed as well as every thread created later, so
//! it has to go on while the process is still single-threaded and before the
//! runtime or any request thread exists (§4).

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
            Self::View { egress } => egress,
            Self::DbProxy => true,
            Self::Http | Self::Scheduler => false,
        }
    }

    /// Whether this role may create sockets at all.
    pub const fn sockets(self) -> bool {
        !matches!(self, Self::Http | Self::Scheduler)
    }
}

/// Installs the filter for `role` on the calling process (ARCHITECTURE.md §7.3).
///
/// What every role refuses: `ptrace`, `process_vm_readv`, `process_vm_writev`,
/// `bpf`, `keyctl`, `add_key`, `request_key`, `perf_event_open`, `userfaultfd`,
/// the `io_uring` calls, `kexec_load`, `kexec_file_load`, `init_module`,
/// `finit_module`, `delete_module`, `mount`, `umount2`, `pivot_root`, `swapon`,
/// `swapoff`, `reboot`, `setns`, `unshare` and `open_by_handle_at`. They are
/// the calls with which one process reads another's memory, loads code into the
/// kernel, or moves between namespaces, mounts and namespaces; none of them is
/// part of serving a request.
///
/// What each role additionally refuses:
///
/// | Role | `socket()` | `socketpair()` |
/// |---|---|---|
/// | view, no egress | every family but `AF_UNIX` | `AF_UNIX` only |
/// | view, egress | every family but `AF_UNIX`, `AF_INET`, `AF_INET6` | `AF_UNIX` only |
/// | http, scheduler | every `socket()` | `AF_UNIX` only |
/// | dbproxy | every family but `AF_UNIX`, `AF_INET`, `AF_INET6` | `AF_UNIX` only |
///
/// `socketpair()` stays `AF_UNIX`-only everywhere: a pair of descriptors a
/// process already owns is not a new way out of it, and std and tokio use one
/// internally on some platforms.
///
/// # Errors
///
/// [`std::io::Error`] when `PR_SET_NO_NEW_PRIVS` or the filter installation
/// fails, or on a platform without an `AUDIT_ARCH_*` value. The filter itself
/// can no longer fail once the kernel has accepted it.
pub fn install(role: SeccompRole) -> io::Result<()> {
    if AUDIT_ARCH == 0 {
        return Err(Error::UnsupportedArchitecture {
            arch: std::env::consts::ARCH,
        }
        .into());
    }

    // §7.3: this, and nothing else, is what allows an unprivileged process to
    // install a filter. It also means no child of this one may gain privileges.
    // SAFETY: `PR_SET_NO_NEW_PRIVS` takes one integer, 1 to enable it, and
    // ignores the remaining arguments.
    if unsafe { libc::prctl(libc::PR_SET_NO_NEW_PRIVS, 1, 0, 0, 0) } == -1 {
        return Err(step("PR_SET_NO_NEW_PRIVS", io::Error::last_os_error()));
    }

    let mut program = Program::new();
    build(role, &mut program)?;
    let (mut bytes, len) = program.resolve()?;
    let mut descriptor = libc::sock_fprog {
        len,
        filter: bytes.as_mut_ptr(),
    };

    // SAFETY: `descriptor` describes a program of `len` instructions that lives
    // on this stack frame and outlives both calls; the kernel copies the
    // program before returning.
    unsafe {
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
    Err(step("installing the filter", io::Error::last_os_error()))
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

/// The syscalls every role refuses, with the name of each for documentation.
///
/// The names are not part of the filter — a filter compares numbers — but they
/// are what makes this list readable and testable.
const DENIED: &[(&str, libc::c_long)] = &[
    ("ptrace", libc::SYS_ptrace),
    ("process_vm_readv", libc::SYS_process_vm_readv),
    ("process_vm_writev", libc::SYS_process_vm_writev),
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
    ("kexec_file_load", libc::SYS_kexec_file_load),
    ("init_module", libc::SYS_init_module),
    ("finit_module", libc::SYS_finit_module),
    ("delete_module", libc::SYS_delete_module),
    ("mount", libc::SYS_mount),
    ("umount2", libc::SYS_umount2),
    ("pivot_root", libc::SYS_pivot_root),
    ("swapon", libc::SYS_swapon),
    ("swapoff", libc::SYS_swapoff),
    ("reboot", libc::SYS_reboot),
    ("setns", libc::SYS_setns),
    ("unshare", libc::SYS_unshare),
    ("open_by_handle_at", libc::SYS_open_by_handle_at),
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
    /// `args[0]`, the address family of `socket`/`socketpair`.
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

/// The longest program [`program`] builds, in instructions.
///
/// `install` runs in a freshly forked child, where allocating is not safe, so
/// the program lives on the stack; the current worst case is 42 instructions
/// (`View { egress: true }`) and there is room for the list to grow.
const MAX_PROGRAM: usize = 64;

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
    /// the longest one here is about 30 — and can only go forward.
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

/// Builds the filter for `role`, in the order the kernel walks it.
///
/// The shape is: check the architecture, then the syscall number against the
/// refusal list, then the address family for the two socket calls, and finally
/// the two shared actions. Every branch is forward, which is all classic BPF
/// has.
fn build(role: SeccompRole, program: &mut Program) -> io::Result<()> {
    // A filter written for one architecture's syscall table means nothing on
    // another, so a foreign `arch` kills the process (§7.3). Nothing
    // legitimate gets here: blastit's children are all this same image.
    program.push(load(data::ARCH))?;
    program.push(compare(
        JEQ,
        AUDIT_ARCH,
        Target::Label(Label::AfterArch),
        Target::Next,
    ))?;
    program.push(action(libc::SECCOMP_RET_KILL_PROCESS))?;

    // From here on A holds the syscall number.
    program.push(labelled(Label::AfterArch, load(data::NR)))?;
    #[cfg(target_arch = "x86_64")]
    program.push(compare(
        JSET,
        X32_SYSCALL_BIT,
        Target::Label(Label::Deny),
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

    program.push(labelled(Label::Allow, action(libc::SECCOMP_RET_ALLOW)))?;
    program.push(labelled(
        Label::Deny,
        // The refused syscall fails with `EPERM`, which every caller in the
        // process reports as "not permitted" instead of dying.
        action(libc::SECCOMP_RET_ERRNO | libc::EPERM as u32),
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
    use crate::testutil::child_ok;

    /// Every role, so the tests cannot forget one.
    const ROLES: &[SeccompRole] = &[
        SeccompRole::View { egress: false },
        SeccompRole::View { egress: true },
        SeccompRole::Http,
        SeccompRole::Scheduler,
        SeccompRole::DbProxy,
    ];

    /// `socket(domain, SOCK_STREAM, 0)`, closing what it created.
    ///
    /// Returns the `errno` of a refusal, or `None` when the call succeeded.
    fn try_socket(domain: libc::c_int) -> Option<i32> {
        // SAFETY: `socket` takes three integers and returns a descriptor.
        let fd = unsafe { libc::socket(domain, libc::SOCK_STREAM, 0) };
        if fd == -1 {
            return io::Error::last_os_error().raw_os_error();
        }
        // SAFETY: the descriptor was just created here and nothing else owns it.
        unsafe { libc::close(fd) };
        None
    }

    /// `socketpair(AF_UNIX, SOCK_STREAM, 0, sv)`, closing what it created.
    fn try_socketpair_unix() -> Option<i32> {
        let mut fds = [0 as libc::c_int; 2];
        // SAFETY: `socketpair` writes two descriptors into the two-element
        // array, which is exactly the storage it needs.
        let created =
            unsafe { libc::socketpair(libc::AF_UNIX, libc::SOCK_STREAM, 0, fds.as_mut_ptr()) };
        if created == -1 {
            return io::Error::last_os_error().raw_os_error();
        }
        // SAFETY: both descriptors were just created here.
        unsafe {
            libc::close(fds[0]);
            libc::close(fds[1]);
        }
        None
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
            -1 => io::Error::last_os_error().raw_os_error(),
            _ => None,
        }
    }

    #[test]
    fn a_view_without_egress_keeps_unix_and_loses_inet() {
        child_ok("view socket", || {
            if install(SeccompRole::View { egress: false }).is_err() {
                return 1;
            }
            if try_socket(libc::AF_UNIX).is_some() {
                return 2;
            }
            match try_socket(libc::AF_INET) {
                Some(libc::EPERM) => {}
                _ => return 3,
            }
            match try_socket(libc::AF_INET6) {
                Some(libc::EPERM) => {}
                _ => return 4,
            }
            match try_socket(libc::AF_NETLINK) {
                Some(libc::EPERM) => {}
                _ => return 5,
            }
            // A pair of descriptors this process owns stays available.
            match try_socketpair_unix() {
                None => 0,
                _ => 6,
            }
        });
    }

    #[test]
    fn a_view_with_egress_keeps_inet_and_inet6() {
        child_ok("egress view socket", || {
            if install(SeccompRole::View { egress: true }).is_err() {
                return 1;
            }
            if try_socket(libc::AF_INET).is_some() {
                return 2;
            }
            if try_socket(libc::AF_INET6).is_some() {
                return 3;
            }
            if try_socket(libc::AF_UNIX).is_some() {
                return 4;
            }
            // Egress is a permission, not a requirement: everything else is
            // still refused.
            match try_socket(libc::AF_NETLINK) {
                Some(libc::EPERM) => 0,
                _ => 5,
            }
        });
    }

    #[test]
    fn http_and_the_scheduler_cannot_open_a_socket_at_all() {
        for (label, role) in [
            ("http socket", SeccompRole::Http),
            ("scheduler socket", SeccompRole::Scheduler),
        ] {
            child_ok(label, move || {
                if install(role).is_err() {
                    return 1;
                }
                for domain in [libc::AF_UNIX, libc::AF_INET, libc::AF_INET6] {
                    match try_socket(domain) {
                        Some(libc::EPERM) => {}
                        _ => return 2,
                    }
                }
                match try_socketpair_unix() {
                    None => 0,
                    _ => 3,
                }
            });
        }
    }

    #[test]
    fn dbproxy_keeps_unix_and_tcp() {
        child_ok("dbproxy socket", || {
            if install(SeccompRole::DbProxy).is_err() {
                return 1;
            }
            for domain in [libc::AF_UNIX, libc::AF_INET, libc::AF_INET6] {
                if try_socket(domain).is_some() {
                    return 2;
                }
            }
            match try_socket(libc::AF_NETLINK) {
                Some(libc::EPERM) => 0,
                _ => 3,
            }
        });
    }

    #[test]
    fn every_role_refuses_ptrace() {
        for (label, role) in [
            ("view ptrace", SeccompRole::View { egress: false }),
            ("egress view ptrace", SeccompRole::View { egress: true }),
            ("http ptrace", SeccompRole::Http),
            ("dbproxy ptrace", SeccompRole::DbProxy),
        ] {
            child_ok(label, move || {
                if install(role).is_err() {
                    return 1;
                }
                match try_ptrace() {
                    Some(libc::EPERM) => 0,
                    _ => 2,
                }
            });
        }
    }

    #[test]
    fn the_refusal_list_names_the_documented_syscalls() {
        let names: Vec<&str> = DENIED.iter().map(|(name, _)| *name).collect();
        for expected in [
            "ptrace",
            "process_vm_readv",
            "process_vm_writev",
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
            "swapon",
            "swapoff",
            "reboot",
            "setns",
            "unshare",
            "open_by_handle_at",
        ] {
            assert!(names.contains(&expected), "{expected} is not refused");
        }
    }

    #[test]
    fn every_role_fits_the_program_buffer_and_resolves() {
        for role in ROLES {
            let mut program = Program::new();
            build(*role, &mut program).unwrap();
            assert!(
                program.len <= MAX_PROGRAM,
                "{role:?} needs {} instructions",
                program.len
            );
            let (filter, len) = program.resolve().unwrap();
            assert_eq!(usize::from(len), program.len);
            // The last instruction is the refusal, and the one before it the
            // allow: a program that lost its actions would refuse everything.
            assert_eq!(
                filter[program.len - 1].k,
                libc::SECCOMP_RET_ERRNO | libc::EPERM as u32
            );
            assert_eq!(filter[program.len - 2].k, libc::SECCOMP_RET_ALLOW);
            // The architecture check comes first, and the kill is right behind
            // the branch that fails it.
            assert_eq!(filter[0].k, data::ARCH);
            assert_eq!(filter[2].k, libc::SECCOMP_RET_KILL_PROCESS);
        }
    }

    #[test]
    fn an_x32_syscall_number_is_refused() {
        // The x32 table shares x86_64's syscall instruction but not its
        // numbers, so a number with that bit set is refused rather than
        // interpreted (§7.3). `getpid` is the number that means something else
        // entirely in the x32 table.
        #[cfg(target_arch = "x86_64")]
        child_ok("x32", || {
            if install(SeccompRole::View { egress: false }).is_err() {
                return 1;
            }
            // SAFETY: `syscall` takes a number and up to six word arguments;
            // `getpid` takes none.
            let called = unsafe { libc::syscall((X32_SYSCALL_BIT | 39) as libc::c_long) };
            if called != -1 {
                return 2;
            }
            match io::Error::last_os_error().raw_os_error() {
                Some(libc::EPERM) => 0,
                _ => 3,
            }
        });
    }

    #[test]
    fn an_ordinary_syscall_still_works() {
        for role in ROLES {
            child_ok("ordinary", move || {
                if install(*role).is_err() {
                    return 1;
                }
                // SAFETY: `getpid` takes no arguments and cannot fail. If the
                // filter killed or refused it, the child would not get here.
                match unsafe { libc::getpid() } {
                    pid if pid > 0 => 0,
                    _ => 2,
                }
            });
        }
    }

    #[test]
    fn the_roles_describe_themselves() {
        assert_eq!(SeccompRole::View { egress: true }.name(), "view");
        assert_eq!(SeccompRole::Http.name(), "http");
        assert_eq!(SeccompRole::Scheduler.name(), "scheduler");
        assert_eq!(SeccompRole::DbProxy.name(), "dbproxy");
        assert!(!SeccompRole::View { egress: false }.egress());
        assert!(SeccompRole::View { egress: true }.egress());
        assert!(SeccompRole::DbProxy.egress());
        assert!(!SeccompRole::Http.egress());
        assert!(!SeccompRole::Scheduler.egress());
        assert!(!SeccompRole::Http.sockets());
        assert!(!SeccompRole::Scheduler.sockets());
        assert!(SeccompRole::View { egress: false }.sockets());
        assert!(SeccompRole::DbProxy.sockets());
    }
}
