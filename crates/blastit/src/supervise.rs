//! The supervisor: init's event loop after the `ready` event (§2.1, §4, §8).
//!
//! One `poll(2)` over everything init owns:
//!
//! ```text
//! signal fd │ ctl.chan │ dbproxy/http/scheduler events + stdout + stderr
//!           │          │ every live view's stdout + stderr
//! ```
//!
//! * **`SIGCHLD`** → a `waitpid` loop. A view's exit is reported to the
//!   scheduler as `EXITED` and logged as `process.exited`; the exit of http,
//!   scheduler or dbproxy is fatal, because a half-started sandbox is worse than
//!   a crash loop the platform can see (§8).
//! * **`SPAWN`** → the zygote forks a view and answers `SPAWNED` with the
//!   scheduler's end of `view.chan` by `SCM_RIGHTS`
//!   ([`blastit_view_python::zygote`]).
//! * **`RETIRE`** → `SIGKILL` that view. Views are retired when the scheduler
//!   finds them idle, over their request cap or damaged (§2.4); the scheduler
//!   logs `process.retired` itself, and init reports the exit when it is reaped.
//! * **`SIGTERM`/`SIGINT`** → `DRAIN` to the scheduler, wait for it or for
//!   `BLASTIT_GRACEFUL_TIMEOUT`, then `SIGTERM` and finally `SIGKILL` whatever
//!   is left, and exit `0` (§8).
//! * **log pipes** → complete lines become `app.output` events, or a forwarded
//!   event whose `component` and `route` are restamped from the pipe (§8.1).
//!
//! init is single-threaded and holds the GIL for the whole loop: nothing else
//! could want it, and the zygote needs it to fork.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::ffi::CString;
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, OwnedFd, RawFd};
#[cfg(feature = "python")]
use std::os::unix::ffi::OsStrExt;
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
#[cfg(feature = "python")]
use std::sync::atomic::Ordering;
use std::time::{Duration, Instant, SystemTime};

use blastit_events::ReloadBoot;
use blastit_events::{
    Component, DevReload, DevReloadUnavailable, Event, EventKind, ExitKind, Level, Lifecycle,
    ManifestWarning, ProcessExited, ReloadReason,
};
use blastit_frame::{Ctl, Decoder, Frame, Limits};
#[cfg(feature = "python")]
use blastit_manifest::Manifest;
use blastit_scheduler::RouteInfo;
use blastit_sys::{
    ExitStatus, Pid, Signal, SignalFd, SignalMask, kill, pipe, reap, recv_with_fd, send_with_fd,
};

#[cfg(feature = "python")]
use crate::child::wait_readable;
use crate::child::{Deadline, wait_writable};
use crate::config::Config;
use crate::dev_reload::{self, Class, Effect, Reload};
use crate::dev_reload::{ReloadConfig, SystemClock};
use crate::logs::{Emitter, LineBuffer, Pipe, lossy};
use crate::py::PyForkHooks;
#[cfg(feature = "python")]
use crate::py::PyRuntime;
use crate::stage1::ViewChildPlans;
#[cfg(feature = "php")]
use blastit_view::spawn::ForkProtocol;
#[cfg(any(feature = "php", feature = "python"))]
use blastit_view::spawn::SpawnContext;

/// How long the surviving children get after `SIGTERM` before `SIGKILL`, once
/// the drain is over.
pub const TERMINATE_GRACE: Duration = Duration::from_secs(2);

/// How long init waits for room on `ctl.chan` before it decides the scheduler
/// is gone.
///
/// `ctl.chan` carries a handful of bytes per message and its reader never stops
/// reading, so a full pipe means the scheduler is wedged or dead. Waiting
/// forever would wedge init with it, which is exactly what the reap loop must
/// not do.
pub const SEND_CTL_TIMEOUT: Duration = Duration::from_secs(1);

/// How long a `Blastfile` save may hold the supervisor for dbproxy's policy ack
/// (§4.6 step 2).
///
/// The document is small and dbproxy answers before it retires a view, so this
/// is the whole cost a save can add: one hundred milliseconds at worst, after
/// which the old grants stay in force and one `manifest.warning` says so. It is
/// deliberately the design's number, not a guess.
#[cfg(feature = "python")]
pub const POLICY_ACK_TIMEOUT: Duration = Duration::from_millis(100);

/// One descriptor in the poll set, with what to do with its bytes.
struct Source {
    fd: OwnedFd,
    pipe: Pipe,
    lines: LineBuffer,
}

/// How many extra processes a view's uid may have beyond its workers.
///
/// `ARCHITECTURE.md` §7.2 allows one unit of margin because gVisor leaves the
/// main thread out of the count; sixteen keeps the same headroom
/// `crates/blastit/src/php_view.rs` installs in `RLIMIT_NPROC`. Unifying the two
/// constants is a follow-up: that module is `php`-gated and private.
const VIEW_PROCESS_MARGIN: u32 = 16;

/// A live view process.
#[derive(Debug)]
struct ViewChild {
    route: usize,
    uid: u32,
    /// The live-reload source this pid reports as, when it has one (`dev` with
    /// reload on, and a WSGI slot).
    ///
    /// The report end is registered by `spawn_now` and revoked by
    /// `reap_children` — but only by the pid that still owns it: a route can
    /// fork a fresh view while a stale busy one is still serving, and the
    /// second registration replaces the first end for the same source (§4.4).
    source: Option<u16>,
}

/// Every supervised view child, by pid.
///
/// Its `route` is the one init forked it for, and the only route it may be
/// retired or reported as (`ADR-0015` decision 6). The pid is the key because a
/// route owns a pool now: the pid is what the kernel, the scheduler and the
/// events all name, and it is the one fact that tells a route's workers apart.
#[derive(Debug, Default)]
struct ChildTable {
    by_pid: HashMap<i32, ViewChild>,
}

impl ChildTable {
    /// Records a child init just forked.
    fn insert(&mut self, pid: i32, child: ViewChild) {
        self.by_pid.insert(pid, child);
    }

    /// Forgets a child that was reaped, giving back the record its exit needs.
    fn remove(&mut self, pid: i32) -> Option<ViewChild> {
        self.by_pid.remove(&pid)
    }

    /// The route init forked `pid` for, if it is still a live child.
    fn route_of(&self, pid: i32) -> Option<usize> {
        self.by_pid.get(&pid).map(|child| child.route)
    }

    /// How many children `route` owns right now.
    fn count_for(&self, route: usize) -> u32 {
        u32::try_from(
            self.by_pid
                .values()
                .filter(|child| child.route == route)
                .count(),
        )
        .unwrap_or(u32::MAX)
    }

    /// The `(route, pid)` check `RETIRE` makes: only a pid this boot forked for
    /// *that* route may be signalled.
    fn may_retire(&self, route: usize, pid: i32) -> bool {
        self.route_of(pid) == Some(route)
    }

    /// Every live child's pid.
    fn pids(&self) -> impl Iterator<Item = i32> + '_ {
        self.by_pid.keys().copied()
    }

    /// Whether init currently supervises no view at all.
    fn is_empty(&self) -> bool {
        self.by_pid.is_empty()
    }

    /// Gives up the report source of every live child of `route`.
    ///
    /// A fresh view's `spawn_now` registers a new end for the route's source
    /// ([`ReportTable::register`] keys the source, so the old end is gone); the
    /// stale pid must then not revoke the new one when it is reaped.
    fn forget_source(&mut self, route: usize) {
        for child in self.by_pid.values_mut() {
            if child.route == route {
                child.source = None;
            }
        }
    }
}

/// Whether init may fork one more worker for `route` (§4.5).
///
/// `ceiling` is `view_worker_cap() + VIEW_PROCESS_MARGIN`: init is the process
/// holding root or `CAP_KILL`, so it keeps a bound of its own rather than
/// forking purely on a less-privileged component's accounting. The scheduler
/// never legitimately asks for more, and `RLIMIT_NPROC` is absent under
/// `blastit dev`.
fn may_fork(children: &ChildTable, route: usize, ceiling: u32) -> bool {
    children.count_for(route) < ceiling
}

/// What one view fork is for: the slot's data, its scratch directory, and its
/// report socket (which the WSGI arm takes and the PHP arm has none of).
struct Spawn<'a> {
    /// Which route slot is being forked (`SPAWN`'s route).
    index: usize,
    /// Route metadata and target uid.
    info: &'a RouteInfo,
    /// Whether the route's filter keeps `AF_INET`/`AF_INET6` (§7.3).
    egress: bool,
    /// Private scratch directory (`HOME`, `TMPDIR`, `chdir`).
    dir: &'a std::path::Path,
    /// `true` in production (`setresuid` + `RLIMIT_NPROC`), `false` in `dev`.
    drop_privileges: bool,
    /// The dev-only loaded-file report, when this slot has one.
    report: ViewReport,
}

/// What `spawn_now` hands the view fork: its own report socket, or nothing.
///
/// One shape for both backends, and it is the transport's own
/// [`blastit_view::LoadedReport`] — the write end of a `SOCK_DGRAM` socketpair
/// and the source its batches name, so neither fork converts anything at its
/// boundary. `None` is every boot with nothing to report to — production,
/// `check`, `--no-reload`.
type ViewReport = Option<blastit_view::LoadedReport>;

/// A freshly forked worker of either runtime, as the parent sees it.
/// `ForkedView` (WSGI) and `ForkedPhpView` (embedded PHP) describe the same
/// five things, so the supervisor's bookkeeping — `SPAWNED`, the two log pipes,
/// the pid maps, the `process.spawned` event — is written once instead of once
/// per backend (§4.3).
struct Forked {
    pid: Pid,
    scheduler_end: OwnedFd,
    stdout: OwnedFd,
    stderr: OwnedFd,
    fork_ms: f64,
}

#[cfg(feature = "python")]
impl From<crate::py_view::ForkedView> for Forked {
    fn from(view: crate::py_view::ForkedView) -> Self {
        Self {
            pid: view.pid,
            scheduler_end: view.scheduler_end,
            stdout: view.stdout,
            stderr: view.stderr,
            fork_ms: view.fork_ms,
        }
    }
}

#[cfg(feature = "php")]
impl From<blastit_view_php::ForkedPhpView> for Forked {
    fn from(view: blastit_view_php::ForkedPhpView) -> Self {
        Self {
            pid: view.pid,
            scheduler_end: view.scheduler_end,
            stdout: view.stdout,
            stderr: view.stderr,
            fork_ms: view.fork_ms,
        }
    }
}

/// Where the shutdown is.
#[derive(Clone, Copy, Debug)]
enum Shutdown {
    /// Still serving.
    Serving,
    /// `DRAIN` sent; waiting for the scheduler, or for the graceful timeout.
    Drain(Deadline),
    /// `SIGTERM` sent to everyone; waiting for them, or for the kick deadline.
    Terminate(Deadline),
}

/// Why a write of one `ctl.chan` message stopped short.
#[derive(Clone, Debug, PartialEq, Eq)]
enum CtlWriteError {
    /// The write itself failed with this errno.
    Write(rustix::io::Errno),
    /// The scheduler took nothing for the whole of [`SEND_CTL_TIMEOUT`], with
    /// the reason to log.
    Wedged(String),
}

impl CtlWriteError {
    /// Whether this error says the scheduler's end of `ctl.chan` is gone.
    fn peer_gone(&self) -> bool {
        match self {
            CtlWriteError::Write(err) => ctl_peer_gone(err.raw_os_error()),
            CtlWriteError::Wedged(_) => false,
        }
    }
}

/// The three pipes one component sends its events and output on.
pub struct ComponentPipes {
    /// The events pipe's read end.
    pub events: OwnedFd,
    /// The stdout pipe's read end.
    pub stdout: OwnedFd,
    /// The stderr pipe's read end.
    pub stderr: OwnedFd,
}

/// What one reload cycle re-execs this image with (§4.6 step 6).
///
/// `config` is the exact byte string stage 0 wrote into `--config-fd` on this
/// image's boot, re-supplied through a fresh pipe; `program` is the stage-1
/// program prefix this image was started with (`/proc/self/cmdline` up to the
/// first `--stage=1` token, or the handover stage 0 recorded), so a wheel
/// re-execs `python -I -m blastit` and a standalone binary re-execs itself —
/// never `/proc/self/exe`, which for a wheel is the interpreter.
pub struct ReloadSource {
    /// The configuration document bytes this image received.
    pub config: Vec<u8>,
    /// `argv[0..]` of the stage-1 program, without the stage-1 arguments.
    pub program: Vec<CString>,
    /// `--dev-root`: the runtime root this boot adopted.
    pub dev_root: PathBuf,
    /// `--dev-host`: the address `http` already owns.
    pub dev_host: String,
}

/// The descriptors the adoption document names, captured before the pipes are
/// moved into the poll sources (§4.6 step 6).
struct ComponentFds {
    pid: i32,
    stdout: RawFd,
    stderr: RawFd,
    events: RawFd,
}

/// Everything the new image adopts instead of creating (§4.6 step 7).
///
/// Built once, at boot, from the components this image forked: their pids, the
/// read ends of their log pipes, the scheduler's end of `http.chan` (kept open
/// for the whole run so `http`'s peer never sees end-of-file), the `--learn`
/// channels, the dev policy socket end and the Python handoff descriptors.
/// `None` on every boot that cannot cycle (production, `check`, `--no-reload`).
struct Adoption {
    http: ComponentFds,
    dbproxy: Option<ComponentFds>,
    chan: OwnedFd,
    learn: Option<(RawFd, Option<RawFd>)>,
    policy: Option<OwnedFd>,
    /// The `(libpython, shim)` descriptors this image inherited from stage 0,
    /// kept open (with `FD_CLOEXEC` set) until `exec_cycle` clears it for the
    /// next `execve` (design §1.2.1 item 6). The single `OwnedFd` owner on this
    /// side of the barrier: `stage1::Adoption` carries the raw numbers only.
    py_fds: Option<(OwnedFd, OwnedFd)>,
}

