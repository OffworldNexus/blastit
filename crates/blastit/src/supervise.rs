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
use std::time::Duration;

use blastit_events::{
    Component, Event, EventKind, ExitKind, Level, Lifecycle, ManifestWarning, ProcessExited,
};
use blastit_frame::{Ctl, Decoder, Frame, Limits};
use blastit_scheduler::RouteInfo;
use blastit_sys::{
    ExitStatus, Pid, Signal, SignalFd, SignalMask, kill, reap, recv_with_fd, send_with_fd,
};
use pyo3::prelude::*;

use crate::child::Deadline;
use crate::config::Config;
use crate::logs::{Emitter, LineBuffer, Pipe, fail, lossy};
use crate::zygote;

/// How long the surviving children get after `SIGTERM` before `SIGKILL`, once
/// the drain is over.
pub const TERMINATE_GRACE: Duration = Duration::from_secs(2);

/// One descriptor in the poll set, with what to do with its bytes.
struct Source {
    fd: OwnedFd,
    pipe: Pipe,
    lines: LineBuffer,
}

/// A live view process.
struct ViewChild {
    route: usize,
    uid: u32,
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
    /// The imported WSGI callable.
    pub app: Py<PyAny>,
    /// init's end of `ctl.chan`.
    pub ctl: OwnedFd,
    /// The signalfd init listens on.
    pub signals: SignalFd,
    /// The mask `signals` replaced, for every forked child.
    pub mask: SignalMask,
    /// The components that were forked, with the pipes they log on.
    pub components: BTreeMap<i32, (Component, ComponentPipes)>,
}

