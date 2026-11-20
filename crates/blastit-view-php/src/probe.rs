//! The contained probe child: one process per file that proves the pair loads
//! (`ADR-0010` §3, design §4.2.3).
//!
//! A boot's `entrypoint php` is proved **before** anything listens: the child
//! loads the exact descriptors the workers will load — the pair stage 0
//! inspected, re-opened by [`boot_bindings`](crate::pinning::boot_bindings) —
//! runs `blastit_php_worker_init` and `blastit_php_worker_shutdown`, and reports
//! the `php_version()` it loaded plus the `php.ini` PHP actually opened. A pair
//! that cannot start is the boot's refusal (`exit 1`, §4.1's sentence) instead
//! of a worker's first request.
//!
//! The child is contained exactly as its worker will be, plus the process group
//! the timeout needs: [`blastit_sys::child::enter`] runs its `setpgid(0, 0)`
//! **before** the `EmbeddedView` filter, which refuses `setpgid` afterwards, so
//! the timeout's `kill(-pgid, SIGKILL)` always names a group that exists.
//!
//! The load stays below the fork: nothing in stage 0 or stage 1 proper reaches a
//! `libphp` symbol.

use std::os::fd::{AsFd, AsRawFd, OwnedFd};
use std::time::{Duration, Instant};

use blastit_php_sapi::SharedObject;
use blastit_sys::child::{child_exit, wait_readable, ChildIo, ChildPlan};
use blastit_sys::{ExitStatus, Fork, Pid, Rlimits, Signal};
use blastit_view::spawn::{ForkProtocol, Forker};

use crate::discovery::{php_probe_refusal, PhpRuntime};
use crate::pinning::{PhpBinding, PhpSlot};
use crate::spawn::set_view_environment;

/// Everything a boot's `entrypoint php` probe needs (`ADR-0010` §3).
///
/// `bindings` is what [`boot_bindings`](crate::pinning::boot_bindings) built —
/// the **pinned** descriptors stage 0 inspected — so the probe loads exactly the
/// files the workers will, one step earlier. A slot without a binding is another
/// backend and has nothing to prove.
pub struct PhpProbeBoot<'a> {
    /// One entry per view slot, `None` for a slot no manifest route names.
    pub slots: &'a [Option<PhpSlot>],
    /// One binding per PHP slot, `None` for every other backend.
    pub bindings: &'a [Option<PhpBinding>],
    /// The probe child's `RLIMIT_*`: the same shape stage 1 hands a worker, and
    /// `nproc` only when the child drops privileges.
    pub limits: Rlimits,
    /// Stage 0's record, for the `(PHP X.Y, <arch>-<libc>)` half of a refusal.
    pub libphp: Option<&'a PhpRuntime>,
    /// `BLASTIT_PROBE_TIMEOUT_MS`, parsed by stage 0.
    pub timeout: Duration,
    /// The fork protocol this image needs; the forker refuses a disagreement.
    pub protocol: ForkProtocol,
}

/// What the one probe child proved: the `php_version()` it loaded and the
/// `php.ini` PHP opened (`None` when it opened none).
///
/// The caller is the process that knows how to log it: a `libphp` line is
/// public knowledge, but the event stream belongs to the composition root, so
/// this module returns the two facts and never builds an event.
pub type PhpProbeReport = (String, Option<String>);

/// The most of a probe child's stderr a refusal quotes: PHP's own startup log
/// can be long, and this becomes one log line (≤ 2048 bytes, §1.1).
const PROBE_MESSAGE_MAX: usize = 512;

/// `ADR-0010` §3: one contained probe child per `entrypoint php`, forked
/// **before** `bind_tcp`, that loads the pair stage 0 inspected and runs
/// `blastit_php_worker_init` + `blastit_php_worker_shutdown` in it.
///
/// The load stays below the fork. Nothing in stage 0 or stage 1 proper reaches a
/// `libphp` symbol, and a pair that cannot start is this boot's refusal — `exit
/// 1` with `ADR-0010` §4.1's sentence — instead of a worker's first request.
///
/// The child is contained exactly as its worker will be: its own process group,
/// the route's uid (production), `PR_SET_PDEATHSIG(SIGKILL)`, the route's
/// `RLIMIT_*` and [`blastit_sys::SeccompRole::EmbeddedView`] with the route's `egress`.
///
/// # Errors
///
/// The refusal sentence: the route, `(PHP X.Y, <arch>-<libc>)`, the distro
/// package and `BLASTIT_LIBPHP`, plus what the child said (or that it hung).
pub fn probe_bindings(
    probe: PhpProbeBoot<'_>,
    forker: Forker<'_>,
) -> Result<Option<PhpProbeReport>, String> {
    let mut opened: Option<PhpProbeReport> = None;
    for (slot, binding) in probe.bindings.iter().enumerate() {
        let Some(binding) = binding else {
            continue;
        };
        let Some(values) = probe.slots.get(slot).and_then(Option::as_ref) else {
            return Err(format!(
                "PHP view slot {slot} holds a libphp binding but no route; the manifest and the \
                 scheduler's routing table disagree"
            ));
        };
        let reported = probe_route(&probe, values, binding, forker)?;
        // §12.2 is one `entrypoint php` for the whole file: every slot loads the
        // same library, so the first slot's report is the file's.
        if let Some(reported) = reported {
            opened.get_or_insert(reported);
        }
    }
    Ok(opened)
}