/// Where one reload cycle is (§4.6 steps 3–6).
///
/// The cycle never blocks inside a `poll` wakeup: [`Supervisor::apply_effects`]
/// asks the scheduler for the all-views transaction and records `Draining`; the
/// loop keeps polling (refusing new `SPAWN`s), and [`Supervisor::progress_cycle`]
/// stops the scheduler once no view is left, then execs.
struct Cycle {
    stage: CycleStage,
    /// A bound on the wait, so a wedged scheduler is `SIGKILL`ed instead of
    /// keeping the listener up forever.
    deadline: Deadline,
}

/// The two states of a cycle that can be observed between wakeups.
#[cfg_attr(not(feature = "python"), allow(dead_code))]
enum CycleStage {
    /// `Ctl::Reload` is out; wait for every view to be reaped.
    Draining,
    /// The scheduler was asked to stop; wait for it to be reaped.
    Stopping,
}

/// How long a cycle waits for the views to drain, and then for the scheduler.
const CYCLE_GRACE: Duration = Duration::from_secs(10);

/// What stage 1 hands the supervisor.
pub struct Parts<'a> {
    /// The configuration, already parsed.
    pub cfg: Config,
    /// Frame limits of every channel.
    pub limits: Limits,
    /// One entry per manifest route, in routing order.
    pub routes: Vec<RouteInfo>,
    /// One entry per route slot, in routing order: the embedded-PHP worker this
    /// slot forks, or `None` for a WSGI slot (`ADR-0010` §1, design §4.3).
    ///
    /// Built and verified at boot by
    /// [`blastit_view_php::pinning::boot_bindings`], so the `SPAWN` path is a
    /// lookup and never a question about the backend kind.
    #[cfg(feature = "php")]
    pub php: Vec<Option<blastit_view_php::PhpBinding>>,
    /// One entry per manifest route, in routing order: whether its view may
    /// reach the network (§7.3).
    pub egress: Vec<bool>,
    /// One entry per manifest route, in routing order: the identity and the
    /// filter its view child is forked with (`view-crates-architecture` §4.4
    /// D1), decided by stage 1 where the boot mode exists. Laid out in every
    /// build; only the WSGI fork reads it.
    pub plans: ViewChildPlans,
    /// The composition root's whole handle on the preloaded interpreter
    /// (design §1.2.1 item 2): the opaque zygote, the three `PyOS_*Fork` hooks
    /// and the frozen vtable. Only a build with an interpreter has one, and
    /// only a WSGI view is forked from it.
    #[cfg(feature = "python")]
    pub runtime: Option<PyRuntime<'a>>,
    /// A build with no interpreter holds no runtime, and `PyRuntime` is the only
    /// user of `'a` in that shape: the marker is what keeps `Parts<'a>` a legal
    /// type there without inventing a second struct (and is never read).
    #[cfg(not(feature = "python"))]
    pub _runtime: std::marker::PhantomData<&'a ()>,
    /// The `(libpython, shim)` descriptors this image inherited from stage 0,
    /// when it may reload: `exec_cycle` hands them to the replacement instead of
    /// re-opening the objects by path (§1.2.1 item 6).
    pub py_fds: Option<(OwnedFd, OwnedFd)>,
    /// init's end of `ctl.chan`.
    pub ctl: OwnedFd,
    /// The signalfd init listens on.
    pub signals: SignalFd,
    /// The mask `signals` replaced, for every forked child.
    pub mask: SignalMask,
    /// The components that were forked, with the pipes they log on.
    pub components: BTreeMap<i32, (Component, ComponentPipes)>,
    /// The `blastit dev` runtime root, when this is a dev boot (`R20`).
    pub dev: Option<crate::dev_learn::Dev>,
    /// The `dev --learn` observation and attribution channels (`R29`).
    pub learn: Option<crate::dev_learn::Learn>,
    /// The one live-reload watcher of this boot (`ADR-0014`), when `dev` has
    /// reload on. `None` for every production boot, for `check`, and for a dev
    /// boot with reload off. A `php`-only standalone build gets one too: a PHP
    /// worker needs no interpreter, so its saves are served by repinning the
    /// entry.
    ///
    /// It is the live-reload gate **itself**: a boot with no
    /// watcher — `--no-reload`, or a `fanotify_mark` that could not run, which
    /// Docker's `overlayfs` answers with `EPERM` — has nothing that will ever
    /// report the save, so the boot's `EntryPin` has to stand.
    pub reload: Option<Reload>,
    /// What a reload **cycle** re-execs with (§4.6 step 6). `Some` only on a
    /// dev boot with reload on; the reload cycle has no other way to rebuild the
    /// pre-fork heap while keeping `:8080`.
    pub reload_source: Option<ReloadSource>,
    /// The scheduler's end of `http.chan`, kept open on a dev boot with reload
    /// on so `http`'s peer survives the scheduler the cycle stops (§4.6 step 5).
    pub chan: Option<OwnedFd>,
    /// This image's end of the dev-only dbproxy policy socket
    /// (`--dev-policy-fd`). Carried into the adoption document; the refresh
    /// itself is the policy channel's.
    pub policy: Option<OwnedFd>,
    /// The `(route, proxy)` pairs dbproxy's **listeners** were bound for at boot
    /// (`§4.8`): the set a save's document is compared against, so a store that
    /// is added or removed is one `manifest.warning` naming the restart instead
    /// of a silent refusal. Filled by stage 1 from the manifest it parsed.
    pub boot_pairs: BTreeSet<(String, String)>,
    /// The `--learn` descriptor numbers, for the adoption document.
    pub learn_fds: Option<(RawFd, Option<RawFd>)>,
}

/// The supervisor of all of blastit's processes.
pub struct Supervisor<'a> {
    cfg: Config,
    limits: Limits,
    routes: Vec<RouteInfo>,
    /// One entry per route slot: the PHP worker it forks, decided at boot
    /// (`ADR-0010` §1). Empty slots are WSGI views.
    #[cfg(feature = "php")]
    php: Vec<Option<blastit_view_php::PhpBinding>>,
    /// Whether each route's view may reach the network: only the view fork
    /// reads it, and only a build with an interpreter forks one.
    #[cfg_attr(not(feature = "python"), allow(dead_code))]
    egress: Vec<bool>,
    /// The identity and the filter each route's view child is forked with
    /// (`view-crates-architecture` §4.4 D1), from stage 1 — the only place the
    /// boot mode exists. Read by the WSGI fork alone.
    #[cfg_attr(not(feature = "python"), allow(dead_code))]
    plans: ViewChildPlans,
    /// The composition root's handle on the preloaded interpreter, in a build
    /// that has one.
    #[cfg(feature = "python")]
    runtime: Option<PyRuntime<'a>>,
    /// The live interpreter's fork protocol, injected by stage 1 (§1.2.1);
    /// `None` for a PHP/static image.
    #[cfg_attr(not(feature = "python"), allow(dead_code))]
    hooks: Option<PyForkHooks<'a>>,
    emitter: &'a Emitter,
    ctl: Option<OwnedFd>,
    decoder: Decoder,
    sources: Vec<Source>,
    signals: SignalFd,
    /// The mask a forked view puts back: read by the view fork alone.
    #[cfg_attr(not(feature = "python"), allow(dead_code))]
    mask: SignalMask,
    components: BTreeMap<i32, Component>,
    /// Every supervised view child, by pid: the one authority for which route
    /// owns a pid (`ADR-0015` decision 6).
    children: ChildTable,
    shutting_down: bool,
    shutdown: Shutdown,
    /// Where the sources start in the poll set: two when `ctl.chan` is polled.
    source_base: usize,
    /// The `blastit dev` runtime root, removed when the supervisor exits.
    dev: Option<crate::dev_learn::Dev>,
    /// The `dev --learn` observation pipe, attribution socket and collector.
    learn: Option<crate::dev_learn::Learn>,
    /// The one live-reload watcher of a `dev` boot with reload on (§4.1).
    ///
    /// Exactly one `Source` in the poll set, one armed deadline and no second
    /// event loop: a boot that never constructed it (production, `check`,
    /// `--no-reload`) costs the poll loop nothing. It is also the live-reload
    /// gate: while it is `None` no save may move a PHP `EntryPin`.
    reload: Option<Reload>,
    /// What a reload cycle re-execs with; `None` on every boot that cannot
    /// cycle (§4.6).
    reload_source: Option<ReloadSource>,
    /// The descriptors the adoption document names, or `None` when this boot
    /// did not fork the components a cycle adopts.
    adoption: Option<Adoption>,
    /// The cycle in flight, if any.
    cycle: Option<Cycle>,
    /// Whether the dev policy channel is left in a state this image cannot
    /// trust, so no later save may report a swap it did not get (§4.6 step 2).
    ///
    /// Set when a document was written but its ack never arrived (a timeout, a
    /// broken read or write): dbproxy may still be applying that document, so
    /// the next ack byte on the socket cannot be attributed to the next
    /// document. Every later save then emits the one warning and sends nothing,
    /// which is why the sentence ends "restart blastit dev".
    #[cfg_attr(not(feature = "python"), allow(dead_code))]
    policy_out_of_step: AtomicBool,
    /// The `(route, proxy)` pairs dbproxy's listeners were bound for at boot
    /// (§4.8), read by [`Supervisor::changed_pairs`].
    #[cfg_attr(not(feature = "python"), allow(dead_code))]
    boot_pairs: BTreeSet<(String, String)>,
    /// Whether this image is stopping its scheduler on purpose as part of a
    /// cycle (§4.6 step 5).
    ///
    /// The one gate: while it is set, the scheduler's exit is the expected stop
    /// (`component_exit`) and a closed `ctl.chan` is not a death (`ctl_closed`).
    /// It is set by [`Supervisor::begin_cycle`] and cleared when the scheduler's
    /// expected exit is reaped — which is the same wakeup that execs the
    /// replacement, so a *later* genuine `scheduler-died` is fatal again.
    reloading: bool,
}

impl<'a> Supervisor<'a> {
    /// Builds the supervisor and registers every pipe it must drain.
    pub fn new(parts: Parts<'a>, emitter: &'a Emitter) -> Self {
        // The adoption snapshot is taken before the pipes move into `sources`:
        // the numbers stay valid because the `OwnedFd`s that own them live as
        // long as the supervisor, and a component that closed its pipe is fatal
        // outside a cycle anyway.
        let mut parts = parts;
        let adoption = match (&parts.reload_source, parts.chan.take()) {
            (Some(_), Some(chan)) => {
                let mut http: Option<ComponentFds> = None;
                let mut dbproxy: Option<ComponentFds> = None;
                for (pid, (component, pipes)) in &parts.components {
                    let fds = ComponentFds {
                        pid: *pid,
                        stdout: pipes.stdout.as_raw_fd(),
                        stderr: pipes.stderr.as_raw_fd(),
                        events: pipes.events.as_raw_fd(),
                    };
                    match component {
                        Component::Http => http = Some(fds),
                        Component::Dbproxy => dbproxy = Some(fds),
                        _ => {}
                    }
                }
                http.map(|http| Adoption {
                    http,
                    dbproxy,
                    chan,
                    learn: parts.learn_fds,
                    policy: match parts.policy.take() {
                        Some(policy) => {
                            // The supervisor writes a policy document and waits
                            // for one ack byte through `poll`, so this end must
                            // never block: a dbproxy that stopped reading has to
                            // be a 100 ms timeout, not an init that hangs (§4.6).
                            if let Err(err) =
                                rustix::fs::fcntl_setfl(&policy, rustix::fs::OFlags::NONBLOCK)
                            {
                                if let Some(dev) = &parts.dev {
                                    dev.cleanup();
                                }
                                crate::logs::fail(emitter, &format!("policy socket: {err}"));
                            }
                            Some(policy)
                        }
                        None => None,
                    },
                    py_fds: parts.py_fds.take(),
                })
            }
            _ => None,
        };
        let mut sources = Vec::new();
        let mut components = BTreeMap::new();
        for (pid, (component, pipes)) in parts.components {
            components.insert(pid, component);
            sources.push(Source {
                fd: pipes.stdout,
                pipe: Pipe::Stdout {
                    component,
                    route: None,
                },
                lines: LineBuffer::new(),
            });
            sources.push(Source {
                fd: pipes.stderr,
                pipe: Pipe::Stderr {
                    component,
                    route: None,
                },
                lines: LineBuffer::new(),
            });
            sources.push(Source {
                fd: pipes.events,
                pipe: Pipe::Events { component },
                lines: LineBuffer::new(),
            });
        }
        // `ctl.chan` is written from the reap path, so it must never block it:
        // a scheduler that stopped reading has to show up as a timeout, not as
        // an init that hangs. Only this end is affected; the scheduler's own
        // socket is a different open file description.
        if let Err(err) = rustix::fs::fcntl_setfl(&parts.ctl, rustix::fs::OFlags::NONBLOCK) {
            if let Some(dev) = &parts.dev {
                dev.cleanup();
            }
            crate::logs::fail(emitter, &format!("ctl.chan: {err}"));
        }
        Self {
            cfg: parts.cfg,
            limits: parts.limits,
            routes: parts.routes,
            #[cfg(feature = "php")]
            php: parts.php,
            egress: parts.egress,
            plans: parts.plans,
            #[cfg(feature = "python")]
            runtime: parts.runtime,
            // One owner: the hooks are the runtime's own three pointers (§1.2.1).
            // A build with no interpreter loaded none, so it has none.
            #[cfg(feature = "python")]
            hooks: parts.runtime.map(|runtime| runtime.hooks),
            #[cfg(not(feature = "python"))]
            hooks: None,
            emitter,
            ctl: Some(parts.ctl),
            decoder: Decoder::new(parts.limits),
            sources,
            signals: parts.signals,
            mask: parts.mask,
            components,
            children: ChildTable::default(),
            shutting_down: false,
            shutdown: Shutdown::Serving,
            source_base: 2,
            dev: parts.dev,
            learn: parts.learn,
            reload: parts.reload,
            reload_source: parts.reload_source,
            adoption,
            cycle: None,
            policy_out_of_step: AtomicBool::new(false),
            boot_pairs: parts.boot_pairs,
            reloading: false,
        }
    }

