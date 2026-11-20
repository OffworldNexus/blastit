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

def _sockets():
    # The inodes of every AF_UNIX socket this process holds. A view is
    # `PR_SET_DUMPABLE(0)`, so no other process may read /proc/<pid>/fd; the
    # worker's own report is the only witness of its descriptor table.
    inodes = []
    for name in os.listdir("/proc/self/fd"):
        try:
            link = os.readlink("/proc/self/fd/" + name)
        except OSError:
            continue
        if link.startswith("socket:["):
            inodes.append(link[8:-1])
    return ",".join(sorted(inodes))

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
    body = ("%s|%d|%s|%s|%d|%s" % (view.render(), os.getpid(), template, outside,
                                    len(os.listdir("/proc/self/fd")),
                                    _sockets())).encode()
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
        Self::start_with_env(dir, tag, format, extra, &[])
    }

    /// Boots `blastit dev` with extra environment for the debug-only fault
    /// seams (`BLASTIT_TEST_RELOAD_FAULT`, read by stage 0 and carried across
    /// the `execve` in the configuration document).
    fn start_with_env(
        dir: &Path,
        tag: &str,
        format: &str,
        extra: &[&str],
        env: &[(&str, &str)],
    ) -> Self {
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
        for (name, value) in env {
            command.env(name, value);
        }
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
        let components = self.component_pids();
        assert_eq!(
            components.len(),
            2,
            "http and the scheduler, and no other component: {components:?}"
        );
        components[1]
    }

    /// Every live non-view child of init: `http`, the scheduler (and dbproxy
    /// when the fixture declares one), in init's fork order.
    fn component_pids(&self) -> Vec<i32> {
        let children =
            fs::read_to_string(format!("/proc/{}/task/{}/children", self.pid(), self.pid()))
                .expect("init's child list");
        let views: Vec<u32> = self.live_views().into_iter().map(|(_, pid)| pid).collect();
        children
            .split_whitespace()
            .filter_map(|pid| pid.parse::<i32>().ok())
            .filter(|pid| !views.contains(&u32::try_from(*pid).unwrap_or(u32::MAX)))
            .collect()
    }

    /// Opens one keep-alive connection and sends the first request on it.
    ///
    /// The response is read by `Content-Length`, not to end-of-file, so the
    /// same socket can carry a second request — which is the whole point of the
    /// `:8080` leg: the listener and the accepted connection survive a cycle.
    fn keep_alive(&self, path: &str) -> TcpStream {
        let mut stream = TcpStream::connect(("127.0.0.1", self.port))
            .unwrap_or_else(|err| panic!("connecting for {path}: {err}\n{}", self.diagnostic()));
        stream
            .set_read_timeout(Some(EVENT_TIMEOUT))
            .expect("a read timeout");
        write!(
            stream,
            "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: keep-alive\r\n\r\n"
        )
        .expect("the request goes out");
        let body = read_one_response(&mut stream, path);
        assert!(body.is_some(), "{path}: the first response had no body");
        stream
    }

    /// Sends a second request on an already-open keep-alive connection.
    fn keep_alive_get(&self, stream: &mut TcpStream, path: &str) -> String {
        write!(
            stream,
            "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: keep-alive\r\n\r\n"
        )
        .expect("the request goes out");
        read_one_response(stream, path).unwrap_or_else(|| {
            panic!(
                "{path}: the keep-alive connection had no answer\n{}",
                self.diagnostic()
            )
        })
    }

    /// Waits (bounded) until `path` answers `200`, and returns the body.
    ///
    /// Used only after a cycle, where the request can arrive while the new
    /// scheduler is still attaching to `http.chan`.
    fn wait_answers(&self, path: &str) -> String {
        let deadline = Instant::now() + EVENT_TIMEOUT;
        while Instant::now() < deadline {
            if let Ok(Some(body)) = try_get(self.port, path) {
                return body;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!(
            "{path} never answered after the reload\n{}",
            self.diagnostic()
        );
    }

    /// The `--dev-root` this boot adopted, read from init's own argv.
    fn dev_root(&self) -> PathBuf {
        let raw = fs::read(format!("/proc/{}/cmdline", self.pid())).expect("init's argv");
        raw.split(|byte| *byte == 0)
            .filter_map(|part| std::str::from_utf8(part).ok())
            .find_map(|token| token.strip_prefix("--dev-root="))
            .map(PathBuf::from)
            .unwrap_or_else(|| panic!("no --dev-root in init's argv\n{}", self.diagnostic()))
    }

    /// Waits (bounded) until `path` exists.
    fn wait_path(&self, path: &Path) {
        let deadline = Instant::now() + EVENT_TIMEOUT;
        while Instant::now() < deadline {
            if path.is_dir() {
                return;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!("{} never appeared\n{}", path.display(), self.diagnostic());
    }

    /// Waits (bounded) until a scheduler pid different from `before` is live
    /// beside `http` — the state that says a cycle's replacement image is up.
    fn wait_new_scheduler(&self, before: i32) -> i32 {
        let deadline = Instant::now() + EVENT_TIMEOUT;
        while Instant::now() < deadline {
            let components = self.component_pids();
            if components.len() == 2 && components[1] != before {
                return components[1];
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!(
            "no replacement scheduler within {EVENT_TIMEOUT:?}: {:?}\n{}",
            self.component_pids(),
            self.diagnostic()
        )
    }

    /// Waits (bounded) until `path` answers a body containing `needle`.
    ///
    /// After a cycle the old scheduler can still answer for a moment, so a
    /// plain `200` is not evidence that the replacement image is serving; the
    /// new content is.
    fn wait_body_containing(&self, path: &str, needle: &str) -> String {
        let deadline = Instant::now() + EVENT_TIMEOUT;
        while Instant::now() < deadline {
            if let Ok(Some(body)) = try_get(self.port, path)
                && body.contains(needle)
            {
                return body;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!(
            "{path} never served a body containing {needle:?}\n{}",
            self.diagnostic()
        )
    }

    /// Waits (bounded) until one `dev.reload.unavailable` has been emitted.
    fn wait_unavailable(&self) -> Value {
        let deadline = Instant::now() + EVENT_TIMEOUT;
        while Instant::now() < deadline {
            if let Some(event) = self
                .events()
                .into_iter()
                .find(|event| event["event"] == "dev.reload.unavailable")
            {
                return event;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!(
            "no `dev.reload.unavailable` within {EVENT_TIMEOUT:?}\n{}",
            self.diagnostic()
        )
    }

    /// Waits (bounded) until a `manifest.warning` whose sentence contains
    /// `needle` has been emitted, and returns it.
    fn wait_warning(&self, needle: &str) -> Value {
        let deadline = Instant::now() + EVENT_TIMEOUT;
        while Instant::now() < deadline {
            if let Some(event) = self.events().into_iter().find(|event| {
                event["event"] == "manifest.warning"
                    && event["message"]
                        .as_str()
                        .is_some_and(|message| message.contains(needle))
            }) {
                return event;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        panic!(
            "no `manifest.warning` naming {needle:?} within {EVENT_TIMEOUT:?}\n{}",
            self.diagnostic()
        );
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

    /// The dev policy channel's stage-1 end (`MB.23.5.3`): its descriptor number
    /// and the socket inode `/proc/<pid>/fd/<n>` names, or `None` when this boot
    /// has no channel (`--no-reload`, and every non-`dev` boot).
    ///
    /// init is the one process that is readable from here (the forked roles keep
    /// `PR_SET_DUMPABLE(0)`), and its `--dev-policy-fd` argument is what stage 0
    /// handed stage 1 across the `execve`.
    fn policy_channel(&self) -> Option<(i32, u64)> {
        let cmdline = fs::read(format!("/proc/{}/cmdline", self.pid())).ok()?;
        let text = String::from_utf8_lossy(&cmdline);
        let number = text
            .split('\0')
            .find_map(|argument| argument.strip_prefix("--dev-policy-fd="))?
            .parse::<i32>()
            .ok()?;
        let link = fs::read_link(format!("/proc/{}/fd/{number}", self.pid())).ok()?;
        let inode = link
            .to_str()?
            .strip_prefix("socket:[")?
            .strip_suffix(']')?
            .parse::<u64>()
            .ok()?;
        Some((number, inode))
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

/// One `GET`, returning the body for a `200` and `None` for anything else.
///
/// The non-panicking sibling of [`Dev::get`], for the window right after a
/// cycle when `http` may not have a scheduler attached yet — or may still be
/// holding a request for the scheduler it is about to lose. The read timeout is
/// short on purpose: a request that raced the old scheduler's death may never
/// be answered, and [`Dev::wait_answers`] retries on a fresh connection.
fn try_get(port: u16, path: &str) -> std::io::Result<Option<String>> {
    let mut stream = TcpStream::connect(("127.0.0.1", port))?;
    stream.set_read_timeout(Some(Duration::from_millis(500)))?;
    write!(
        stream,
        "GET {path} HTTP/1.1\r\nHost: 127.0.0.1\r\nConnection: close\r\n\r\n"
    )?;
    let mut raw = Vec::new();
    let mut buf = [0u8; 4096];
    while raw.len() < MAX_RESPONSE {
        match stream.read(&mut buf) {
            Ok(0) => break,
            Ok(read) => raw.extend_from_slice(&buf[..read]),
            Err(err) => return Err(err),
        }
    }
    let text = String::from_utf8_lossy(&raw).into_owned();
    match text.split_once("\r\n\r\n") {
        Some((head, body)) if head.starts_with("HTTP/1.1 200") => Ok(Some(body.to_string())),
        _ => Ok(None),
    }
}

/// Reads one HTTP/1.1 response off a keep-alive connection.
///
/// The head is read one byte at a time so not one byte of the next response is
/// consumed, and the body by its `Content-Length` — the only way a second
/// request on the same socket can be answered unambiguously.
fn read_one_response(stream: &mut TcpStream, path: &str) -> Option<String> {
    let mut head = Vec::new();
    let mut byte = [0u8; 1];
    while !head.ends_with(b"\r\n\r\n") {
        match stream.read(&mut byte) {
            Ok(1) => head.push(byte[0]),
            Ok(_) => return None,
            Err(_) => return None,
        }
        if head.len() > MAX_RESPONSE {
            return None;
        }
    }
    let head = String::from_utf8_lossy(&head).into_owned();
    if !head.starts_with("HTTP/1.1 200") {
        return None;
    }
    let length = head
        .lines()
        .find_map(|line| line.strip_prefix("Content-Length: "))
        .and_then(|value| value.trim().parse::<usize>().ok())?;
    let mut body = vec![0u8; length];
    stream.read_exact(&mut body).ok()?;
    let _ = path;
    Some(String::from_utf8_lossy(&body).into_owned())
}

/// The body an answer carries:
/// `"<tag>:<version>|<pid>|<template>|<outside>|<open descriptors>|<socket inodes>"`.
///
/// The descriptor count is how `--no-reload` is witnessed: the view that got no
/// report socket (and so no audit hook, §5 `S3`) holds exactly one fewer. The
/// socket inodes are how the view's own fd table witnesses that it holds no end
/// of the dev policy channel (§4.6 step 2): a view is non-dumpable, so no other
/// process may read its `/proc/<pid>/fd`.
#[derive(Debug, PartialEq, Eq)]
struct Body {
    tag: String,
    version: u32,
    pid: u32,
    template: String,
    outside: String,
    fds: usize,
    sockets: Vec<u64>,
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
        sockets: fields
            .next()
            .unwrap_or_default()
            .split(',')
            .filter(|inode| !inode.is_empty())
            .map(|inode| {
                inode
                    .parse()
                    .unwrap_or_else(|_| panic!("no socket inode in {inode:?}"))
            })
            .collect(),
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
    let previous = dev.scheduler_pid();

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
    // A manifest save takes the **cycle**: the image re-execs and its
    // replacement answers on the same `:8080`. Wait for the replacement's
    // scheduler before the requests below, so they pin its views and not a
    // request that raced the scheduler the cycle stopped.
    dev.wait_new_scheduler(previous);
    dev.wait_answers("/cart/x");

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
    assert!(
        watched.policy_channel().is_some(),
        "a reload-on boot creates the dev policy channel and hands it to stage 1"
    );
    watched.stop();

    let mut dev = fixture.start_with("no-reload", "json", &["--no-reload"]);
    let cart = body_of(&dev.get("/cart/x"));
    let products = body_of(&dev.get("/products/x"));
    assert_eq!(cart.tag, "cart-1", "{cart:?}");
    assert_eq!(products.tag, "products-1", "{products:?}");
    assert!(
        dev.policy_channel().is_none(),
        "`--no-reload` creates no policy channel: stage 1 gets no `--dev-policy-fd`"
    );
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
    let previous_scheduler = dev.scheduler_pid();

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

    // Every view pid changed, and each answer comes from a replacement. An
    // overflow takes the **cycle** (a dropped event may have been the
    // manifest save), so a request sent before the replacement image answers
    // can be held by `http` for the scheduler the cycle stopped: wait for a
    // `200` rather than read on one connection until it times out.
    dev.wait_replaced(&before);
    dev.wait_new_scheduler(previous_scheduler);
    dev.wait_answers("/cart/x");
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

/// The boot report: one pretty `reload` event in the same columns as every
/// other line, and exactly one `reload` event on the machine stream — never a
/// raw free-text line (§4.7, §4.9).
#[test]
fn the_boot_line_is_one_pretty_line_and_never_an_event() {
    let fixture = Fixture::new("line");
    let mut pretty = fixture.start("line-pretty", "pretty");
    let transcript = pretty.stop();
    // The summary column, measured as the byte offset of the fourth whitespace
    // token, so "aligned with the other events" is a fact, not an assumption.
    let summary_column = |line: &str| -> usize {
        let mut tokens = 0;
        let mut in_token = false;
        for (index, character) in line.char_indices() {
            if character.is_whitespace() {
                in_token = false;
            } else if !in_token {
                in_token = true;
                tokens += 1;
                if tokens == 4 {
                    return index;
                }
            }
        }
        panic!("a line with a summary column: {line:?}")
    };
    let reloads: Vec<&String> = transcript
        .iter()
        .filter(|line| line.split_whitespace().nth(2) == Some("reload"))
        .collect();
    assert_eq!(reloads.len(), 1, "one `reload` event, once: {transcript:?}");
    let line = reloads[0];
    assert!(
        !transcript.iter().any(|line| line.contains("reload: on (")),
        "the report is an event, not free text: {transcript:?}"
    );
    let tokens: Vec<&str> = line.split_whitespace().collect();
    let count: usize = tokens
        .get(3)
        .and_then(|count| count.parse().ok())
        .unwrap_or_else(|| panic!("the documented count, exactly: {line:?}"));
    let noun = if count == 1 {
        "directory"
    } else {
        "directories"
    };
    assert_eq!(
        tokens.get(4),
        Some(&noun),
        "`1 directory` / `N directories`: {line:?}"
    );
    assert!(count >= 1, "the app root is always marked: {line:?}");
    let ready = transcript
        .iter()
        .find(|line| line.split_whitespace().nth(2) == Some("ready"))
        .unwrap_or_else(|| panic!("a `ready` event: {transcript:?}"));
    assert_eq!(
        summary_column(line),
        summary_column(ready),
        "the reload report sits in every other event's columns: {line:?} vs {ready:?}"
    );

    let mut json = fixture.start("line-json", "json");
    assert!(
        json.is_marked(&fixture.path("")),
        "a reload-on boot has one group and marks the app root"
    );
    let transcript = json.stop();
    assert!(
        !transcript.iter().any(|line| line.contains("reload: on (")),
        "no free-text report survives on either stream: {transcript:?}"
    );
    let reloads: Vec<Value> = json
        .events()
        .into_iter()
        .filter(|event| event["event"] == "reload")
        .collect();
    assert_eq!(
        reloads.len(),
        1,
        "one `reload` event on the machine stream, once: {reloads:?}"
    );
    let directories = reloads[0]["directories"]
        .as_u64()
        .unwrap_or_else(|| panic!("the documented count: {}", reloads[0]));
    assert!(
        directories >= 1,
        "the app root is always marked: {reloads:?}"
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

// ------------------------------------------------- the reload cycle (§4.6)

/// The `Blastfile` the cycle writes over the fixture's: valid, both routes.
fn two_route_blastfile() -> String {
    BLASTFILE.to_string()
}

/// Row 7a–7c: an invalid `Blastfile` save is refused with one sentence and the
/// topology keeps serving; the next valid save replaces the topology exactly
/// once, and `:8080` — including a keep-alive connection opened **before** the
/// save — never drops.
///
/// What moves: the scheduler and every view (the cycle re-execs the image).
/// What must not: the supervisor and `http`, because `http` owns the listener
/// and the accepted connection, and this test names only those two.
#[test]
fn an_invalid_blastfile_keeps_serving_and_the_next_valid_one_reloads_once() {
    let fixture = Fixture::new("blastfile");
    let mut dev = fixture.start("blastfile", "json");
    let first = body_of(&dev.get("/products/x"));
    // Opened before the save: its byte stream has to survive the cycle.
    let mut kept = dev.keep_alive("/products/x");
    let supervisor = dev.pid();
    let before = dev.component_pids();
    assert_eq!(before.len(), 2, "http and the scheduler: {before:?}");
    let (http, scheduler) = (before[0], before[1]);

    // 7a: a half-saved manifest. The parser's own sentence, one event, no
    // reload, no exec, and the old topology keeps answering.
    fs::write(
        fixture.path("Blastfile"),
        "app \"shop\"\n\nentrypoint python \"app:app\"\n\nview products regex '/products/.*' {\n",
    )
    .expect("the invalid manifest");
    let warning = dev.wait_warning("Blastfile");
    assert_eq!(warning["level"], "warn", "{warning}");
    let message = warning["message"].as_str().unwrap_or_default().to_string();
    assert!(
        message.contains("Blastfile") && message.contains("6:1"),
        "one sentence carrying the file's own position: {message}"
    );
    assert_eq!(
        dev.events()
            .iter()
            .filter(|event| event["event"] == "manifest.warning")
            .count(),
        1,
        "one warning, never one per attempt"
    );
    assert!(
        dev.reloads().is_empty(),
        "an invalid manifest must not reload anything"
    );
    assert_eq!(
        body_of(&dev.get("/products/x")).pid,
        first.pid,
        "the previous topology keeps serving the invalid save"
    );

    // 7b: the valid save. One `dev.reload`, the **re-parsed** manifest's routes
    // in their new order — `cart` first, a `checkout` route added — and the
    // added route answers on `:8080` from a private directory the first image
    // never created. Emitting the boot's `self.routes` fails this.
    fs::write(
        fixture.path("Blastfile"),
        "app \"shop\"\n\nentrypoint python \"app:app\"\n\n\
         view cart regex '/cart/.*' {\n}\n\n\
         view products regex '/products/.*' {\n}\n\n\
         view checkout regex '/checkout/.*' {\n}\n",
    )
    .expect("the valid manifest");
    let reloads = dev.wait_reloads(1);
    assert_eq!(reloads.len(), 1, "exactly one reload for the valid save");
    assert_eq!(
        routes_of(&reloads[0]),
        vec![
            "cart".to_string(),
            "products".to_string(),
            "checkout".to_string()
        ],
        "the re-parsed manifest's routes, in their new order: {reloads:?}"
    );
    assert!(
        files_of(&reloads[0])
            .iter()
            .any(|file| file.ends_with("Blastfile")),
        "the event names the file that changed: {reloads:?}"
    );
    let cart = dev.wait_answers("/cart/x");
    assert!(
        cart.contains("cart-1:1|"),
        "the renamed first route serves the new topology: {cart:?}"
    );
    let checkout = dev.wait_answers("/checkout/x");
    assert!(
        checkout.contains("|"),
        "the route the save added answers on `:8080`: {checkout:?}"
    );
    // The added route's private directory (index 2, past the two the first
    // image created) is this image's work (`stage1` adopt path).
    dev.wait_path(&dev.dev_root().join("views/2"));

    // 7c: the listener never dropped — the connection opened before the save
    // still gets its response.
    let after_keep_alive = dev.keep_alive_get(&mut kept, "/products/x");
    assert!(
        after_keep_alive.contains("products-1:1|"),
        "the pre-save keep-alive connection still answers: {after_keep_alive:?}"
    );

    // 7c: the non-view pids survive, the scheduler does not.
    let after = dev.component_pids();
    assert_eq!(after.len(), 2, "http and the new scheduler: {after:?}");
    assert_eq!(dev.pid(), supervisor, "the supervisor is the same process");
    assert_eq!(after[0], http, "http owns `:8080` and is never replaced");
    assert_ne!(
        after[1], scheduler,
        "the cycle starts a new scheduler on the adopted `http.chan` end"
    );
    dev.stop();
}

/// Row 7d: a `preload` module edit re-runs the preload cycle exactly once, and
/// a half-written module keeps the old preload serving with one warning.
///
/// The module is imported before the fork (`preload "models"`), so every view's
/// heap already holds it: only a fresh interpreter image can serve a new one,
/// which is why this takes the cycle and not the all-views transaction.
#[test]
fn editing_a_preload_module_reloads_the_cycle_once() {
    let fixture = Fixture::new("preload-cycle");
    fs::write(
        fixture.path("Blastfile"),
        "app \"shop\"\n\nentrypoint python \"app:app\" {\n    preload \"models\"\n}\n\nview products regex '/products/.*' {\n}\n",
    )
    .expect("the preload manifest");
    let mut dev = fixture.start("preload-cycle", "json");
    let first = body_of(&dev.get("/products/x"));
    assert_eq!(first.version, 1, "the preloaded module says VERSION = 1");

    // A half-written module: the compile check refuses it before anything is
    // torn down, and the old preload keeps serving.
    fs::write(fixture.path("models.py"), "VERSION = (\n").expect("the half-written module");
    let warning = dev.wait_warning("models.py");
    assert_eq!(warning["level"], "warn", "{warning}");
    assert!(dev.reloads().is_empty(), "a broken module reloads nothing");
    let still = body_of(&dev.get("/products/x"));
    assert_eq!(still.version, 1, "the old preload keeps serving");
    assert_eq!(still.pid, first.pid, "and its view was never replaced");

    // The valid module: exactly one cycle, and the next view serves it.
    fs::write(fixture.path("models.py"), "VERSION = 2\n").expect("the fixed module");
    let reloads = dev.wait_reloads(1);
    assert_eq!(reloads.len(), 1, "exactly one cycle for the fixed module");
    assert!(
        files_of(&reloads[0])
            .iter()
            .any(|file| file.ends_with("models.py")),
        "the event names the preload module: {reloads:?}"
    );
    let fresh = dev.wait_body_containing("/products/x", ":2|");
    assert!(
        fresh.contains(":2|"),
        "the replacement image imported the new preload module: {fresh:?}"
    );
    dev.stop();
}

/// Row 8: the failure ladder's `EPERM` row is one `dev.reload.unavailable` and
/// a run that keeps serving — never a retry, never a second watcher.
///
/// The boot is real: `BLASTIT_TEST_RELOAD_FAULT=open` (debug builds only, read
/// by stage 0 and carried across the `execve` in the configuration document)
/// makes the watcher construction fail `EPERM`, which is the ladder's
/// "not permitted here" row. What is asserted is the whole contract: exactly one
/// warning, no `dev.reload` at all, no `fanotify` descriptor (so no watcher and
/// therefore no second one), and `200` for the rest of the run.
#[test]
fn an_eperm_watcher_is_one_unavailable_and_the_run_keeps_serving() {
    let fixture = Fixture::new("eperm");
    let mut dev = Dev::start_with_env(
        &fixture.dir,
        "eperm",
        "json",
        &[],
        &[("BLASTIT_TEST_RELOAD_FAULT", "open")],
    );
    let unavailable = dev.wait_unavailable();
    assert_eq!(unavailable["level"], "warn", "{unavailable}");
    assert_eq!(unavailable["errno"], "EPERM", "the ladder's symbolic errno");
    assert!(
        unavailable["filesystem"]
            .as_str()
            .is_some_and(|filesystem| !filesystem.is_empty()),
        "the event names the filesystem it read: {unavailable}"
    );
    assert!(
        dev.reloads().is_empty(),
        "the ladder never retries a reload"
    );
    assert_eq!(
        dev.fanotify_fds(),
        0,
        "no watcher exists, so no group and no poll duplicate"
    );
    for _ in 0..3 {
        assert!(
            dev.get("/products/x").contains("products-1:1|"),
            "the run keeps answering after the ladder's one warning"
        );
    }
    assert_eq!(
        dev.events()
            .iter()
            .filter(|event| event["event"] == "dev.reload.unavailable")
            .count(),
        1,
        "one warning, never one per wakeup"
    );
    dev.stop();
}

/// A scheduler death in a reload's **replacement image** is fatal and still
/// `kill_all`s every view.
///
/// What this witnesses, exactly: the replacement image's `Supervisor::new`
/// starts with `reloading == false` (the gate is a field of the old image's
/// supervisor and dies with its `execve`), so after a completed cycle the very
/// next scheduler death takes the base fatal path — non-zero exit, no surviving
/// view. That is the state the epic needs: a reload must never leave a boot that
/// excuses a later `scheduler-died`.
///
/// What it does **not** witness: a gate that "survived" the replacement, since
/// there is no such value — `reloading` is never carried across the exec. The
/// two in-image clears are pinned by unit legs in `supervise.rs`:
/// `a_late_ctl_write_to_a_gone_scheduler_is_best_effort` for the expected
/// scheduler exit, and
/// `a_cycle_whose_views_never_drain_is_abandoned_without_a_signal` for the
/// drain-expiry abandon (it asserts `reloading == false` on real state, and
/// that a reinstated `kill_all` kills its children).
///
/// Why the earlier row could not do this
/// (`a_scheduler_exit_after_a_reload_still_stops_the_boot`): it kills the
/// scheduler after an *all-views transaction*, where no gate is ever set, and
/// `http` dies with it — fatal in every version — so it can pass through
/// `http-died` and proves nothing about a post-cycle image.
#[test]
fn a_scheduler_exit_after_a_cycle_is_still_fatal() {
    let fixture = Fixture::new("cycle-gate");
    let mut dev = fixture.start("cycle-gate", "json");
    let first = body_of(&dev.get("/products/x"));
    let previous = dev.scheduler_pid();
    fs::write(fixture.path("Blastfile"), two_route_blastfile()).expect("the valid manifest");
    dev.wait_reloads(1);
    // A view in the replacement image, so there is a pid for `kill_all` to reap.
    dev.wait_replaced(&[("products".to_string(), first.pid)]);
    dev.wait_new_scheduler(previous);
    let fresh = body_of(&dev.wait_answers("/products/x"));
    assert_ne!(fresh.pid, first.pid, "the cycle replaced the view");
    let scheduler = dev.scheduler_pid();

    // SAFETY: `kill` takes a pid this test owns and a signal number.
    unsafe { libc::kill(scheduler, libc::SIGKILL) };
    let status = dev.wait_exit();
    assert!(
        !status.success(),
        "a scheduler death in the replacement image is fatal, got {status}"
    );
    let deadline = Instant::now() + EVENT_TIMEOUT;
    while Path::new(&format!("/proc/{}", fresh.pid)).exists() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(
        !Path::new(&format!("/proc/{}", fresh.pid)).exists(),
        "the view of the replacement image outlived init: `kill_all` did not run"
    );
}

/// Acceptance 6: under `--learn` the manifest cycle does not run.
///
/// The one-pass tally lives in this image, so a re-exec would discard it: a
/// `Blastfile` save emits one `manifest.warning` naming the restart and keeps
/// the previous topology serving, while a `loaded` change still reloads views.
#[test]
fn learn_skips_the_manifest_cycle_and_still_reloads_views() {
    let fixture = Fixture::new("learn-skip");
    let mut dev = fixture.start_with("learn-skip", "json", &["--learn"]);
    let first = body_of(&dev.get("/products/x"));

    let manifest = fixture.path("Blastfile");
    let original = fs::read_to_string(&manifest).expect("the manifest");
    fs::write(
        &manifest,
        format!("{original}\n# under --learn this is not applied\n"),
    )
    .expect("the save");
    let warning = dev.wait_warning("restart `blastit dev --learn`");
    assert_eq!(warning["level"], "warn", "{warning}");
    assert!(
        dev.reloads().is_empty(),
        "the manifest cycle must not run under --learn"
    );
    assert_eq!(
        body_of(&dev.get("/products/x")).pid,
        first.pid,
        "the previous topology keeps serving"
    );
    assert_eq!(
        dev.events()
            .iter()
            .filter(|event| event["event"] == "manifest.warning")
            .count(),
        1,
        "one warning, not one per wakeup"
    );

    // A view file is `loaded`: the all-views transaction still runs.
    fs::write(fixture.path("views/products.py"), view_source("products-2")).expect("the view save");
    let reloads = dev.wait_reloads(1);
    assert_eq!(reloads.len(), 1, "one reload for the view save");
    assert_eq!(reloads[0]["reason"], "loaded", "{reloads:?}");
    let fresh = dev.wait_body_containing("/products/x", "products-2");
    assert!(fresh.contains("products-2"), "{fresh:?}");
    dev.stop();
}

// ------------------------------- the dev policy channel (`MB.23.5.3`, §4.6/§4.8)

/// §4.6 step 2 / §4.8: a dev boot with reload on hands stage 1 one end of a
/// `socket`-pair and dbproxy the other, and **no view holds either end**.
///
/// What is witnessed, and how:
///
/// * stage 1's `/proc/<pid>/cmdline` carries `--dev-policy-fd=N` and
///   `/proc/<pid>/fd/N` is a socket — stage 0 created the channel and passed it
///   across the `execve` exactly once;
/// * the answering view's own report (`/proc/self/fd`, the only readable fd
///   table: a view is non-dumpable) holds no socket inode equal to that end;
/// * a view born after a reload holds none either;
/// * the channel never moves: the same descriptor number and inode after the
///   reload that replaced every view, because the supervisor is not replaced and
///   the socket is never re-bound (§4.8);
/// * dbproxy's end cannot be in a view: stage 0 drops the parent's copy of it
///   once dbproxy has its own (`run_dev`'s `drop(dbproxy)`), so the only process
///   a fork could inherit it from never holds it.
#[test]
fn the_policy_channel_reaches_stage_one_and_no_view_holds_it() {
    let fixture = Fixture::new("policy-fd");
    let mut dev = fixture.start("policy-fd", "json");
    let first = body_of(&dev.get("/products/x"));
    let (number, inode) = dev
        .policy_channel()
        .expect("a reload-on dev boot hands stage 1 a policy descriptor");
    assert!(number >= 0, "the descriptor number is a real fd: {number}");
    assert!(
        !first.sockets.contains(&inode),
        "the answering view holds no end of the policy channel: its sockets are {:?}, \
         the channel is {inode}",
        first.sockets
    );

    // A reload replaces every view; the replacement still inherits nothing.
    // The view's report is what puts `views/products.py` in the watched set, so
    // the mark is the state — not a sleep — that says the save can be seen.
    dev.wait_marked(&[fixture.path("views")]);
    fs::write(fixture.path("views/products.py"), view_source("products-2")).expect("the save");
    dev.wait_reloads(1);
    let fresh = body_of(&dev.wait_body_containing("/products/x", "products-2"));
    assert_ne!(fresh.pid, first.pid, "the reload replaced the view");
    assert!(
        !fresh.sockets.contains(&inode),
        "the replacement view holds no end either: {:?}",
        fresh.sockets
    );
    assert_eq!(
        dev.policy_channel(),
        Some((number, inode)),
        "one channel per boot image: the descriptor and the socket are never re-created"
    );
    dev.stop();
}

// ------------------------------------------- row 10: PHP live reload (§4.4, §4.6)

/// The `Blastfile` row 10 uses: two routes, one embeddable PHP front controller
/// — the shape `docs/frameworks/php.md` documents (`entrypoint php`, no app).
#[cfg(feature = "php")]
const PHP_BLASTFILE: &str = "\
entrypoint php \"public/index.php\"

view app regex '/app/.*' {
}

view api regex '/api/.*' {
}
";

/// The front controller: it echoes the tag an **included** file carries, its own
/// pid, and the content of a **compiled template artefact** the framework reads
/// and revalidates (`file_get_contents`, never `include`) — so a save of the
/// artefact is not a loaded-file change and must not reload anything.
#[cfg(feature = "php")]
const PHP_ENTRY: &str = r#"<?php
header('Content-Type: text/plain');
require __DIR__ . '/version.php';
$compiled = @file_get_contents(__DIR__ . '/../storage/framework/views/compiled.php');
echo $TAG . ':' . getmypid() . ':' . trim((string) $compiled) . "\n";
"#;

/// The included file row 10 saves.
#[cfg(feature = "php")]
fn php_version_source(tag: &str) -> String {
    format!("<?php\n$TAG = '{tag}';\n")
}

/// A two-route PHP fixture: the manifest, the front controller and the included
/// file, under a fresh temporary directory.
#[cfg(feature = "php")]
struct PhpFixture {
    dir: PathBuf,
}

#[cfg(feature = "php")]
impl PhpFixture {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!(
            "blastit-php-reload-{tag}-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(dir.join("public")).expect("the fixture tree");
        fs::create_dir_all(dir.join("storage/framework/views")).expect("the cache tree");
        fs::write(dir.join("Blastfile"), PHP_BLASTFILE).expect("the manifest");
        fs::write(dir.join("public/index.php"), PHP_ENTRY).expect("the front controller");
        fs::write(dir.join("public/version.php"), php_version_source("v1")).expect("the include");
        fs::write(
            dir.join("storage/framework/views/compiled.php"),
            "compiled-v1",
        )
        .expect("the warm compiled artefact");
        Self { dir }
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.dir.join(relative)
    }
}

#[cfg(feature = "php")]
impl Drop for PhpFixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.dir);
    }
}

/// The host's embeddable PHP: the NTS `libphp` and the shim for its minor.
#[cfg(feature = "php")]
struct HostPhp {
    libphp: PathBuf,
    shim: PathBuf,
}

/// The host probe the PHP legs of `crates/blastit/tests/e2e_dev.rs` established,
/// copied small: raw `std` file access and the `ADR-0010` §4.1 names, never the
/// code under test. The library's bytes must name `executor_globals` and
/// `php_version\0` and must not name `tsrm_startup` (a ZTS build, which the boot
/// refuses), and the shim for the same minor must exist next to the test binary.
#[cfg(feature = "php")]
fn host_php() -> Option<HostPhp> {
    fn names(bytes: &[u8], needle: &[u8]) -> bool {
        bytes.windows(needle.len()).any(|window| window == needle)
    }
    let libphp = php_libphp()?;
    let bytes = fs::read(&libphp).ok()?;
    if !names(&bytes, b"executor_globals")
        || !names(&bytes, b"php_version\0")
        || names(&bytes, b"tsrm_startup")
    {
        return None;
    }
    let (major, minor) = php_minor(&libphp)?;
    let shim = php_shim(major, minor)?;
    Some(HostPhp { libphp, shim })
}

/// `BLASTIT_LIBPHP`, else the probe order stage 0 walks: minor first, spelling
/// second, so a host with two embed packages serves the newest.
#[cfg(feature = "php")]
fn php_libphp() -> Option<PathBuf> {
    if let Some(path) = std::env::var_os("BLASTIT_LIBPHP") {
        let path = PathBuf::from(path);
        return path.is_file().then(|| path.canonicalize().ok()).flatten();
    }
    let libc = if cfg!(target_env = "musl") {
        "musl"
    } else {
        "gnu"
    };
    let triplet = format!("{}-linux-{libc}", std::env::consts::ARCH);
    let dirs = [
        format!("/usr/lib/{triplet}"),
        "/usr/lib64".to_string(),
        "/usr/lib".to_string(),
    ];
    for minor in [85u32, 84, 83] {
        let dotted = minor % 10;
        for name in [
            format!("libphp8.{dotted}.so"),
            format!("libphp-8.{dotted}.so"),
            format!("libphp{minor}.so"),
        ] {
            for dir in &dirs {
                let path = Path::new(dir).join(&name);
                if path.is_file() {
                    return path.canonicalize().ok();
                }
            }
        }
        let path = Path::new("/usr/lib")
            .join(format!("php{minor}"))
            .join("libphp.so");
        if path.is_file() {
            return path.canonicalize().ok();
        }
    }
    None
}

/// `(major, minor)` from the file name (`libphp8.4.so`, `libphp84.so`,
/// `libphp-8.4.so`) or from the per-minor directory (`…/php84/libphp.so`).
#[cfg(feature = "php")]
fn php_minor(path: &Path) -> Option<(u32, u32)> {
    let name = path.file_name()?.to_str()?;
    let stem = name.strip_prefix("libphp")?.strip_suffix(".so")?;
    let stem = stem.strip_prefix('-').unwrap_or(stem);
    if let Some((major, minor)) = stem.split_once('.') {
        return Some((major.parse().ok()?, minor.parse().ok()?));
    }
    let packed = |digits: &str| -> Option<(u32, u32)> {
        let bytes = digits.as_bytes();
        (bytes.len() == 2 && bytes.iter().all(u8::is_ascii_digit))
            .then(|| (u32::from(bytes[0] - b'0'), u32::from(bytes[1] - b'0')))
    };
    if let Some(minor) = packed(stem) {
        return Some(minor);
    }
    packed(path.parent()?.file_name()?.to_str()?.strip_prefix("php")?)
}

/// The shim for `(major, minor)`, from `BLASTIT_PHP_SHIM_DIR`, beside the test
/// binary, or the `target/php-sapi` directory `make php-sapi` writes.
#[cfg(feature = "php")]
fn php_shim(major: u32, minor: u32) -> Option<PathBuf> {
    let arch = if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else {
        std::env::consts::ARCH
    };
    let libc = if cfg!(target_env = "musl") {
        "musl"
    } else {
        "gnu"
    };
    let name = format!("libblastit_php{major}{minor}-{arch}-{libc}.so");
    let exe = PathBuf::from(env!("CARGO_BIN_EXE_blastit"));
    let mut dirs = Vec::new();
    if let Some(dir) = std::env::var_os("BLASTIT_PHP_SHIM_DIR") {
        dirs.push(PathBuf::from(dir));
    }
    if let Some(dir) = exe.parent() {
        dirs.push(dir.to_path_buf());
    }
    if let Some(target) = exe
        .ancestors()
        .find(|dir| dir.file_name() == Some(std::ffi::OsStr::new("target")))
    {
        dirs.push(target.join("php-sapi"));
    }
    dirs.into_iter()
        .map(|dir| dir.join(&name))
        .find(|path| path.is_file())
        .and_then(|path| path.canonicalize().ok())
}

/// Boots `blastit dev` once and waits for it to exit, bounded: the refusal
/// branch's boot must not be allowed to serve for ever if the probe was wrong.
#[cfg(feature = "php")]
fn boot_refusal(dir: &Path) -> (Option<i32>, String) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_blastit"));
    command
        .args(["dev", "--port", &free_port().to_string()])
        .env_clear()
        .env("BLASTIT_MEM_HIGH_WATERMARK", "100")
        .env("BLASTIT_MAX_VIEW_PROCESSES", "8")
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
    let deadline = Instant::now() + EVENT_TIMEOUT;
    let mut exited = false;
    while Instant::now() < deadline {
        if child.try_wait().expect("waiting for the boot").is_some() {
            exited = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    if !exited {
        let _ = child.kill();
    }
    let output = child.wait_with_output().expect("the boot's output");
    let text = format!(
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    (output.status.code(), text)
}

/// Row 10: editing an included `.php` file reloads **every** view together, and
/// the next request on each route returns the new body.
///
/// The row runs in the PHP-enabled configuration (`--features php`) and needs the
/// host's NTS `libphp` + shim. [`host_php`] is that oracle, independent of the
/// code under test: a host without the pair has its boot refuse with the
/// documented package sentence (asserted here, never skipped), and the test says
/// so in one line. On a host with the pair — this one: `/usr/lib/libphp8.4.so`
/// and `target/php-sapi/` — the served branch runs.
#[cfg(feature = "php")]
#[test]
fn editing_an_included_php_file_reloads_every_view_once_and_serves_the_new_body() {
    let fixture = PhpFixture::new("php-reload");
    let Some(host) = host_php() else {
        // No pair on this host: `blastit dev` must refuse the PHP entrypoint
        // with the package sentence, and that is what is asserted.
        let (code, text) = boot_refusal(&fixture.dir);
        assert_eq!(
            code,
            Some(1),
            "a PHP boot without libphp must refuse: {text}"
        );
        assert!(
            text.contains("install the PHP embed package"),
            "the refusal names the distro package: {text}"
        );
        assert!(
            text.contains("public/index.php"),
            "and the entry it could not serve: {text}"
        );
        println!("refused (no usable libphp + shim on this host): {text}");
        return;
    };

    let libphp = host.libphp.to_string_lossy().into_owned();
    let shim_dir = host
        .shim
        .parent()
        .expect("the shim has a directory")
        .to_string_lossy()
        .into_owned();
    let mut dev = Dev::start_with_env(
        &fixture.dir,
        "php-reload",
        "json",
        &[],
        &[
            ("BLASTIT_LIBPHP", &libphp),
            ("BLASTIT_PHP_SHIM_DIR", &shim_dir),
        ],
    );
    let app = dev.get("/app/x");
    assert!(
        app.contains("v1:"),
        "the PHP front controller answers: {app}"
    );
    let api = dev.get("/api/x");
    assert!(api.contains("v1:"), "both routes answer: {api}");

    // The workers' own reports are what put the *included* file in the watched
    // set: the mark on `public/` is the state that says the batch was absorbed
    // (the same wait the WSGI rows use, not a sleep).
    dev.wait_marked(&[fixture.path("public")]);
    fs::write(fixture.path("public/version.php"), php_version_source("v2")).expect("the save");

    let reloads = dev.wait_reloads(1);
    assert_eq!(
        reloads.len(),
        1,
        "exactly one reload for the save: {reloads:?}"
    );
    assert_eq!(
        routes_of(&reloads[0]),
        vec!["app".to_string(), "api".to_string()],
        "every PHP view reloads together, in manifest order: {reloads:?}"
    );
    assert!(
        files_of(&reloads[0])
            .iter()
            .any(|file| file.ends_with("public/version.php")),
        "the event names the included file that changed: {reloads:?}"
    );

    // The next request on each route is served by a view that included the new
    // file — a saved `$_TAG` is what a real application's include looks like.
    let app = dev.wait_body_containing("/app/x", "v2:");
    assert!(
        app.contains("v2:"),
        "the app route serves the new include: {app}"
    );
    let api = dev.wait_body_containing("/api/x", "v2:");
    assert!(
        api.contains("v2:"),
        "the api route serves the new include: {api}"
    );

    // Acceptance 8's second half: a **warm compiled template artefact** needs no
    // restart. The framework reads and revalidates it (`file_get_contents`, never
    // `include`), so it is not in the workers' loaded-file report and a save of
    // it must produce **no** `dev.reload` — the next request picks it up by
    // itself. A reporting regression that put cache artefacts in the watched set
    // turns this into a reload storm, and this row goes red.
    fs::write(
        fixture.path("storage/framework/views/compiled.php"),
        "compiled-v2",
    )
    .expect("the warm artefact save");
    std::thread::sleep(SETTLE);
    assert_eq!(
        dev.reloads().len(),
        1,
        "a warm compiled artefact is revalidated, not reloaded: {:?}",
        dev.reloads()
    );
    let warm = dev.wait_body_containing("/app/x", "compiled-v2");
    assert!(
        warm.contains("compiled-v2"),
        "the framework revalidated the artefact with no restart: {warm}"
    );

    println!(
        "reloaded (php, {} + {}): {} view(s), {} file(s), both routes serve v2; \
         a warm compiled artefact needs no reload",
        host.libphp.display(),
        host.shim.display(),
        routes_of(&reloads[0]).len(),
        files_of(&reloads[0]).len()
    );
    dev.stop();
}

// ---------------------------------- row 7: a store added or removed (§4.8)

/// The manifest row 7 boots with: one `postgresql` store and one route that
/// grants a query on it. The `DATABASE_URL` the test sets is never dialled — a
/// dev boot binds the listener and stops there — so no database is needed.
const STORE_BLASTFILE: &str = "\
app \"shop\"

entrypoint python \"app:app\"

postgres $DATABASE_URL

view products regex '/products/.*' {
    postgres {
        query 'SELECT 1'
    }
}

view cart regex '/cart/.*' {
}
";

/// Row 7 (acceptance 7): a save that **removes** the store is one
/// `manifest.warning` naming the change and the restart, and `:8080` keeps
/// serving through it.
///
/// The sockets are bound at boot and never re-bound (§4.8), so a save can change
/// the grants of the listeners that exist but cannot create or destroy one. The
/// document is still applied afterwards, which is why the removed store fails
/// closed rather than lingering: dbproxy's listener for it is replaced by the
/// empty allowlist.
#[test]
fn removing_a_store_is_one_warning_and_the_run_keeps_serving() {
    let fixture = Fixture::new("stores");
    fs::write(fixture.path("Blastfile"), STORE_BLASTFILE).expect("the store manifest");
    let mut dev = Dev::start_with_env(
        &fixture.dir,
        "stores",
        "json",
        &[],
        &[("DATABASE_URL", "postgres://shop:secret@127.0.0.1:5433/shop")],
    );
    assert_eq!(body_of(&dev.get("/products/x")).tag, "products-1");
    // `scheduler_pid()` assumes two components; this fixture forks dbproxy too,
    // so the replacement is found by watching the component list instead.
    let before = dev.component_pids();
    assert_eq!(
        before.len(),
        3,
        "dbproxy, http and the scheduler: {before:?}"
    );

    // The save drops the store and its grant. No new socket can be bound, so the
    // documented answer is one warning saying exactly that.
    fs::write(fixture.path("Blastfile"), BLASTFILE).expect("the save");
    let warning = dev.wait_warning("listener set changed");
    assert_eq!(warning["level"], "warn", "{warning}");
    let message = warning["message"].as_str().unwrap_or_default().to_string();
    assert!(
        message.contains("products→db"),
        "the warning names the pair that went away: {message}"
    );
    assert!(
        message.contains("restart blastit dev"),
        "and the fix it needs: {message}"
    );

    // `:8080` never drops: the cycle replaces the scheduler behind the same
    // listener, and the run keeps answering. Waiting for the replacement first
    // keeps the request out of the drain window, where a refused `SPAWN` would
    // add its own (unrelated) warning.
    let deadline = Instant::now() + EVENT_TIMEOUT;
    let mut after = before.clone();
    while Instant::now() < deadline {
        after = dev.component_pids();
        if after.len() == before.len() && after != before {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_ne!(
        after, before,
        "the cycle replaced the scheduler, not `http` or dbproxy: {before:?} -> {after:?}"
    );
    let body = body_of(&dev.wait_answers("/products/x"));
    assert_eq!(body.tag, "products-1", "the run keeps serving: {body:?}");
    assert_eq!(body.tag, "products-1", "the run keeps serving: {body:?}");
    assert_eq!(
        dev.events()
            .iter()
            .filter(|event| event["event"] == "manifest.warning")
            .filter(|event| event["message"]
                .as_str()
                .is_some_and(|message| message.contains("listener set changed")))
            .count(),
        1,
        "one warning for the save, not one per attempt: {}",
        dev.diagnostic()
    );
    dev.stop();
}

/// Row 7's other direction, in the same process family: the fixture's boot has
/// no store at all, and a save that **adds** one warns too. It is a separate boot
/// because adding a store needs a `DATABASE_URL` the first one never had.
#[test]
fn adding_a_store_is_one_warning_naming_the_restart() {
    let fixture = Fixture::new("stores-add");
    let mut dev = Dev::start_with_env(
        &fixture.dir,
        "stores-add",
        "json",
        &[],
        &[("DATABASE_URL", "postgres://shop:secret@127.0.0.1:5433/shop")],
    );
    assert_eq!(body_of(&dev.get("/products/x")).tag, "products-1");
    fs::write(fixture.path("Blastfile"), STORE_BLASTFILE).expect("the save");
    let warning = dev.wait_warning("listener set changed");
    let message = warning["message"].as_str().unwrap_or_default().to_string();
    assert!(
        message.contains("added") && message.contains("products→db"),
        "the warning names the pair that appeared: {message}"
    );
    assert!(
        message.contains("restart blastit dev"),
        "and the fix it needs: {message}"
    );
    assert_eq!(body_of(&dev.wait_answers("/products/x")).tag, "products-1");
    dev.stop();
}