/// The supervisor of all of blastit's processes.
pub struct Supervisor<'a> {
    cfg: Config,
    limits: Limits,
    routes: Vec<RouteInfo>,
    app: Py<PyAny>,
    emitter: &'a Emitter,
    ctl: Option<OwnedFd>,
    decoder: Decoder,
    sources: Vec<Source>,
    signals: SignalFd,
    mask: SignalMask,
    components: BTreeMap<i32, Component>,
    views: HashMap<i32, ViewChild>,
    route_pids: HashMap<usize, i32>,
    shutting_down: bool,
    shutdown: Shutdown,
    /// Where the sources start in the poll set: two when `ctl.chan` is polled.
    source_base: usize,
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
        Self {
            cfg: parts.cfg,
            limits: parts.limits,
            routes: parts.routes,
            app: parts.app,
            emitter,
            ctl: Some(parts.ctl),
            decoder: Decoder::new(parts.limits),
            sources,
            signals: parts.signals,
            mask: parts.mask,
            components,
            views: HashMap::new(),
            route_pids: HashMap::new(),
            shutting_down: false,
            shutdown: Shutdown::Serving,
            source_base: 2,
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
        let timeout = match self.shutdown {
            Shutdown::Serving => None,
            Shutdown::Drain(deadline) | Shutdown::Terminate(deadline) => Some(deadline.left()),
        };
        let ready: Vec<rustix::event::PollFlags> = {
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
            for source in &self.sources {
                pollfds.push(rustix::event::PollFd::new(
                    &source.fd,
                    rustix::event::PollFlags::IN,
                ));
            }
            let timespec = timeout.map(|left| rustix::event::Timespec {
                tv_sec: left.as_secs().min(i64::MAX as u64) as _,
                tv_nsec: left.subsec_nanos() as _,
            });
            let flags: Vec<rustix::event::PollFlags> =
                match rustix::event::poll(&mut pollfds, timespec.as_ref()) {
                    Ok(_) => pollfds.iter().map(|fd| fd.revents()).collect(),
                    Err(err) => fail(self.emitter, &format!("poll: {err}")),
                };
            // `ctl.chan` leaves the poll set once the scheduler's end is gone,
            // and the sources then start one entry earlier.
            self.source_base = if self.ctl.is_some() { 2 } else { 1 };
            flags
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

        let emitter = self.emitter;
        let base = self.source_base;
        let mut closed: Vec<usize> = Vec::new();
        for index in 0..self.sources.len() {
            let Some(flags) = ready.get(index + base) else {
                continue;
            };
            if !flags.intersects(readable()) {
                continue;
            }
            let source = &mut self.sources[index];
            if matches!(read_source(source, emitter), SourceRead::Closed) {
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
    }

    /// Reads every pending signal from the `signalfd`.
    fn drain_signals(&mut self) {
        loop {
            match self.signals.read() {
                Ok(Some(Signal::CHILD)) => {}
                Ok(Some(Signal::TERM | Signal::INT)) => self.begin_shutdown(),
                Ok(Some(_)) => {}
                Ok(None) => return,
                Err(err) => fail(self.emitter, &format!("signalfd: {err}")),
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
            Err(err) => fail(self.emitter, &format!("ctl.chan: {err}")),
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
                Err(err) => fail(self.emitter, &format!("ctl.chan: {err}")),
            }
        }
    }

    /// The scheduler's end of `ctl.chan` reached end-of-file.
    ///
    /// While shutting down that is the normal end of the drain: the scheduler
    /// left, so init stops polling a dead channel and moves on to terminating
    /// everyone else. At any other time it means the scheduler died without
    /// being reaped, which is fatal (§8).
    fn ctl_closed(&mut self) {
        self.ctl = None;
        if !self.shutting_down {
            fail(self.emitter, "ctl.chan: the scheduler closed the channel");
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
            other => fail(
                self.emitter,
                &format!("ctl.chan: unexpected {other:?} from the scheduler"),
            ),
        };
        match message {
            Ctl::Spawn { route } => self.spawn(py, route),
            Ctl::Retire { pid, .. } => self.retire(pid),
            // init is the sender of SPAWNED, EXITED and DRAIN; a scheduler that
            // sends one back has broken the protocol, but that is not worth
            // killing a process over.
            Ctl::Drain | Ctl::Exited { .. } | Ctl::Spawned { .. } => {
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
    fn spawn(&mut self, py: Python<'_>, route: u32) {
        let index = route as usize;
        let Some(info) = self.routes.get(index).cloned() else {
            fail(
                self.emitter,
                &format!("ctl.chan: SPAWN for unknown route {route}"),
            );
        };
        if self.route_pids.contains_key(&index) {
            self.emitter.emit(&Event::now(
                Component::Init,
                EventKind::ManifestWarning(ManifestWarning::new(format!(
                    "route {} already has a live view; ignoring SPAWN",
                    info.name
                ))),
            ));
            return;
        }
        let child = match zygote::fork_view(py, &self.cfg, self.limits, &info, &self.app, self.mask)
        {
            Ok(child) => child,
            Err(err) => fail(self.emitter, &format!("forking a view: {err}")),
        };

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
            fail(self.emitter, &format!("encoding SPAWNED: {err}"));
        }
        let Some(ctl) = self.ctl.as_ref().map(AsFd::as_fd) else {
            let _ = kill(child.pid, Signal::KILL);
            fail(self.emitter, "ctl.chan is closed; SPAWNED cannot be sent");
        };
        match send_with_fd(ctl, &bytes, child.scheduler_end.as_fd()) {
            Ok(written) if written == bytes.len() => {}
            Ok(_) => fail(self.emitter, "SPAWNED was written short"),
            Err(err) => fail(self.emitter, &format!("ctl.chan: {err}")),
        }
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
        self.views.insert(
            child.pid.as_raw(),
            ViewChild {
                route: index,
                uid: info.uid,
            },
        );
        self.route_pids.insert(index, child.pid.as_raw());

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
    fn retire(&mut self, pid: u32) {
        let Some(pid) = Pid::from_raw(pid as i32) else {
            fail(
                self.emitter,
                "ctl.chan: RETIRE with a pid that cannot exist",
            );
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
            Err(err) => fail(self.emitter, &format!("reaping: {err}")),
        };
        for (pid, status) in reaped {
            let raw = pid.as_raw();
            if let Some(component) = self.components.get(&raw).copied() {
                self.components.remove(&raw);
                self.component_exit(component, pid, status);
                continue;
            }
            let Some(view) = self.views.remove(&raw) else {
                // An orphan PID 1 inherited: blastit did not fork it, so there
                // is nothing to report.
                continue;
            };
            self.route_pids.remove(&view.route);
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
                if self.views.is_empty() && self.components.is_empty() {
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
    fn finish(&mut self) -> ! {
        self.emitter.emit(&Event::now(
            Component::Init,
            EventKind::Shutdown(Lifecycle::default().with_stage("stopped")),
        ));
        // SAFETY: init is exiting; `_exit` runs no handler.
        unsafe { libc::_exit(0) }
    }

    /// `signal` to every live child.
    fn signal_all(&self, signal: Signal) {
        for raw in self.views.keys().chain(self.components.keys()) {
            if let Some(pid) = Pid::from_raw(*raw) {
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
            if self.views.is_empty() && self.components.is_empty() {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// Writes one `ctl.chan` message; a failure here is fatal.
    fn send_ctl(&self, message: &Ctl) {
        let Some(ctl) = self.ctl.as_ref() else {
            // The scheduler is gone; nothing can act on the message any more.
            return;
        };
        let mut bytes = Vec::new();
        if let Err(err) = self
            .limits
            .encode(&Frame::Ctl(message.clone()), 0, &mut bytes)
        {
            fail(self.emitter, &format!("encoding a ctl.chan message: {err}"));
        }
        let mut written = 0;
        while written < bytes.len() {
            match rustix::io::write(ctl, &bytes[written..]) {
                Ok(0) => break,
                Ok(count) => written += count,
                Err(rustix::io::Errno::INTR) => {}
                Err(_) => break,
            }
        }
    }
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
fn read_source(source: &mut Source, emitter: &Emitter) -> SourceRead {
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
    for line in &lines {
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