    /// Runs until the process is asked to stop, then exits (§8).
    pub fn run(&mut self) -> ! {
        loop {
            self.poll_once();
            self.reap_children();
            self.progress_cycle();
            self.progress_shutdown();
        }
    }

    /// One `poll` and everything it found ready.
    fn poll_once(&mut self) {
        // The shutdown deadline and the debounce window share the timeout: the
        // loop must wake for whichever comes first, and it sleeps forever while
        // neither is armed (§4.5).
        let mut timeout = match self.shutdown {
            Shutdown::Serving => None,
            Shutdown::Drain(deadline) | Shutdown::Terminate(deadline) => Some(deadline.left()),
        };
        if let Some(deadline) = self.reload.as_ref().and_then(Reload::next_deadline) {
            let left = deadline.saturating_duration_since(Instant::now());
            timeout = Some(timeout.map_or(left, |current| current.min(left)));
        }
        // A cycle between wakeups needs a bounded timeout of its own: the
        // scheduler retires views and exits without a reload descriptor or a
        // shutdown deadline ever being armed.
        if let Some(cycle) = self.cycle.as_ref() {
            let left = cycle.deadline.left();
            timeout = Some(timeout.map_or(left, |current| current.min(left)));
        }
        // The reload's descriptors are appended last: the watcher's poll
        // duplicate first, then every live report end (§4.5).
        let mut reload_fds: Vec<std::os::fd::RawFd> = Vec::new();
        let (ready, polled_sources): (Vec<rustix::event::PollFlags>, usize) = {
            let mut pollfds: Vec<rustix::event::PollFd<'_>> =
                Vec::with_capacity(self.sources.len() + 2);
            pollfds.push(rustix::event::PollFd::from_borrowed_fd(
                self.signals.as_fd(),
                rustix::event::PollFlags::IN,
            ));
            if let Some(ctl) = self.ctl.as_ref() {
                pollfds.push(rustix::event::PollFd::new(
                    ctl,
                    rustix::event::PollFlags::IN,
                ));
            }
            if let Some(learn) = self.learn.as_ref().filter(|learn| learn.open) {
                pollfds.push(rustix::event::PollFd::new(
                    &learn.obs,
                    rustix::event::PollFlags::IN,
                ));
            }
            for source in &self.sources {
                pollfds.push(rustix::event::PollFd::new(
                    &source.fd,
                    rustix::event::PollFlags::IN,
                ));
            }
            if let Some(reload) = self.reload.as_mut() {
                reload.poll_fds(&mut reload_fds);
            }
            for fd in &reload_fds {
                // SAFETY: every number came from `Reload::poll_fds`, whose
                // descriptors the `Reload` owns; nothing drops it before `poll`
                // returns below, and the loop is single-threaded.
                let borrowed = unsafe { BorrowedFd::borrow_raw(*fd) };
                pollfds.push(rustix::event::PollFd::from_borrowed_fd(
                    borrowed,
                    rustix::event::PollFlags::IN,
                ));
            }
            let timespec = timeout.map(|left| rustix::event::Timespec {
                tv_sec: left.as_secs().min(i64::MAX as u64) as _,
                tv_nsec: left.subsec_nanos() as _,
            });
            // The number of log sources **in the poll set**, read here while it
            // is being built: `read_ctl` below can fork a view and push two
            // more, and the reload's descriptors sit after the ones that were
            // polled, not after the ones init now owns.
            let polled_sources = self.sources.len();
            let flags: Vec<rustix::event::PollFlags> =
                match rustix::event::poll(&mut pollfds, timespec.as_ref()) {
                    Ok(_) => pollfds.iter().map(|fd| fd.revents()).collect(),
                    Err(err) => self.fail(&format!("poll: {err}")),
                };
            // `ctl.chan` leaves the poll set once the scheduler's end is gone,
            // and the sources then start an entry earlier.
            let learn_polled = self.learn.as_ref().is_some_and(|learn| learn.open);
            self.source_base = usize::from(self.ctl.is_some()) + usize::from(learn_polled) + 1;
            (flags, polled_sources)
        };

        if ready
            .first()
            .is_some_and(|flags| flags.intersects(readable()))
        {
            self.drain_signals();
        }
        if self.ctl.is_some()
            && ready
                .get(1)
                .is_some_and(|flags| flags.intersects(readable()))
        {
            self.read_ctl();
        }
        let learn_at = if self.ctl.is_some() { 2 } else { 1 };
        if self.learn.as_ref().is_some_and(|learn| learn.open)
            && ready
                .get(learn_at)
                .is_some_and(|flags| flags.intersects(readable()))
            && let Some(learn) = self.learn.as_mut()
        {
            learn.poll();
        }

        let emitter = self.emitter;
        let base = self.source_base;
        let mut closed: Vec<usize> = Vec::new();
        let polled = polled_source_range(polled_sources, self.sources.len());
        for index in polled {
            let Some(flags) = ready.get(index + base) else {
                continue;
            };
            if !flags.intersects(readable()) {
                continue;
            }
            let source = &mut self.sources[index];
            if matches!(
                read_source(source, emitter, self.learn.as_mut()),
                SourceRead::Closed
            ) {
                closed.push(index);
            }
        }
        for index in closed.into_iter().rev() {
            let mut source = self.sources.remove(index);
            let mut lines: Vec<Vec<u8>> = Vec::new();
            source.lines.flush(&mut lines);
            for line in lines {
                emit_line(emitter, &source.pipe, &line);
            }
        }

        // The reload's own wakeups: a ready watcher or report end, and then the
        // armed deadline. `tick` is a no-op while the window is not due, so the
        // deadline needs no separate "did poll time out" question.
        if !reload_fds.is_empty() {
            let first = reload_base(base, polled_sources);
            let mut effects: Vec<Effect> = Vec::new();
            if let Some(reload) = self.reload.as_mut() {
                let now = reload.now();
                for (offset, fd) in reload_fds.iter().enumerate() {
                    if let Some(flags) = ready.get(first + offset)
                        && flags.intersects(readable())
                    {
                        // The flags travel with the descriptor: a hangup is the
                        // one thing a report end's `read` cannot report.
                        effects.extend(reload.on_fd(*fd, *flags, now));
                    }
                }
                effects.extend(reload.tick(now));
            }
            self.apply_effects(effects);
        }
    }

    /// Turns one policy step into what init does: one event, one `Ctl::Reload`,
    /// and — when the watcher is gone — one warning and no retry (§4.6, §4.7).
    fn apply_effects(&mut self, effects: Vec<Effect>) {
        for effect in effects {
            match effect {
                // The all-views transaction, or the cycle when the changed file
                // is one every view's heap already holds (§4.5, §4.6).
                #[cfg(feature = "python")]
                Effect::Reload(fire) if fire.needs_cycle() && self.reload_source.is_some() => {
                    self.begin_cycle(&fire);
                }
                Effect::Reload(fire) => self.reload_all(&fire),
                // The ladder's one `dev.reload.unavailable`: marking ran out of
                // room (`ENOSPC`, or the `MAX_MARKS` cap). The marks already
                // placed keep watching, so the watcher is **kept** — this is
                // not the dead arm (§4.7, `T32`).
                Effect::Unavailable(why) => reload_unavailable(self.emitter, &why),
                // A running watcher that dies produces one `manifest.warning`
                // and no second watcher: closing the group and its duplicate is
                // dropping the one watcher (§4.7).
                Effect::Stopped(why) => {
                    reload_stopped(self.emitter, &why);
                    self.reload = None;
                }
            }
        }
    }

    /// One `dev.reload` and one `Ctl::Reload` for **every** view (§4.6).
    ///
    /// `routes` is the whole set of view routes in routing order — the event
    /// says what the reload replaced, and the frame carries no route at all:
    /// there is no per-view restart to name.
    fn reload_all(&mut self, fire: &dev_reload::Fire) {
        let routes: Vec<String> = self.routes.iter().map(|route| route.name.clone()).collect();
        self.emit_reload(&routes, fire.files.as_slice(), fire.reason);
        // The file every worker will run was just saved, so the
        // boot's `EntryPin` is stale by definition. Re-record it **before** the
        // old workers are retired: a replacement worker forked from the old pin
        // would answer every request with "changed since this boot recorded it",
        // which is `500` for a save that is exactly what dev promises to serve.
        #[cfg(feature = "php")]
        self.repin_php_bindings();
        self.send_ctl(&Ctl::Reload);
    }

    /// Re-records every PHP slot's `entrypoint php` after a save (§4.6).
    ///
    /// Only a boot whose watcher is live gets here: `--no-reload`
    /// and a `fanotify_mark` that could not run (Docker's `overlayfs` answers
    /// `EPERM`, so `boot_reload` publishes `dev.reload.unavailable` and hands
    /// back `None`) are one thing — no live reload is running, so the boot's pin
    /// stands. A slot with no binding is skipped, and a refusal — the file
    /// mid-save, or a symlink that now points out of the manifest directory — is
    /// one `manifest.warning`, with the previous pin kept, so the worker keeps
    /// refusing that path instead of running something the boot did not resolve.
    #[cfg(feature = "php")]
    fn repin_php_bindings(&mut self) {
        if self.dev.is_none() || self.reload.is_none() {
            return;
        }
        let mut failures: Vec<String> = Vec::new();
        for binding in self.php.iter_mut().flatten() {
            let dir = binding.document_root.clone();
            let declared = binding.declared_entry.clone();
            if let Err(message) = binding.repin_entry(&dir, &declared) {
                failures.push(message);
            }
        }
        for message in failures {
            self.warn(message);
        }
    }

    /// One `dev.reload` in the §4.9 shape.
    fn emit_reload(&self, routes: &[String], files: &[std::path::PathBuf], reason: ReloadReason) {
        let files: Vec<String> = files
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect();
        self.emitter.emit(&Event::now(
            Component::Init,
            EventKind::DevReload(DevReload::new(routes.to_vec(), files, reason)),
        ));
    }

    /// One `manifest.warning`, the shape every refusal of the cycle uses: one
    /// event, one sentence, and the previous topology keeps serving (§4.6).
    fn warn(&self, message: String) {
        self.emitter.emit(&Event::now(
            Component::Init,
            EventKind::ManifestWarning(ManifestWarning::new(message)),
        ));
    }

    /// Replaces dbproxy's whole route-policy map with `manifest`'s grants, and
    /// waits for the one-byte ack (§4.6 step 2, §4.8).
    ///
    /// The channel is the socketpair stage 0 created for this boot, held by
    /// [`Adoption::policy`](struct@Adoption); it is **never** re-created, so a
    /// save costs no descriptor and no re-bind. `None` is every boot with no
    /// channel (no dbproxy was forked, or the descriptor was not handed over):
    /// there is nothing to refresh and no warning to invent.
    ///
    /// A save whose document changes the **pair set** — a store added or removed
    /// — cannot bind a new socket (they are fixed at boot, §4.8), so it emits one
    /// warning naming the restart *and* still sends the document: the entries
    /// that exist are updated, and the ones that do not can never fail open.
    ///
    /// A document that cannot be built, a write that cannot finish, an ack that
    /// does not arrive inside [`POLICY_ACK_TIMEOUT`] and a `0x01` all take the
    /// same arm: the previous grants stay in force, one `manifest.warning` names
    /// the reason and the restart, and the cycle continues. Dev saves never fail
    /// and never block — the wait is a bounded synchronous exchange
    /// ([`POLICY_ACK_TIMEOUT`], measured in
    /// `the_ack_wait_is_bounded_by_the_design_s_deadline`), the one deviation
    /// from §4.6's `policy_ack` poll state, recorded in this node's `RESULT.md`.
    #[cfg(feature = "python")]
    fn refresh_policy(&self, manifest: &Manifest) {
        let Some(adoption) = self.adoption.as_ref() else {
            return;
        };
        // The listener set is a property of the topology, not of the channel: a
        // boot that forked no dbproxy still owes the warning when a save adds a
        // store (and a boot that has one owes it when a save removes one).
        if let Some(changed) = self.changed_pairs(manifest) {
            self.warn(changed);
        }
        // No dbproxy was forked (a boot whose manifest grants nothing on a
        // `postgresql` proxy): there is no reader for the document.
        if adoption.dbproxy.is_none() {
            return;
        }
        let Some(policy) = adoption.policy.as_ref() else {
            return;
        };
        // A channel this image left out of step can never be trusted again: the
        // next ack byte could belong to the document whose ack timed out, so a
        // "swap" would be a report of something that did not happen (§4.6's
        // "never report a success it did not get"). One warning, nothing sent.
        if self.policy_out_of_step.load(Ordering::Relaxed) {
            self.warn(
                "the new grants were not applied; restart blastit dev (an earlier save left \
                 the policy channel out of step, so its message can no longer be attributed)"
                    .to_string(),
            );
            return;
        }

        let outcome = match policy_document(manifest) {
            Ok(document) => exchange_policy(
                policy.as_fd(),
                &document,
                Deadline::after(POLICY_ACK_TIMEOUT),
            ),
            Err(cause) => PolicyOutcome::Broken(cause),
        };
        match outcome {
            PolicyOutcome::Swapped => {}
            PolicyOutcome::Refused(cause) => self.warn(format!(
                "the new grants were not applied; restart blastit dev ({cause})"
            )),
            PolicyOutcome::Broken(cause) => {
                self.policy_out_of_step.store(true, Ordering::Relaxed);
                self.warn(format!(
                    "the new grants were not applied; restart blastit dev ({cause})"
                ));
            }
        }
    }

