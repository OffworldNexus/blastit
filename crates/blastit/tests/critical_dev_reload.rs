//! Protected critical suite for `blastit dev`'s live reload (`MB.23.5.2.1`,
//! `docs/design/dev-fanotify-reload.md` §4.4–§4.9, `ADR-0014`).
//!
//! What is proved here, and how:
//!
//! * a real `blastit dev` boot with the unprivileged `fanotify` group, two WSGI
//!   routes and the app's own lazily-imported modules. Saving a **view** file
//!   produces exactly one `dev.reload` naming **every** route, every view pid
//!   changes, and the next request on each route serves the new body;
//! * the same for a **shared module**: both routes serve the new version, so no
//!   two views can be left serving different versions of one module;
//! * a file the app loaded from an **editable package outside the project**
//!   triggers it, while a write under `.git/`, a write into `node_modules/` and
//!   a log file the app appends to in its own directory trigger none — and
//!   neither directory is ever marked;
//! * an **atomic save** (temp write + `rename`) is exactly one reload, never a
//!   storm;
//! * a worker rewriting its own **framework cache** file — in a directory the
//!   reloader has marked — causes zero further reloads, which is what stops a
//!   reload from feeding itself;
//! * the debounce and the global floor are asserted through the **injected
//!   clock** and the **fake watcher**, never by sleeping for a verdict, and a
//!   real kernel **queue overflow** — init is stopped while the group's queue
//!   fills — reloads every view exactly once, from `reason: overflow`;
//! * `--no-reload` watches **nothing**: no `fanotify` descriptor, no mark, no
//!   report socket (so no audit hook), no reload event, and the old body after
//!   an edit;
//! * the boot line is `reload: on (N directories)`, printed once, pretty output
//!   only — and it is not an event, so the JSON contract stays at two.
//!
//! The only waits that decide anything are bounded polls for a *state* (the
//! boot's `ready`, a mark the kernel holds, an event that has been emitted);
//! [`SETTLE`] is used only after a positive control, so a fire from a write that
//! must be ignored would be its own event by then. Every 50/250/500 ms edge is
//! asserted with [`TestClock`].
//!
//! # What this file cannot prove yet
//!
//! The reload **cycle** (`execve` and the stage-1 adoption) is `MB.23.5.2.2`'s:
//! a change that needs it still emits one `dev.reload` with the honest `reason`
//! word (`blastfile`, `preload`, `overflow`) and replaces every view, but the
//! boot does not restart and re-import the `pre_fork` heap, so a `Blastfile` or
//! preload edit is recorded in the node's `RESULT.md` as a follow-up rather than
//! asserted as a fresh boot here. No test here is skipped, no test here is
//! marked ignored (R42), and nothing pins a per-route restart or an unchanged
//! pid for another route: **every** view pid changes on a reload.
#![cfg(feature = "python")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::collections::BTreeSet;
use std::fs;
use std::io::{BufRead, Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use blastit::dev_reload::{
    Class, Clock, Effect, FakeWatcher, PRELOAD_SOURCE, Reload, ReloadConfig, TestClock,
    report_pair, send_batch,
};
use blastit_events::ReloadReason;
use blastit_sys::fanotify::Event;
use blastit_view::loaded::Batch;
use rustix::event::PollFlags;
use serde_json::Value;

/// How long a boot may take to print `ready`.
const READY_TIMEOUT: Duration = Duration::from_secs(60);
/// How long an effect (a reload, a mark, a fresh view) may take to appear.
const EVENT_TIMEOUT: Duration = Duration::from_secs(30);
/// How long "nothing more happened" is observed before it is asserted.
///
/// Longer than the 500 ms global floor, so a fire from a write that must be
/// ignored would be its **own** event by the time the next assertion reads the
/// transcript. No debounce *verdict* is decided by this: the exact 50 / 250 /
/// 500 ms edges are asserted through [`TestClock`] below.
const SETTLE: Duration = Duration::from_millis(750);
/// The most of a response a case will read before calling it broken.
const MAX_RESPONSE: usize = 64 * 1024;

/// A counter for unique fixture directories.
static COUNTER: AtomicUsize = AtomicUsize::new(0);

// --------------------------------------------------------------- the fixture

/// The WSGI application every case serves.
///
/// Two things make it the design's `loaded` case rather than a `pre_fork` one:
/// the route modules are imported **inside** the callable (so the *view* loads
/// them, and its report is what puts them in the watched set), and so is the
/// editable package outside the project. It also reads a `.git` file, a
/// `node_modules` file and a compiled template it then rewrites — the paths
/// §4.8 says must never trigger a reload of their own.
const APP: &str = r#"
import importlib
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
OUTSIDE = HERE + "-editable"

def _read(path):
    try:
        with open(path) as handle:
            return handle.read().strip()
    except OSError:
        return "missing"

def _append(path, text):
    try:
        with open(path, "a") as handle:
            handle.write(text)
    except OSError:
        pass

def app(environ, start_response):
    if OUTSIDE not in sys.path:
        sys.path.insert(0, OUTSIDE)
    path = environ.get("PATH_INFO", "/")
    name = "cart" if path.startswith("/cart") else "products"
    # What a real worker reads on the way to an answer: VCS metadata, a
    # dependency tree, a compiled template — and the cache file it rewrites.
    _read(os.path.join(HERE, ".git", "HEAD"))
    _read(os.path.join(HERE, "node_modules", "pkg", "index.js"))
    cache = os.path.join(HERE, "storage", "framework", "views", "compiled.html")
    _read(cache)
    template = _read(os.path.join(HERE, "templates", "index.html"))
    outside = importlib.import_module("editable_pkg.mod").EDIT
    view = importlib.import_module("views." + name)
    _append(os.path.join(HERE, "app.log"), "request\n")
    _append(cache, "<p>%d</p>\n" % os.getpid())
    body = ("%s|%d|%s|%s|%d" % (view.render(), os.getpid(), template, outside,
                                 len(os.listdir("/proc/self/fd")))).encode()
    start_response("200 OK", [("Content-Type", "text/plain"),
                              ("Content-Length", str(len(body)))])
    return [body]
"#;

/// The `Blastfile`: two routes, `products` declared first, so the event's
/// `routes` array is in manifest order and not in request order.
const BLASTFILE: &str = "\
app \"shop\"

entrypoint python \"app:app\"

view products regex '/products/.*' {
}

view cart regex '/cart/.*' {
}
";

/// One route module's source; `tag` is what a save changes.
fn view_source(tag: &str) -> String {
    format!(
        "import models\n\nTAG = \"{tag}\"\n\n\ndef render():\n    return \"{tag}:%d\" % models.VERSION\n"
    )
}

/// A fixture tree under the machine's temp directory.
///
/// Not under `target/`: `target` is on §1.5's skip list, so a watched tree
/// inside it would legitimately never reload and every row would be green for
/// the wrong reason.
struct Fixture {
    dir: PathBuf,
}

impl Fixture {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "blastit-reload-{tag}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let outside = outside_dir(&dir);
        let _ = fs::remove_dir_all(&dir);
        let _ = fs::remove_dir_all(&outside);
        for sub in [
            "views",
            "templates",
            "storage/framework/views",
            ".git/objects",
            "node_modules/pkg",
        ] {
            fs::create_dir_all(dir.join(sub)).expect("the fixture tree");
        }
        fs::write(dir.join("Blastfile"), BLASTFILE).expect("the manifest");
        fs::write(dir.join("app.py"), APP).expect("the application");
        fs::write(dir.join("models.py"), "VERSION = 1\n").expect("the shared module");
        fs::write(dir.join("views/__init__.py"), "").expect("the package");
        fs::write(dir.join("views/cart.py"), view_source("cart-1")).expect("a view");
        fs::write(dir.join("views/products.py"), view_source("products-1")).expect("a view");
        fs::write(dir.join("templates/index.html"), "<p>template-1</p>\n").expect("a template");
        fs::write(
            dir.join("storage/framework/views/compiled.html"),
            "<p>compiled-1</p>\n",
        )
        .expect("a compiled template");
        fs::write(dir.join(".git/HEAD"), "ref: refs/heads/main\n").expect("VCS metadata");
        fs::write(
            dir.join("node_modules/pkg/index.js"),
            "module.exports = \"node-1\";\n",
        )
        .expect("a dependency");
        fs::write(dir.join("app.log"), "").expect("a log");
        // The editable install, a sibling of the project — outside the working
        // directory, which is the "even outside the project" row of §4.4.
        fs::create_dir_all(outside.join("editable_pkg")).expect("the editable package");
        fs::write(outside.join("editable_pkg/__init__.py"), "").expect("its init");
        fs::write(
            outside.join("editable_pkg/mod.py"),
            "EDIT = \"outside-1\"\n",
        )
        .expect("its module");
        Self { dir }
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.dir.join(relative)
    }

    fn outside(&self) -> PathBuf {
        outside_dir(&self.dir).join("editable_pkg")
    }

    fn start(&self, tag: &str, format: &str) -> Dev {
        Dev::start(&self.dir, tag, format)
    }

    /// A boot with extra `dev` arguments — `--no-reload` is the only one.
    fn start_with(&self, tag: &str, format: &str, extra: &[&str]) -> Dev {
        Dev::start_with(&self.dir, tag, format, extra)
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
        let _ = fs::remove_dir_all(outside_dir(&self.dir));
    }
}