/// The marker a probe child prefixes to the one report it sends back on its own
/// pipe: the `php_version()` it loaded and the `php.ini` PHP opened, as
/// `["8.4.27","/etc/php/8.4/embed/php.ini"]` (`null` when it opened none).
const RUNTIME_REPORT_PREFIX: &str = "blastit-probe runtime=";

/// Forks `info`'s probe child and reads the verdict, plus what the child's
/// `php_module_startup` proved: the version it loaded and the `php.ini` it
/// opened.
fn probe_route(
    probe: &PhpProbeBoot<'_>,
    values: &PhpSlot,
    binding: &PhpBinding,
    forker: Forker<'_>,
) -> Result<Option<PhpProbeReport>, String> {
    let (message, message_write) = blastit_sys::pipe().map_err(|err| {
        format!(
            "route {:?}: the entrypoint php probe child's diagnostic pipe: {err}",
            values.route
        )
    })?;
    // Verdict and ini travel on different pipes: PHP's own startup log can be
    // long, and [`PROBE_MESSAGE_MAX`] deliberately keeps only its head for a
    // refusal — a report at the tail of that buffer would be the first thing
    // lost.
    let (report, report_write) = blastit_sys::pipe().map_err(|err| {
        format!(
            "route {:?}: the entrypoint php probe child's runtime pipe: {err}",
            values.route
        )
    })?;
    let mut keep = binding.pinned_fds();
    keep.push(report_write.as_raw_fd());
    // The child keeps the pinned descriptors and nothing else: no channel, no
    // config pipe, no listener. Its stdout is `/dev/null`; its stderr is the
    // pipe this reads, so the reason a load failed comes back with the verdict.
    let child = ChildIo {
        stdout: None,
        stderr: Some(message_write),
        keep,
        limits: probe.limits,
        working_dir: Some(binding.view_tmp.clone()),
    };
    let fork = forker(probe.protocol).map_err(|err| {
        format!(
            "route {:?}: the entrypoint php probe child could not be forked: {err}",
            values.route
        )
    })?;
    let pid = match fork {
        Fork::Parent(pid) => {
            // The write end is the child's alone: holding a copy here would make
            // the read below wait for a writer that never closes.
            drop(child);
            drop(report_write);
            pid
        }
        Fork::Child => probe_child(values, binding, child, message, report_write),
    };
    // Best effort, and the child asks for the same group a moment later: this
    // closes the window in which the timeout's `kill(-pid)` would name a group
    // that does not exist yet.
    join_group(pid);
    await_probe(pid, &message, probe.timeout).map_or_else(
        |problem| {
            // The manifest's `entry`, as written: the absolute path is what the
            // worker uses, and the operator edits the manifest.
            let declared = binding
                .entry
                .strip_prefix(&binding.document_root)
                .unwrap_or(&binding.entry);
            Err(php_probe_refusal(
                &values.route,
                &declared.display().to_string(),
                &problem,
                probe.libphp.map(|record| record.version),
            ))
        },
        |()| {
            // The child is gone, so it holds no writer: this reads to end of
            // file instead of blocking on a live process.
            let mut text = Vec::new();
            drain_to_end(&report, &mut text);
            // A missing or malformed report costs the log line, never the boot:
            // the probe itself succeeded, and an invented version would be worse
            // than saying nothing.
            Ok(parse_report(&text))
        },
    )
}

/// What a probe child reported, or `None`.
///
/// The child sends one line, `blastit-probe runtime=["8.4.27", <ini or null>]`;
/// anything else — no line, a malformed one, a value that is not the documented
/// shape — is "this build cannot say", never a guessed version or path. The JSON
/// keeps a path with a newline from splitting the line.
fn parse_report(text: &[u8]) -> Option<PhpProbeReport> {
    let text = String::from_utf8_lossy(text);
    let line = text
        .lines()
        .find(|line| line.starts_with(RUNTIME_REPORT_PREFIX))?;
    let value = line.get(RUNTIME_REPORT_PREFIX.len()..)?;
    serde_json::from_str::<(String, Option<String>)>(value).ok()
}