    /// The one warning a save whose document changes the `(route, proxy)` set
    /// owes (§4.8), or `None` when the set is unchanged.
    #[cfg(feature = "python")]
    fn changed_pairs(&self, manifest: &Manifest) -> Option<String> {
        store_set_warning(&self.boot_pairs, manifest)
    }

    /// Starts the reload **cycle** for a change to a file every view's heap
    /// already holds (§4.6).
    ///
    /// Validate before touching anything: the manifest is re-read and re-parsed,
    /// and a changed Python source is compiled (never executed), so the common
    /// half-saved file is refused with the file's own sentence and no view, no
    /// scheduler and no listener is disturbed. Only a boot that passes all of
    /// that asks for the all-views transaction and, once no view is left, stops
    /// the scheduler and execs this image's replacement.
    ///
    /// Under `--learn` the manifest cycle does not run at all: the one-pass
    /// tally lives in this image and a re-exec would discard it (§4.6 step 0).
    #[cfg(feature = "python")]
    fn begin_cycle(&mut self, fire: &dev_reload::Fire) {
        if self.learn.is_some() {
            self.warn(
                "the Blastfile changed; restart `blastit dev --learn` to apply it".to_string(),
            );
            return;
        }
        if self.reload_source.is_none() {
            self.reload_all(fire);
            return;
        }
        let manifest = match crate::config::read_manifest(&self.cfg.manifest) {
            Ok(text) => match Manifest::parse_blastfile(&text, self.cfg.uid_base) {
                Ok((manifest, _warnings)) => manifest,
                Err(errors) => {
                    self.warn(manifest_sentence(
                        &self.cfg.manifest,
                        errors.errors().first(),
                    ));
                    return;
                }
            },
            Err(error) => {
                self.warn(error.sentence(&self.cfg.manifest));
                return;
            }
        };
        // A `preload` module mid-write is the same story one import later: the
        // old preload keeps serving until every changed module compiles.
        for path in &fire.files {
            if path == &self.cfg.manifest {
                continue;
            }
            if path.extension().and_then(std::ffi::OsStr::to_str) != Some("py") {
                continue;
            }
            let Ok(bytes) = std::fs::read(path) else {
                // A deleted file is not a half-written one: the import that
                // fails is the boot's own sentence, after the exec.
                continue;
            };
            // `PyCode::compile` with `PyCodeInput::File`, in the preloaded
            // parent, through the frozen vtable (design §1.2.1 item 4): compile
            // only, no import and no execution. The refusal the `dev` reload
            // path prints is the shim's own `<path>: <message>` sentence.
            let Some(runtime) = self.runtime else {
                // One file this boot cannot preflight is not the whole cycle: warn
                // (the save is named) and keep checking the rest, never abandon the
                // reload silently.
                self.warn(format!(
                    "{}: this boot has no interpreter to compile with; the save was not \
                     checked",
                    path.display()
                ));
                continue;
            };
            if let Err(err) = runtime.check_syntax(&bytes, path.as_os_str().as_bytes()) {
                self.warn(crate::py::refusal_message(&err));
                return;
            }
        }
        // One `dev.reload` naming "every route serving after the reload" — the
        // new manifest's view routes, in order.
        let routes: Vec<String> = manifest
            .routes()
            .iter()
            .map(|route| route.name().to_string())
            .collect();
        self.emit_reload(&routes, &fire.files, fire.reason);
        // §4.6 step 2, before `Ctl::Reload`: dbproxy's allowlist is refreshed
        // from the new manifest's own pairs, so the first query a replacement
        // view runs is decided by the new grants. A refusal or a timeout keeps
        // the previous grants and warns once; the retirement below continues
        // either way, because a dev save never fails and never blocks.
        self.refresh_policy(&manifest);
        self.send_ctl(&Ctl::Reload);
        self.reloading = true;
        self.cycle = Some(Cycle {
            stage: CycleStage::Draining,
            deadline: Deadline::after(CYCLE_GRACE),
        });
    }

    /// Moves a cycle along: stop the scheduler once no view is left, then
    /// replace this image (§4.6 steps 4–6).
    fn progress_cycle(&mut self) {
        let Some(cycle) = self.cycle.as_mut() else {
            return;
        };
        let expired = cycle.deadline.expired();
        match cycle.stage {
            CycleStage::Draining => {
                if self.children.is_empty() {
                    // Only the scheduler: `http` keeps the listener and every
                    // accepted connection, dbproxy is untouched.
                    if let Some(pid) = self.scheduler_pid()
                        && let Err(err) = kill(pid, Signal::TERM)
                    {
                        self.warn(format!("stopping the scheduler for a reload: {err}"));
                    }
                    self.cycle = Some(Cycle {
                        stage: CycleStage::Stopping,
                        deadline: Deadline::after(CYCLE_GRACE),
                    });
                } else if expired {
                    // Never `kill_all`: that would signal `http` and dbproxy
                    // too, and their exits are fatal — one request stuck in a
                    // view would turn every save into a dead dev tree. The
                    // cycle is abandoned instead, the old topology keeps
                    // serving, and one warning names the route that would not
                    // drain (§4.6 step 4, `R9`).
                    let wedged = self.wedged_routes();
                    self.abandon_cycle(&wedged);
                }
            }
            CycleStage::Stopping => {
                if self.scheduler_pid().is_none() {
                    self.cycle = None;
                    self.reloading = false;
                    self.exec_cycle();
                } else if expired && let Some(pid) = self.scheduler_pid() {
                    let _ = kill(pid, Signal::KILL);
                }
            }
        }
    }

    /// The routes whose views kept a cycle from draining.
    fn wedged_routes(&self) -> Vec<String> {
        let mut routes: Vec<String> = self
            .children
            .pids()
            .filter_map(|pid| self.children.route_of(pid))
            .filter_map(|route| self.routes.get(route))
            .map(|route| route.name.clone())
            .collect();
        routes.sort();
        routes.dedup();
        routes
    }

    /// Drops a cycle whose views never drained, and says so once.
    ///
    /// Nothing is signalled: `http`, dbproxy and the scheduler keep running
    /// with the topology that was serving, which is the only outcome that
    /// cannot kill a `dev` tree over one slow request. The scheduler already
    /// has the all-views transaction, so the next request gets a fresh view and
    /// a later save can try again.
    fn abandon_cycle(&mut self, wedged: &[String]) {
        self.cycle = None;
        self.reloading = false;
        self.warn(abandoned_cycle_warning(wedged));
    }

    /// The scheduler's pid, while it is still a live component.
    fn scheduler_pid(&self) -> Option<Pid> {
        self.components
            .iter()
            .find(|(_, component)| **component == Component::Scheduler)
            .and_then(|(pid, _)| Pid::from_raw(*pid))
    }

    /// Replaces this image with a fresh stage 1 that adopts `:8080` (§4.6 step 6).
    ///
    /// An `execve` that returns is fatal: there is no event loop left to rebuild,
    /// and a half-started reload must never keep serving.
    fn exec_cycle(&mut self) -> ! {
        let Some(source) = self.reload_source.as_ref() else {
            self.fail("reload: there is no stage-1 program to re-exec");
        };
        let Some(adoption) = self.adoption.take() else {
            self.fail("reload: this boot forked no components to adopt");
        };
        #[cfg(debug_assertions)]
        reload_pause();
        let since = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or(0);
        let document = match adoption_document(&adoption) {
            Ok(document) => document,
            Err(message) => self.fail(&message),
        };
        // The configuration bytes this image was started with, on a fresh pipe.
        let (config_read, config_write) = match pipe() {
            Ok(pair) => pair,
            Err(err) => self.fail(&format!("reload: the configuration pipe: {err}")),
        };
        let (adopt_read, adopt_write) = match pipe() {
            Ok(pair) => pair,
            Err(err) => self.fail(&format!("reload: the adoption pipe: {err}")),
        };
        {
            use std::io::Write as _;
            for (fd, bytes) in [
                (config_write, source.config.as_slice()),
                (adopt_write, document.as_slice()),
            ] {
                let mut file = std::fs::File::from(fd);
                if let Err(err) = file.write_all(bytes) {
                    self.fail(&format!("reload: writing the reload pipes: {err}"));
                }
            }
        }

        let mut argv: Vec<CString> = source.program.clone();
        for token in [
            "--stage=1".to_string(),
            format!("--config-fd={}", config_read.as_raw_fd()),
            format!("--dev-root={}", source.dev_root.display()),
            format!("--dev-host={}", source.dev_host),
            "--dev-reload=on".to_string(),
            format!("--dev-reload-since={since}"),
            format!("--dev-adopt-fd={}", adopt_read.as_raw_fd()),
        ] {
            match CString::new(token) {
                Ok(token) => argv.push(token),
                Err(_) => self.fail("reload: a reload argument contains a NUL byte"),
            }
        }
        let mut keep = vec![
            config_read.as_raw_fd(),
            adopt_read.as_raw_fd(),
            adoption.chan.as_raw_fd(),
            adoption.http.stdout,
            adoption.http.stderr,
            adoption.http.events,
        ];
        if let Some(dbproxy) = &adoption.dbproxy {
            keep.extend([dbproxy.stdout, dbproxy.stderr, dbproxy.events]);
        }
        if let Some((obs, ctl)) = adoption.learn {
            keep.push(obs);
            if let Some(ctl) = ctl {
                keep.push(ctl);
            }
        }
        if let Some(policy) = &adoption.policy {
            keep.push(policy.as_raw_fd());
        }
        if let Some((lib, shim)) = &adoption.py_fds {
            keep.push(lib.as_raw_fd());
            keep.push(shim.as_raw_fd());
        }
        for fd in keep {
            // SAFETY: every number came from a descriptor this process owns.
            let borrowed = unsafe { BorrowedFd::borrow_raw(fd) };
            if let Err(err) = blastit_sys::set_cloexec(borrowed, false) {
                self.fail(&format!("reload: clearing FD_CLOEXEC: {err}"));
            }
        }
        let env: Vec<(String, String)> = std::env::vars_os()
            .filter_map(|(key, value)| Some((key.into_string().ok()?, value.into_string().ok()?)))
            .collect();
        let Some(program) = argv.first().cloned() else {
            self.fail("reload: the stage-1 program prefix is empty");
        };
        if let Err(err) = blastit_sys::restore_signal_mask_in_child(self.mask) {
            self.fail(&format!("reload: restoring the signal mask: {err}"));
        }
        let err = blastit_sys::exec(&program, &argv, &env);
        // A reload that never started must not keep serving.
        self.kill_all();
        crate::logs::write_line(libc::STDERR_FILENO, &format!("reload: exec stage 1: {err}"));
        crate::coverage::flush();
        // SAFETY: init is exiting; `_exit` runs no handler and cannot deadlock.
        unsafe { libc::_exit(1) }
    }

    /// Reads every pending signal from the `signalfd`.
    fn drain_signals(&mut self) {
        loop {
            match self.signals.read() {
                Ok(Some(Signal::CHILD)) => {}
                Ok(Some(Signal::TERM | Signal::INT)) => self.begin_shutdown(),
                Ok(Some(_)) => {}
                Ok(None) => return,
                Err(err) => self.fail(&format!("signalfd: {err}")),
            }
        }
    }

    /// Reads `ctl.chan` once and handles every complete frame it holds.
    fn read_ctl(&mut self) {
        let Some(raw) = self.ctl.as_ref().map(AsRawFd::as_raw_fd) else {
            return;
        };
        let mut buffer = [0u8; 4096];
        // SAFETY: `raw` is the descriptor of `self.ctl`, which is still open
        // here; the borrow is deliberately short so the end-of-file path below
        // can drop the channel.
        let ctl = unsafe { BorrowedFd::borrow_raw(raw) };
        let (read, fd) = match recv_with_fd(ctl, &mut buffer) {
            Ok(read) => read,
            // The socket is non-blocking and `poll` said there was something to
            // read; `EAGAIN` here is a race that costs nothing, because the poll
            // set is level-triggered.
            Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => return,
            // A scheduler that closes its end while messages are still queued
            // for it reads back as a reset, not as end-of-file. To init that is
            // the same event, and `ctl_closed` decides what a gone scheduler
            // costs: fatal while serving, the normal end of a drain.
            Err(err) if ctl_read_lost_peer(&err) => {
                self.ctl_closed();
                return;
            }
            Err(err) => self.fail(&format!("ctl.chan: {err}")),
        };
        // init never receives a descriptor on ctl.chan; if one arrives it is
        // closed rather than leaked.
        drop(fd);
        if read == 0 {
            self.ctl_closed();
            return;
        }
        self.decoder.push(&buffer[..read]);
        loop {
            match self.decoder.next_frame() {
                Ok(Some((_id, frame))) => self.on_ctl(frame),
                Ok(None) => break,
                Err(err) => self.fail(&format!("ctl.chan: {err}")),
            }
        }
    }