/// The editable package's root, one directory beside the project.
fn outside_dir(dir: &Path) -> PathBuf {
    let name = dir
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    dir.with_file_name(format!("{name}-editable"))
}

// ------------------------------------------------------------- a live boot

/// A running `blastit dev`, with its event stream parsed as it arrives.
struct Dev {
    child: Child,
    port: u16,
    lines: Arc<Mutex<Vec<String>>>,
    reader: Option<JoinHandle<()>>,
    stderr: Arc<Mutex<Vec<String>>>,
    err_reader: Option<JoinHandle<()>>,
}

impl Dev {
    fn start(dir: &Path, tag: &str, format: &str) -> Self {
        Self::start_with(dir, tag, format, &[])
    }

    /// Boots `blastit dev` with extra arguments — `--no-reload` and nothing
    /// else is a user-facing flag (`docs/reference/cli.md`).
    fn start_with(dir: &Path, tag: &str, format: &str, extra: &[&str]) -> Self {
        let port = free_port();
        let mut command = Command::new(env!("CARGO_BIN_EXE_blastit"));
        command
            .args(["dev", "--port", &port.to_string()])
            .args(extra)
            .env_clear()
            // The host's free memory must never decide whether a view may start.
            .env("BLASTIT_MEM_HIGH_WATERMARK", "100")
            .env("BLASTIT_MAX_VIEW_PROCESSES", "8")
            .env("BLASTIT_LOG_FORMAT", format)
            .current_dir(dir)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for name in ["LD_LIBRARY_PATH", "PYTHONHOME"] {
            if let Some(value) = std::env::var_os(name) {
                command.env(name, value);
            }
        }
        let mut child = command.spawn().expect("blastit dev starts");
        let lines = Arc::new(Mutex::new(Vec::new()));
        let stderr = Arc::new(Mutex::new(Vec::new()));
        let reader = child
            .stdout
            .take()
            .map(|out| drain(out, Arc::clone(&lines)));
        let err_reader = child
            .stderr
            .take()
            .map(|err| drain(err, Arc::clone(&stderr)));
        let mut dev = Self {
            child,
            port,
            lines,
            reader,
            stderr,
            err_reader,
        };
        dev.wait_ready(tag);
        dev
    }

    /// The init pid: stage 0 `execve`s stage 1, so the spawned pid is the
    /// supervisor's own.
    fn pid(&self) -> i32 {
        i32::try_from(self.child.id()).expect("a pid")
    }

    fn transcript(&self) -> Vec<String> {
        self.lines.lock().expect("the line sink").clone()
    }

    fn diagnostic(&self) -> String {
        format!(
            "stdout:\n{}\nstderr:\n{}",
            self.transcript().join("\n"),
            self.stderr.lock().expect("the error sink").join("\n")
        )
    }

    /// Every parsed event line of the run so far.
    fn events(&self) -> Vec<Value> {
        self.transcript()
            .iter()
            .filter_map(|line| serde_json::from_str::<Value>(line).ok())
            .filter(|value| value.get("event").is_some())
            .collect()
    }

    /// Every `dev.reload` of the run so far.
    fn reloads(&self) -> Vec<Value> {
        self.events()
            .into_iter()
            .filter(|event| event["event"] == "dev.reload")
            .collect()
    }