/// Everything the probe child does after the fork. Never returns.
fn probe_child(
    values: &PhpSlot,
    binding: &PhpBinding,
    child: ChildIo,
    message: OwnedFd,
    report: OwnedFd,
) -> ! {
    // `ChildIo::prepare` closes every descriptor that is not in `keep`, this one
    // included; the `OwnedFd` must not drop afterwards or it would close a
    // number the child may since have reused.
    std::mem::forget(message);
    // The parent composes the sentence (it knows the route and the manifest's
    // `entry`), so the child says only what went wrong.
    let fail = |what: &str| -> ! { child_exit(what, 1) };
    // One sequence, spelled once: `setpgid(0, 0)` runs inside `enter`, before
    // the `EmbeddedView` filter that refuses it (§4.4 D1, §7.3).
    let plan = ChildPlan {
        name: "entrypoint php probe".to_string(),
        io: child,
        privileges: values.privileges,
        filter: values.filter,
        // The one child that becomes a group leader: the timeout kills `-pgid`
        // so a hung `worker_init` cannot outlive the boot that refused it.
        pgroup: true,
        // The probe keeps the inherited mask: it must not be interrupted by
        // anything the boot set up, and it has no signal contract of its own.
        mask: None,
    };
    if let Err(message) = blastit_sys::child::enter(plan) {
        fail(&message);
    }
    if let Err(err) = set_view_environment(&binding.view_tmp) {
        fail(&format!("HOME/TMPDIR: {err}"));
    }
    match load_and_shut_down(binding) {
        Ok((version, ini)) => {
            // The one report this child sends back, after the module started and
            // before it exits: the version it loaded and the `php.ini` PHP
            // opened, or `null`. The parent reads it on its own pipe, so PHP's
            // startup log can never push it out of the verdict.
            if let Ok(line) = serde_json::to_string(&(version, ini)) {
                let text = format!("{RUNTIME_REPORT_PREFIX}{line}\n");
                // `report` was in the child's `keep` list, so `prepare` left it
                // open; nothing else in this child owns the number, and this
                // process exits before it could be dropped.
                let _ = rustix::io::write(&report, text.as_bytes());
            }
            child_exit("", 0)
        }
        Err(message) => fail(&message),
    }
}

/// Loads the pinned pair, starts the module, and shuts it down again: the whole
/// of what `ADR-0010` §3 asks a probe to prove.
///
/// [`blastit_php_sapi::Loaded::load`] is the ABI check
/// (`blastit_php_abi_version() == 1`) and the minor check
/// (`blastit_php_minor()` against the host's `php_version()`), so a pair that
/// disagrees never reaches `worker_init`; `worker_init` is what proves the
/// module and the INI lockdown actually start on this host.
///
/// The `php.ini` is read between `worker_init` and `worker_shutdown`, which is
/// the only window in which PHP's own global is meaningful: it is the file
/// `php_init_config()` opened, and `None` when it opened none. The
/// version is the loaded library's own `php_version()`, the full `"8.4.27"` the
/// line reports.
fn load_and_shut_down(binding: &PhpBinding) -> Result<(String, Option<String>), String> {
    let mut loaded = blastit_php_sapi::Loaded::load(
        SharedObject::Fd(binding.libphp.as_raw_fd()),
        SharedObject::Fd(binding.shim.as_raw_fd()),
    )
    .map_err(|err| err.to_string())?;
    loaded
        .worker_init(&binding.ini, &binding.view_tmp)
        .map_err(|err| err.to_string())?;
    let version = loaded.php_version().to_owned();
    let ini = loaded.ini_opened_path();
    loaded.worker_shutdown();
    Ok((version, ini))
}

/// The parent's half of the child's own `setpgid(0, 0)`, for the fork's race.
///
/// `EACCES`/`ESRCH` mean the child already did it or has exited, which is the
/// answer this asked for, so the error is discarded on purpose.
fn join_group(pid: Pid) {
    if let Some(raw) = rustix_pid(pid) {
        let _ = rustix::process::setpgid(Some(raw), Some(raw));
    }
}

/// `pid` as the spelling the `rustix` process wrappers take.
///
/// [`blastit_sys::Pid`] refuses a value the kernel never reports for a live
/// process, so a conversion can only fail for a number this boot did not get
/// from a fork — in which case the caller does nothing rather than name the
/// wrong process.
fn rustix_pid(pid: Pid) -> Option<rustix::process::Pid> {
    rustix::process::Pid::from_raw(pid.as_raw())
}

