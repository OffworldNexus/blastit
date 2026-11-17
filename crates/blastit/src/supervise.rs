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
//!   scheduler's end of `view.chan` by `SCM_RIGHTS` ([`crate::zygote`]).
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

use std::collections::{BTreeMap, HashMap};
use std::os::fd::{AsFd, AsRawFd, BorrowedFd, OwnedFd};
#[cfg(feature = "python")]
use std::path::PathBuf;
use std::time::{Duration, Instant};

use blastit_events::{
    Component, DevReload, DevReloadUnavailable, Event, EventKind, ExitKind, Level, Lifecycle,
    ManifestWarning, ProcessExited,
};
use blastit_frame::{Ctl, Decoder, Frame, Limits};
use blastit_scheduler::RouteInfo;
use blastit_sys::{
    ExitStatus, Pid, Signal, SignalFd, SignalMask, kill, reap, recv_with_fd, send_with_fd,
};
#[cfg(feature = "python")]
use pyo3::{Py, PyAny};

use crate::child::{Deadline, wait_writable};
use crate::config::Config;
use crate::dev_reload::{self, Effect, Reload};
#[cfg(feature = "python")]
use crate::dev_reload::{Class, ReloadConfig, SystemClock};
use crate::logs::{Emitter, LineBuffer, Pipe, lossy};
use crate::py::Python;
#[cfg(feature = "python")]
use crate::zygote;

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
    #[cfg(feature = "python")]
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
/// A `python`-less build forks no WSGI view at all, so the type is uninhabited
/// there and the parameter costs nothing.
#[cfg(feature = "python")]
type ViewReport = Option<blastit_view::LoadedReport>;
/// See the `python` half: a build with no view runtime has no report.
#[cfg(not(feature = "python"))]
type ViewReport = Option<std::convert::Infallible>;

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
impl From<zygote::ForkedView> for Forked {
    fn from(view: zygote::ForkedView) -> Self {
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
impl From<crate::php_view::ForkedPhpView> for Forked {
    fn from(view: crate::php_view::ForkedPhpView) -> Self {
        Self {
            pid: view.pid,
            scheduler_end: view.scheduler_end,
            stdout: view.stdout,
            stderr: view.stderr,
            fork_ms: view.fork_ms,
        }
    }
}

/// Whether CPython is live in this image and the caller holds the GIL — the
/// `interpreter_live` [`crate::forking::fork`] documents.
///
/// init holds the GIL for its whole loop (`Supervisor::run`), and a build with
/// `python` off has no interpreter in any role, so this is exactly the
/// installation question: a wheel install forks under CPython's protocol and
/// the standalone binary has nothing to stop (§9.3).
#[cfg(all(feature = "php", feature = "python"))]
fn interpreter_live(_py: Python<'_>) -> bool {
    true
}

/// The token of a build that carries no interpreter: there is nothing to stop.
#[cfg(all(feature = "php", not(feature = "python")))]
fn interpreter_live(_py: Python<'_>) -> bool {
    false
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

/// What stage 1 hands the supervisor.
pub struct Parts {
    /// The configuration, already parsed.
    pub cfg: Config,
    /// Frame limits of every channel.
    pub limits: Limits,
    /// One entry per manifest route, in routing order.
    pub routes: Vec<RouteInfo>,
    /// One entry per route slot, in routing order: the embedded-PHP worker this
    /// slot forks, or `None` for a WSGI slot (`ADR-0010` §1, design §4.3).
    ///
    /// Built and verified at boot by [`crate::php_view::boot_bindings`], so the
    /// `SPAWN` path is a lookup and never a question about the backend kind.
    #[cfg(feature = "php")]
    pub php: Vec<Option<crate::php_view::PhpBinding>>,
    /// One entry per manifest route, in routing order: whether its view may
    /// reach the network (§7.3).
    pub egress: Vec<bool>,
    /// The imported WSGI callable. Only a build with an interpreter can import
    /// one, and only a WSGI view can be forked from it (`MB.3.11`).
    #[cfg(feature = "python")]
    pub app: Py<PyAny>,
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
    /// boot with reload off.
    pub reload: Option<Reload>,
}

/// The supervisor of all of blastit's processes.
pub struct Supervisor<'a> {
    cfg: Config,
    limits: Limits,
    routes: Vec<RouteInfo>,
    /// One entry per route slot: the PHP worker it forks, decided at boot
    /// (`ADR-0010` §1). Empty slots are WSGI views.
    #[cfg(feature = "php")]
    php: Vec<Option<crate::php_view::PhpBinding>>,
    /// Whether each route's view may reach the network: only the view fork
    /// reads it, and only a build with an interpreter forks one (`MB.3.11`).
    #[cfg_attr(not(feature = "python"), allow(dead_code))]
    egress: Vec<bool>,
    /// The imported WSGI callable, in a build that has an interpreter.
    #[cfg(feature = "python")]
    app: Py<PyAny>,
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
    /// `--no-reload`) costs the poll loop nothing.
    reload: Option<Reload>,
}

impl<'a> Supervisor<'a> {
    /// Builds the supervisor and registers every pipe it must drain.
    pub fn new(parts: Parts, emitter: &'a Emitter) -> Self {
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
            #[cfg(feature = "python")]
            app: parts.app,
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
        }
    }

    /// Runs until the process is asked to stop, then exits (§8).
    pub fn run(&mut self, py: Python<'_>) -> ! {
        loop {
            self.poll_once(py);
            self.reap_children();
            self.progress_shutdown();
        }
    }

    /// One `poll` and everything it found ready.
    fn poll_once(&mut self, py: Python<'_>) {
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
            // polled, not after the ones init now owns (a same-wakeup `SPAWN`
            // used to shift them, so report datagrams were never drained).
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
            self.read_ctl(py);
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
    ///
    /// Nothing here stops the scheduler: the reload **cycle** (§4.6 step 5) is
    /// `MB.23.5.2.2`'s, and until it lands no reload can excuse a scheduler
    /// exit. A gate opened for a cycle this leaf could not complete would stay
    /// set for the boot's life and swallow the next genuine `scheduler-died`.
    fn reload_all(&mut self, fire: &dev_reload::Fire) {
        let routes: Vec<String> = self.routes.iter().map(|route| route.name.clone()).collect();
        let files: Vec<String> = fire
            .files
            .iter()
            .map(|path| path.to_string_lossy().into_owned())
            .collect();
        self.emitter.emit(&Event::now(
            Component::Init,
            EventKind::DevReload(DevReload::new(routes, files, fire.reason)),
        ));
        self.send_ctl(&Ctl::Reload);
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
    fn read_ctl(&mut self, py: Python<'_>) {
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
                Ok(Some((_id, frame))) => self.on_ctl(py, frame),
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
    /// everyone else. At any other time it means the scheduler died without
    /// being reaped, which is fatal (§8).
    fn ctl_closed(&mut self) {
        self.ctl = None;
        // The scheduler is never expected to close its channel in this leaf: the
        // reload **cycle** that stops it on purpose is `MB.23.5.2.2`'s, so any
        // close outside a shutdown is a scheduler that died (§8).
        if !self.shutting_down {
            self.fail("ctl.chan: the scheduler closed the channel");
        }
        let scheduler = self
            .components
            .iter()
            .find(|(_, component)| **component == Component::Scheduler)
            .map(|(pid, _)| *pid);
        if let Some(pid) = scheduler {
            self.components.remove(&pid);
        }
    }

    /// Handles one `ctl.chan` message from the scheduler.
    fn on_ctl(&mut self, py: Python<'_>, frame: Frame) {
        let message = match frame {
            Frame::Ctl(message) => message,
            other => self.fail(&format!(
                "ctl.chan: unexpected {other:?} from the scheduler"
            )),
        };
        match message {
            Ctl::Spawn { route } => self.spawn(py, route),
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
    fn spawn(&mut self, py: Python<'_>, route: u32) {
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
        self.spawn_now(py, index, &info);
    }

    /// Forks the view for `index`, hands its channel over, and records it.
    ///
    /// Which runtime to fork is **boot data**, not a question asked here: a
    /// slot with a [`crate::php_view::PhpBinding`] forks the embedded-PHP worker
    /// (`ADR-0010` §1) and every other slot the WSGI zygote. The binding was
    /// verified at boot (`st_dev`/`st_ino` against stage 0's record), so a
    /// `SPAWN` can only ever start the runtime this boot already admitted.
    fn spawn_now(&mut self, py: Python<'_>, index: usize, info: &RouteInfo) {
        let egress = self.egress.get(index).copied().unwrap_or(false);
        // Production gives each route its own uid and `<scratch>/views/<uid>`;
        // `dev` gives every view the caller's uid and names the directory by
        // route index instead (`R19`, `R20`).
        let (dir, drop_privileges) = match &self.dev {
            Some(dev) => (dev.view_dir(index), false),
            None => (crate::config::view_dir(info.uid), true),
        };
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
        let child = match self.fork_view(
            py,
            Spawn {
                index,
                info,
                egress,
                dir: &dir,
                drop_privileges,
                report,
            },
        ) {
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
    /// `None` is every production boot, `check`, a dev boot with reload off, and
    /// a PHP slot (`php_view.rs` carries no report yet — `MB.23.5.3`). A view
    /// whose slot has no report is a view that never triggers a reload, which is
    /// exactly what it did before `MB.23.5`.
    fn open_report(&mut self, index: usize) -> std::io::Result<(ViewReport, Option<u16>)> {
        #[cfg(feature = "python")]
        {
            #[cfg(feature = "php")]
            let php_slot = self.php.get(index).is_some_and(Option::is_some);
            #[cfg(not(feature = "php"))]
            let php_slot = false;
            let Some(source) = u16::try_from(index).ok().filter(|_| !php_slot) else {
                return Ok((None, None));
            };
            let Some(reload) = self.reload.as_mut() else {
                return Ok((None, None));
            };
            let (read, write) = dev_reload::report_socket()?;
            // A route can fork a fresh view while a stale busy one still serves:
            // the table keys the source, so the newest end wins and the stale
            // pid must not revoke it later.
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
        // A `python`-less build forks no WSGI view, so it has no report to open.
        #[cfg(not(feature = "python"))]
        {
            let _ = index;
            Ok((None, None))
        }
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
    fn fork_view(&self, py: Python<'_>, spawn: Spawn<'_>) -> std::io::Result<Forked> {
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
            // `php_view.rs` carries no report socket yet (`MB.23.5.3`); the slot
            // has none, so there is nothing to pass and nothing to drop.
            let _ = report;
            return crate::php_view::fork_php_view(
                crate::php_view::PhpViewSpawn {
                    cfg: &self.cfg,
                    limits: self.limits,
                    info,
                    binding,
                    mask: self.mask,
                    egress,
                    // The binding's directory is the one its INI pins as
                    // `sys_temp_dir`, so the fork cannot disagree with it.
                    dir: binding.view_tmp.clone(),
                    drop_privileges,
                },
                interpreter_live(py),
            )
            .map(Forked::from)
            .map_err(|err| {
                std::io::Error::other(format!("forking the PHP view for {:?}: {err}", info.name))
            });
        }
        #[cfg(feature = "python")]
        {
            zygote::fork_view(
                py,
                zygote::ViewSpawn {
                    cfg: &self.cfg,
                    limits: self.limits,
                    info,
                    app: &self.app,
                    mask: self.mask,
                    egress,
                    dir: dir.to_path_buf(),
                    drop_privileges,
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
            let _ = (py, dir, drop_privileges, egress, report);
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
    fn reap_children(&mut self) {
        let reaped = match reap() {
            Ok(reaped) => reaped,
            Err(err) => self.fail(&format!("reaping: {err}")),
        };
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
    /// No exception is carved out here: the reload **cycle** that stops the
    /// scheduler on purpose is `MB.23.5.2.2`'s, and until it lands every
    /// component exit — the scheduler's included — is fatal. A reload of any
    /// kind must not turn a later genuine `scheduler-died` into a wedged boot.
    fn component_exit(&mut self, component: Component, pid: Pid, status: ExitStatus) {
        let event = Event::now(
            Component::Init,
            EventKind::ProcessExited(ProcessExited::new(
                pid.as_raw() as u32,
                0,
                exit_kind(status),
            )),
        );
        let event = event.with_level(if self.shutting_down {
            Level::Info
        } else {
            Level::Error
        });
        self.emitter.emit(&event);

        if self.shutting_down {
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
                if self.children.is_empty() && self.components.is_empty() {
                    self.finish();
                }
                if deadline.expired() {
                    self.kill_all();
                    self.reap_until(TERMINATE_GRACE);
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
    fn reap_until(&mut self, budget: Duration) {
        let deadline = Deadline::after(budget);
        while !deadline.expired() {
            self.reap_children();
            if self.children.is_empty() && self.components.is_empty() {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
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
            Err(err) if !ctl_failure_is_fatal(self.shutting_down, &err) => {}
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

/// Boots the one dev-only live-reload watcher (`ADR-0014`, design §4.1).
///
/// Called by stage 1 **before** the application's first import, so the
/// preload phase's files are watched too. Returns `None` — and exactly one
/// `dev.reload.unavailable` — for every row of the failure ladder: there is no
/// retry and no second watcher (§4.7). Production, `check` and a dev boot with
/// reload off never call it.
#[cfg(feature = "python")]
#[must_use]
pub fn boot_reload(cfg: &Config, emitter: &Emitter) -> Option<Reload> {
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
pub fn open_preload_report(reload: &mut Reload) -> std::io::Result<blastit_view::Reporter> {
    let (read, write) = dev_reload::report_socket()?;
    let reporter = blastit_view::Reporter::new(write, dev_reload::PRELOAD_SOURCE)?;
    reload.policy().reports().register(
        read.into(),
        dev_reload::PRELOAD_SOURCE,
        Class::Loaded,
        true,
    );
    Ok(reporter)
}

/// The boot line: `reload: on (N directories)`, pretty output only (§4.7).
///
/// It is deliberately **not** an event: the machine contract stays at the two
/// events of §4.9. `N` is the number of directories the boot marked.
#[cfg(feature = "python")]
pub fn announce_reload(format: crate::config::LogFormat, dirs: usize) {
    if format == crate::config::LogFormat::Pretty {
        crate::logs::write_line(
            libc::STDOUT_FILENO,
            &format!("reload: on ({dirs} directories)"),
        );
    }
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
fn ctl_failure_is_fatal(shutting_down: bool, error: &CtlWriteError) -> bool {
    !(shutting_down && error.peer_gone())
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

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use std::os::unix::net::UnixStream;

    use super::*;

    /// A wakeup must read exactly the log sources its poll set carried.
    ///
    /// The shape that wedged `make check`: the poll set carried four log
    /// sources (slots `2..6`, after the signalfd and `ctl.chan`), a watcher and
    /// one report end (`6`, `7`); a `Ctl::Spawn` in the same wakeup forked a
    /// view, so `self.sources` grew to six. Reading six sources made source
    /// index 4 read the watcher's flag and index 5 a report end's, and then
    /// `read_source`ed the forked view's empty, blocking pipe — a permanent
    /// wedge, not a lost wakeup.
    #[test]
    fn a_wakeup_reads_only_the_sources_its_poll_set_carried() {
        assert_eq!(
            polled_source_range(4, 6),
            0..4,
            "the two pipes a same-wakeup SPAWN added have no ready slot, and \
             reading them indexes the reload descriptors instead"
        );
        assert_eq!(
            reload_base(2, 4),
            6,
            "the reload descriptors sit after the sources that were polled, not \
             after the ones init owns now"
        );
        // A source that went away is never indexed either.
        assert_eq!(polled_source_range(4, 2), 0..2);
        // The ordinary wakeup: nothing changed, so every polled source is read.
        assert_eq!(polled_source_range(4, 4), 0..4);
        assert_eq!(polled_source_range(0, 0), 0..0, "a boot with no source yet");
    }

    /// Finding F4: a view reaped after `DRAIN` still owes the scheduler an
    /// `EXITED`, and once the scheduler has gone that write can only fail. A
    /// closed `ctl.chan` must behave like the gone-fd arm rather than ending a
    /// clean shutdown with exit `1`.
    ///
    /// Deterministic on purpose: the peer is closed before the write, so there
    /// is no scheduling race and no sleep to tune.
    #[test]
    fn a_late_ctl_write_to_a_gone_scheduler_is_best_effort() {
        let (init_end, scheduler_end) = UnixStream::pair().expect("a socketpair");
        let deadline = Deadline::after(SEND_CTL_TIMEOUT);

        // A live scheduler still takes the message in full.
        assert_eq!(
            write_ctl(init_end.as_fd(), b"EXITED", deadline),
            Ok(()),
            "a live ctl.chan must accept a message"
        );

        // The scheduler exits first, as it does between `DRAIN` and the last
        // `EXITED` of the drain.
        drop(scheduler_end);
        let error = write_ctl(init_end.as_fd(), b"EXITED", deadline)
            .expect_err("a closed peer cannot take the message");
        assert!(error.peer_gone(), "{error:?}");

        assert!(
            !ctl_failure_is_fatal(true, &error),
            "a late ctl.chan write during the drain must be best effort, \
             or a clean SIGTERM exits 1 (finding F4)"
        );
        assert!(
            ctl_failure_is_fatal(false, &error),
            "the same write before shutdown is a dead scheduler init must not \
             pretend is alive (§8)"
        );
    }

    /// The best-effort arm is exactly a gone peer during shutdown: every other
    /// failure keeps the exit code it always had.
    #[test]
    fn every_other_ctl_failure_stays_fatal() {
        // A scheduler that stops reading for `SEND_CTL_TIMEOUT` is dead, drain
        // or not: that budget is what keeps init from wedging with it (§8).
        let wedged = CtlWriteError::Wedged("the scheduler stopped reading".to_string());
        assert!(!wedged.peer_gone());
        assert!(ctl_failure_is_fatal(false, &wedged));
        assert!(ctl_failure_is_fatal(true, &wedged));

        // A write error that is not a gone peer is not swallowed either.
        let failed = CtlWriteError::Write(rustix::io::Errno::BADF);
        assert!(!failed.peer_gone());
        assert!(ctl_failure_is_fatal(false, &failed));
        assert!(ctl_failure_is_fatal(true, &failed));
    }

    /// The half of finding F4 that does not look like end-of-file: a scheduler
    /// that exits while `DRAIN` (or anything else) is still queued for it makes
    /// init's next *read* of `ctl.chan` reset instead of returning `0`.
    ///
    /// Deterministic: the peer is closed after the message is queued, so the
    /// kernel produces the reset with no timing window at all.
    #[test]
    fn a_scheduler_that_closes_with_data_queued_resets_the_channel() {
        let (init_end, scheduler_end) = UnixStream::pair().expect("a socketpair");
        let deadline = Deadline::after(SEND_CTL_TIMEOUT);

        // init wrote a `DRAIN`; the scheduler exited without reading it.
        assert_eq!(
            write_ctl(init_end.as_fd(), b"DRAIN", deadline),
            Ok(()),
            "the message reaches the scheduler's queue"
        );
        drop(scheduler_end);

        let mut buffer = [0u8; 64];
        let read = loop {
            match rustix::io::read(init_end.as_fd(), &mut buffer) {
                Err(rustix::io::Errno::INTR) => continue,
                other => break other,
            }
        };
        let err = read.expect_err("a reset is an error, not end-of-file and not data");
        assert_eq!(err.raw_os_error(), libc::ECONNRESET, "{err}");
        assert!(
            ctl_read_lost_peer(&std::io::Error::from(err)),
            "a reset means the scheduler's end is gone, so `read_ctl` must go \
             through `ctl_closed` (fatal while serving, the end of a drain) \
             instead of failing the drain with exit 1"
        );
        // A read error that is not a lost peer stays fatal.
        assert!(!ctl_read_lost_peer(&std::io::Error::from_raw_os_error(
            libc::EINVAL
        )));
    }

    /// A child record carries the route init forked it for, so `EXITED` names
    /// that route and never the one a scheduler message happened to mention
    /// (`ADR-0015` decision 6, R7).
    #[test]
    fn exited_names_the_childs_own_route() {
        let mut children = ChildTable::default();
        children.insert(
            41,
            ViewChild {
                route: 0,
                uid: 21_000,
                source: None,
            },
        );
        children.insert(
            42,
            ViewChild {
                route: 1,
                uid: 21_001,
                source: None,
            },
        );
        children.insert(
            43,
            ViewChild {
                route: 1,
                uid: 21_001,
                source: None,
            },
        );

        // Two workers of one route, told apart by pid alone.
        assert_eq!(children.route_of(42), Some(1));
        assert_eq!(children.route_of(43), Some(1));

        let reaped = children
            .remove(42)
            .expect("a reaped child gives back its record");
        assert_eq!(reaped.route, 1, "EXITED must carry the child's own route");
        assert_eq!(children.route_of(42), None);
        assert_eq!(children.route_of(41), Some(0), "the sibling is untouched");
        assert_eq!(children.count_for(1), 1);
    }

    /// `RETIRE` may only signal a pid init itself forked for *that* route: a
    /// sibling route naming it is refused, which is the check the supervisor
    /// makes before any `kill(2)` (R7).
    #[test]
    fn retire_refuses_a_sibling_routes_pid() {
        let mut children = ChildTable::default();
        children.insert(
            41,
            ViewChild {
                route: 0,
                uid: 21_000,
                source: None,
            },
        );

        assert!(children.may_retire(0, 41));
        assert!(
            !children.may_retire(1, 41),
            "a pid forked for route 0 must never be retired as route 1's"
        );
        assert!(!children.may_retire(0, 99), "an unknown pid is refused too");
        // A pid that has been reaped is no longer a child of any route.
        children.remove(41);
        assert!(!children.may_retire(0, 41));
    }

    /// init keeps its own per-route fork bound, independent of the scheduler's
    /// accounting: one route cannot make the privileged process fork past
    /// `view_worker_cap() + VIEW_PROCESS_MARGIN` (R7).
    #[test]
    fn a_route_cannot_be_forked_past_its_ceiling() {
        let ceiling = 4u32.saturating_add(VIEW_PROCESS_MARGIN);
        let mut children = ChildTable::default();
        for pid in 0..ceiling {
            assert!(
                may_fork(&children, 0, ceiling),
                "fork {pid} is below the ceiling"
            );
            children.insert(
                i32::try_from(pid).expect("a small pid"),
                ViewChild {
                    route: 0,
                    uid: 21_000,
                    source: None,
                },
            );
        }
        assert_eq!(children.count_for(0), ceiling);
        assert!(
            !may_fork(&children, 0, ceiling),
            "the ceiling is a ceiling, not a suggestion"
        );
        // Another route is unaffected: the bound is per route, not global.
        assert!(may_fork(&children, 1, ceiling));
    }
}