    /// Waits for `ready` and for the listener to accept.
    ///
    /// The `ready` check matches the event itself, in either encoding: in
    /// pretty output the name is a whitespace-delimited token, which is also
    /// why a substring match would be wrong (`Address al**ready** in use`).
    fn wait_ready(&mut self, tag: &str) {
        let is_ready = |line: &str| {
            line.contains("\"event\":\"ready\"")
                || line.split_whitespace().any(|word| word == "ready")
        };
        let deadline = Instant::now() + READY_TIMEOUT;
        while Instant::now() < deadline {
            if self.transcript().iter().any(|line| is_ready(line))
                && TcpStream::connect(("127.0.0.1", self.port)).is_ok()
            {
                return;
            }
            if let Ok(Some(status)) = self.child.try_wait() {
                panic!(
                    "blastit dev[{tag}] exited {status} before `ready` on port {}\n{}",
                    self.port,
                    self.diagnostic()
                );
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!(
            "blastit dev[{tag}] never reached `ready` on port {}\n{}",
            self.port,
            self.diagnostic()
        );
    }

    /// One `GET`, answered by the response that belongs to it.
    fn get(&self, path: &str) -> String {
        let mut stream = TcpStream::connect(("127.0.0.1", self.port))
            .unwrap_or_else(|err| panic!("connecting for {path}: {err}\n{}", self.diagnostic()));
        stream
            .set_read_timeout(Some(EVENT_TIMEOUT))
            .expect("a read timeout");
        write!(
            stream,
            "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
        )
        .expect("the request goes out");
        let mut raw = Vec::new();
        let mut buf = [0u8; 4096];
        while raw.len() < MAX_RESPONSE {
            match stream.read(&mut buf) {
                Ok(0) => break,
                Ok(read) => raw.extend_from_slice(&buf[..read]),
                Err(err) => panic!("reading the answer to {path}: {err}"),
            }
        }
        let text = String::from_utf8_lossy(&raw).into_owned();
        let (head, body) = match text.split_once("\r\n\r\n") {
            Some(parts) => parts,
            None => panic!("{path}: no response head:\n{text}"),
        };
        assert!(
            head.starts_with("HTTP/1.1 200"),
            "{path}: expected a `200`:\n{text}\n{}",
            self.diagnostic()
        );
        body.to_string()
    }

    /// Waits (bounded) until `count` reloads have been emitted.
    fn wait_reloads(&self, count: usize) -> Vec<Value> {
        let deadline = Instant::now() + EVENT_TIMEOUT;
        while Instant::now() < deadline {
            let reloads = self.reloads();
            if reloads.len() >= count {
                return reloads;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!(
            "no {count}th `dev.reload` within {EVENT_TIMEOUT:?}\n{}",
            self.diagnostic()
        );
    }

    /// The inodes this boot's `fanotify` group (and its poll duplicate) has
    /// marked, read from `/proc/<init>/fdinfo`.
    ///
    /// This is §4.4's mark set as the kernel holds it — the only way to observe
    /// "which directories are watched" without a test seam.
    fn marked_inodes(&self) -> BTreeSet<u64> {
        let fdinfo = PathBuf::from(format!("/proc/{}/fdinfo", self.pid()));
        let mut inodes = BTreeSet::new();
        let Ok(entries) = fs::read_dir(&fdinfo) else {
            return inodes;
        };
        for entry in entries.flatten() {
            let info = match fs::read_to_string(entry.path()) {
                Ok(info) if info.contains("fanotify flags:") => info,
                _ => continue,
            };
            for line in info
                .lines()
                .filter(|line| line.starts_with("fanotify ino:"))
            {
                let Some(rest) = line.strip_prefix("fanotify ino:") else {
                    continue;
                };
                let Some(hex) = rest.split_whitespace().next() else {
                    continue;
                };
                if let Ok(ino) = u64::from_str_radix(hex, 16) {
                    inodes.insert(ino);
                }
            }
        }
        inodes
    }

    fn is_marked(&self, path: &Path) -> bool {
        match fs::symlink_metadata(path) {
            Ok(meta) => self.marked_inodes().contains(&meta.ino()),
            Err(_) => false,
        }
    }

    /// Waits (bounded) until every path is marked — the deterministic barrier
    /// for "the views' reports have been absorbed and their parents marked".
    fn wait_marked(&self, paths: &[PathBuf]) {
        let deadline = Instant::now() + EVENT_TIMEOUT;
        while Instant::now() < deadline {
            if paths.iter().all(|path| self.is_marked(path)) {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!(
            "not every directory was marked within {EVENT_TIMEOUT:?}: still unmarked {:?} (marks: {:?})\ninit state: {:?}\ninit wait: {:?}\ninit syscall: {:?}\n{}",
            paths
                .iter()
                .filter(|path| !self.is_marked(path))
                .collect::<Vec<_>>(),
            self.marked_inodes(),
            self.state(),
            fs::read_to_string(format!("/proc/{}/wchan", self.pid())).ok(),
            fs::read_to_string(format!("/proc/{}/syscall", self.pid())).ok(),
            self.diagnostic()
        );
    }

    /// Waits (bounded) until no pid of `before` is a live view any more.
    ///
    /// A `dev.reload` event is emitted **before** the scheduler's transaction
    /// runs, so a request sent right after it can still be answered by the view
    /// the reload is about to replace. This is the barrier that makes "the new
    /// view serves the new body" a fact rather than a race.
    fn wait_replaced(&self, before: &[(String, u32)]) {
        let deadline = Instant::now() + EVENT_TIMEOUT;
        while Instant::now() < deadline {
            let live = self.live_views();
            if before
                .iter()
                .all(|(_, pid)| !live.iter().any(|(_, live)| live == pid))
            {
                return;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!(
            "the old views were never replaced within {EVENT_TIMEOUT:?}: still live {:?}\n{}",
            before
                .iter()
                .filter(|(_, pid)| self.live_views().iter().any(|(_, live)| live == pid))
                .collect::<Vec<_>>(),
            self.diagnostic()
        );
    }

    /// The `(route, pid)` pairs of the views serving now: every `spawned` minus
    /// every `retired`/`exited`, in announcement order.
    fn live_views(&self) -> Vec<(String, u32)> {
        let mut live: Vec<(String, u32)> = Vec::new();
        for event in self.events() {
            let pid = event["pid"].as_u64().unwrap_or(0) as u32;
            match event["event"].as_str().unwrap_or_default() {
                "process.spawned" => {
                    let route = event["route"].as_str().unwrap_or_default().to_string();
                    live.push((route, pid));
                }
                "process.exited" | "process.retired" => live.retain(|(_, live)| *live != pid),
                _ => {}
            }
        }
        live
    }

    /// Stops init with `SIGSTOP`: its `poll` loop cannot run, so the kernel's
    /// per-group queue can be filled deterministically (`FAN_Q_OVERFLOW`).
    fn pause(&self) {
        // SAFETY: `kill` takes a pid this process owns and a signal number.
        unsafe { libc::kill(self.pid(), libc::SIGSTOP) };
        let deadline = Instant::now() + EVENT_TIMEOUT;
        while Instant::now() < deadline {
            if self.state() == Some('T') {
                return;
            }
            std::thread::sleep(Duration::from_millis(5));
        }
        panic!("init {} never stopped\n{}", self.pid(), self.diagnostic());
    }

    /// Lets a paused init run again.
    fn resume(&self) {
        // SAFETY: `kill` takes a pid this process owns and a signal number.
        unsafe { libc::kill(self.pid(), libc::SIGCONT) };
    }

    /// The scheduler state letter from `/proc/<pid>/stat`, if it is readable.
    fn state(&self) -> Option<char> {
        let stat = fs::read_to_string(format!("/proc/{}/stat", self.pid())).ok()?;
        // The state follows the parenthesised command name, which may itself
        // hold spaces and parentheses.
        let rest = stat.rsplit_once(')')?.1;
        rest.split_whitespace().next()?.chars().next()
    }

    /// The scheduler's pid.
    ///
    /// Stage 1 forks **http first and the scheduler second** (`stage1.rs`), and
    /// every view is forked later, so the two components are the first two
    /// entries of init's child list and the scheduler is the second. Their own
    /// `/proc` entries are deliberately unreadable (`PR_SET_DUMPABLE` is cleared
    /// for a component), and they share init's argv, so this is the one handle
    /// on "the scheduler and nothing else".
    fn scheduler_pid(&self) -> i32 {
        let children =
            fs::read_to_string(format!("/proc/{}/task/{}/children", self.pid(), self.pid()))
                .expect("init's child list");
        let views: Vec<u32> = self.live_views().into_iter().map(|(_, pid)| pid).collect();
        let components: Vec<i32> = children
            .split_whitespace()
            .filter_map(|pid| pid.parse::<i32>().ok())
            .filter(|pid| !views.contains(&u32::try_from(*pid).unwrap_or(u32::MAX)))
            .collect();
        assert_eq!(
            components.len(),
            2,
            "http and the scheduler, and no other component: {components:?}"
        );
        components[1]
    }

    /// Waits for init to exit and returns its status.
    fn wait_exit(&mut self) -> std::process::ExitStatus {
        let deadline = Instant::now() + EVENT_TIMEOUT;
        while Instant::now() < deadline {
            if let Ok(Some(status)) = self.child.try_wait() {
                return status;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!(
            "init never exited within {EVENT_TIMEOUT:?}; it is wedged\n{}",
            self.diagnostic()
        );
    }

    /// How many `fanotify` descriptors this boot holds: the group and its poll
    /// duplicate, read from `/proc/<init>/fdinfo` (§4.4's only observable).
    fn fanotify_fds(&self) -> usize {
        let Ok(entries) = fs::read_dir(format!("/proc/{}/fdinfo", self.pid())) else {
            return 0;
        };
        entries
            .flatten()
            .filter(|entry| {
                fs::read_to_string(entry.path()).is_ok_and(|info| info.contains("fanotify flags:"))
            })
            .count()
    }

    /// `SIGTERM`, wait for the exit, and return the whole transcript.
    fn stop(&mut self) -> Vec<String> {
        // SAFETY: `kill` takes a pid this process owns and a signal number.
        unsafe { libc::kill(self.pid(), libc::SIGTERM) };
        let deadline = Instant::now() + EVENT_TIMEOUT;
        while Instant::now() < deadline {
            if let Ok(Some(_)) = self.child.try_wait() {
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(handle) = self.reader.take() {
            let _ = handle.join();
        }
        if let Some(handle) = self.err_reader.take() {
            let _ = handle.join();
        }
        self.transcript()
    }
}

impl Drop for Dev {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(handle) = self.reader.take() {
            let _ = handle.join();
        }
        if let Some(handle) = self.err_reader.take() {
            let _ = handle.join();
        }
    }
}

/// Copies a child's output into `sink`, one line per entry.
fn drain(stream: impl Read + Send + 'static, sink: Arc<Mutex<Vec<String>>>) -> JoinHandle<()> {
    std::thread::spawn(move || {
        let mut reader = std::io::BufReader::new(stream);
        let mut line = String::new();
        loop {
            line.clear();
            match reader.read_line(&mut line) {
                Ok(0) | Err(_) => return,
                Ok(_) => {
                    if let Ok(mut sink) = sink.lock() {
                        sink.push(line.trim_end().to_string());
                    }
                }
            }
        }
    })
}

/// A free ephemeral port: dropped immediately, so the child can take it.
fn free_port() -> u16 {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("a free port");
    listener.local_addr().expect("a bound address").port()
}

/// The body an answer carries:
/// `"<tag>:<version>|<pid>|<template>|<outside>|<open descriptors>"`.
///
/// The descriptor count is how `--no-reload` is witnessed: the view that got no
/// report socket (and so no audit hook, §5 `S3`) holds exactly one fewer.
#[derive(Debug, PartialEq, Eq)]
struct Body {
    tag: String,
    version: u32,
    pid: u32,
    template: String,
    outside: String,
    fds: usize,
}

fn body_of(text: &str) -> Body {
    let trimmed = text.trim();
    let mut fields = trimmed.split('|');
    let (tag, version) = fields
        .next()
        .and_then(|head| head.rsplit_once(':'))
        .unwrap_or_else(|| panic!("no `<tag>:<version>` in {trimmed:?}"));
    Body {
        tag: tag.to_string(),
        version: version
            .parse()
            .unwrap_or_else(|_| panic!("no version in {trimmed:?}")),
        pid: fields
            .next()
            .and_then(|pid| pid.parse().ok())
            .unwrap_or_else(|| panic!("no pid in {trimmed:?}")),
        template: fields.next().unwrap_or_default().to_string(),
        outside: fields.next().unwrap_or_default().to_string(),
        fds: fields
            .next()
            .and_then(|fds| fds.trim().parse().ok())
            .unwrap_or_else(|| panic!("no descriptor count in {trimmed:?}")),
    }
}

/// The `routes` array of one reload event, as strings.
fn routes_of(event: &Value) -> Vec<String> {
    event["routes"]
        .as_array()
        .expect("`routes` is always an array")
        .iter()
        .map(|name| name.as_str().unwrap_or_default().to_string())
        .collect()
}

/// The `files` array of one reload event, as strings.
fn files_of(event: &Value) -> Vec<String> {
    event["files"]
        .as_array()
        .expect("`files` is always an array")
        .iter()
        .map(|name| name.as_str().unwrap_or_default().to_string())
        .collect()
}

/// The manifest order every reload's `routes` must carry.
fn every_route() -> Vec<String> {
    vec!["products".to_string(), "cart".to_string()]
}

// ------------------------------------------------- rows 1–5: the live boot

/// Row 1: saving a view file is **one** reload of **every** view, and the next
/// request on each route serves the new body from a new pid.
#[test]
fn saving_a_view_reloads_every_view_once_and_the_new_body_is_served() {
    let fixture = Fixture::new("view");
    let mut dev = fixture.start("view", "json");
    let cart = dev.get("/cart/x");
    let products = dev.get("/products/x");
    assert_eq!(body_of(&cart).tag, "cart-1", "{cart}");
    assert_eq!(body_of(&products).tag, "products-1", "{products}");
    // The views' reports are what put these files in the watched set: waiting
    // for the marks is the state — not a sleep — that says they were absorbed.
    dev.wait_marked(&[
        fixture.path("views"),
        fixture.path("templates"),
        fixture.path("storage/framework/views"),
        fixture.outside(),
    ]);
    let before = dev.live_views();
    assert_eq!(
        before.len(),
        2,
        "one live view per route before the reload: {before:?}"
    );

    fs::write(fixture.path("views/cart.py"), view_source("cart-2")).expect("the save");
    let reloads = dev.wait_reloads(1);
    assert_eq!(reloads.len(), 1, "one save is one reload: {reloads:?}");
    assert_eq!(
        routes_of(&reloads[0]),
        every_route(),
        "every view route, in manifest order"
    );
    assert_eq!(
        files_of(&reloads[0]),
        vec![fixture.path("views/cart.py").to_string_lossy().into_owned()],
        "the file that changed"
    );
    assert_eq!(reloads[0]["reason"], "loaded");
    assert_eq!(reloads[0]["level"], "info");
    dev.wait_replaced(&before);

    let cart = body_of(&dev.get("/cart/x"));
    let products = body_of(&dev.get("/products/x"));
    assert_eq!(cart.tag, "cart-2", "the next request serves the new body");
    assert_eq!(
        products.tag, "products-1",
        "the untouched view still serves its own module"
    );
    // Every view pid changes: the reload replaced all of them, and no assertion
    // here (or anywhere in this file) expects a surviving pid.
    let after = dev.live_views();
    for (route, pid) in &before {
        assert!(
            !after.iter().any(|(_, live)| live == pid),
            "route {route}'s view {pid} was replaced by the reload (now {after:?})"
        );
        let served = if route == "cart" {
            cart.pid
        } else {
            products.pid
        };
        assert!(
            after
                .iter()
                .any(|(name, pid)| name == route && *pid == served),
            "the answer for {route} came from a view this boot spawned ({after:?})"
        );
    }
    assert_eq!(dev.reloads().len(), 1, "and nothing fired twice");
    dev.stop();
}

/// Row 2: a shared module is the same single all-views reload, and **both**
/// routes then serve the new version — the assertion that fails if one view
/// were left on the old module.
#[test]
fn saving_a_shared_module_leaves_no_view_on_the_old_version() {
    let fixture = Fixture::new("shared");
    let mut dev = fixture.start("shared", "json");
    assert_eq!(body_of(&dev.get("/cart/x")).version, 1);
    assert_eq!(body_of(&dev.get("/products/x")).version, 1);
    dev.wait_marked(&[fixture.path("views")]);
    let before = dev.live_views();

    fs::write(fixture.path("models.py"), "VERSION = 2\n").expect("the save");
    let reloads = dev.wait_reloads(1);
    assert_eq!(reloads.len(), 1, "one shared-module save is one reload");
    assert_eq!(routes_of(&reloads[0]), every_route());
    assert_eq!(
        files_of(&reloads[0]),
        vec![fixture.path("models.py").to_string_lossy().into_owned()]
    );
    dev.wait_replaced(&before);

    let cart = body_of(&dev.get("/cart/x"));
    let products = body_of(&dev.get("/products/x"));
    assert_eq!(cart.version, 2, "the cart view re-imported the module");
    assert_eq!(products.version, 2, "and so did the products view");
    assert_ne!(cart.pid, products.pid, "two routes, two processes");
    assert_eq!(dev.reloads().len(), 1);
    dev.stop();
}

/// Row 3: a file loaded from an editable package **outside** the working
/// directory reloads; `.git/`, `node_modules/` and a log file do not, and
/// neither directory is ever marked.
#[test]
fn outside_the_project_reloads_and_vcs_dependencies_and_logs_do_not() {
    let fixture = Fixture::new("outside");
    let mut dev = fixture.start("outside", "json");
    assert_eq!(body_of(&dev.get("/cart/x")).outside, "outside-1");
    assert_eq!(body_of(&dev.get("/products/x")).outside, "outside-1");
    dev.wait_marked(&[fixture.outside()]);
    let before = dev.live_views();

    // The three writes that must not fire. Two are files the app loads from
    // directories on the skip list, so a broken skip list would mark them.
    fs::write(fixture.path(".git/HEAD"), "ref: refs/heads/other\n").expect("a VCS write");
    fs::write(
        fixture.path("node_modules/pkg/index.js"),
        "module.exports = \"node-2\";\n",
    )
    .expect("a dependency write");
    let mut log = fs::OpenOptions::new()
        .append(true)
        .open(fixture.path("app.log"))
        .expect("the log");
    log.write_all(b"a log line\n").expect("a log write");
    drop(log);
    std::thread::sleep(SETTLE);
    assert!(
        dev.reloads().is_empty(),
        "no write under `.git/`, `node_modules/` or a log file reloads: {:?}\n{}",
        dev.reloads(),
        dev.diagnostic()
    );
    assert!(
        !dev.is_marked(&fixture.path(".git")),
        "a path on the skip list is never marked"
    );
    assert!(
        !dev.is_marked(&fixture.path("node_modules")),
        "a dependency tree is never marked"
    );

    // The positive control: the same kind of edit, from outside the project.
    let outside = fixture.outside().join("mod.py");
    fs::write(&outside, "EDIT = \"outside-2\"\n").expect("the outside save");
    let reloads = dev.wait_reloads(1);
    assert_eq!(reloads.len(), 1, "one reload, from the one tracked file");
    assert_eq!(
        files_of(&reloads[0]),
        vec![outside.to_string_lossy().into_owned()],
        "the outside file is what changed"
    );
    dev.wait_replaced(&before);
    let cart = body_of(&dev.get("/cart/x"));
    let products = body_of(&dev.get("/products/x"));
    assert_eq!(cart.outside, "outside-2");
    assert_eq!(products.outside, "outside-2", "both views, not one");
    assert_eq!(dev.reloads().len(), 1);
    dev.stop();
}

/// A `SPAWN` and a report datagram in **one** wakeup: init must read only the
/// sources it polled, stay responsive, and keep absorbing reports.
///
/// The log-source loop used to iterate `self.sources.len()` while `ready` holds
/// only the sources the poll set carried. A `Ctl::Spawn` handled in the same
/// wakeup forks a view and pushes two more log pipes, so the loop read a reload
/// descriptor's ready flag as if it were a source's and then `read_source`ed an
/// empty pipe — and the pipes are blocking, so init parked in `pipe_read` for
/// good. That is the hang `make check` saw while this suite passed alone.
///
/// The rounds are separated only by the reload they wait for, so each save's
/// watcher event lands next to the previous reload's `SPAWN` and the fresh
/// views' boot reports: the collision window, with no sleep deciding anything.
#[test]
fn a_spawn_and_a_report_in_one_wakeup_never_wedge_init() {
    let fixture = Fixture::new("wakeup");
    let mut dev = fixture.start("wakeup", "json");
    assert_eq!(body_of(&dev.get("/cart/x")).tag, "cart-1");
    assert_eq!(body_of(&dev.get("/products/x")).tag, "products-1");
    dev.wait_marked(&[fixture.path("views"), fixture.path("templates")]);

    for round in 1..=3usize {
        let tag = format!("cart-w{round}");
        fs::write(fixture.path("views/cart.py"), view_source(&tag)).expect("the save");
        let reloads = dev.wait_reloads(round);
        assert_eq!(
            reloads.len(),
            round,
            "one save is one reload: {reloads:?}\n{}",
            dev.diagnostic()
        );
        assert_eq!(
            files_of(&reloads[round - 1]),
            vec![fixture.path("views/cart.py").to_string_lossy().into_owned()],
            "the reload names the file that changed"
        );
        // A misaligned loop parks init in `read(2)` on the empty pipe; this
        // request is what proves it did not: it can only be answered by a live
        // boot.
        let served = body_of(&dev.get("/cart/x"));
        assert!(
            served.tag.starts_with("cart-"),
            "init answered during the spawn/report window: {served:?}"
        );
    }

    // And nothing was lost on the way: both routes still answer, the views'
    // marks survived every replacement, and no boot declared reload unavailable.
    assert_eq!(body_of(&dev.get("/products/x")).tag, "products-1");
    dev.wait_marked(&[fixture.path("views"), fixture.path("templates")]);
    assert!(
        !dev.events()
            .iter()
            .any(|event| event["event"] == "dev.reload.unavailable"),
        "no reload was lost in the collision:\n{}",
        dev.diagnostic()
    );
    dev.stop();
}

/// A component dying right after a reload is still fatal, and takes the views
/// with it.
///
/// **What this witnesses:** a `blastfile` reload (the reason that would take the
/// cycle in `MB.23.5.2.2`), then a `SIGKILL` to a component; init exits
/// non-zero and **no view survives** — as content, a component death during
/// reload traffic is not absorbed: `kill_all` ran and `_exit(1)` followed.
///
/// **What this does NOT witness:** that no gate exists. Which component dies
/// first is the kernel's choice: killing the scheduler closes `ctl.chan`, http
/// exits with it, and *http's* death is fatal in every version of this code, so
/// a re-introduced scheduler gate would still satisfy the exit and `kill_all`
/// assertions here. A true gate witness — a scheduler stopped by a cycle,
/// excused, and then a genuine death swallowed — needs the cycle itself and
/// therefore belongs to `MB.23.5.2.2`. This test claims nothing about it.
#[test]
fn a_scheduler_exit_after_a_reload_still_stops_the_boot() {
    let fixture = Fixture::new("sched");
    let mut dev = fixture.start("sched", "json");
    assert_eq!(body_of(&dev.get("/cart/x")).tag, "cart-1");
    assert_eq!(body_of(&dev.get("/products/x")).tag, "products-1");
    dev.wait_marked(&[fixture.path("views")]);

    // A manifest save is the reload that would need the cycle: its reason is the
    // one that names the manifest.
    let manifest = fixture.path("Blastfile");
    let original = fs::read_to_string(&manifest).expect("the manifest");
    fs::write(
        &manifest,
        format!("{original}\n# a save that must not gate the next death\n"),
    )
    .expect("the save");
    let reloads = dev.wait_reloads(1);
    assert_eq!(
        reloads[0]["reason"], "blastfile",
        "the cycle's own reason: {reloads:?}"
    );
    assert_eq!(routes_of(&reloads[0]), every_route(), "every view reloads");

    // The pids that must not outlive init are the ones that **answered**. The
    // scheduler retires views lazily, so a route nobody requested has no live
    // view at all and the table's length says nothing about what is running:
    // these two requests are what makes the set exact, on the first poll, with
    // no wait at all.
    let cart = body_of(&dev.get("/cart/x"));
    let products = body_of(&dev.get("/products/x"));
    assert_eq!(cart.tag, "cart-1");
    assert_eq!(products.tag, "products-1");
    assert_ne!(cart.pid, products.pid, "two routes, two views");
    let views = vec![
        (String::from("cart"), cart.pid),
        (String::from("products"), products.pid),
    ];

    // The scheduler dies for real, not as part of any reload: init must treat
    // that as fatal (§8), kill every view and exit non-zero.
    let scheduler = dev.scheduler_pid();
    // SAFETY: `kill` takes a pid this process owns and a signal number.
    unsafe { libc::kill(scheduler, libc::SIGKILL) };

    let status = dev.wait_exit();
    assert!(
        !status.success(),
        "any component dying during a reload is fatal, so the boot must not \
         exit cleanly: {status}"
    );
    let transcript = dev.stop();
    // Deliberately wording-agnostic; do not re-narrow it. A component death
    // during reload traffic surfaces through whichever fatal arm wins the race —
    // the reaped `<component>-died` shutdown stage, the `ctl.chan` read-EOF arm,
    // or `send_ctl`'s write failure (`ctl.chan: Broken pipe`) — and all three
    // are the same contract: `kill_all` and `_exit(1)`. Demanding one phrasing
    // turns this into a wording lottery (it has already flaked on each arm in
    // turn), so the asserted evidence is the non-zero exit above and the
    // emptied view set below. A log witness is recorded for whoever reads a
    // failure, and `None` is a pass.
    let witness = transcript.iter().find_map(|line| {
        let event: Value = serde_json::from_str(line).ok()?;
        let text = event["line"].as_str().unwrap_or_default();
        let stage = event["stage"].as_str().unwrap_or_default();
        (text.starts_with("ctl.chan:") || stage.ends_with("-died")).then(|| line.clone())
    });

    // `kill_all` ran: no view outlives the boot.
    let deadline = Instant::now() + EVENT_TIMEOUT;
    for (route, pid) in &views {
        while Path::new(&format!("/proc/{pid}")).exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert!(
            !Path::new(&format!("/proc/{pid}")).exists(),
            "route {route}'s view {pid} outlived init: `kill_all` did not run \
             (last fatal arm seen: {witness:?})"
        );
    }
}

/// Row 4: a temp-write + `rename` — how every editor saves — is exactly one
/// reload, never one per step.
#[test]
fn an_atomic_save_is_one_reload() {
    let fixture = Fixture::new("atomic");
    let mut dev = fixture.start("atomic", "json");
    assert_eq!(body_of(&dev.get("/cart/x")).tag, "cart-1");
    dev.wait_marked(&[fixture.path("views")]);
    let before = dev.live_views();

    let target = fixture.path("views/cart.py");
    let temporary = fixture.path("views/.cart.py.swp");
    fs::write(&temporary, view_source("cart-3")).expect("the temp write");
    fs::rename(&temporary, &target).expect("the atomic save");
    dev.wait_reloads(1);
    std::thread::sleep(SETTLE);
    let reloads = dev.reloads();
    assert_eq!(
        reloads.len(),
        1,
        "the temp file and the rename are one change: {reloads:?}\n{}",
        dev.diagnostic()
    );
    assert_eq!(
        files_of(&reloads[0]),
        vec![target.to_string_lossy().into_owned()],
        "the saved name, not the temporary one"
    );
    dev.wait_replaced(&before);
    assert_eq!(body_of(&dev.get("/cart/x")).tag, "cart-3");
    dev.stop();
}

/// Row 5: a worker rewriting its own framework cache — in a directory the
/// reloader *has* marked — causes zero further reloads.
#[test]
fn a_worker_cache_write_never_triggers_a_reload() {
    let fixture = Fixture::new("cache");
    let mut dev = fixture.start("cache", "json");
    // Every request rewrites `storage/framework/views/compiled.html`.
    assert_eq!(body_of(&dev.get("/cart/x")).tag, "cart-1");
    assert_eq!(body_of(&dev.get("/products/x")).tag, "products-1");
    dev.wait_marked(&[fixture.path("storage/framework/views")]);
    assert_eq!(
        body_of(&dev.get("/cart/x")).tag,
        "cart-1",
        "a second request rewrites the cache again"
    );
    std::thread::sleep(SETTLE);
    assert!(
        dev.reloads().is_empty(),
        "the worker's own cache writes must not reload anything: {:?}\n{}",
        dev.reloads(),
        dev.diagnostic()
    );

    // The positive control, so "no reload" cannot be a dead watcher.
    fs::write(fixture.path("views/products.py"), view_source("products-2")).expect("the save");
    let reloads = dev.wait_reloads(1);
    assert_eq!(reloads.len(), 1);
    assert_eq!(
        files_of(&reloads[0]),
        vec![
            fixture
                .path("views/products.py")
                .to_string_lossy()
                .into_owned()
        ],
        "only the real edit, never the cache"
    );
    assert_eq!(dev.reloads().len(), 1, "and nothing fired twice");
    dev.stop();
}

// ------------------------------- row 6: `--no-reload` means nothing at all

/// Row 6: `--no-reload` yields no watcher group, no mark, no report socket (and
/// therefore no audit hook, §5 `S3`), no `dev.reload.unavailable`, no
/// `dev.reload` and the **old** body after an edit — "off" is not "a watcher
/// that ignores events" (§1, `R1`).
#[test]
fn no_reload_watches_nothing_and_serves_the_old_body() {
    let fixture = Fixture::new("no-reload");
    // The control: the same fixture with reload on. Its view holds exactly one
    // more socket — the report end — which is what `--no-reload` must not hand
    // over.
    let mut watched = fixture.start("no-reload-on", "json");
    let watched_cart = body_of(&watched.get("/cart/x"));
    assert_eq!(watched_cart.tag, "cart-1");
    assert_eq!(body_of(&watched.get("/products/x")).tag, "products-1");
    watched.stop();

    let mut dev = fixture.start_with("no-reload", "json", &["--no-reload"]);
    let cart = body_of(&dev.get("/cart/x"));
    let products = body_of(&dev.get("/products/x"));
    assert_eq!(cart.tag, "cart-1", "{cart:?}");
    assert_eq!(products.tag, "products-1", "{products:?}");
    assert_eq!(
        dev.fanotify_fds(),
        0,
        "`--no-reload` opens no fanotify group at all, not even an unmarked one"
    );
    assert!(
        dev.marked_inodes().is_empty(),
        "and places no mark: {:?}",
        dev.marked_inodes()
    );
    assert_eq!(
        cart.fds + 1,
        watched_cart.fds,
        "the view holds one descriptor fewer: no report socket, so no audit hook"
    );
    assert!(
        !dev.transcript()
            .iter()
            .any(|line| line.contains("reload: on (")),
        "no boot line: nothing was watched"
    );
    assert!(
        !dev.events()
            .iter()
            .any(|event| event["event"] == "dev.reload.unavailable"),
        "and no warning either: `--no-reload` emits neither event"
    );

    // A save of the very file a reload-on boot watches: nothing may happen.
    fs::write(fixture.path("views/cart.py"), view_source("cart-2")).expect("the save");
    std::thread::sleep(SETTLE);
    let after = body_of(&dev.get("/cart/x"));
    assert_eq!(after.tag, "cart-1", "the old body is still served");
    assert_eq!(after.pid, cart.pid, "no view was replaced");
    assert!(dev.reloads().is_empty(), "zero `dev.reload`");
    assert_eq!(dev.fanotify_fds(), 0, "and still no group");
    dev.stop();
}

// ------------------------------------- row 7: a real queue overflow, live

/// Row 7: one `FAN_Q_OVERFLOW` reloads every view exactly once and logs one
/// line — the kernel's own overflow, not a scripted `Readout`.
///
/// The queue is per group and holds 16384 events, which an idle reader drains
/// far faster than a test can write. `SIGSTOP` on init is what makes the window
/// deterministic: while it cannot run, the burst files fill the queue, and the
/// kernel hands back one `FAN_Q_OVERFLOW` record on the first read after
/// `SIGCONT`. No sleep decides any verdict — the wait is for the reload.
#[test]
fn a_queue_overflow_reloads_every_view_once_and_logs_one_line() {
    let fixture = Fixture::new("overflow-live");
    let mut dev = fixture.start("overflow-live", "json");
    assert_eq!(body_of(&dev.get("/cart/x")).tag, "cart-1");
    assert_eq!(body_of(&dev.get("/products/x")).tag, "products-1");
    // The views' reports are what mark `views/`: waiting for the mark is the
    // state that says the burst directory is watched.
    dev.wait_marked(&[fixture.path("views")]);
    let before = dev.live_views();
    assert_eq!(before.len(), 2, "one live view per route: {before:?}");

    // Two events (create + close-write) per file, so this is comfortably past
    // the kernel's 16384-event queue; with init stopped, none of it drains.
    dev.pause();
    for n in 0..20_000 {
        fs::write(fixture.path("views").join(format!("burst-{n}.py")), "x").expect("a burst file");
    }
    dev.resume();

    let reloads = dev.wait_reloads(1);
    assert_eq!(reloads.len(), 1, "one overflow is one reload: {reloads:?}");
    let event = &reloads[0];
    assert_eq!(event["reason"], "overflow", "the dropped-event reason");
    assert!(
        files_of(event).is_empty(),
        "the changed set is unknown: {event:?}"
    );
    assert_eq!(
        routes_of(event),
        every_route(),
        "every view route, in manifest order"
    );
    assert_eq!(event["level"], "info");
    // One line in the log, not one per dropped event.
    let lines = dev
        .transcript()
        .iter()
        .filter(|line| line.contains("\"event\":\"dev.reload\""))
        .count();
    assert_eq!(lines, 1, "exactly one log line for the overflow");

    // Every view pid changed, and each answer comes from a replacement.
    dev.wait_replaced(&before);
    let cart = body_of(&dev.get("/cart/x"));
    let products = body_of(&dev.get("/products/x"));
    let after = dev.live_views();
    assert_eq!(after.len(), 2, "two replacements: {after:?}");
    for (route, pid) in &before {
        assert!(
            !after.iter().any(|(_, live)| live == pid),
            "route {route}'s view {pid} was replaced by the overflow reload (now {after:?})"
        );
    }
    assert!(
        after
            .iter()
            .any(|(name, pid)| name == "cart" && *pid == cart.pid),
        "the cart answer came from a live replacement: {after:?}"
    );
    assert!(
        after
            .iter()
            .any(|(name, pid)| name == "products" && *pid == products.pid),
        "the products answer came from a live replacement: {after:?}"
    );
    // And nothing fired again: the burst is one overflow, not a storm.
    std::thread::sleep(SETTLE);
    assert_eq!(dev.reloads().len(), 1, "still exactly one reload");
    dev.stop();
}

// ------------------------------------------- the boot line and the contract

/// The boot line: one pretty-only line, `reload: on (N directories)`, and no
/// event for it — the machine contract stays at two events (§4.7, §4.9).
#[test]
fn the_boot_line_is_one_pretty_line_and_never_an_event() {
    let fixture = Fixture::new("line");
    let mut pretty = fixture.start("line-pretty", "pretty");
    let transcript = pretty.stop();
    let lines: Vec<&String> = transcript
        .iter()
        .filter(|line| line.contains("reload: on ("))
        .collect();
    assert_eq!(lines.len(), 1, "one boot line, once: {transcript:?}");
    let line = lines[0];
    let count = line
        .trim()
        .strip_prefix("reload: on (")
        .and_then(|rest| rest.strip_suffix(" directories)"))
        .and_then(|count| count.parse::<usize>().ok())
        .unwrap_or_else(|| panic!("the documented sentence, exactly: {line:?}"));
    assert!(count >= 1, "the app root is always marked: {line:?}");

    let mut json = fixture.start("line-json", "json");
    assert!(
        json.is_marked(&fixture.path("")),
        "a reload-on boot has one group and marks the app root"
    );
    let transcript = json.stop();
    assert!(
        !transcript.iter().any(|line| line.contains("reload: on (")),
        "the line is pretty output, not part of the JSON contract: {transcript:?}"
    );
    assert!(
        json.reloads().is_empty(),
        "a boot with no save emits no `dev.reload`"
    );
    assert!(
        !json
            .events()
            .iter()
            .any(|event| event["event"] == "dev.reload.unavailable"),
        "this host allows the unprivileged group, so nothing is unavailable"
    );
}

// ---------------------------------------- rows 9 & 11: the policy, no kernel

/// A policy booted over a real tree, with the scripted watcher and the injected
/// clock — the same entry points the supervisor calls, no kernel, no sleeps.
struct Script {
    reload: Reload,
    /// The watcher's poll duplicate: `on_fd` on it is "the kernel had events".
    poll: i32,
    clock: TestClock,
    /// The watcher's recorded marks, shared with the boxed fake.
    marks: Arc<Mutex<Vec<PathBuf>>>,
}

impl Script {
    /// Boots the policy over `fixture`, with `script` queued as its readouts.
    fn boot(fixture: &Fixture, script: Vec<(Vec<Event>, bool)>) -> Self {
        let mut watcher = FakeWatcher::default();
        for (events, overflow) in script {
            watcher.push(events, overflow);
        }
        let marks = watcher.marks();
        let config = ReloadConfig::new(
            fixture.dir.clone(),
            fixture.path("Blastfile"),
            "tmpfs".to_string(),
        );
        let clock = TestClock::new(Instant::now());
        let mut reload = Reload::boot(
            config,
            Box::new(watcher),
            duplicate_stdout(),
            Box::new(clock.clone()),
        )
        .expect("the fake watcher never refuses a mark");
        let mut fds = Vec::new();
        reload.poll_fds(&mut fds);
        let watcher_fd = *fds.first().expect("the watcher's poll duplicate");
        Self {
            reload,
            poll: watcher_fd,
            clock,
            marks,
        }
    }

    fn now(&mut self) -> Instant {
        self.clock.now()
    }

    fn advance(&mut self, millis: u64) {
        self.clock.advance(Duration::from_millis(millis));
    }

    /// One ready-descriptor wakeup, as the supervisor's poll would deliver it.
    fn wake(&mut self) -> Vec<Effect> {
        let now = self.now();
        let poll = self.poll;
        self.reload.on_fd(poll, PollFlags::IN, now)
    }

    fn tick(&mut self) -> Vec<Effect> {
        let now = self.now();
        self.reload.tick(now)
    }
}

/// A `CLOEXEC` duplicate of stdout: any open descriptor will do for a `Reload`
/// whose watcher is a fake, and stdout is guaranteed open.
fn duplicate_stdout() -> std::os::fd::OwnedFd {
    use std::os::fd::BorrowedFd;
    // SAFETY: descriptor 1 is open for this process's whole life.
    unsafe { BorrowedFd::borrow_raw(1) }
        .try_clone_to_owned()
        .expect("a duplicate of stdout")
}

/// One tracked change on `path`, as the watcher reports it.
fn save_event(dir: &Path, name: &str) -> Event {
    Event {
        mask: libc::FAN_CLOSE_WRITE,
        dir: dir.to_path_buf(),
        name: name.as_bytes().to_vec(),
    }
}

/// Row 9: one `FAN_Q_OVERFLOW` readout is exactly one all-views reload, and the
/// floor still separates it from the next one.
#[test]
fn a_queue_overflow_is_one_all_views_reload() {
    let fixture = Fixture::new("overflow");
    // Two overflow readouts: the first fires, the second proves the floor.
    let mut script = Script::boot(&fixture, vec![(Vec::new(), true), (Vec::new(), true)]);
    assert!(
        !script.marks.lock().expect("the marks").is_empty(),
        "the boot marked the app root"
    );

    // A readout that says the kernel dropped events: no event can be trusted,
    // so the window arms at once with reason `overflow` (§4.5) and fires on the
    // quiet edge — exactly one all-views reload.
    assert!(
        script.wake().is_empty(),
        "an overflow arms the window; the quiet edge still applies"
    );
    script.advance(50);
    let effects = script.tick();
    assert_eq!(effects.len(), 1, "one fire: {effects:?}");
    let fire = match &effects[0] {
        Effect::Reload(fire) => fire,
        other => panic!("an overflow is a reload, not {other:?}"),
    };
    assert_eq!(fire.reason, ReloadReason::Overflow);
    assert!(fire.files.is_empty(), "the changed set is unknown");

    // Nothing fires while the window is idle, and the floor holds the next one.
    assert!(script.tick().is_empty());
    script.advance(100);
    assert!(
        script.wake().is_empty(),
        "100 ms after the first fire the floor is still holding"
    );
    script.advance(399);
    assert!(script.tick().is_empty(), "499 ms is still inside the floor");
    script.advance(1);
    assert_eq!(
        script.tick().len(),
        1,
        "and at 500 ms the second overflow fires once"
    );
}

/// Row 11: the 50 ms quiet window, the 250 ms hard maximum and the 500 ms floor
/// are exact, asserted through the injected clock.
#[test]
fn the_debounce_windows_are_exact_through_the_injected_clock() {
    let fixture = Fixture::new("clock");
    let manifest = fixture.path("Blastfile");
    let name = "Blastfile";
    let save = |fixture: &Fixture| save_event(&fixture.dir, name);
    // Every readout this test needs, queued before the boot so the fake is the
    // supervisor's own watcher: one save, one save 400 ms later, then a stream.
    let mut planned: Vec<(Vec<Event>, bool)> =
        vec![(vec![save(&fixture)], false), (vec![save(&fixture)], false)];
    for _ in 0..12 {
        planned.push((vec![save(&fixture)], false));
    }
    planned.push((vec![save(&fixture)], false));
    let mut script = Script::boot(&fixture, planned);
    assert!(
        script.reload.policy().union().contains_key(&manifest),
        "the manifest is always in the union (§4.4)"
    );

    // One save at t0: armed, not fired.
    assert!(script.wake().is_empty(), "a save only arms the window");
    script.advance(49);
    assert!(
        script.tick().is_empty(),
        "49 ms is still inside the quiet window"
    );
    script.advance(1);
    let fired = script.tick();
    assert_eq!(fired.len(), 1, "50 ms of quiet fires exactly once");
    assert!(
        matches!(&fired[0], Effect::Reload(fire) if fire.reason == ReloadReason::Blastfile),
        "the reason names what changed: {fired:?}"
    );

    // A second window 400 ms after the fire: the 500 ms floor wins over the
    // 50 ms quiet edge, which would have released it at 450 ms.
    script.advance(400);
    assert!(script.wake().is_empty(), "the second save arms the window");
    script.advance(49);
    assert!(
        script.tick().is_empty(),
        "the quiet edge elapsed, but only 449 ms have passed since the fire"
    );
    script.advance(1);
    assert!(
        script.tick().is_empty(),
        "450 ms: the floor still holds the second window"
    );
    script.advance(50);
    assert_eq!(
        script.tick().len(),
        1,
        "the floor releases the second window at 500 ms"
    );

    // A stream of saves cannot postpone the fire past 250 ms from the first.
    script.advance(600);
    assert!(script.wake().is_empty(), "the stream's first save");
    for step in 1..=12u64 {
        script.advance(20);
        assert!(
            script.tick().is_empty(),
            "{step}: the stream keeps the quiet edge moving, and {} ms is inside the hard maximum",
            step * 20
        );
        if step < 12 {
            assert!(
                script.wake().is_empty(),
                "{step}: another save in the stream"
            );
        } else {
            // 240 ms of stream: the next save would be at 240, still inside.
            break;
        }
    }
    script.advance(9);
    assert!(
        script.tick().is_empty(),
        "249 ms is inside the maximum even though the stream never quiets"
    );
    assert!(script.wake().is_empty(), "one more save at 249 ms");
    script.advance(1);
    assert_eq!(
        script.tick().len(),
        1,
        "the 250 ms maximum fires despite the stream"
    );
}

/// The policy's own boot does not arm the window: a boot batch replaces a
/// source's set without firing the reload it was born from (`T30`).
#[test]
fn a_boot_report_replaces_a_set_and_arms_nothing() {
    let fixture = Fixture::new("bootreport");
    let mut script = Script::boot(&fixture, Vec::new());
    let view = fixture.path("views/cart.py");
    let write = {
        let reports = script.reload.policy().reports();
        // A report socket is how the supervisor hands a view its channel.
        report_pair(reports, 0, Class::Loaded, false).expect("a socketpair")
    };
    let path = view.to_string_lossy().into_owned();
    let batch = Batch::new(0, 0, true, false, vec![path]).expect("a valid boot batch");
    send_batch(&write, &batch);

    let mut fds = Vec::new();
    script.reload.poll_fds(&mut fds);
    let end = *fds.get(1).expect("one report end");
    assert_eq!(
        script.reload.policy().reports().source_of(end),
        Some(0),
        "the packet is attributed by the descriptor it arrived on"
    );
    let now = script.now();
    let effects = script.reload.on_fd(end, PollFlags::IN, now);
    assert!(
        effects.is_empty(),
        "a boot batch must not fire the reload it was born from: {effects:?}"
    );
    assert!(
        script.reload.policy().union().contains_key(&view),
        "but its paths are tracked"
    );
    assert!(
        script.marks.lock().expect("the marks").len() >= 2,
        "and the parent was marked in the same wakeup (§4.4)"
    );
    script.advance(1_000);
    assert!(script.tick().is_empty(), "a second later, still nothing");
}

/// §4.4: a view can only ever speak as itself — a datagram whose `source` field
/// is not the one whose descriptor it arrived on is dropped, not applied.
#[test]
fn a_forged_source_is_dropped_and_never_tracked() {
    let fixture = Fixture::new("forged");
    let mut script = Script::boot(&fixture, Vec::new());
    let other = fixture.path("views/products.py");
    let write = {
        let reports = script.reload.policy().reports();
        report_pair(reports, 0, Class::Loaded, false).expect("a socketpair")
    };
    // The socket belongs to route 0; the packet claims the preload phase.
    let path = other.to_string_lossy().into_owned();
    let batch =
        Batch::new(PRELOAD_SOURCE, 0, true, false, vec![path]).expect("a well-formed batch");
    send_batch(&write, &batch);
    let mut fds = Vec::new();
    script.reload.poll_fds(&mut fds);
    let end = *fds.get(1).expect("one report end");
    let now = script.now();
    assert!(
        script.reload.on_fd(end, PollFlags::IN, now).is_empty(),
        "a forged `source` never arms the window"
    );
    assert!(
        !script.reload.policy().union().contains_key(&other),
        "and the path it named is never tracked"
    );
}