    /// The scheduler's end of `ctl.chan` is gone: end-of-file, or the reset a
    /// scheduler that closed with messages still queued for it leaves behind.
    ///
    /// While shutting down that is the normal end of the drain: the scheduler
    /// left, so init stops polling a dead channel and moves on to terminating
    /// everyone else. During a reload **cycle** it is the scheduler being
    /// stopped on purpose (§4.6 step 5), so it is not a death either. At any
    /// other time it means the scheduler died without being reaped, which is
    /// fatal (§8).
    fn ctl_closed(&mut self) {
        self.ctl = None;
        if self.shutting_down {
            let scheduler = self
                .components
                .iter()
                .find(|(_, component)| **component == Component::Scheduler)
                .map(|(pid, _)| *pid);
            if let Some(pid) = scheduler {
                self.components.remove(&pid);
            }
            return;
        }
        // A cycle stopped the scheduler on purpose: its exit is expected, and
        // the pid is kept until it is reaped so `progress_cycle` never execs
        // over a live scheduler that still owns `http.chan`.
        if self.reloading {
            return;
        }
        self.fail("ctl.chan: the scheduler closed the channel");
    }

    /// Handles one `ctl.chan` message from the scheduler.
    fn on_ctl(&mut self, frame: Frame) {
        let message = match frame {
            Frame::Ctl(message) => message,
            other => self.fail(&format!(
                "ctl.chan: unexpected {other:?} from the scheduler"
            )),
        };
        match message {
            Ctl::Spawn { route } => self.spawn(route),
            Ctl::Retire { route, pid, .. } => self.retire(route, pid),
            // init is the sender of SPAWNED, EXITED, DRAIN and RELOAD; a
            // scheduler that sends one back has broken the protocol, but that
            // is not worth killing a process over.
            Ctl::Drain | Ctl::Reload | Ctl::Exited { .. } | Ctl::Spawned { .. } => {
                self.emitter.emit(&Event::now(
                    Component::Init,
                    EventKind::ManifestWarning(ManifestWarning::new(
                        "the scheduler sent a ctl.chan message init sends; ignored",
                    )),
                ))
            }
        }
    }

    /// `SPAWN`: fork a view process for `route` and answer `SPAWNED` (§4).
    ///
    /// Each worker gets its own `view.chan` from its own `socketpair`, and
    /// sharing the route's scratch directory is the pool's design, so a route
    /// may own several processes: there is no held `SPAWN` any more. What is
    /// kept is init's own fork bound — see [`may_fork`] — because the scheduler
    /// must not be the only thing deciding how many processes init holds.
    fn spawn(&mut self, route: u32) {
        // A cycle has asked every view to retire and is about to replace this
        // image: forking a view now would fork the **old** pre-fork heap after
        // the reload, which is exactly the mixed-version bug the cycle exists
        // to prevent. The scheduler is owed an answer, so the slot is refused.
        if self.reloading {
            self.send_spawn_refused(route);
            return;
        }
        let index = route as usize;
        let Some(info) = self.routes.get(index).cloned() else {
            self.fail(&format!("ctl.chan: SPAWN for unknown route {route}"));
        };
        let ceiling = self
            .cfg
            .view_worker_cap()
            .saturating_add(VIEW_PROCESS_MARGIN);
        if !may_fork(&self.children, index, ceiling) {
            self.emitter.emit(&Event::now(
                Component::Init,
                EventKind::ManifestWarning(ManifestWarning::new(format!(
                    "SPAWN for route {route} refused: {ceiling} processes already live for it"
                ))),
            ));
            self.send_spawn_refused(route);
            return;
        }
        self.spawn_now(index, &info);
    }

    /// Forks the view for `index`, hands its channel over, and records it.
    ///
    /// Which runtime to fork is **boot data**, not a question asked here: a
    /// slot with a [`crate::php_view::PhpBinding`] forks the embedded-PHP worker
    /// (`ADR-0010` §1) and every other slot the WSGI zygote. The binding was
    /// verified at boot (`st_dev`/`st_ino` against stage 0's record), so a
    /// `SPAWN` can only ever start the runtime this boot already admitted.
    fn spawn_now(&mut self, index: usize, info: &RouteInfo) {
        let egress = self.egress.get(index).copied().unwrap_or(false);
        // Production gives each route its own uid and `<scratch>/views/<uid>`;
        // `dev` gives every view the caller's uid and names the directory by
        // route index instead (`R19`, `R20`).
        let (dir, drop_privileges) = match &self.dev {
            Some(dev) => (dev.view_dir(index), false),
            None => (crate::config::view_dir(info.uid), true),
        };
        // A save can land before the watcher's debounce fired, or
        // before this boot's very first request. A PHP worker forked from a
        // stale `EntryPin` answers every request with "changed since this boot
        // recorded it", so the entry is recorded again here — and a refusal (a
        // file mid-save, or a symlink out of the tree) is one warning, never a
        // failed boot. With no watcher (`--no-reload`, or
        // `fanotify_mark` refused on Docker's `overlayfs`) nothing will ever
        // report the save, so the boot's pin stands and no repin runs.
        #[cfg(feature = "php")]
        if self.dev.is_some() && self.reload.is_some() {
            let mut refusal: Option<String> = None;
            if let Some(binding) = self.php.get_mut(index).and_then(Option::as_mut)
                && !binding.entry_is_current()
            {
                let root = binding.document_root.clone();
                let declared = binding.declared_entry.clone();
                if let Err(message) = binding.repin_entry(&root, &declared) {
                    refusal = Some(message);
                }
            }
            if let Some(message) = refusal {
                self.warn(message);
            }
        }
        // The view's report socket, created **before** the fork so its read end
        // is in the supervisor's poll set before the child could send its boot
        // batch, and a fresh reporter per boot (§4.4).
        let (report, source) = match self.open_report(index) {
            Ok(pair) => pair,
            Err(err) => {
                self.emitter.emit(&Event::now(
                    Component::Init,
                    EventKind::ManifestWarning(ManifestWarning::new(format!(
                        "live reload: a report socket for route {:?}: {err}",
                        info.name
                    ))),
                ));
                self.send_spawn_refused(index as u32);
                return;
            }
        };
        let child = match self.fork_view(Spawn {
            index,
            info,
            egress,
            dir: &dir,
            drop_privileges,
            report,
        }) {
            Ok(child) => child,
            Err(err) => {
                // Never `fail`: one route's resource exhaustion is not the
                // tree's. The SPAWN is still owed an answer, so the scheduler
                // frees the slot and backs this route's growth off (§4.7).
                self.emitter.emit(&Event::now(
                    Component::Init,
                    EventKind::ManifestWarning(ManifestWarning::new(format!(
                        "forking a view for route {:?}: {err}",
                        info.name
                    ))),
                ));
                self.send_spawn_refused(index as u32);
                return;
            }
        };
        self.record_view(index, info, child, source);
    }

    /// Registers this view's report end with the reload policy, or returns
    /// `None` when there is nothing to report to.
    ///
    /// `None` is every production boot, `check` and a dev boot with reload off.
    /// Both backends report — a WSGI view's loader and a PHP worker's file sink
    /// are the same wire — so a slot gets its socket whatever
    /// runtime serves it, and a fresh reporter is created **per boot** (this
    /// call) because a source's first batch is its `boot` set (`loaded.rs`).
    fn open_report(&mut self, index: usize) -> std::io::Result<(ViewReport, Option<u16>)> {
        let Some(source) = u16::try_from(index).ok() else {
            return Ok((None, None));
        };
        let Some(reload) = self.reload.as_mut() else {
            return Ok((None, None));
        };
        let (read, write) = dev_reload::report_socket()?;
        // A route can fork a fresh view while a stale busy one still serves:
        // the table keys the source, so the newest end wins and the stale pid
        // must not revoke it later.
        self.children.forget_source(index);
        reload
            .policy()
            .reports()
            .register(read.into(), source, Class::Loaded, false);
        Ok((
            Some(blastit_view::LoadedReport {
                socket: write,
                source,
            }),
            Some(source),
        ))
    }

    /// `SPAWNED` with pid 0 is the one answer that names no process: this
    /// `SPAWN` could not be honoured (`ADR-0015` decision 8).
    fn send_spawn_refused(&mut self, route: u32) {
        self.send_ctl(&Ctl::Exited {
            route,
            pid: 0,
            status: blastit_frame::ExitStatus::Code(1),
        });
    }

    /// Forks the worker this slot was assigned at boot (§4.3).
    ///
    /// The PHP arm reads the binding the boot stored; the WSGI arm is the
    /// zygote. Neither is reached by asking what backend a request is — the
    /// scheduler sends `SPAWN { route }` and the slot's own data decides.
    fn fork_view(&self, spawn: Spawn<'_>) -> std::io::Result<Forked> {
        let Spawn {
            index,
            info,
            egress,
            dir,
            drop_privileges,
            report,
        } = spawn;
        #[cfg(feature = "php")]
        if let Some(binding) = self.php.get(index).and_then(Option::as_ref) {
            // The same wire the WSGI arm gets, and the same value
            // (the PHP worker's sink is the other half): the worker reports the
            // files each request included, and its first batch is the source's
            // `boot` set.
            // `view-crates-architecture` §4.4 D2: the boot data travels as
            // values. The identity and the filter are the binding's — the two
            // stage 1 decided when it built this slot (`PhpSlot`) — so a fork
            // cannot be given a pair the boot did not verify.
            let forker = |protocol| crate::forking::fork_for(protocol, self.hooks);
            return blastit_view_php::fork_php_view(
                blastit_view_php::PhpViewSpawn {
                    ctx: SpawnContext {
                        route: info.name.clone(),
                        uid: info.uid,
                        frame_limits: self.limits,
                        rlimits: blastit_sys::Rlimits {
                            // `dev` shares one uid with every other process of
                            // the caller, where `RLIMIT_NPROC` would cap the
                            // developer's whole session (`R21`, `ADR-0001`).
                            nproc: drop_privileges.then(|| {
                                self.cfg
                                    .view_worker_cap()
                                    .saturating_add(blastit_view_php::spawn::VIEW_PROCESS_MARGIN)
                                    as u64
                            }),
                            nofile: Some(blastit_view_php::spawn::VIEW_NOFILE),
                            core_zero: true,
                        },
                        privileges: binding.privileges,
                        filter: binding.filter,
                        egress,
                        mask: self.mask,
                    },
                    binding,
                    protocol: if self.hooks.is_some() {
                        ForkProtocol::InterpreterLive
                    } else {
                        ForkProtocol::NoInterpreter
                    },
                    loaded: report,
                },
                &forker,
            )
            .map(Forked::from)
            .map_err(|err| {
                std::io::Error::other(format!("forking the PHP view for {:?}: {err}", info.name))
            });
        }
        #[cfg(feature = "python")]
        {
            // `view-crates-architecture` §4.4 D2: the root maps its own
            // configuration into the view crate's values — this is the one
            // place that knows both crates, so no view crate has to name
            // `Config` or `RouteInfo`.
            //
            // The two per-slot vectors are built by stage 1 for exactly these
            // routes, so a missing entry is a boot bug: the fork is refused
            // rather than inventing an identity stage 1 did not decide.
            let (Some(privileges), Some(filter)) = (
                self.plans.privileges.get(index).copied(),
                self.plans.filter.get(index).copied(),
            ) else {
                return Err(std::io::Error::other(format!(
                    "route {:?} (slot {index}): this boot has no view child plan for the slot, \
                     so no identity or filter was decided for it",
                    info.name
                )));
            };
            let ctx = SpawnContext {
                route: info.name.clone(),
                uid: info.uid,
                frame_limits: self.limits,
                rlimits: blastit_sys::Rlimits {
                    // `dev` shares one uid with every other process of the
                    // caller, where `RLIMIT_NPROC` would cap the developer's
                    // whole session (`R21`, `ADR-0001`).
                    nproc: drop_privileges.then(|| {
                        self.cfg
                            .view_worker_cap()
                            .saturating_add(crate::py_view::VIEW_PROCESS_MARGIN)
                            as u64
                    }),
                    nofile: Some(crate::py_view::VIEW_NOFILE),
                    core_zero: true,
                },
                privileges,
                filter,
                egress,
                mask: self.mask,
            };
            // The handle carries the three fork hooks and the vtable: the
            // shim's `serve_view` is the child's whole entry (§1.2.1).
            let runtime = self.runtime.ok_or_else(|| {
                std::io::Error::other(format!(
                    "route {:?} (slot {index}): ctl.chan asked for a view but this boot \
                     preloaded no interpreter",
                    info.name
                ))
            })?;
            crate::py_view::fork_view(
                runtime,
                crate::py_view::ViewSpawn {
                    ctx,
                    script_name: crate::config::mount_point(self.cfg.script_name.as_deref()),
                    max_threads: self.cfg.view_worker_cap(),
                    dir: dir.to_path_buf(),
                    loaded: report,
                },
            )
            .map(Forked::from)
            .map_err(|err| std::io::Error::other(format!("forking a view: {err}")))
        }
        #[cfg(not(feature = "python"))]
        {
            // A `python`-less build has no WSGI worker to fork. It cannot happen
            // from a boot this build accepts: the routes that would ask for one
            // were refused before a listener was bound
            // ([`crate::stage0::no_python_support`]). Failing the boot keeps it
            // true: a scheduler that asks anyway is one the manifest gate never
            // admitted.
            let _ = (dir, drop_privileges, egress, report);
            Err(std::io::Error::other(format!(
                "route {:?} (slot {index}): ctl.chan asked for a view and this blastit build \
                 has no Python worker runtime",
                info.name
            )))
        }
    }