/// Waits for `pid` to finish, or kills its group at `cap`.
///
/// The verdict is the child's own exit plus what it printed: a clean `0` is the
/// only success. Every other outcome — a non-zero exit, a signal, a child that
/// never returns — is a boot refusal, and the timeout kills `-pgid` (then the
/// pid, if the group never came to exist) and reaps before answering.
fn await_probe(pid: Pid, message: &OwnedFd, cap: Duration) -> Result<(), String> {
    let deadline = Instant::now() + cap;
    let mut text = Vec::new();
    loop {
        if let Some(status) = try_wait(pid) {
            // The child is gone, so it holds no writer: this reads to end of
            // file instead of blocking on a live process.
            drain_to_end(message, &mut text);
            return match status {
                ExitStatus::Code(0) => Ok(()),
                other => Err(format!(
                    "the probe child (pid {pid}) {other}{}",
                    detail(&text)
                )),
            };
        }
        let now = Instant::now();
        if now >= deadline {
            kill_probe(pid);
            return Err(format!(
                "the probe child (pid {pid}) did not load `libphp` and its shim within {} ms, \
                 so it was killed; raise BLASTIT_PROBE_TIMEOUT_MS if this host needs longer",
                cap.as_millis()
            ));
        }
        let wait = (deadline - now).min(Duration::from_millis(20));
        if wait_readable(message.as_fd(), wait)
            .map_err(|err| format!("the probe child (pid {pid}) diagnostic pipe: {err}"))?
        {
            read_available(message, &mut text);
        }
    }
}

/// `waitpid(pid, WNOHANG)`: the child's status once it ended, `None` while it is
/// still running.
fn try_wait(pid: Pid) -> Option<ExitStatus> {
    // `None` would mean "any child", which is a different question: an
    // unconvertible pid is simply not this boot's child.
    let raw = rustix_pid(pid)?;
    match rustix::process::waitpid(Some(raw), rustix::process::WaitOptions::NOHANG) {
        Ok(Some((_, status))) => ExitStatus::from_raw(status.as_raw()),
        // `ECHILD` (already reaped) and a child that is still running are both
        // "no status yet" to this caller.
        Ok(None) | Err(_) => None,
    }
}

/// `SIGKILL`s the probe's process group, then reaps it.
///
/// The group is what the child made for itself; the single-pid `kill` is the
/// fallback for the race where the parent's `setpgid` and the child's have not
/// landed. Reaping is bounded, so a child stuck in uninterruptible sleep cannot
/// hold the boot that is refusing on its account — `PR_SET_PDEATHSIG` is the
/// backstop.
fn kill_probe(pid: Pid) {
    // The group first (the child made itself a leader inside `enter`), then the
    // pid, for the race where the parent's `join_group` and the child's
    // `setpgid` have not landed.
    if let Some(raw) = rustix_pid(pid) {
        let _ = rustix::process::kill_process_group(raw, Signal::KILL);
    }
    let _ = blastit_sys::kill(pid, Signal::KILL);
    let deadline = Instant::now() + Duration::from_secs(2);
    while Instant::now() < deadline {
        if try_wait(pid).is_some() {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Reads what is already in the pipe, without waiting for more.
fn read_available(fd: &OwnedFd, text: &mut Vec<u8>) {
    let mut buf = [0u8; 256];
    if let Ok(read) = rustix::io::read(fd, &mut buf) {
        keep_probe_text(text, &buf[..read]);
    }
}

/// Reads the pipe to end of file: only correct once every writer is gone.
fn drain_to_end(fd: &OwnedFd, text: &mut Vec<u8>) {
    let mut buf = [0u8; 256];
    loop {
        match rustix::io::read(fd, &mut buf) {
            Ok(0) => return,
            Ok(read) => keep_probe_text(text, &buf[..read]),
            Err(rustix::io::Errno::INTR) => continue,
            Err(_) => return,
        }
    }
}

/// Appends at most [`PROBE_MESSAGE_MAX`] bytes: the rest is read and dropped, so
/// a chatty child never fills the pipe and blocks.
fn keep_probe_text(text: &mut Vec<u8>, bytes: &[u8]) {
    let room = PROBE_MESSAGE_MAX.saturating_sub(text.len());
    if room > 0 {
        text.extend_from_slice(&bytes[..bytes.len().min(room)]);
    }
}

/// The child's stderr as one clause: whitespace collapsed (PHP prints several
/// lines), empty when it said nothing.
fn detail(text: &[u8]) -> String {
    let text = String::from_utf8_lossy(text);
    let words = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if words.is_empty() {
        String::new()
    } else {
        format!(": {words}")
    }
}