    /// Tells the scheduler where the new view is, then records the process and
    /// its two log pipes (§3, §4).
    fn record_view(&mut self, index: usize, info: &RouteInfo, child: Forked, source: Option<u16>) {
        let route = index as u32;
        // The scheduler's end of `view.chan` travels next to `SPAWNED` (§3).
        let mut bytes = Vec::new();
        if let Err(err) = self.limits.encode(
            &Frame::Ctl(Ctl::Spawned {
                route,
                pid: child.pid.as_raw() as u32,
            }),
            0,
            &mut bytes,
        ) {
            let _ = kill(child.pid, Signal::KILL);
            self.fail(&format!("encoding SPAWNED: {err}"));
        }
        self.send_spawned(&bytes, child.scheduler_end.as_fd());
        drop(child.scheduler_end);

        self.sources.push(Source {
            fd: child.stdout,
            pipe: Pipe::Stdout {
                component: Component::View,
                route: Some(info.name.clone()),
            },
            lines: LineBuffer::new(),
        });
        self.sources.push(Source {
            fd: child.stderr,
            pipe: Pipe::Stderr {
                component: Component::View,
                route: Some(info.name.clone()),
            },
            lines: LineBuffer::new(),
        });
        self.children.insert(
            child.pid.as_raw(),
            ViewChild {
                route: index,
                uid: info.uid,
                source,
            },
        );

        let event = Event::now(
            Component::Init,
            EventKind::ProcessSpawned(blastit_events::ProcessSpawned::new(
                child.pid.as_raw() as u32,
                info.uid,
                child.fork_ms,
            )),
        )
        .with_route(info.name.clone());
        self.emitter.emit(&event);
    }

    /// `RETIRE`: kill the view the scheduler no longer wants (§2.4).
    ///
    /// Views are retired when the scheduler finds them idle, over their request
    /// cap or damaged. Only a pid init itself forked for *that* route is killed:
    /// a RETIRE naming anything else is a protocol error the supervisor
    /// ignores, because init is the one process that must not be talked into
    /// signalling an arbitrary pid.
    fn retire(&mut self, route: u32, pid: u32) {
        let raw = pid as i32;
        if !self.children.may_retire(route as usize, raw) {
            self.emitter.emit(&Event::now(
                Component::Init,
                EventKind::ManifestWarning(ManifestWarning::new(format!(
                    "RETIRE for pid {pid}, which is not a live view of route {route}; ignored"
                ))),
            ));
            return;
        }
        let Some(pid) = Pid::from_raw(raw) else {
            return;
        };
        if let Err(err) = kill(pid, Signal::KILL) {
            // A view that is already gone is fine: its exit is reported when it
            // is reaped.
            self.emitter.emit(&Event::now(
                Component::Init,
                EventKind::ManifestWarning(ManifestWarning::new(format!(
                    "killing view {pid}: {err}"
                ))),
            ));
        }
    }

    /// Reaps every child that has exited, and reports each one (§4, §8).
    ///
    /// Returns `true` when the sweep reached `ECHILD` — no child is left — which
    /// is what tells the shutdown drain ([`Supervisor::reap_every_child`]) that
    /// there is nothing more to wait for.
    fn reap_children(&mut self) -> bool {
        match reap() {
            Ok(reaped) => {
                self.handle_reaped(reaped.ended);
                reaped.exhausted
            }
            Err(err) => self.fail(&format!("reaping: {err}")),
        }
    }

    /// Reports the children one `waitpid` sweep collected.
    ///
    /// Split out of [`Supervisor::reap_children`] so the last drain of a
    /// shutdown ([`Supervisor::reap_every_child`]) reports an exit it reaped
    /// itself exactly the way the `SIGCHLD` wakeup would.
    fn handle_reaped(&mut self, reaped: Vec<(Pid, ExitStatus)>) {
        for (pid, status) in reaped {
            let raw = pid.as_raw();
            if let Some(component) = self.components.get(&raw).copied() {
                self.components.remove(&raw);
                self.component_exit(component, pid, status);
                continue;
            }
            let Some(view) = self.children.remove(raw) else {
                // An orphan PID 1 inherited: blastit did not fork it, so there
                // is nothing to report.
                continue;
            };
            // The view's report end goes with the process: a closed end is
            // revoked in the same wakeup that reaped it, so the poll set never
            // spins on a dead peer (§4.4). Only the pid that still owns the
            // source may revoke it — a route's replacement end outlives it.
            if let (Some(source), Some(reload)) = (view.source, self.reload.as_mut()) {
                reload.policy().reports().revoke(source);
            }
            self.view_exit(view, pid, status);
        }
    }

    /// Reports a view's exit to the scheduler and to the log (§4).
    fn view_exit(&mut self, view: ViewChild, pid: Pid, status: ExitStatus) {
        let event = Event::now(
            Component::Init,
            EventKind::ProcessExited(ProcessExited::new(
                pid.as_raw() as u32,
                view.uid,
                exit_kind(status),
            )),
        );
        let event = match self.routes.get(view.route) {
            Some(route) => event.with_route(route.name.clone()),
            None => event,
        };
        self.emitter.emit(&event);

        self.send_ctl(&Ctl::Exited {
            route: view.route as u32,
            pid: pid.as_raw() as u32,
            status: frame_status(status),
        });
    }

    /// A component of blastit exited: fatal unless init is shutting down (§8).
    ///
    /// The one exception is the scheduler a reload **cycle** stopped on purpose
    /// (§4.6 step 5): while `reloading` is set its exit is the expected stop,
    /// logged at `info` with no `shutdown` event, and the cycle then re-execs
    /// this image. The flag is cleared in the same wakeup that execs, and a
    /// replacement image starts with it clear, so a **later** scheduler death —
    /// including one in the new image — is fatal again.
    fn component_exit(&mut self, component: Component, pid: Pid, status: ExitStatus) {
        let expected = self.reloading && component == Component::Scheduler;
        let event = Event::now(
            Component::Init,
            EventKind::ProcessExited(ProcessExited::new(
                pid.as_raw() as u32,
                0,
                exit_kind(status),
            )),
        );
        let event = event.with_level(if self.shutting_down || expected {
            Level::Info
        } else {
            Level::Error
        });
        self.emitter.emit(&event);

        if self.shutting_down || expected {
            return;
        }
        self.kill_all();
        self.emitter.emit(&Event::now(
            Component::Init,
            EventKind::Shutdown(Lifecycle::default().with_stage(format!("{component}-died"))),
        ));
        if let Some(dev) = self.dev.take() {
            dev.cleanup();
        }
        crate::coverage::flush();
        // SAFETY: init is exiting; `_exit` runs no handler and cannot deadlock
        // with the GIL held.
        unsafe { libc::_exit(1) }
    }

    /// `SIGTERM` or `SIGINT`: drain, then terminate (§8).
    fn begin_shutdown(&mut self) {
        if self.shutting_down {
            return;
        }
        self.shutting_down = true;
        self.shutdown = Shutdown::Drain(Deadline::after(self.cfg.graceful_timeout));
        self.emitter.emit(&Event::now(
            Component::Init,
            EventKind::Shutdown(Lifecycle::default().with_stage("drain")),
        ));
        self.send_ctl(&Ctl::Drain);
    }

    /// Moves the shutdown along as the deadline passes or children leave.
    fn progress_shutdown(&mut self) {
        match self.shutdown {
            Shutdown::Serving => {}
            Shutdown::Drain(deadline) => {
                let scheduler_gone = !self.components.values().any(|c| *c == Component::Scheduler);
                if scheduler_gone || deadline.expired() {
                    self.shutdown = Shutdown::Terminate(Deadline::after(TERMINATE_GRACE));
                    self.signal_all(Signal::TERM);
                }
            }
            Shutdown::Terminate(deadline) => {
                // A drained topology is owed no signal, but a child that exited
                // between two wakes is still an unreaped zombie here; the final
                // sweep below is what collects it before init exits.
                let drained = self.children.is_empty() && self.components.is_empty();
                if drained || deadline.expired() {
                    if !drained {
                        self.kill_all();
                    }
                    self.reap_every_child(TERMINATE_GRACE);
                    self.finish();
                }
            }
        }
    }

    /// Emits the last event and exits `0` (§8).
    ///
    /// `blastit dev` does the two things a dev boot owes the operator before it
    /// goes: it writes the `--learn` proposals (`R29`) and removes the
    /// ephemeral runtime root (`R20`). The exit code is `dev`'s: `0` clean,
    /// `1` when `--learn` refused a statement or could not write the manifest.
    fn finish(&mut self) -> ! {
        self.emitter.emit(&Event::now(
            Component::Init,
            EventKind::Shutdown(Lifecycle::default().with_stage("stopped")),
        ));
        let code = match self.learn.take() {
            Some(learn) => learn.run.finish(
                &self.cfg.manifest,
                self.cfg.uid_base,
                self.cfg.log_format == crate::config::LogFormat::Pretty,
            ),
            None => 0,
        };
        if let Some(dev) = self.dev.take() {
            dev.cleanup();
        }
        crate::coverage::flush();
        // SAFETY: init is exiting; `_exit` runs no handler.
        unsafe { libc::_exit(code) }
    }

    /// `signal` to every live child.
    fn signal_all(&self, signal: Signal) {
        for raw in self.children.pids().chain(self.components.keys().copied()) {
            if let Some(pid) = Pid::from_raw(raw) {
                let _ = kill(pid, signal);
            }
        }
    }

    /// `SIGKILL` to every live child.
    fn kill_all(&self) {
        self.signal_all(Signal::KILL);
    }

    /// Reaps for at most `budget`, so a killed child is gone before init exits.
    ///
    /// One [`blastit_sys::reap`] sweep per pass until it reports `ECHILD`, not
    /// until the bookkeeping looks empty: a scheduler that closes `ctl.chan` on
    /// its way out drops off `components` before its exit is reaped
    /// ([`Supervisor::ctl_closed`]), so an init that trusted the tables would
    /// `_exit` over a zombie. A container's PID 1 is often a shell or a test
    /// binary that never reaps, and a zombie keeps its `/proc` entry and start
    /// time — that is a "survivor" no liveness check can tell from a live one
    /// (§8). The budget bounds the wait for a child that ignores every signal;
    /// one that still runs past it is init's last resort.
    fn reap_every_child(&mut self, budget: Duration) {
        let deadline = Deadline::after(budget);
        loop {
            if self.reap_children() || deadline.expired() {
                return;
            }
            std::thread::sleep(Duration::from_millis(2));
        }
    }

    /// Writes one `ctl.chan` message; a failure here is fatal unless the
    /// scheduler was already gone when the shutdown began.
    ///
    /// The socket is non-blocking, so a scheduler that stopped reading cannot
    /// wedge init: the write is retried for at most [`SEND_CTL_TIMEOUT`], and a
    /// timeout means the scheduler is gone and the whole container has to go
    /// with it (§8: init must not pretend a dead component is alive).
    ///
    /// The one exception is [`ctl_failure_is_fatal`]'s: a scheduler that closed
    /// the channel during or after `drain` is the closed-fd arm by another
    /// name, and a message nobody can act on must not turn a clean `SIGTERM`
    /// into exit `1` (§1.1, finding F4).
    fn send_ctl(&self, message: &Ctl) {
        let Some(ctl) = self.ctl.as_ref().map(AsFd::as_fd) else {
            // The scheduler is gone; nothing can act on the message any more.
            return;
        };
        let mut bytes = Vec::new();
        if let Err(err) = self
            .limits
            .encode(&Frame::Ctl(message.clone()), 0, &mut bytes)
        {
            self.fail(&format!("encoding a ctl.chan message: {err}"));
        }
        match write_ctl(ctl, &bytes, Deadline::after(SEND_CTL_TIMEOUT)) {
            Ok(()) => {}
            Err(err) if !ctl_failure_is_fatal(self.shutting_down, self.reloading, &err) => {}
            Err(err) => self.ctl_fail(&err),
        }
    }

    /// Sends `SPAWNED`, with the scheduler's end of the new `view.chan`
    /// attached (§3).
    ///
    /// Fatal on any failure, except a scheduler that closed the channel after
    /// `SIGTERM` had begun (§1.1), and waited for the same way as
    /// [`Supervisor::send_ctl`]. A view whose `SPAWNED` never arrives must not
    /// outlive init: the child's `PR_SET_PDEATHSIG` is what makes that true,
    /// and init does not get past this point anyway.
    #[cfg_attr(not(feature = "python"), allow(dead_code))]
    fn send_spawned(&self, bytes: &[u8], fd: BorrowedFd<'_>) {
        let Some(ctl) = self.ctl.as_ref().map(AsFd::as_fd) else {
            self.fail("ctl.chan is closed; SPAWNED cannot be sent");
        };
        let deadline = Deadline::after(SEND_CTL_TIMEOUT);
        loop {
            match send_with_fd(ctl, bytes, fd) {
                Ok(written) if written == bytes.len() => return,
                Ok(_) => self.fail("SPAWNED was written short"),
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    if let Err(wedged) = wait_for_room(ctl, deadline) {
                        self.ctl_fail(&wedged);
                    }
                }
                // A SPAWN that raced the `SIGTERM` still gets its view, which
                // `Terminate` kills; a scheduler that closed the channel cannot
                // take the `SPAWNED` it is owed, and that is not a reason to
                // fail the clean exit the signal asked for (§1.1).
                Err(err) if self.shutting_down && err.raw_os_error().is_some_and(ctl_peer_gone) => {
                    return;
                }
                Err(err) => self.fail(&format!("ctl.chan: {err}")),
            }
        }
    }

    /// Ends init over a failed `ctl.chan` write (§8).
    fn ctl_fail(&self, error: &CtlWriteError) -> ! {
        match error {
            CtlWriteError::Write(err) => self.fail(&format!("ctl.chan: {err}")),
            CtlWriteError::Wedged(reason) => self.fail(&format!("ctl.chan: {reason}")),
        }
    }

    /// Kills live children, removes an adopted `dev` runtime root (`R20`), and
    /// terminates init with a fatal event.
    fn fail(&self, message: &str) -> ! {
        self.kill_all();
        if let Some(dev) = &self.dev {
            dev.cleanup();
        }
        crate::logs::fail(self.emitter, message)
    }
}

/// One `Blastfile` refusal as the editor sentence: `<path>:<line>:<col>: …`.
///
/// The cycle reports the **first** problem only: one event, one sentence, and
/// the previous topology keeps serving (§4.6 step 1). A refusal with no
/// position (a structural read error) still carries the path.
#[cfg(feature = "python")]
fn manifest_sentence(
    path: &std::path::Path,
    error: Option<&blastit_manifest::ManifestError>,
) -> String {
    match error {
        Some(error) => format!("{}: {error}", path.display()),
        None => format!("{}: refused", path.display()),
    }
}

/// The adoption document of §4.6 step 6, UTF-8 JSON and bounded.
///
/// Unknown or missing key in the new image refuses the boot; every descriptor
/// named here was cleared of `FD_CLOEXEC` by [`Supervisor::exec_cycle`]. It is
/// written as one line so a bound of 4096 bytes is a fact about the document
/// a test can check, not a hope.
fn adoption_document(adoption: &Adoption) -> Result<Vec<u8>, String> {
    let component = |fds: &ComponentFds| {
        serde_json::json!({
            "pid": fds.pid,
            "stdout": fds.stdout,
            "stderr": fds.stderr,
            "events": fds.events,
        })
    };
    let mut document = serde_json::json!({
        "version": 1,
        "http": component(&adoption.http),
        "chan": adoption.chan.as_raw_fd(),
    });
    if let Some(dbproxy) = &adoption.dbproxy {
        document["dbproxy"] = component(dbproxy);
    }
    if let Some((obs, ctl)) = adoption.learn {
        document["learn"] = match ctl {
            Some(ctl) => serde_json::json!({ "obs": obs, "ctl": ctl }),
            None => serde_json::json!({ "obs": obs }),
        };
    }
    if let Some(policy) = &adoption.policy {
        document["policy"] = serde_json::json!(policy.as_raw_fd());
    }
    // §1.2.1 item 6: the Python handoff crosses the same barrier, so the next
    // image re-verifies the objects instead of re-opening them by path.
    if let Some((lib, shim)) = &adoption.py_fds {
        document["py_lib"] = serde_json::json!(lib.as_raw_fd());
        document["py_shim"] = serde_json::json!(shim.as_raw_fd());
    }
    serde_json::to_vec(&document).map_err(|err| format!("reload: the adoption document: {err}"))
}

/// The one sentence a cycle owes when it is abandoned (§4.6 step 4).
///
/// It names the route(s) that would not drain, so the operator knows what to
/// look at, and it says the reload did not happen — never a silent drop.
fn abandoned_cycle_warning(wedged: &[String]) -> String {
    let names = match wedged {
        [] => "a view".to_string(),
        routes => routes
            .iter()
            .map(|route| format!("\"{route}\""))
            .collect::<Vec<_>>()
            .join(", "),
    };
    format!(
        "live reload: route {names} kept a view alive for {}s; the reload was abandoned, \
         the previous topology keeps serving — save again to retry",
        CYCLE_GRACE.as_secs()
    )
}

/// The debug-only rendezvous between "the scheduler is stopped" and the
/// `execve` (§4.6 step 5, `T31`).
///
/// `BLASTIT_TEST_RELOAD_PAUSE` names a path; the supervisor creates
/// `<path>.ready` and waits (bounded) for `<path>.go`, so a test can save a
/// tracked file precisely in the no-watcher window. Compiled out of a release
/// build — an environment variable that holds a boot open before it re-execs is
/// a denial-of-service surface in a shipped image, the same reason
/// `BLASTIT_TEST_SWAP_POINT` is debug-only.
#[cfg(debug_assertions)]
fn reload_pause() {
    use std::ffi::OsString;
    let Some(path) = std::env::var_os("BLASTIT_TEST_RELOAD_PAUSE") else {
        return;
    };
    let append = |suffix: &str| {
        let mut name = OsString::from(&path);
        name.push(suffix);
        std::path::PathBuf::from(name)
    };
    let ready = append(".ready");
    let go = append(".go");
    let _ = std::fs::write(&ready, b"ready\n");
    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        if go.exists() {
            return;
        }
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// Boots the one dev-only live-reload watcher (`ADR-0014`, design §4.1).
///
/// Called by stage 1 on a `dev` boot with reload on — before the application's
/// first import on a WSGI boot (so the `preload` phase's files are watched too)
/// and before `ready` on a `php`-only standalone build, which serves its views
/// without an interpreter. Returns `None` — and exactly one
/// `dev.reload.unavailable` — for every row of the failure ladder: there is no
/// retry and no second watcher (§4.7). Production, `check` and a dev boot with
/// reload off never call it.
#[must_use]
pub fn boot_reload(cfg: &Config, emitter: &Emitter, fault: Option<&str>) -> Option<Reload> {
    use std::os::unix::ffi::OsStringExt;

    // The union is absolute-only (§4.4): the app root comes from the `dev` boot
    // and `./Blastfile` is the dev default, so both are made absolute before the
    // policy sees them. A relative root would mark the right inode under a
    // relative name, and every event for a file beside the manifest would then be
    // joined as `./name` and miss the union.
    let root = PathBuf::from(std::ffi::OsString::from_vec(cfg.cwd.clone()?));
    let root = std::path::absolute(&root).ok()?;
    let manifest = std::path::absolute(&cfg.manifest).ok()?;
    let filesystem = blastit_sys::fanotify::filesystem_type(&root)
        .unwrap_or("unknown")
        .to_string();
    // The debug-only fault seam: `BLASTIT_TEST_RELOAD_FAULT=open`
    // makes the boot take the ladder's `EPERM` row, so a critical test can
    // witness "one `dev.reload.unavailable`, then serve" against real
    // processes. Stage 0 read the variable before the environment was stripped
    // and carried it in the configuration document; a release build has
    // neither the name nor the member.
    if fault == Some(FAULT_OPEN) {
        reload_unavailable(
            emitter,
            &dev_reload::Unavailable {
                errno: libc::EPERM,
                filesystem,
            },
        );
        return None;
    }
    let (watcher, poll) = match dev_reload::open() {
        Ok(pair) => pair,
        Err(err) => {
            reload_unavailable(emitter, &dev_reload::Unavailable::of(&err, &filesystem));
            return None;
        }
    };
    let config = ReloadConfig::new(root, manifest, filesystem.clone());
    match Reload::boot(config, Box::new(watcher), poll, Box::new(SystemClock)) {
        Ok(mut reload) => {
            // `ENOSPC` (or the marks cap) boots and keeps the marks it placed,
            // but it owes the ladder's one warning: the boot line must not print
            // `reload: on` over a policy that can never mark again (§4.7, T32).
            if let Some(why) = reload.take_latch() {
                reload_unavailable(emitter, &why);
            }
            Some(reload)
        }
        Err(why) => {
            reload_unavailable(emitter, &why);
            None
        }
    }
}

/// The `preload` phase's report socket: source [`dev_reload::PRELOAD_SOURCE`],
/// registered before the first import (§4.1 step 3).
///
/// Its paths are `pre_fork`: the boot imported them, so the heap every view was
/// forked from is stale when one changes (§4.4).
///
/// # Errors
///
/// The `socketpair` errno, which a normal boot never sees.
#[cfg(feature = "python")]
pub fn open_preload_report(
    reload: &mut Reload,
) -> std::io::Result<std::os::unix::net::UnixDatagram> {
    let (read, write) = dev_reload::report_socket()?;
    reload.policy().reports().register(
        read.into(),
        dev_reload::PRELOAD_SOURCE,
        Class::Loaded,
        true,
    );
    // The **shim** owns the write end now: it adopts this descriptor and sends
    // the `preload` phase's boot batch through it (`boot::report_boot`). This
    // process keeps the read end in the policy above, and nothing else — a
    // second writer would be a second source's batch.
    write.set_nonblocking(true)?;
    Ok(write)
}

/// The one fault the debug-only reload seam injects (§4.6 step 5, `T31`).
#[cfg(debug_assertions)]
pub const FAULT_OPEN: &str = "open";

/// The fault seam in a release build can never match: it has no name.
#[cfg(not(debug_assertions))]
pub const FAULT_OPEN: &str = "\u{0}release-has-no-seam";

/// The live-reload boot line, as an event (§4.7 `reload`).
///
/// One event, emitted exactly once by a boot whose marking pass placed marks:
/// `directories` is how many directories it marked. It is deliberately no
/// longer free text printed beside the stream — a `json` boot carries it on the
/// machine stream, and a `pretty` one puts it in the same columns as every
/// other line, with `1 directory` spelled in the singular.
pub fn announce_reload(emitter: &Emitter, dirs: usize) {
    emitter.emit(&Event::now(
        Component::Init,
        EventKind::Reload(ReloadBoot::new(u32::try_from(dirs).unwrap_or(u32::MAX))),
    ));
}

/// The one `dev.reload.unavailable` the ladder emits (§4.7).
///
/// One event, emitted exactly once: a refused boot, and the `ENOSPC`/`MAX_MARKS`
/// row that stops marking while the marks already placed keep watching. Its
/// pretty sentence names the errno and the filesystem, never a value.
pub(crate) fn reload_unavailable(emitter: &Emitter, why: &dev_reload::Unavailable) {
    emitter.emit(&Event::now(
        Component::Init,
        EventKind::DevReloadUnavailable(DevReloadUnavailable::new(
            why.errno_name(),
            why.filesystem.clone(),
        )),
    ));
}

/// The one mid-run `manifest.warning` of a watcher that latched dead (§4.7).
///
/// Not the boot event: the design separates "serve without reload" from
/// "reload stopped while serving", and the sentence says which errno ended it.
pub(crate) fn reload_stopped(emitter: &Emitter, why: &dev_reload::Unavailable) {
    emitter.emit(&Event::now(
        Component::Init,
        EventKind::ManifestWarning(ManifestWarning::new(format!(
            "live reload stopped: {}; restart blastit dev",
            why.errno_name()
        ))),
    ));
}

/// What reading one source found.
enum SourceRead {
    /// Still open.
    Open,
    /// Reached end-of-file.
    Closed,
}

/// Reads a ready pipe once, turning complete lines into events.
///
/// One `read` per `poll` wakeup: the pipes are blocking, and a second read
/// would wait for a writer that may never come.
fn read_source(
    source: &mut Source,
    emitter: &Emitter,
    learn: Option<&mut crate::dev_learn::Learn>,
) -> SourceRead {
    let mut buffer = [0u8; 16 * 1024];
    let read = loop {
        match rustix::io::read(&source.fd, &mut buffer) {
            Ok(read) => break read,
            Err(rustix::io::Errno::INTR) => continue,
            Err(_) => break 0,
        }
    };
    if read == 0 {
        return SourceRead::Closed;
    }
    let mut lines: Vec<Vec<u8>> = Vec::new();
    source.lines.push(&buffer[..read], &mut lines);
    let mut learn = learn;
    for line in &lines {
        // `dev --learn` needs the requests the scheduler could not route: they
        // are the ones a `[[route]]` is proposed for (`R28`).
        if let Some(learn) = learn.as_deref_mut()
            && source.pipe.is_events()
            && source.pipe.component() == Component::Scheduler
        {
            learn.run.note_event(&lossy(line));
        }
        emit_line(emitter, &source.pipe, line);
    }
    SourceRead::Open
}

/// Logs one line of a pipe's output.
fn emit_line(emitter: &Emitter, pipe: &Pipe, line: &[u8]) {
    let line = lossy(line);
    match pipe.is_events() {
        true => emitter.forward(pipe, &line),
        false => emitter.emit_output(pipe, &line),
    }
}

/// The `poll` flags that mean "there is something to read".
fn readable() -> rustix::event::PollFlags {
    rustix::event::PollFlags::IN | rustix::event::PollFlags::HUP | rustix::event::PollFlags::ERR
}

/// The log-source indices one wakeup may read.
///
/// Only the sources that were **in the poll set** have a ready slot: `ready`
/// carries `polled_sources` of them and the reload's descriptors come after. A
/// `Ctl::Spawn` handled by `read_ctl` in the same wakeup forks a view, which
/// pushes two log pipes onto `self.sources` — but `ready` has no slot for them.
/// Reading them indexes a reload descriptor's flag as if it were a source's and
/// then `read_source`s an empty pipe, and the pipes are blocking: one
/// misalignment parks init in `read(2)` for good instead of costing a wakeup.
/// The live count is still applied, because a source that went away must never
/// be indexed either.
fn polled_source_range(polled_sources: usize, live_sources: usize) -> std::ops::Range<usize> {
    0..polled_sources.min(live_sources)
}

/// The `ready` index where this wakeup's reload descriptors start.
///
/// The reload's descriptors are appended to the poll set after every log source,
/// so their base is the number of sources that were **polled** — never the
/// number init owns after `read_ctl` forked (the same shift, in the other
/// direction).
fn reload_base(base: usize, polled_sources: usize) -> usize {
    base + polled_sources
}

/// A wait status as the `process.exited` event wants it (§8.1).
pub fn exit_kind(status: ExitStatus) -> ExitKind {
    match status {
        ExitStatus::Code(code) => ExitKind::Status(code),
        ExitStatus::Signal(signal) => ExitKind::Signal(signal),
    }
}

/// A wait status as the `EXITED` message wants it (§3).
pub fn frame_status(status: ExitStatus) -> blastit_frame::ExitStatus {
    match status {
        ExitStatus::Code(code) => blastit_frame::ExitStatus::Code(code),
        ExitStatus::Signal(signal) => blastit_frame::ExitStatus::Signal(signal),
    }
}

/// Writes `bytes` to `ctl`, waiting out a full pipe for at most `deadline`.
///
/// `ctl` is non-blocking ([`Supervisor::new`]), so this loops rather than
/// blocking: `EAGAIN` means the scheduler has not read yet, and the deadline is
/// the only thing that ends the wait. `Ok(0)` is "nothing more to write", as it
/// has always been. Whether a returned error ends the boot is
/// [`ctl_failure_is_fatal`]'s decision, not this function's.
fn write_ctl(fd: BorrowedFd<'_>, bytes: &[u8], deadline: Deadline) -> Result<(), CtlWriteError> {
    let mut written = 0;
    while written < bytes.len() {
        match rustix::io::write(fd, &bytes[written..]) {
            Ok(0) => return Ok(()),
            Ok(count) => written += count,
            Err(rustix::io::Errno::INTR) => {}
            Err(rustix::io::Errno::AGAIN) => wait_for_room(fd, deadline)?,
            Err(err) => return Err(CtlWriteError::Write(err)),
        }
    }
    Ok(())
}

/// The one warning a save whose document changes the `(route, proxy)` set owes
/// (§4.8: "adding or removing a store emits one `manifest.warning` naming the
/// restart"), or `None` when the set is unchanged.
///
/// The sockets are bound at boot and never re-bound, so a pair the new file adds
/// has no listener to reach it, and a pair it removes leaves a listener whose
/// grants are now empty. The document is still sent afterwards: an entry that
/// exists is applied and an entry that does not is never widened, so this
/// warning never stands in for the refresh and nothing fails open.
#[cfg_attr(not(feature = "python"), allow(dead_code))]
fn store_set_warning(
    boot_pairs: &BTreeSet<(String, String)>,
    manifest: &blastit_manifest::Manifest,
) -> Option<String> {
    let next: BTreeSet<(String, String)> = policy_pairs(manifest).into_iter().collect();
    if next == *boot_pairs {
        return None;
    }
    let added: Vec<String> = next
        .difference(boot_pairs)
        .map(|(route, proxy)| format!("{route}→{proxy}"))
        .collect();
    let removed: Vec<String> = boot_pairs
        .difference(&next)
        .map(|(route, proxy)| format!("{route}→{proxy}"))
        .collect();
    let mut parts: Vec<String> = Vec::new();
    if !added.is_empty() {
        parts.push(format!("added {}", added.join(", ")));
    }
    if !removed.is_empty() {
        parts.push(format!("removed {}", removed.join(", ")));
    }
    Some(format!(
        "the database listener set changed ({}); every socket was bound at boot, so the \
         existing listeners keep serving and the new grants apply only to them — restart \
         blastit dev to bind the new set",
        parts.join("; ")
    ))
}

/// Every `postgresql` proxy the manifest declares, in name order.
fn postgres_proxies(manifest: &blastit_manifest::Manifest) -> Vec<&blastit_manifest::ProxyName> {
    manifest
        .proxies()
        .values()
        .filter(|proxy| proxy.protocol() == blastit_manifest::Protocol::Postgresql)
        .map(blastit_manifest::Proxy::name)
        .collect()
}

/// The `(route, proxy)` pairs `manifest` declares or grants — exactly the set
/// [`policy_document`] carries, and therefore the set stage 1 records as the
/// boot's listeners (`§4.8`).
///
/// A pair exists when the route's effective grants hold it (an empty grant
/// included) or when the route — or an attached group — names the proxy, which
/// is [`crate::stage0::route_declares_proxy`], the same predicate stage 0 bound
/// its sockets with.
#[cfg_attr(not(feature = "python"), allow(dead_code))]
pub(crate) fn policy_pairs(manifest: &blastit_manifest::Manifest) -> Vec<(String, String)> {
    let proxies = postgres_proxies(manifest);
    let mut pairs = Vec::new();
    for (index, route) in manifest.routes().iter().enumerate() {
        let grants = manifest.effective_grants(index);
        for proxy in proxies.iter().copied() {
            if grants
                .get(proxy)
                .and_then(|grant| grant.as_postgresql())
                .is_none()
                && !crate::stage0::route_declares_proxy(manifest, index, proxy)
            {
                continue;
            }
            pairs.push((route.name().to_string(), proxy.as_str().to_string()));
        }
    }
    pairs
}

/// The dev-only policy document for `manifest` (§4.6 step 2, §4.8).
///
/// `u32 length | JSON`, both little-endian, `{"version":1,"route_policies":[…]}`;
/// one entry per `(route, proxy)` the new manifest **declares or grants** —
/// [`crate::stage0::route_declares_proxy`], the same predicate that bound the
/// sockets at boot — with the crate's own member names and neither `fd` nor
/// `uid` (the boot's identity wins and the socket is never re-bound). It is
/// authoritative: a pair the document does not name evaluates against the empty
/// allowlist, so a route that dropped its block, or a save that removed a
/// grant, can never fail open.
///
/// # Errors
///
/// A one-line reason when the document cannot be encoded or exceeds
/// [`blastit_dbproxy::MAX_POLICY_DOCUMENT`]; it never contains a literal, a
/// parameter or a credential (its members are the manifest's own grant text).
#[cfg(feature = "python")]
fn policy_document(manifest: &Manifest) -> Result<Vec<u8>, String> {
    let proxies = postgres_proxies(manifest);
    let mut policies: Vec<serde_json::Value> = Vec::new();
    for (index, route) in manifest.routes().iter().enumerate() {
        let grants = manifest.effective_grants(index);
        for proxy in proxies.iter().copied() {
            let grant = grants.get(proxy).and_then(|grant| grant.as_postgresql());
            let policy = match grant {
                Some(grant) => serde_json::json!({
                    "route": route.name(),
                    "proxy": proxy.as_str(),
                    "queries": grant.queries(),
                    "transaction_control": grant.transaction_control(),
                    "unrestricted": grant.is_unrestricted(),
                    "tables": grant.tables(),
                }),
                // A route that names the proxy with an empty grant keeps its
                // listener and allows nothing; a route that names nothing at all
                // gets no entry, and an omitted pair denies.
                None if crate::stage0::route_declares_proxy(manifest, index, proxy) => {
                    serde_json::json!({"route": route.name(), "proxy": proxy.as_str()})
                }
                None => continue,
            };
            policies.push(policy);
        }
    }
    let document = serde_json::json!({"version": 1, "route_policies": policies});
    let body =
        serde_json::to_vec(&document).map_err(|err| format!("the policy document: {err}"))?;
    if body.len() > blastit_dbproxy::MAX_POLICY_DOCUMENT {
        return Err(format!(
            "the policy document is {} bytes, more than the {}-byte limit",
            body.len(),
            blastit_dbproxy::MAX_POLICY_DOCUMENT
        ));
    }
    let length = u32::try_from(body.len())
        .map_err(|_| "the policy document is too large to frame".to_string())?;
    let mut framed = Vec::with_capacity(4 + body.len());
    framed.extend_from_slice(&length.to_le_bytes());
    framed.extend_from_slice(&body);
    Ok(framed)
}

/// What one policy exchange did (§4.6 step 2).
///
/// The distinction the design needs is between "the peer answered, and the answer
/// is a refusal" (the stream is in step, the previous grants are in force, a
/// later save may try again) and "the answer cannot be trusted" (a byte was left
/// on the socket, the peer went away, or nothing arrived) — after which no later
/// save may report a swap it did not get.
#[cfg(feature = "python")]
enum PolicyOutcome {
    /// `0x00`: the whole map was replaced.
    Swapped,
    /// `0x01`: the peer refused the document whole, in step.
    Refused(String),
    /// The exchange left the channel in an unknown state.
    Broken(String),
}

/// Writes one already-framed policy document and waits for dbproxy's one-byte
/// ack, both inside `deadline` (§4.6 step 2).
///
/// Non-blocking on a non-blocking socket: a short write waits for writability
/// and the ack for readability, so a dbproxy that stopped reading or answering
/// costs at most the deadline and never a wedged supervisor.
///
/// **Alignment first.** Before writing, a readable byte is an ack nobody waited
/// for — the previous save timed out while dbproxy was still working, so that
/// byte belongs to the *previous* document and cannot answer this one. It is
/// consumed (the stream is then back in step) and reported as [`PolicyOutcome::
/// Broken`], so the save warns instead of reporting a success it did not get. A
/// timeout or a broken read leaves the channel untrusted for the same reason:
/// dbproxy may still be applying the document whose ack never arrived, and the
/// next byte would be attributed to the wrong document.
///
/// # Errors
///
/// A one-line cause naming the stage. It never carries a byte of the document.
#[cfg(feature = "python")]
fn exchange_policy(fd: BorrowedFd<'_>, document: &[u8], deadline: Deadline) -> PolicyOutcome {
    let millis = POLICY_ACK_TIMEOUT.as_millis();
    // The stray-ack check: one non-blocking read before anything is written.
    let mut stray = [0u8; 1];
    match rustix::io::read(fd, &mut stray) {
        Ok(0) => {
            return PolicyOutcome::Broken(
                "dbproxy closed the policy channel before this save".to_string(),
            );
        }
        Ok(_read) => {
            return PolicyOutcome::Broken(format!(
                "an earlier save's policy answer (0x{:02x}) was still unread, so it cannot be \
                 attributed to this document",
                stray[0]
            ));
        }
        Err(rustix::io::Errno::AGAIN) | Err(rustix::io::Errno::INTR) => {}
        Err(err) => {
            return PolicyOutcome::Broken(format!("reading the policy channel: {err}"));
        }
    }
    let mut written = 0;
    while written < document.len() {
        match rustix::io::write(fd, &document[written..]) {
            Ok(0) => {
                return PolicyOutcome::Broken("the policy socket accepted no bytes".to_string());
            }
            Ok(count) => written += count,
            Err(rustix::io::Errno::INTR) => {}
            Err(rustix::io::Errno::AGAIN) => {
                let room =
                    !deadline.expired() && matches!(wait_writable(fd, deadline.left()), Ok(true));
                if !room {
                    return PolicyOutcome::Broken(format!(
                        "the document could not be written within {millis} ms"
                    ));
                }
            }
            Err(err) => {
                return PolicyOutcome::Broken(format!("writing the policy document: {err}"));
            }
        }
    }
    let mut ack = [0u8; 1];
    loop {
        match rustix::io::read(fd, &mut ack) {
            Ok(0) => {
                return PolicyOutcome::Broken(
                    "dbproxy closed the policy channel before acknowledging".to_string(),
                );
            }
            Ok(_read) => break,
            Err(rustix::io::Errno::INTR) => {}
            Err(rustix::io::Errno::AGAIN) => {
                let ready =
                    !deadline.expired() && matches!(wait_readable(fd, deadline.left()), Ok(true));
                if !ready {
                    return PolicyOutcome::Broken(format!(
                        "dbproxy did not acknowledge within {millis} ms"
                    ));
                }
            }
            Err(err) => return PolicyOutcome::Broken(format!("reading the policy ack: {err}")),
        }
    }
    match ack[0] {
        blastit_dbproxy::ACK_SWAPPED => PolicyOutcome::Swapped,
        blastit_dbproxy::ACK_REFUSED => PolicyOutcome::Refused(
            "dbproxy refused the document, so the previous grants are still in force".to_string(),
        ),
        other => PolicyOutcome::Broken(format!(
            "dbproxy answered the policy document with the unknown byte {other:#04x}"
        )),
    }
}

/// Waits until `fd` takes a write, or reports the scheduler wedged.
///
/// A scheduler that stopped reading is a dead scheduler: waiting for it forever
/// would wedge init, and init is the one process in the container that must
/// never wedge (§8). Shared by both writers on `ctl.chan`.
fn wait_for_room(fd: BorrowedFd<'_>, deadline: Deadline) -> Result<(), CtlWriteError> {
    if deadline.expired() {
        return Err(CtlWriteError::Wedged(
            "the scheduler stopped reading; treating it as dead".to_string(),
        ));
    }
    match wait_writable(fd, deadline.left()) {
        Ok(_) => Ok(()),
        Err(err) => Err(CtlWriteError::Wedged(err.to_string())),
    }
}

/// Whether a failed `ctl.chan` write ends the boot.
///
/// The order of the two rules is the whole point:
///
/// * Once `SIGTERM`/`SIGINT` has begun, the scheduler may be gone already: a
///   view reaped during the drain still owes it an `EXITED`, and a `SPAWN` that
///   raced the signal still owes it a `SPAWNED`. Nothing can act on either
///   message any more, so a peer that closed the channel is the closed-fd arm by
///   another name (`self.ctl == None`) — best effort, and never a reason to turn
///   a clean shutdown into exit `1` (§1.1, finding F4).
/// * Everything else stays fatal, exactly as before. Before shutdown a gone
///   peer *is* a dead scheduler init must not pretend is alive, and a scheduler
///   that took nothing for [`SEND_CTL_TIMEOUT`] is treated as dead whenever it
///   happens: that budget is what keeps init from wedging with it (§8).
fn ctl_failure_is_fatal(shutting_down: bool, reloading: bool, error: &CtlWriteError) -> bool {
    !((shutting_down || reloading) && error.peer_gone())
}

/// Whether `raw` errno says the scheduler's end of `ctl.chan` is gone.
///
/// `EPIPE` is what a closed peer gives the next write; `ECONNRESET` is what a
/// socket the peer tore down gives, on a write *and* on a read: a scheduler
/// that exits while messages are still queued for it makes the next read reset
/// instead of returning end-of-file. Both mean the same thing to init: nothing
/// can read this channel or write to it any more.
fn ctl_peer_gone(raw: i32) -> bool {
    raw == libc::EPIPE || raw == libc::ECONNRESET
}

/// Whether a failed `ctl.chan` read says the scheduler's end is gone.
///
/// The reset case is the one that does not look like end-of-file, so it is
/// named here rather than folded into the fatal arm of [`Supervisor::read_ctl`].
fn ctl_read_lost_peer(err: &std::io::Error) -> bool {
    err.raw_os_error().is_some_and(ctl_peer_gone)
}
