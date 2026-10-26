//! Smoke tests: the real `blastit` binary, as root, over HTTP.
//!
//! Every test starts `cargo`'s built binary with the environment the platform
//! would give it (`PORT`, `BLASTIT_APP`, `BLASTIT_MANIFEST`, `PYTHONPATH`, a
//! per-test `BLASTIT_UID_BASE`), talks HTTP over a `TcpStream`, and checks both
//! the answer and the event stream on stdout.
//!
//! init has to be PID 1's uid — root — to bind the view sockets, drop
//! privileges and fork, so a test that is not root prints `skipped: needs root`
//! and passes. The `smoke (root)` CI job runs this file with `sudo`.
//!
//! They are written to run **one at a time** (`--test-threads=1`, which is how
//! that job runs them): every server uses the same `/run/blastit` (§7.2), and
//! the rootless-binary test has to `chown` the two directories of
//! `/run/blastit` that init creates in — never recursively, because the per-uid
//! directories under `views/` belong to whichever test created them. A loaded
//! machine also makes a few of them (the signal and dead-component ones) timing
//! sensitive. Everything here still passes with the default thread count on a
//! quiet machine; it is not a promise.
//!
//! The PostgreSQL checks only run when `BLASTIT_TEST_PG_URL` is set (the CI job
//! provides one as a service container). They speak the PostgreSQL wire
//! protocol by hand over a Unix socket, so the tests need no Python packages
//! beyond the standard library.
//!
//! One consequence of running under `sudo`: sudo removes `LD_LIBRARY_PATH` from
//! the environment, and that is how a Python outside the loader's default
//! search path (the CI tool cache) is normally found. The spawned binary is
//! therefore given it back, derived from `PYO3_PYTHON` (see `library_path`),
//! and — for dbproxy, which is `execve`'d with an empty environment (§4) and
//! so cannot be helped that way — `build.rs` puts the interpreter's library
//! directory in the binary's `RUNPATH`.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use std::ffi::CString;
use std::fs::Permissions;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How long a test waits for the server to be ready.
const READY_TIMEOUT: Duration = Duration::from_secs(30);
/// How long a test waits for a process to exit.
const EXIT_TIMEOUT: Duration = Duration::from_secs(30);
/// Counter for unique temporary directories.
static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// Skips a test that cannot run without root, which is how `cargo test` behaves
/// for a developer who is not root.
fn needs_root(name: &str) -> bool {
    // SAFETY: `getuid` takes no arguments and cannot fail.
    if unsafe { libc::getuid() } == 0 {
        return true;
    }
    println!("{name}: skipped: needs root");
    false
}

/// The PostgreSQL URL the CI job provides, if there is one.
fn test_pg_url() -> Option<String> {
    std::env::var("BLASTIT_TEST_PG_URL")
        .ok()
        .filter(|url| !url.is_empty())
}

/// The password inside a `postgresql://user:password@host/db` URL.
fn password_of(url: &str) -> Option<String> {
    let rest = url.split_once("://")?.1;
    let before_host = rest.split('@').next()?;
    let (_, password) = before_host.split_once(':')?;
    if password.is_empty() {
        None
    } else {
        Some(password.to_string())
    }
}

/// A world-readable temporary directory of code and manifests.
struct Fixture {
    path: PathBuf,
}

impl Fixture {
    /// Creates `/tmp/blastit-smoke-<pid>-<tag>-<n>`, mode `0755` so the view
    /// uids can read the code (§7.2).
    fn new(tag: &str) -> Self {
        let mut path = PathBuf::from("/tmp");
        path.push(format!(
            "blastit-smoke-{}-{}-{}",
            std::process::id(),
            tag,
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        std::fs::set_permissions(&path, Permissions::from_mode(0o755)).unwrap();
        Self { path }
    }

    /// Writes a file the view uids may read (`0644`).
    fn write(&self, name: &str, contents: &str) -> PathBuf {
        let path = self.path.join(name);
        std::fs::write(&path, contents).unwrap();
        std::fs::set_permissions(&path, Permissions::from_mode(0o644)).unwrap();
        path
    }

    fn python_path(&self) -> String {
        self.path.to_string_lossy().into_owned()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// A running `blastit`, with its event stream collected as it goes.
struct Server {
    child: Child,
    port: u16,
    stdout: Arc<Mutex<Vec<String>>>,
    stderr: Arc<Mutex<Vec<String>>>,
    _fixture: Fixture,
}

impl Server {
    /// Starts the binary and waits until it reports `ready` and answers HTTP.
    fn start(
        tag: &str,
        uid_base: u32,
        manifest: &str,
        app: &str,
        python: &[(&str, &str)],
        env: &[(&str, &str)],
    ) -> Self {
        Self::start_with(tag, uid_base, manifest, app, python, env, &[], None)
    }

    /// The same, with the program to start instead of the binary itself —
    /// `setpriv` wrapping it, a copied binary with file capabilities — and,
    /// when it is not this process's own uid, a uid to start it as.
    ///
    /// D6 needs both: a container that dropped `cap_setpcap` (M1) and a rootless
    /// standalone install whose own file carries the capabilities (B2).
    #[allow(clippy::too_many_arguments)]
    fn start_with(
        tag: &str,
        uid_base: u32,
        manifest: &str,
        app: &str,
        python: &[(&str, &str)],
        env: &[(&str, &str)],
        argv: &[&str],
        as_uid: Option<u32>,
    ) -> Self {
        let fixture = Fixture::new(tag);
        let manifest_path = fixture.write("blastit.toml", manifest);
        for (name, contents) in python {
            fixture.write(name, contents);
        }
        let port = free_port();

        let (program, arguments) = match argv.split_first() {
            Some((program, rest)) => ((*program).to_string(), rest.to_vec()),
            None => (env!("CARGO_BIN_EXE_blastit").to_string(), Vec::new()),
        };
        let mut command = Command::new(program);
        command.args(arguments);
        if let Some(uid) = as_uid {
            use std::os::unix::process::CommandExt;
            command.uid(uid).gid(uid);
        }
        command
            .env("PORT", port.to_string())
            .env("BLASTIT_APP", app)
            .env("BLASTIT_MANIFEST", &manifest_path)
            .env("PYTHONPATH", fixture.python_path())
            .env("BLASTIT_UID_BASE", uid_base.to_string())
            .env("BLASTIT_LOG_FORMAT", "json")
            .env("BLASTIT_LOG_LEVEL", "info")
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (key, value) in env {
            command.env(key, value);
        }
        if let Some(path) = library_path() {
            command.env("LD_LIBRARY_PATH", path);
        }
        let mut child = command.spawn().expect("the blastit binary starts");

        let stdout = Arc::new(Mutex::new(Vec::new()));
        let stderr = Arc::new(Mutex::new(Vec::new()));
        if let Some(out) = child.stdout.take() {
            spawn_reader(out, Arc::clone(&stdout));
        }
        if let Some(err) = child.stderr.take() {
            spawn_reader(err, Arc::clone(&stderr));
        }

        let mut server = Self {
            child,
            port,
            stdout,
            stderr,
            _fixture: fixture,
        };
        server.wait_for_ready();
        server
    }

    /// Waits for the `ready` event and for the port to answer.
    fn wait_for_ready(&mut self) {
        let deadline = Instant::now() + READY_TIMEOUT;
        while Instant::now() < deadline {
            if self
                .stdout_lines()
                .iter()
                .any(|line| line.contains("\"ready\""))
            {
                return;
            }
            if let Some(status) = self.exited() {
                panic!(
                    "blastit exited with {status} before it was ready{}\nstdout:\n{}\nstderr:\n{}",
                    library_hint(&self.stderr_text()),
                    self.stdout_text(),
                    self.stderr_text()
                );
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!(
            "blastit was not ready within {READY_TIMEOUT:?}\nstdout:\n{}\nstderr:\n{}",
            self.stdout_text(),
            self.stderr_text()
        );
    }

    /// Every complete line stdout has produced so far.
    fn stdout_lines(&self) -> Vec<String> {
        self.stdout.lock().unwrap().clone()
    }

    fn stdout_text(&self) -> String {
        self.stdout_lines().join("\n")
    }

    fn stderr_text(&self) -> String {
        self.stderr.lock().unwrap().join("\n")
    }

    /// Waits until stdout holds a line containing `needle`.
    fn wait_for_event(&self, needle: &str) -> bool {
        let deadline = Instant::now() + EXIT_TIMEOUT;
        while Instant::now() < deadline {
            if self.stdout_lines().iter().any(|line| line.contains(needle)) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        false
    }

    /// The exit status, if the process has already ended.
    fn exited(&mut self) -> Option<std::process::ExitStatus> {
        self.child.try_wait().ok().flatten()
    }

    /// One HTTP/1.1 request, answered in full.
    fn request(&self, method: &str, target: &str) -> Response {
        self.request_with(method, target, &[])
    }

    /// The same, with headers of the caller's own (E1: the client-IP rules are
    /// about what a request *says*, so a test has to be able to lie).
    fn request_with(&self, method: &str, target: &str, extra: &[(&str, &str)]) -> Response {
        let mut stream = TcpStream::connect(("127.0.0.1", self.port)).unwrap_or_else(|err| {
            panic!(
                "connecting to the server for {target}: {err}\nstdout:\n{}",
                self.stdout_text()
            )
        });
        stream
            .set_read_timeout(Some(Duration::from_secs(20)))
            .unwrap();
        let mut request =
            format!("{method} {target} HTTP/1.1\r\nHost: blastit.test\r\nConnection: close\r\n");
        for (name, value) in extra {
            request.push_str(&format!("{name}: {value}\r\n"));
        }
        request.push_str("\r\n");
        stream.write_all(request.as_bytes()).unwrap();
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).unwrap();
        Response::parse(&raw)
    }

    /// `GET` with a body of assertions about the answer.
    fn get(&self, target: &str) -> Response {
        self.request("GET", target)
    }

    /// Every event of the given name that stdout has produced, parsed.
    fn events(&self, name: &str) -> Vec<serde_json::Value> {
        self.stdout_lines()
            .iter()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .filter(|value| value["event"].as_str() == Some(name))
            .collect()
    }

    /// The same, once at least `count` of them have arrived.
    ///
    /// A response is complete before the scheduler's `request` event has been
    /// written to init, through its pipe and out to stdout, so a test that reads
    /// the stream the moment its request returns is racing the writer. Waiting
    /// for the count it asked for is the difference between a test and a flake
    /// on a loaded machine (CI runs this file with `--test-threads=1`, a
    /// developer's `cargo test` does not).
    fn wait_for_events(&self, name: &str, count: usize) -> Vec<serde_json::Value> {
        let deadline = Instant::now() + EXIT_TIMEOUT;
        while Instant::now() < deadline {
            let events = self.events(name);
            if events.len() >= count {
                return events;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!(
            "only {} `{name}` event(s) arrived, wanted {count}\nstdout:\n{}",
            self.events(name).len(),
            self.stdout_text()
        );
    }

    /// The pids of the processes init forked, oldest first.
    ///
    /// `/proc/<pid>/task/<pid>/children` is the kernel's own list of a thread's
    /// children; init is single-threaded, so its main thread's entry has them
    /// all.
    fn children(&self) -> Vec<i32> {
        let path = format!(
            "/proc/{}/task/{}/children",
            self.child.id(),
            self.child.id()
        );
        let text = std::fs::read_to_string(&path)
            .unwrap_or_else(|err| panic!("reading {path}: {err}\n{}", self.stdout_text()));
        let mut pids: Vec<i32> = text
            .split_whitespace()
            .filter_map(|pid| pid.parse().ok())
            .collect();
        pids.sort_unstable();
        pids
    }

    /// The pid of the component that runs under `uid`, if it is alive.
    ///
    /// Every process of blastit has `gid == uid` and no supplementary groups
    /// (§2), so the uid is enough to tell http, the scheduler, dbproxy and the
    /// views apart.
    fn component_pid(&self, uid: u32) -> Option<i32> {
        self.children()
            .into_iter()
            .find(|pid| process_uid(*pid) == Some(uid))
    }

    /// Sends a signal to the process.
    fn signal(&self, signal: i32) {
        // SAFETY: the pid came from `spawn`, so it is a live child of this
        // process and `kill` takes two integers.
        let killed = unsafe { libc::kill(self.child.id() as i32, signal) };
        assert_eq!(killed, 0, "signalling pid {}", self.child.id());
    }

    /// Waits for the process to exit, returning its status.
    fn wait_for_exit(&mut self) -> Option<std::process::ExitStatus> {
        let deadline = Instant::now() + EXIT_TIMEOUT;
        while Instant::now() < deadline {
            if let Some(status) = self.child.try_wait().unwrap() {
                return Some(status);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        None
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A parsed HTTP response.
struct Response {
    status: u16,
    body: String,
}

impl Response {
    fn parse(raw: &[u8]) -> Self {
        let text = String::from_utf8_lossy(raw);
        let (head, body) = match text.split_once("\r\n\r\n") {
            Some((head, body)) => (head, body.to_string()),
            None => (text.as_ref(), String::new()),
        };
        let status = head
            .lines()
            .next()
            .and_then(|line| line.split(' ').nth(1))
            .and_then(|code| code.parse::<u16>().ok())
            .unwrap_or_else(|| panic!("no status line in {:?}", &text[..text.len().min(200)]));
        Self { status, body }
    }
}

/// Reads a child's pipe into a shared list, one line at a time.
fn spawn_reader(pipe: impl Read + Send + 'static, sink: Arc<Mutex<Vec<String>>>) {
    std::thread::spawn(move || {
        let mut reader = std::io::BufReader::new(pipe);
        let mut line = String::new();
        loop {
            line.clear();
            match std::io::BufRead::read_line(&mut reader, &mut line) {
                Ok(0) => return,
                Ok(_) => {
                    sink.lock()
                        .unwrap()
                        .push(line.trim_end_matches('\n').to_string());
                }
                Err(_) => return,
            }
        }
    });
}

/// A port nothing is listening on yet, for the server to bind.
fn free_port() -> u16 {
    let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
    let port = listener.local_addr().unwrap().port();
    drop(listener);
    port
}

/// One plain request from a test thread that does not own the `Server`.
///
/// Panics rather than hanging: the read timeout is the test's "none hang"
/// assertion.
fn raw_request(port: u16, target: &str) -> u16 {
    let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
    stream
        .set_read_timeout(Some(Duration::from_secs(30)))
        .unwrap();
    let request =
        format!("GET {target} HTTP/1.1\r\nHost: blastit.test\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).unwrap();
    let mut raw = Vec::new();
    stream
        .read_to_end(&mut raw)
        .expect("the server answered instead of hanging");
    Response::parse(&raw).status
}

/// The real uid of `pid`, from `/proc/<pid>/status`; `None` when it is gone.
fn process_uid(pid: i32) -> Option<u32> {
    let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    status
        .lines()
        .find_map(|line| line.strip_prefix("Uid:"))
        .and_then(|line| line.split_whitespace().next())
        .and_then(|uid| uid.parse().ok())
}

/// A world-readable, executable directory for a test that needs **file
/// capabilities** (D6/B2).
///
/// Not `/tmp` (many hosts mount it `nosuid`) and not `/run` (systemd mounts it
/// `noexec`), and not the cargo target directory either (`/home/<user>` is often
/// not traversable by the view uids): `/opt` is on the root filesystem, which
/// honours file capabilities, and a `0755` directory there is readable,
/// executable and traversable by every uid the test needs.
struct RunDir {
    path: PathBuf,
}

impl RunDir {
    /// A directory in a place no other uid can write, or `None` when this host
    /// has none (a runner with a world-writable `/opt`, a shared `/`): the tests
    /// that need one print and pass, the way a root-only test does. The checks
    /// that *refuse* something do not need it.
    ///
    /// Not `/tmp`: many hosts mount it `nosuid`, and the kernel ignores file
    /// capabilities there. Not `/run` either: systemd mounts it `noexec`.
    fn protected(tag: &str) -> Option<Self> {
        for parent in ["/opt", "/usr/local", "/srv"] {
            if !Path::new(parent).is_dir() {
                continue;
            }
            let dir = Self::under(parent, tag);
            let probe = dir.executable("probe", b"#!/bin/sh\n");
            // SAFETY: `geteuid` takes no arguments and cannot fail.
            let uid = unsafe { libc::geteuid() };
            if blastit::host::plain_is_protected(&probe, uid).is_ok() {
                return Some(dir);
            }
            drop(dir);
        }
        println!("skipped: no directory only its owner can write, on this host");
        None
    }

    /// Creates `<parent>/blastit-smoke-<pid>-<tag>-<n>`, mode `0755`.
    fn under(parent: &str, tag: &str) -> Self {
        let mut path = PathBuf::from(parent);
        path.push(format!(
            "blastit-smoke-{}-{}-{}",
            std::process::id(),
            tag,
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&path);
        std::fs::create_dir_all(&path).unwrap();
        std::fs::set_permissions(&path, Permissions::from_mode(0o755)).unwrap();
        Self { path }
    }

    /// Writes a file anything may read and execute (`0755`).
    fn executable(&self, name: &str, contents: &[u8]) -> PathBuf {
        let path = self.path.join(name);
        std::fs::write(&path, contents).unwrap();
        std::fs::set_permissions(&path, Permissions::from_mode(0o755)).unwrap();
        path
    }

    /// Writes a file anything may read (`0644`), for a manifest or an app.
    fn readable(&self, name: &str, contents: &str) -> PathBuf {
        let path = self.path.join(name);
        std::fs::write(&path, contents).unwrap();
        std::fs::set_permissions(&path, Permissions::from_mode(0o644)).unwrap();
        path
    }

    /// Copies this test's own binary in, `0755`, with `security.capability` set
    /// to `mask` when one is asked for.
    ///
    /// Returns whether the attribute was written; it needs `CAP_SETFCAP`, which
    /// is what a deployment's `setcap` needs too.
    fn binary(&self, name: &str, mask: Option<u32>) -> (PathBuf, bool) {
        let path = self.path.join(name);
        std::fs::copy(env!("CARGO_BIN_EXE_blastit"), &path).unwrap();
        std::fs::set_permissions(&path, Permissions::from_mode(0o755)).unwrap();
        let wrote = match mask {
            None => true,
            Some(mask) => write_file_caps(&path, mask),
        };
        (path, wrote)
    }

    /// The directory as a `PYTHONPATH` entry.
    fn python_path(&self) -> String {
        self.path.to_string_lossy().into_owned()
    }
}

impl Drop for RunDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

/// Runs a program to its exit and returns everything it printed (D6).
///
/// [`Server`] insists on a `ready` event, which is exactly what a boot that is
/// *supposed* to fail never produces; this is the bounded, both-pipes version
/// the refusals are checked with.
fn run_to_exit(program: &[&std::ffi::OsStr], env: &[(&str, &str)], uid: u32) -> String {
    use std::os::unix::process::CommandExt;
    let (program, arguments) = program.split_first().expect("a program to run");
    let mut command = Command::new(program);
    command
        .args(arguments)
        .uid(uid)
        .gid(uid)
        .env_clear()
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    for (key, value) in env {
        command.env(key, value);
    }
    if let Some(path) = library_path() {
        command.env("LD_LIBRARY_PATH", path);
    }
    let child = command.spawn().expect("the program starts");
    let output = child.wait_with_output().expect("it ends on its own");
    let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
    text.push_str(&String::from_utf8_lossy(&output.stderr));
    assert!(
        !output.status.success(),
        "a refused boot must not exit zero:\n{text}"
    );
    text
}

/// Writes `security.capability` with `permitted = mask` (D6/B2).
///
/// The attribute the kernel reads at an `execve`, written directly — a
/// deployment uses `setcap`, which is not guaranteed to be in the image the
/// tests run in. Needs `CAP_SETFCAP`, so this is root-only, like `setcap`.
fn write_file_caps(path: &Path, mask: u32) -> bool {
    // `struct vfs_cap_data`: `VFS_CAP_REVISION_2 | VFS_CAP_FLAGS_EFFECTIVE`,
    // then two words of permitted and inheritable per half.
    let mut value = [0_u8; 20];
    value[..4].copy_from_slice(&0x0200_0001_u32.to_le_bytes());
    value[4..8].copy_from_slice(&mask.to_le_bytes());
    let name = c"security.capability";
    let path = CString::new(path.as_os_str().as_bytes()).unwrap();
    // SAFETY: `setxattr` takes a NUL-terminated path, a NUL-terminated name, a
    // buffer of the length given and a flags word.
    unsafe {
        libc::setxattr(
            path.as_ptr(),
            name.as_ptr(),
            value.as_ptr().cast(),
            value.len(),
            0,
        ) == 0
    }
}

/// Everything `/proc/<pid>/environ` holds, as text.
///
/// The kernel's view of the *initial* environment block of the process image:
/// exactly what survives a `fork`, and what an `execve` replaces (§4).
fn environ_of(pid: i32) -> String {
    let bytes = std::fs::read(format!("/proc/{pid}/environ"))
        .unwrap_or_else(|err| panic!("reading /proc/{pid}/environ: {err}"));
    String::from_utf8_lossy(&bytes).into_owned()
}

/// One open descriptor of `pid`: its number and what it points at.
fn open_fds(pid: i32) -> Vec<(i32, String)> {
    let dir = format!("/proc/{pid}/fd");
    let entries = std::fs::read_dir(&dir).unwrap_or_else(|err| panic!("reading {dir}: {err}"));
    let mut fds: Vec<(i32, String)> = entries
        .flatten()
        .filter_map(|entry| {
            let number = entry.file_name().to_string_lossy().parse::<i32>().ok()?;
            let target = std::fs::read_link(entry.path())
                .map(|path| path.to_string_lossy().into_owned())
                .unwrap_or_default();
            Some((number, target))
        })
        .collect();
    fds.sort_by_key(|(number, _)| *number);
    fds
}

/// `LD_LIBRARY_PATH` for the spawned binary, so it can load `libpython`.
///
/// A Python that is not in the loader's default search path (the CI tool cache,
/// for instance) is normally found through `LD_LIBRARY_PATH`, which the
/// `setup-python` action exports. The smoke tests run under `sudo`, and sudo
/// *removes* `LD_LIBRARY_PATH` from the environment, so the binary init spawns
/// would not find `libpython` and would exit 127 before its first event.
///
/// `build.rs` also puts the interpreter's library directory in the binary's
/// `RUNPATH`, which is what makes dbproxy — `execve`'d with an empty
/// environment (§4) — work; this is the belt to that pair of braces, for a
/// build whose `libpython` is only reachable through the environment.
fn library_path() -> Option<String> {
    let mut entries: Vec<String> = Vec::new();
    if let Some(python) = std::env::var_os("PYO3_PYTHON") {
        let interpreter = PathBuf::from(python);
        if let Some(prefix) = interpreter.parent().and_then(Path::parent) {
            for candidate in ["lib", "lib64"] {
                let dir = prefix.join(candidate);
                if holds_libpython(&dir) {
                    entries.push(dir.to_string_lossy().into_owned());
                }
            }
        }
    }
    if let Some(existing) = std::env::var_os("LD_LIBRARY_PATH") {
        entries.push(existing.to_string_lossy().into_owned());
    }
    if entries.is_empty() {
        None
    } else {
        Some(entries.join(":"))
    }
}

/// A hint for a binary that could not load `libpython`, or nothing.
fn library_hint(stderr: &str) -> String {
    if stderr.contains("libpython") {
        "\n(the binary could not load libpython: it was built against PYO3_PYTHON, \
         so either set that or put its lib directory in LD_LIBRARY_PATH)"
            .to_string()
    } else {
        String::new()
    }
}

/// Whether `dir` contains a shared `libpython`.
fn holds_libpython(dir: &Path) -> bool {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return false;
    };
    entries.flatten().any(|entry| {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        name.starts_with("libpython") && name.contains(".so")
    })
}

/// The WSGI application every test that is not about the database uses.
///
/// It reports who served the request (uid, pid, route), prints one line so the
/// event stream carries an `app.output`, can exit the process in the middle of
/// a request, and can hold two concurrent requests on a barrier.
const APP: &str = r#"
import json
import os
import threading

BARRIER = threading.Barrier(2, timeout=5)


def respond(start_response, body):
    start_response("200 OK", [("Content-Type", "text/plain"),
                              ("Content-Length", str(len(body)))])
    return [body]


def application(environ, start_response):
    path = environ.get("PATH_INFO", "")
    if path == "/crash":
        os._exit(3)
    if path == "/parallel":
        BARRIER.wait()
        return respond(start_response, b"both-ran")
    if path == "/env":
        body = json.dumps({
            "home": os.environ.get("HOME"),
            "tmpdir": os.environ.get("TMPDIR"),
            "cwd": os.getcwd(),
            "blastit": sorted(k for k in os.environ if k.startswith("BLASTIT_")),
            "database_url": os.environ.get("DATABASE_URL"),
            "page_url": os.environ.get("PAGE_URL"),
            "environ": dict(os.environ),
            "uid": os.getuid(),
            "pid": os.getpid(),
            "gid": os.getgid(),
            "route": environ.get("blastit.route"),
            "script": environ.get("SCRIPT_NAME"),
        }).encode()
        return respond(start_response, body)
    body = ("uid=%d pid=%d path=%s" % (os.getuid(), os.getpid(), path)).encode()
    print("printed by the app", flush=True)
    return respond(start_response, body)
"#;

/// The PostgreSQL wire-protocol helper: no Python packages, by hand.
///
/// It connects to the route's own socket (`/proc/self/cwd/.s.PGSQL.<port>`),
/// sends a `StartupMessage` and one simple `Query`, and reports the reply's
/// message kinds, the first `DataRow` field, and the SQLSTATE of an
/// `ErrorResponse`.
const PG_APP: &str = r#"
import json
import os
import socket
import struct


def recv_exactly(sock, count):
    out = b""
    while len(out) < count:
        chunk = sock.recv(count - len(out))
        if not chunk:
            raise EOFError("the proxy closed the connection")
        out += chunk
    return out


def sqlstate(payload):
    index = 0
    while index < len(payload):
        kind = payload[index:index + 1]
        if kind == b"\x00":
            break
        end = payload.index(b"\x00", index + 1)
        if kind == b"C":
            return payload[index + 1:end].decode("latin-1")
        index = end + 1
    return "?"


def first_field(payload):
    count = struct.unpack("!H", payload[:2])[0]
    if count == 0:
        return ""
    size = struct.unpack("!i", payload[2:6])[0]
    if size < 0:
        return "NULL"
    return payload[6:6 + size].decode("latin-1")


def pg_query(sql, port):
    sock = socket.socket(socket.AF_UNIX, socket.SOCK_STREAM)
    sock.settimeout(15)
    sock.connect("/proc/self/cwd/.s.PGSQL.%d" % port)
    params = b"user\x00blastit\x00database\x00blastit\x00\x00"
    body = struct.pack("!I", 196608) + params
    sock.sendall(struct.pack("!I", len(body) + 4) + body)
    # Startup: the standard reply, which ends at ReadyForQuery.
    while True:
        head = recv_exactly(sock, 5)
        payload = recv_exactly(sock, struct.unpack("!I", head[1:5])[0] - 4)
        if head[0:1] == b"Z":
            break
        if head[0:1] == b"E":
            return "startup-error " + sqlstate(payload)
    payload_sql = sql.encode("utf-8") + b"\x00"
    sock.sendall(b"Q" + struct.pack("!I", len(payload_sql) + 4) + payload_sql)
    # Read until the ReadyForQuery that closes *this* round trip: the first one
    # after the reply's own messages. Anything a backend sends out of band (a
    # NoticeResponse, an asynchronous ReadyForQuery) cannot then be mistaken for
    # the answer.
    kinds = []
    data = None
    content = False
    for _ in range(64):
        head = recv_exactly(sock, 5)
        payload = recv_exactly(sock, struct.unpack("!I", head[1:5])[0] - 4)
        kind = head[0:1]
        kinds.append(kind.decode("latin-1"))
        if kind in (b"T", b"D", b"C"):
            content = True
        if kind == b"D" and data is None:
            data = first_field(payload)
        elif kind == b"E":
            return "error %s kinds=%s" % (sqlstate(payload), "".join(kinds))
        elif kind == b"Z" and content:
            return "ok kinds=%s data=%s" % ("".join(kinds), data)
    return "incomplete kinds=%s" % "".join(kinds)


def application(environ, start_response):
    granted = pg_query("SELECT 1", 5432)
    denied = pg_query("SELECT 2", 5432)
    body = json.dumps({
        "granted": granted,
        "denied": denied,
        "database_url": os.environ.get("DATABASE_URL"),
        "uid": os.getuid(),
    }).encode()
    start_response("200 OK", [("Content-Type", "text/plain"),
                              ("Content-Length", str(len(body)))])
    return [body]
"#;

/// The manifest for the routing tests: two routes with different uids.
const TWO_ROUTES: &str = r#"
version = 1

[[route]]
name = "one"
path = ['/', '/one']

[[route]]
name = "two"
path = ['/two/.*']
"#;

/// The manifest for the concurrency test: `/parallel` has to reach a route.
const PARALLEL_ROUTE: &str = r#"
version = 1

[[route]]
name = "one"
path = ['/', '/one', '/parallel']
"#;

/// The manifest for the crash test: `/crash` has to reach a route.
const CRASH_ROUTE: &str = r#"
version = 1

[[route]]
name = "one"
path = ['/', '/one', '/crash']
"#;

/// The manifest for the environment test: `/env` has to reach a route.
const ENV_ROUTE: &str = r#"
version = 1

[[route]]
name = "one"
path = ['/', '/one', '/env']
"#;

/// One route that matches everything, for tests that need a single view.
const ONE_ROUTE: &str = r#"
version = 1

[[route]]
name = "one"
path = ['.*']
"#;

/// A declared proxy with a grant: dbproxy exists and the view has a socket.
const PROXY_AND_ROUTE: &str = r#"
version = 1

[proxy.db]
protocol = "postgresql"
input = { url = "DATABASE_URL" }

[[route]]
name = "one"
path = ['.*']
[route.db]
queries = ["SELECT 1"]
"#;

/// A declared proxy that no route grants: its variables are rewritten, but it
/// gets no listener and so no dbproxy.
const PROXY_WITHOUT_GRANTS: &str = r#"
version = 1

[proxy.db]
protocol = "postgresql"
input = { url = "DATABASE_URL" }

[[route]]
name = "one"
path = ['.*']
"#;

#[test]
fn routing_reaches_two_routes_and_each_route_has_its_own_process_and_uid() {
    if !needs_root("routing") {
        return;
    }
    let base = 40_000;
    let server = Server::start(
        "routing",
        base,
        TWO_ROUTES,
        "app:application",
        &[("app.py", APP)],
        &[],
    );

    let one = server.get("/one");
    assert_eq!(one.status, 200, "{}", server.stdout_text());
    assert!(
        one.body.contains(&format!("uid={} ", base + 1000)),
        "route one runs under B+1000: {}",
        one.body
    );
    let two = server.get("/two/anything");
    assert_eq!(two.status, 200);
    assert!(
        two.body.contains(&format!("uid={} ", base + 1001)),
        "route two runs under B+1001: {}",
        two.body
    );

    // Different processes, and the same one answers twice.
    let pid_of = |body: &str| {
        body.split_whitespace()
            .find_map(|field| field.strip_prefix("pid=").map(str::to_string))
            .unwrap()
    };
    assert_ne!(
        pid_of(&one.body),
        pid_of(&two.body),
        "one route, one process"
    );
    let again = server.get("/one");
    assert_eq!(pid_of(&again.body), pid_of(&one.body), "no respawn");

    // A catch-all-less manifest leaves unmatched paths to blastit.
    let missing = server.get("/nope");
    assert_eq!(missing.status, 404, "{}", server.stdout_text());

    // The health path is answered by the scheduler, without a view.
    let health = server.get("/_blastit/health");
    assert_eq!(health.status, 200);

    assert_eq!(server.get("/").status, 200, "the root route matches /");
}

#[test]
fn the_app_environment_has_no_blastit_variables_and_home_is_its_scratch_directory() {
    if !needs_root("environment") {
        return;
    }
    let base = 41_000;
    let server = Server::start(
        "environment",
        base,
        ENV_ROUTE,
        "app:application",
        &[("app.py", APP)],
        &[("UNRELATED_VARIABLE", "kept")],
    );

    let response = server.get("/env");
    assert_eq!(response.status, 200, "{}", server.stdout_text());
    let value: serde_json::Value = serde_json::from_str(&response.body).unwrap();

    assert_eq!(
        value["blastit"].as_array().unwrap().len(),
        0,
        "no BLASTIT_* variable survives the rewrite (§5.3)"
    );
    assert_eq!(
        value["home"].as_str().unwrap(),
        format!("/run/blastit/views/{}", base + 1000),
        "({})",
        server.stdout_text()
    );
    assert_eq!(
        value["tmpdir"].as_str().unwrap(),
        format!("/run/blastit/views/{}", base + 1000)
    );
    assert_eq!(
        value["cwd"].as_str().unwrap(),
        format!("/run/blastit/views/{}", base + 1000),
        "a view works in its own socket directory (§2.2)"
    );
    assert_eq!(value["route"].as_str().unwrap(), "one");
    assert_eq!(value["gid"].as_u64().unwrap(), u64::from(base + 1000));
}

#[test]
fn a_declared_proxy_boots_dbproxy_and_replaces_the_credential_with_a_local_endpoint() {
    if !needs_root("proxy") {
        return;
    }
    let base = 42_000;
    let password = "super-secret-not-for-the-app";
    let url =
        format!("postgresql://doadmin:{password}@db.internal:25060/defaultdb?sslmode=require");
    let manifest = r#"
version = 1

[proxy.db]
protocol = "postgresql"
input = { url = "DATABASE_URL", ca_cert = "DATABASE_CA_CERT" }

[[route]]
name = "one"
path = ['/.*']
[route.db]
queries = ["SELECT 1"]
"#;
    let server = Server::start(
        "proxy",
        base,
        manifest,
        "app:application",
        &[("app.py", APP)],
        &[
            ("DATABASE_URL", &url),
            ("DATABASE_CA_CERT", "-----BEGIN CERTIFICATE-----"),
        ],
    );

    // dbproxy was forked, waited for, and the boot got past it.
    let response = server.get("/env");
    assert_eq!(response.status, 200, "{}", server.stdout_text());
    let value: serde_json::Value = serde_json::from_str(&response.body).unwrap();
    let database_url = value["database_url"].as_str().expect("DATABASE_URL is set");
    assert!(
        database_url.contains("/proc/self/cwd") || database_url.contains("%2Fproc%2Fself%2Fcwd"),
        "the app is pointed at its own socket: {database_url}"
    );
    assert!(
        !database_url.contains(password),
        "the rendered URL carries no credential: {database_url}"
    );
    assert_eq!(value["blastit"].as_array().unwrap().len(), 0);

    // The listeners init bound belong to the view's uid, mode 0600 (§2.2).
    let socket = PathBuf::from(format!("/run/blastit/views/{}/.s.PGSQL.5432", base + 1000));
    let meta = std::fs::metadata(&socket).expect("the pg socket exists");
    assert_eq!(meta.permissions().mode() & 0o777, 0o600);
    use std::os::unix::fs::MetadataExt;
    assert_eq!(meta.uid(), base + 1000, "owned by the view uid");

    // No secret reaches the log: the password is in dbproxy's configuration
    // pipe, not in an event (§4).
    assert!(
        !server.stdout_text().contains(password),
        "the password reached stdout:\n{}",
        server.stdout_text()
    );
    assert!(!server.stderr_text().contains(password));
}

#[test]
fn a_view_that_crashes_gives_502_and_the_next_request_respawns_it() {
    if !needs_root("crash") {
        return;
    }
    let base = 43_000;
    let server = Server::start(
        "crash",
        base,
        CRASH_ROUTE,
        "app:application",
        &[("app.py", APP)],
        &[],
    );

    let first = server.get("/one");
    assert_eq!(first.status, 200);

    let crash = server.get("/crash");
    assert_eq!(crash.status, 502, "{}", server.stdout_text());
    assert!(server.wait_for_event("\"event\":\"process.exited\""));

    // The scheduler asks for a new process, and the route works again.
    let deadline = Instant::now() + Duration::from_secs(20);
    let mut recovered = false;
    while Instant::now() < deadline {
        let response = server.get("/one");
        if response.status == 200 {
            recovered = true;
            break;
        }
        assert_eq!(response.status, 502, "{}", server.stdout_text());
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(recovered, "the route respawned\n{}", server.stdout_text());
}

#[test]
fn concurrent_requests_to_one_route_run_in_parallel_threads() {
    if !needs_root("parallel") {
        return;
    }
    let base = 44_000;
    let server = Server::start(
        "parallel",
        base,
        PARALLEL_ROUTE,
        "app:application",
        &[("app.py", APP)],
        &[],
    );
    // Warm the view up so the fork does not race the barrier.
    assert_eq!(server.get("/one").status, 200);

    let port = server.port;
    let handle = std::thread::spawn(move || {
        let mut stream = TcpStream::connect(("127.0.0.1", port)).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(20)))
            .unwrap();
        stream
            .write_all(b"GET /parallel HTTP/1.1\r\nHost: blastit.test\r\nConnection: close\r\n\r\n")
            .unwrap();
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).unwrap();
        Response::parse(&raw)
    });
    let other = server.get("/parallel");
    let first = handle.join().unwrap();

    assert_eq!(other.status, 200, "{}", server.stdout_text());
    assert_eq!(first.status, 200, "{}", server.stdout_text());
    assert_eq!(other.body, "both-ran");
    assert_eq!(first.body, "both-ran");
}

#[test]
fn the_boot_events_say_init_runs_as_root() {
    if !needs_root("privilege") {
        return;
    }
    // D6: the mode belongs in the log, because a rootless boot and a root one
    // look identical from the outside.
    let server = Server::start(
        "privilege",
        46_000,
        ONE_ROUTE,
        "app:application",
        &[("app.py", APP)],
        &[],
    );
    assert_eq!(server.get("/one").status, 200);
    let boots = server.events("boot");
    assert!(boots.len() >= 3, "stage 0, config and preload: {boots:?}");
    for boot in &boots {
        assert_eq!(boot["privilege"], "root", "{boot}");
        assert!(boot.get("capabilities").is_none(), "{boot}");
    }
    assert_eq!(boots[0]["stage"], "stage0");
    assert_eq!(boots[1]["stage"], "config");
    assert_eq!(boots[2]["stage"], "preload");
    // E1: with no preset, every `boot` line says `preset: null` rather than
    // leaving an operator to wonder whether this build has presets at all.
    for boot in &boots {
        assert!(boot["preset"].is_null(), "{boot}");
    }
    // D6: init's uid must be a dedicated one, and this very test harness is
    // another process of uid 0 — so the boot says so, once, before it forks
    // anything. (On a deployment where nothing else runs as that uid, there is no
    // such line; the e2e checks that side.)
    assert!(
        server.wait_for_event("other process(es) run as uid 0"),
        "the boot warns about siblings of its own uid:\n{}",
        server.stdout_text()
    );
}

#[test]
fn a_non_root_init_without_its_capabilities_names_them() {
    if unsafe { libc::getuid() } == 0 {
        println!("a non-root init: skipped: needs a non-root user");
        return;
    }
    // D6: the same binary, as an ordinary user with no capabilities. It must
    // fail at boot with the list of capabilities it is missing, not later with
    // an EPERM nobody can place.
    let fixture = Fixture::new("rootless");
    let manifest = fixture.write("blastit.toml", ONE_ROUTE);
    fixture.write("app.py", APP);
    let mut command = Command::new(env!("CARGO_BIN_EXE_blastit"));
    command
        .env("PORT", "8099")
        .env("BLASTIT_APP", "app:application")
        .env("BLASTIT_MANIFEST", &manifest)
        .env("PYTHONPATH", fixture.python_path())
        .env("BLASTIT_UID_BASE", "47000")
        .env("BLASTIT_LOG_FORMAT", "json")
        .env("BLASTIT_LOG_LEVEL", "info")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(path) = library_path() {
        command.env("LD_LIBRARY_PATH", path);
    }
    let output = command.output().expect("the blastit binary starts");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
    assert!(
        !output.status.success(),
        "a boot without capabilities must fail\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
    // The `boot` event is still the first line: it says it knows nothing about
    // a mode it could not read.
    let first: serde_json::Value = serde_json::from_str(stdout.lines().next().unwrap_or(""))
        .unwrap_or_else(|err| panic!("{err}: {stdout}"));
    assert_eq!(first["event"], "boot");
    assert!(first.get("privilege").is_none(), "{first}");
    for needle in [
        "cap_setuid",
        "cap_setgid",
        "cap_chown",
        "cap_fowner",
        "cap_kill",
        "cap_setpcap",
    ] {
        assert!(
            stdout.contains(needle) || stderr.contains(needle),
            "{needle} is missing from\nstdout:\n{stdout}\nstderr:\n{stderr}"
        );
    }
    assert!(
        stdout.contains("setcap") || stderr.contains("setcap"),
        "the error says how to fix it\nstdout:\n{stdout}\nstderr:\n{stderr}"
    );
}

#[test]
fn a_root_boot_without_cap_setpcap_runs_and_says_the_bounding_set_is_kept() {
    if !needs_root("bounding set") {
        return;
    }
    // M1: a container that cannot give `cap_setpcap` (an old `--cap-drop`, a
    // runtime without `PR_CAPBSET_DROP`) must still boot. `setpriv
    // --bounding-set -setpcap` is that container: for root, the permitted set
    // an `execve` leaves is the bounding set, so the binary starts without
    // `cap_setpcap` anywhere.
    let server = Server::start_with(
        "bounding",
        49_000,
        ONE_ROUTE,
        "app:application",
        &[("app.py", APP)],
        &[],
        &[
            "setpriv",
            "--bounding-set",
            "-setpcap",
            env!("CARGO_BIN_EXE_blastit"),
        ],
        None,
    );
    let boots = server.events("boot");
    assert_eq!(
        boots[0]["privilege"], "root",
        "it is still a root boot: {}",
        boots[0]
    );
    assert_eq!(
        boots[0]["bounding_set"], "kept",
        "and the event says which happened: {}",
        boots[0]
    );
    assert!(
        server.wait_for_event("cap_setpcap is not held"),
        "the warning"
    );
    // And it serves: the children lose every set they hold, `no_new_privs` is
    // set, and the sandbox is otherwise the same one.
    assert_eq!(server.get("/one").status, 200);
    let spawned = server.events("process.spawned");
    assert!(!spawned.is_empty(), "a view was forked");
}

#[test]
fn a_rootless_standalone_install_runs_dbproxy_from_the_capability_free_copy() {
    if !needs_root("rootless binary") {
        return;
    }
    // B2: the file that carries the capabilities is init's alone, and
    // `--role=dbproxy` — a child, whose bounding set is empty by then — runs the
    // capability-free copy `BLASTIT_EXEC` names. Without it the child's
    // `execve` fails: the kernel refuses a file with file capabilities that an
    // empty bounding set cannot honour.
    let uid = 49_500;
    let Some(dir) = RunDir::protected("rootless-bin") else {
        return;
    };
    // The set a deployment is advised to give (`caps::WANTED`), straight into
    // the attribute the kernel reads: `setcap` is not guaranteed to be in the
    // image the tests run in, and `CAP_SETFCAP` is root's anyway.
    let mask = blastit_sys::caps::Capability::WANTED
        .iter()
        .fold(0_u32, |mask, cap| mask | (1 << cap.raw()));
    let (init, wrote) = dir.binary("blastit-init", Some(mask));
    let (plain, _) = dir.binary("blastit", None);
    assert!(
        wrote,
        "setxattr(security.capability): {}",
        std::io::Error::last_os_error()
    );
    assert!(
        blastit::host::has_file_caps(&init).unwrap(),
        "the attribute is there"
    );
    assert!(
        !blastit::host::has_file_caps(&plain).unwrap(),
        "the copy stays capability-free"
    );
    // A rootless init owns the runtime directory (§7.2): `/run` belongs to root,
    // so a deployment gives the service uid a directory of its own — a `tmpfs`
    // in the e2e, a `chown` here.
    //
    // Exactly two directories, never `-R`: every test that starts a server
    // creates `/run/blastit/views/<uid>` of its own, and a recursive `chown`
    // here would take those away from a test running beside this one (the
    // failure is a socket owned by the wrong uid in another test's assertion).
    // `views` is what init has to be able to create *in*; the per-uid
    // directories below it are created by whoever forked the view, and init is
    // the one that chowns them (it holds `CAP_CHOWN` for exactly that).
    let runtime = Path::new("/run/blastit");
    std::fs::create_dir_all(runtime.join("views")).unwrap();
    assert!(
        std::process::Command::new("chown")
            .args([
                &format!("{uid}:{uid}"),
                "/run/blastit",
                "/run/blastit/views"
            ])
            .status()
            .map(|status| status.success())
            .unwrap_or(false),
        "chown {uid} /run/blastit /run/blastit/views"
    );

    let manifest = dir.readable("blastit.toml", PROXY_AND_ROUTE);
    dir.readable("app.py", PG_APP);
    let python_path = dir.python_path();
    let url = "postgresql://nowhere:nowhere@127.0.0.1:1/nowhere";

    // Without `BLASTIT_EXEC` the boot refuses, by name: dbproxy would have to
    // `execve` the file that carries the capabilities.
    let refused = run_to_exit(
        &[init.as_os_str()],
        &[
            ("BLASTIT_APP", "app:application"),
            ("BLASTIT_MANIFEST", manifest.to_str().unwrap()),
            ("BLASTIT_UID_BASE", "20000"),
            ("DATABASE_URL", url),
            ("PYTHONPATH", &python_path),
        ],
        uid,
    );
    assert!(
        refused.contains("BLASTIT_EXEC"),
        "the refusal names the variable:\n{refused}"
    );
    assert!(
        refused.contains("cannot name") && refused.contains("capability-free binary"),
        "and says what is missing:\n{refused}"
    );
    // It never got as far as forking dbproxy, so nothing was served.
    assert!(!refused.contains("\"ready\""), "{refused}");

    // With it, the boot reaches `ready`: dbproxy was `execve`d from the
    // capability-free copy and answered on its ready pipe.
    let server = Server::start_with(
        "rootless-bin",
        20_000,
        PROXY_AND_ROUTE,
        "app:application",
        &[("app.py", PG_APP)],
        &[
            ("BLASTIT_EXEC", plain.to_str().unwrap()),
            ("DATABASE_URL", url),
        ],
        &[init.to_str().unwrap()],
        Some(uid),
    );
    let boots = server.events("boot");
    assert_eq!(
        boots[0]["privilege"], "capabilities",
        "a rootless boot: {}",
        boots[0]
    );
    assert!(
        boots[0]["capabilities"]
            .as_array()
            .is_some_and(|caps| caps.iter().any(|cap| cap == "cap_setuid")),
        "{}",
        boots[0]
    );
    assert_eq!(boots[0]["bounding_set"], "dropped", "{}", boots[0]);
    assert!(server.get("/").status < 500, "{}", server.stdout_text());
}

#[test]
fn stdout_is_one_event_per_line_and_covers_the_lifecycle() {
    if !needs_root("events") {
        return;
    }
    let base = 45_000;
    let server = Server::start(
        "events",
        base,
        TWO_ROUTES,
        "app:application",
        &[("app.py", APP)],
        &[],
    );
    assert_eq!(server.get("/one").status, 200);
    // Wait for the events this test is *about*, one at a time: the response is
    // complete before the scheduler's `request` event has been written to init,
    // through its pipe and out to stdout, so a wait on `app.output` alone is
    // satisfied by the boot's own warning (the sibling line, above) and the
    // `request` event may still be in flight on a loaded machine.
    for expected in ["boot", "ready", "request", "process.spawned", "app.output"] {
        assert!(
            server.wait_for_event(&format!("\"event\":\"{expected}\"")),
            "no `{expected}` event in:\n{}",
            server.stdout_text()
        );
    }

    let lines = server.stdout_lines();
    assert!(lines.len() >= 5, "{}", lines.join("\n"));
    let mut events = Vec::new();
    for line in &lines {
        let value: serde_json::Value =
            serde_json::from_str(line).unwrap_or_else(|err| panic!("{err}: {line}"));
        assert_eq!(value["ts"].as_str().map(str::len), Some(24), "{line}");
        assert!(value["level"].is_string(), "{line}");
        assert!(value["component"].is_string(), "{line}");
        events.push(value["event"].as_str().unwrap().to_string());
    }
    for expected in ["boot", "ready", "request", "process.spawned", "app.output"] {
        assert!(events.iter().any(|event| event == expected), "{events:?}");
    }
    // Every line is at most 2048 bytes, which is what §8.1 promises.
    for line in &lines {
        assert!(line.len() <= 2048, "{} bytes: {}", line.len(), line);
    }
}

#[test]
fn sigterm_drains_and_exits_zero_within_the_graceful_timeout() {
    if !needs_root("sigterm") {
        return;
    }
    let base = 46_000;
    let mut server = Server::start(
        "sigterm",
        base,
        TWO_ROUTES,
        "app:application",
        &[("app.py", APP)],
        &[("BLASTIT_GRACEFUL_TIMEOUT", "2s")],
    );
    assert_eq!(server.get("/one").status, 200);

    server.signal(libc::SIGTERM);
    let status = server
        .wait_for_exit()
        .expect("the server exits within the graceful timeout");
    assert_eq!(status.code(), Some(0), "{}", server.stdout_text());
    assert!(
        server.wait_for_event("\"stage\":\"stopped\""),
        "{}",
        server.stdout_text()
    );
}

#[test]
fn a_granted_query_is_forwarded_and_an_ungranted_one_is_denied() {
    if !needs_root("postgres") {
        return;
    }
    let Some(url) = test_pg_url() else {
        println!("postgres: skipped: BLASTIT_TEST_PG_URL is not set");
        return;
    };
    let base = 47_000;
    let manifest = r#"
version = 1

[proxy.db]
protocol = "postgresql"
input = { url = "DATABASE_URL" }

[[route]]
name = "data"
path = ['/query']
[route.db]
queries = ["SELECT 1"]

[[route]]
name = "other"
path = ['/other']
"#;
    let server = Server::start(
        "postgres",
        base,
        manifest,
        "app:application",
        &[("app.py", PG_APP)],
        &[("DATABASE_URL", &url)],
    );

    let response = server.get("/query");
    assert_eq!(response.status, 200, "{}", server.stdout_text());
    let value: serde_json::Value = serde_json::from_str(&response.body).unwrap();
    let granted = value["granted"]
        .as_str()
        .unwrap_or_else(|| panic!("no `granted` in {}", response.body));
    assert!(
        granted.contains("kinds=TDCZ") && granted.contains("data=1"),
        "SELECT 1 is answered with RowDescription/DataRow/CommandComplete/ReadyForQuery: {granted}"
    );
    let denied = value["denied"]
        .as_str()
        .unwrap_or_else(|| panic!("no `denied` in {}", response.body));
    assert!(
        denied.starts_with("error 42501"),
        "SELECT 2 is refused with 42501: {denied}"
    );

    // The denial is an event (§8.1)…
    assert!(server.wait_for_event("\"event\":\"sql.denied\""));
    assert!(server.stdout_text().contains("\"sqlstate\":\"42501\""));

    // …the app's URL carries no password, and points at the local socket…
    let database_url = value["database_url"]
        .as_str()
        .unwrap_or_else(|| panic!("no `database_url` in {}", response.body));
    let password = password_of(&url).expect("the test URL has a password");
    assert!(
        !database_url.contains(&password),
        "no password in the app's URL: {database_url}"
    );
    assert!(
        database_url.contains("proc") && database_url.contains("5432"),
        "the app is pointed at its own socket: {database_url}"
    );
    // …and the real password is nowhere in the log.
    assert!(
        !server.stdout_text().contains(&password),
        "the password reached stdout:\n{}",
        server.stdout_text()
    );

    // Another view's uid cannot reach this route's socket (§2.2).
    let socket = format!("/run/blastit/views/{}/.s.PGSQL.5432", base + 1000);
    let errno = connect_as(base + 1001, &socket);
    assert_eq!(
        errno,
        Some(libc::EACCES),
        "another uid gets EACCES, not {errno:?}"
    );

    // The route without a grant has no socket at all.
    assert!(
        !PathBuf::from(format!("/run/blastit/views/{}/.s.PGSQL.5432", base + 1001)).exists(),
        "a route with no grant gets no listener"
    );
}

#[test]
fn a_route_keeps_serving_while_its_view_is_retired() {
    if !needs_root("retire") {
        return;
    }
    let base = 48_000;
    let server = Server::start(
        "retire",
        base,
        ONE_ROUTE,
        "app:application",
        &[("app.py", APP)],
        &[("BLASTIT_VIEW_MAX_REQUESTS", "1")],
    );

    // Every dispatch retires the process that served it, so most of these
    // requests arrive while the previous process is killed but not yet reaped.
    // A SPAWN that has to wait must not be dropped: the scheduler sends it once.
    let port = server.port;
    let clients: Vec<std::thread::JoinHandle<u16>> = (0..16)
        .map(|_| std::thread::spawn(move || raw_request(port, "/one")))
        .collect();
    for client in clients {
        let status = client.join().expect("the client thread finishes");
        assert_eq!(status, 200, "{}", server.stdout_text());
    }
    for _ in 0..20 {
        assert_eq!(server.get("/one").status, 200, "{}", server.stdout_text());
    }

    let spawned = server.events("process.spawned");
    assert!(
        spawned.len() >= 8,
        "{} spawns for 36 requests\n{}",
        spawned.len(),
        server.stdout_text()
    );
    assert!(spawned.len() <= 36, "{}", spawned.len());
    let mut pids: Vec<i64> = spawned
        .iter()
        .filter_map(|event| event["pid"].as_i64())
        .collect();
    let total = pids.len();
    pids.sort_unstable();
    pids.dedup();
    assert_eq!(pids.len(), total, "every spawn is a fresh process");

    // Every process is retired on purpose and reaped, so the ledger balances.
    let deadline = Instant::now() + Duration::from_secs(20);
    while Instant::now() < deadline && server.events("process.exited").len() < total {
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(
        server.events("process.exited").len() >= total,
        "{} reaped for {total} spawned\n{}",
        server.events("process.exited").len(),
        server.stdout_text()
    );
}

#[test]
fn a_view_holds_only_its_stdio_and_its_channel() {
    if !needs_root("fds") {
        return;
    }
    let base = 49_000;
    let server = Server::start(
        "fds",
        base,
        ONE_ROUTE,
        "app:application",
        &[("app.py", APP)],
        &[],
    );
    assert_eq!(server.get("/one").status, 200);
    let value: serde_json::Value = serde_json::from_str(&server.get("/env").body).unwrap();
    let view_pid = value["pid"].as_i64().unwrap() as i32;

    // A view inherits its own `view.chan` end and its log pipes, and nothing
    // else: not the listening socket, not `http.chan`, not `ctl.chan`, not
    // another view's channel (§3).
    let fds = open_fds(view_pid);
    assert_eq!(
        fds.iter()
            .map(|(number, _)| *number)
            .take(3)
            .collect::<Vec<_>>(),
        vec![0, 1, 2],
        "the child sequence puts stdio on 0, 1 and 2: {fds:?}"
    );
    assert_eq!(fds[0].1, "/dev/null", "{fds:?}");
    assert!(
        fds[1].1.starts_with("pipe:"),
        "stdout is a log pipe: {fds:?}"
    );
    assert!(
        fds[2].1.starts_with("pipe:"),
        "stderr is a log pipe: {fds:?}"
    );
    let extras: Vec<&(i32, String)> = fds.iter().filter(|(number, _)| *number > 2).collect();
    let channel = extras.first().expect("the view holds its end of view.chan");
    assert!(
        channel.1.starts_with("socket:"),
        "the only non-stdio descriptor is the view channel: {fds:?}"
    );
    for extra in &extras {
        assert_eq!(
            extra.1, channel.1,
            "every extra descriptor is that same channel — the writer's clone — \
             and nothing else: {fds:?}"
        );
    }
}

#[test]
fn the_real_credential_is_in_no_environment() {
    if !needs_root("credential") {
        return;
    }
    let base = 50_000;
    let password = "ci-sentinel-password-9f2c";
    let url =
        format!("postgresql://doadmin:{password}@db.internal:25060/defaultdb?sslmode=require");
    let server = Server::start(
        "credential",
        base,
        PROXY_AND_ROUTE,
        "app:application",
        &[("app.py", APP)],
        &[("DATABASE_URL", &url)],
    );

    let response = server.get("/env");
    assert_eq!(response.status, 200, "{}", server.stdout_text());
    let value: serde_json::Value = serde_json::from_str(&response.body).unwrap();
    let view_pid = value["pid"].as_i64().unwrap() as i32;

    // The application's own environment…
    let app_environ = value["environ"].to_string();
    assert!(
        !app_environ.contains(password),
        "the app's os.environ carries the password"
    );
    // …the kernel's view of it, which is what a `fork` copies (§4)…
    assert!(
        !environ_of(view_pid).contains(password),
        "the view's /proc/<pid>/environ carries the password"
    );
    // …stage 1's, which is the app environment after the `execve` barrier, and
    // which every view inherits…
    let init_environ = environ_of(server.child.id() as i32);
    assert!(
        !init_environ.contains(password),
        "init's stage 1 environ carries the password: {init_environ}"
    );
    for entry in init_environ.split('\0').filter(|entry| !entry.is_empty()) {
        assert!(
            !entry.starts_with("BLASTIT_"),
            "stage 1 kept a BLASTIT_ variable: {entry}"
        );
    }
    // …and dbproxy's, which is the one process that *does* hold the credential,
    // exec'd with an empty environment: its block is empty.
    let dbproxy = server
        .component_pid(base + 3)
        .unwrap_or_else(|| panic!("dbproxy runs\n{}", server.stdout_text()));
    assert_eq!(
        environ_of(dbproxy),
        "",
        "dbproxy's environment is empty (§4)"
    );
    // Nothing reached the log either.
    assert!(!server.stdout_text().contains(password));
}

#[test]
fn a_component_that_dies_takes_init_with_it() {
    if !needs_root("component") {
        return;
    }
    // http, the scheduler and dbproxy are all fatal: init kills everything and
    // exits non-zero, so the platform restarts the container (§8).
    for (tag, offset) in [("http", 1u32), ("scheduler", 2), ("dbproxy", 3)] {
        let base = 51_000 + offset * 100;
        let mut server = Server::start(
            tag,
            base,
            PROXY_AND_ROUTE,
            "app:application",
            &[("app.py", APP)],
            &[("DATABASE_URL", "postgresql://doadmin:pw@127.0.0.1:1/db")],
        );
        let pid = server
            .component_pid(base + offset)
            .unwrap_or_else(|| panic!("{tag} runs\n{}", server.stdout_text()));
        // SAFETY: `pid` is a live child of this process, and `kill` takes two
        // integers.
        assert_eq!(unsafe { libc::kill(pid, libc::SIGKILL) }, 0);
        let status = server
            .wait_for_exit()
            .unwrap_or_else(|| panic!("init noticed {tag} dying\n{}", server.stdout_text()));
        assert_eq!(
            status.code(),
            Some(1),
            "{tag} died and init exited {status:?}\n{}",
            server.stdout_text()
        );
    }
}

#[test]
fn an_idle_view_is_retired_and_the_next_request_respawns_it() {
    if !needs_root("idle") {
        return;
    }
    let base = 52_000;
    let server = Server::start(
        "idle",
        base,
        ONE_ROUTE,
        "app:application",
        &[("app.py", APP)],
        &[("BLASTIT_VIEW_IDLE_TIMEOUT", "1s")],
    );
    let first = server.get("/one");
    assert_eq!(first.status, 200, "{}", server.stdout_text());

    // The scheduler retires the idle view and init kills it.
    assert!(
        server.wait_for_event("\"event\":\"process.retired\""),
        "{}",
        server.stdout_text()
    );
    assert!(
        server.wait_for_event("\"event\":\"process.exited\""),
        "{}",
        server.stdout_text()
    );
    assert_eq!(
        server.children().len(),
        2,
        "only http and the scheduler are left\n{}",
        server.stdout_text()
    );

    // The next request forks a fresh process.
    let second = server.get("/one");
    assert_eq!(second.status, 200, "{}", server.stdout_text());
    assert!(
        server.events("process.spawned").len() >= 2,
        "{}",
        server.stdout_text()
    );
}

#[test]
fn a_proxy_with_no_grant_gets_no_dbproxy_and_no_credential() {
    if !needs_root("ungranted") {
        return;
    }
    let base = 53_000;
    let password = "ci-sentinel-password-4b71";
    let url = format!("postgresql://doadmin:{password}@db.internal:25060/defaultdb");
    let server = Server::start(
        "ungranted",
        base,
        PROXY_WITHOUT_GRANTS,
        "app:application",
        &[("app.py", APP)],
        &[("DATABASE_URL", &url)],
    );

    // It boots, and the input variable is rewritten to the local endpoint even
    // though no route may use it.
    let response = server.get("/env");
    assert_eq!(response.status, 200, "{}", server.stdout_text());
    let value: serde_json::Value = serde_json::from_str(&response.body).unwrap();
    let database_url = value["database_url"].as_str().expect("DATABASE_URL is set");
    assert!(
        !database_url.contains(password),
        "the credential was not replaced: {database_url}"
    );
    assert!(
        database_url.contains("proc"),
        "the app is pointed at the local socket: {database_url}"
    );

    // No listener means no dbproxy, and no view either until it is used.
    assert!(
        server.component_pid(base + 3).is_none(),
        "dbproxy was forked without a listener\n{}",
        server.stdout_text()
    );
    assert!(
        server.component_pid(base + 1000).is_some(),
        "the view was forked for the request\n{}",
        server.stdout_text()
    );
}

/// Forks a child that drops to `uid` and tries to `connect()` to `path`.
///
/// Returns the `errno` of the failed connection, or `None` when it succeeded.
/// The child only makes raw syscalls: it is forked from a test harness that has
/// threads, so anything that allocates could deadlock.
fn connect_as(uid: u32, path: &str) -> Option<i32> {
    let mut address: libc::sockaddr_un = unsafe { std::mem::zeroed() };
    address.sun_family = libc::AF_UNIX as libc::sa_family_t;
    let bytes = path.as_bytes();
    assert!(bytes.len() < address.sun_path.len(), "the path fits");
    for (slot, byte) in address.sun_path.iter_mut().zip(bytes) {
        *slot = *byte as libc::c_char;
    }
    let length = (std::mem::size_of::<libc::sa_family_t>() + bytes.len() + 1) as libc::socklen_t;

    // SAFETY: the child only calls `setresgid`, `setresuid`, `socket`,
    // `connect` and `_exit`, all of which are async-signal-safe, and it reads
    // the `address` on the parent's stack, which the fork copied.
    let fork = unsafe { blastit_sys::fork_unchecked() }.expect("fork");
    match fork {
        blastit_sys::Fork::Child => {
            if blastit_sys::drop_privileges(uid).is_err() {
                unsafe { libc::_exit(9) }
            }
            let fd = unsafe { libc::socket(libc::AF_UNIX, libc::SOCK_STREAM, 0) };
            if fd < 0 {
                unsafe { libc::_exit(8) }
            }
            let connected = unsafe {
                libc::connect(
                    fd,
                    std::ptr::from_ref(&address).cast::<libc::sockaddr>(),
                    length,
                )
            };
            if connected == 0 {
                unsafe { libc::_exit(1) }
            }
            let errno = std::io::Error::last_os_error().raw_os_error().unwrap_or(0);
            unsafe { libc::_exit(errno.min(120) + 10) }
        }
        blastit_sys::Fork::Parent(pid) => {
            let mut status = 0;
            // SAFETY: `waitpid` writes one status word into `status`.
            let waited = unsafe { libc::waitpid(pid.as_raw(), &mut status, 0) };
            assert_eq!(waited, pid.as_raw(), "the child was reaped");
            let code = (status >> 8) & 0xff;
            match code {
                0 => None,
                1 => Some(0),
                other if other >= 10 => Some(other - 10),
                other => panic!("the helper child failed with {other}"),
            }
        }
    }
}

#[test]
fn a_preset_whose_assumption_does_not_hold_says_so_before_anything_else_can_fail() {
    // E1: `compose-traefik` assumes a rootless deployment (D6) and this binary
    // is started as root — which the smoke tests always are. The line is stage
    // 0's, so it is printed before the manifest is read, the environment is
    // rewritten and any view socket is bound.
    if !needs_root("preset assumption") {
        return;
    }
    let server = Server::start_with(
        "preset-assumption",
        48_900,
        ONE_ROUTE,
        "app:application",
        &[("app.py", APP)],
        &[],
        &[env!("CARGO_BIN_EXE_blastit"), "--preset", "compose-traefik"],
        None,
    );
    assert_eq!(server.get("/one").status, 200, "{}", server.stdout_text());
    assert!(
        server.wait_for_event("is for a rootless deployment"),
        "{}",
        server.stdout_text()
    );
    let warnings: Vec<String> = server
        .stdout_lines()
        .into_iter()
        .filter(|line| line.contains("is for a rootless deployment"))
        .collect();
    assert_eq!(warnings.len(), 1, "once: {}", server.stdout_text());
    let event: serde_json::Value = serde_json::from_str(&warnings[0]).expect("one JSON line");
    assert_eq!(event["event"], "app.output");
    assert_eq!(event["level"], "warn");
    assert_eq!(event["stream"], "stderr");
    // It is printed even though stage 1 also runs: the token is stage 0's, and
    // carrying it to stage 1 as well would say it twice.
    assert!(
        !server
            .stdout_text()
            .contains("BLASTIT_PRESET is set and names a different preset"),
        "{}",
        server.stdout_text()
    );

    // The other direction: `digitalocean-paas` assumes root, and this boot is
    // root, so the same code path says nothing at all.
    let quiet = Server::start_with(
        "preset-quiet",
        48_800,
        ONE_ROUTE,
        "app:application",
        &[("app.py", APP)],
        &[],
        &[
            env!("CARGO_BIN_EXE_blastit"),
            "--preset",
            "digitalocean-paas",
        ],
        None,
    );
    assert_eq!(quiet.get("/one").status, 200, "{}", quiet.stdout_text());
    assert!(
        !quiet
            .stdout_text()
            .contains("is for a platform that starts blastit as root"),
        "a root boot under a root preset says nothing: {}",
        quiet.stdout_text()
    );
}

#[test]
fn a_failing_boot_still_says_the_preset_assumption_it_contradicts() {
    // E1: the same line, from a boot that never reaches stage 1 — a manifest
    // that is not there. This is the whole reason stage 0 prints it: the
    // operator who picked the preset is the one who has to read why this boot
    // failed.
    if !needs_root("preset assumption failure") {
        return;
    }
    let fixture = Fixture::new("preset-assumption-fail");
    fixture.write("app.py", APP);
    let text = run_to_exit(
        &[
            std::ffi::OsStr::new(env!("CARGO_BIN_EXE_blastit")),
            std::ffi::OsStr::new("--preset"),
            std::ffi::OsStr::new("compose-traefik"),
        ],
        &[
            ("PORT", "8096"),
            ("BLASTIT_APP", "app:application"),
            ("BLASTIT_MANIFEST", "/nonexistent/blastit.toml"),
            ("PYTHONPATH", &fixture.python_path()),
            ("BLASTIT_UID_BASE", "48200"),
            ("BLASTIT_LOG_FORMAT", "json"),
        ],
        0,
    );
    assert!(
        text.contains("is for a rootless deployment"),
        "the assumption is in the failing boot's output:\n{text}"
    );
    assert!(
        text.contains("nonexistent"),
        "and so is the failure itself:\n{text}"
    );
    assert!(!text.contains("\"event\":\"ready\""), "{text}");
}

#[test]
fn an_unknown_preset_is_refused_with_the_list_of_presets() {
    if !needs_root("preset refusal") {
        return;
    }
    // E1: a startup error, before anything is forked, naming the flag the
    // operator used and every preset this build knows.
    let fixture = Fixture::new("preset-refusal");
    let manifest = fixture.write("blastit.toml", ONE_ROUTE);
    fixture.write("app.py", APP);
    let text = run_to_exit(
        &[
            std::ffi::OsStr::new(env!("CARGO_BIN_EXE_blastit")),
            std::ffi::OsStr::new("--preset=compose-trafik"),
        ],
        &[
            ("PORT", "8097"),
            ("BLASTIT_APP", "app:application"),
            ("BLASTIT_MANIFEST", manifest.to_str().unwrap()),
            ("PYTHONPATH", &fixture.python_path()),
            ("BLASTIT_UID_BASE", "48100"),
            ("BLASTIT_LOG_FORMAT", "json"),
        ],
        0,
    );
    assert!(text.contains("unknown preset"), "{text}");
    assert!(text.contains("compose-trafik"), "{text}");
    assert!(text.contains("--preset"), "{text}");
    for preset in ["digitalocean-paas", "compose-traefik"] {
        assert!(text.contains(preset), "{preset} is missing from\n{text}");
    }

    // The same name through the environment, named as the variable it is.
    let text = run_to_exit(
        &[std::ffi::OsStr::new(env!("CARGO_BIN_EXE_blastit"))],
        &[
            ("PORT", "8097"),
            ("BLASTIT_APP", "app:application"),
            ("BLASTIT_MANIFEST", manifest.to_str().unwrap()),
            ("PYTHONPATH", &fixture.python_path()),
            ("BLASTIT_UID_BASE", "48100"),
            ("BLASTIT_LOG_FORMAT", "json"),
            ("BLASTIT_PRESET", "compose-trafik"),
        ],
        0,
    );
    assert!(text.contains("BLASTIT_PRESET"), "{text}");
    assert!(text.contains("unknown preset"), "{text}");
}

#[test]
fn a_preset_is_accepted_and_reported_at_boot() {
    if !needs_root("preset") {
        return;
    }
    // E1: `--preset` on the command line, reported by the boot with what the
    // preset supplied and what the environment overrode (this harness sets
    // `BLASTIT_LOG_FORMAT`, which the preset also supplies), and in force in the
    // resolver: `compose-traefik` trusts every peer, so a client-IP header is
    // believed.
    let base = 48_000;
    let server = Server::start_with(
        "preset",
        base,
        ONE_ROUTE,
        "app:application",
        &[("app.py", APP)],
        &[],
        &[env!("CARGO_BIN_EXE_blastit"), "--preset=compose-traefik"],
        None,
    );

    let boots = server.events("boot");
    let config = boots
        .iter()
        .find(|boot| boot["stage"] == "config")
        .unwrap_or_else(|| panic!("no config boot event: {boots:?}"));
    assert_eq!(config["preset"], "compose-traefik", "{config}");
    assert_eq!(
        config["preset_settings"],
        serde_json::json!({
            "BLASTIT_CLIENT_IP_HEADER": "x-forwarded-for",
            "BLASTIT_TRUSTED_PROXIES": "*",
            "BLASTIT_FAIR_QUEUING": "on",
        }),
        "{config}"
    );
    assert_eq!(
        config["preset_overridden"],
        serde_json::json!(["BLASTIT_LOG_FORMAT"]),
        "the harness's own `BLASTIT_LOG_FORMAT` wins over the preset's: {config}"
    );
    // The preload line says the same thing, so a log read from the middle is
    // still readable.
    let preload = boots
        .iter()
        .find(|boot| boot["stage"] == "preload")
        .unwrap_or_else(|| panic!("no preload boot event: {boots:?}"));
    assert_eq!(preload["preset"], "compose-traefik", "{preload}");

    // The preset's resolver is in force: `do-connecting-ip` is read (from any
    // peer, because the preset trusts every one) and `X-Forwarded-For` is not.
    assert_eq!(
        server
            .request_with("GET", "/one", &[("X-Forwarded-For", "203.0.113.7")])
            .status,
        200
    );
    let requests = server.wait_for_events("request", 1);
    let forged = requests
        .iter()
        .find(|event| event["path"] == "/one")
        .unwrap_or_else(|| panic!("no request event: {requests:?}"));
    assert_eq!(forged["remote_addr"], "203.0.113.7", "{forged}");

    // Without the header, the socket peer is the client — its own ephemeral
    // port, which is what `REMOTE_ADDR` is when nothing is forwarded.
    assert_eq!(server.get("/one").status, 200);
    let requests = server.wait_for_events("request", 2);
    let honest = requests.last().expect("a request event");
    assert!(
        honest["remote_addr"]
            .as_str()
            .is_some_and(|addr| addr.starts_with("127.0.0.1:")),
        "{honest}"
    );
    assert!(
        server
            .stdout_text()
            .contains("\"preset\":\"compose-traefik\"")
    );
}

#[test]
fn the_command_line_preset_wins_and_says_so() {
    if !needs_root("preset conflict") {
        return;
    }
    // E1: the flag is the more explicit of the two, so it wins — and a variable
    // that lost is worth a line, because an operator who put
    // `BLASTIT_PRESET=compose-traefik` in an image and then ran
    // `blastit --preset=digitalocean-paas` has just changed which header is
    // believed and which peers may say it.
    let server = Server::start_with(
        "preset-conflict",
        48_700,
        ONE_ROUTE,
        "app:application",
        &[("app.py", APP)],
        &[("BLASTIT_PRESET", "compose-traefik")],
        &[env!("CARGO_BIN_EXE_blastit"), "--preset=digitalocean-paas"],
        None,
    );
    assert_eq!(server.get("/one").status, 200, "{}", server.stdout_text());

    // The `config` line names the preset that won, and the settings *it*
    // supplies.
    let boots = server.events("boot");
    let config = boots
        .iter()
        .find(|boot| boot["stage"] == "config")
        .unwrap_or_else(|| panic!("no config boot event: {boots:?}"));
    assert_eq!(config["preset"], "digitalocean-paas", "{config}");
    assert_eq!(
        config["preset_settings"]["BLASTIT_CLIENT_IP_HEADER"], "do-connecting-ip",
        "{config}"
    );

    // And the operator is told that `BLASTIT_PRESET` was not what decided. (The
    // first `app.output` line is the boot's "other process(es) run as uid 0"
    // warning, so the wait is for this sentence, not for the event name.)
    assert!(
        server.wait_for_event("BLASTIT_PRESET is set and names a different preset"),
        "{}",
        server.stdout_text()
    );
    let warnings: Vec<String> = server
        .stdout_lines()
        .into_iter()
        .filter(|line| line.contains("BLASTIT_PRESET is set and names a different preset"))
        .collect();
    assert_eq!(warnings.len(), 1, "{}", server.stdout_text());

    // The preset's resolver is in force: `do-connecting-ip` is read (from any
    // peer, because the preset trusts every one) and `X-Forwarded-For` is not.
    assert_eq!(
        server
            .request_with("GET", "/one", &[("Do-Connecting-Ip", "203.0.113.7")])
            .status,
        200
    );
    let requests = server.wait_for_events("request", 2);
    let event = requests.last().expect("a request event");
    assert_eq!(event["remote_addr"], "203.0.113.7", "{event}");
}

#[test]
fn a_client_ip_header_is_only_believed_when_it_is_configured_to_be() {
    if !needs_root("trusted proxies") {
        return;
    }
    // E1's other side, in the same harness: with no preset and no
    // `BLASTIT_TRUSTED_PROXIES`, the default trusts loopback only — and this
    // test's client *is* loopback. The difference the preset makes is which
    // header is read, so a `do-connecting-ip` sent here is not read at all.
    let server = Server::start(
        "trust",
        48_500,
        ONE_ROUTE,
        "app:application",
        &[("app.py", APP)],
        &[("BLASTIT_CLIENT_IP_HEADER", "do-connecting-ip")],
    );
    assert_eq!(
        server
            .request_with("GET", "/one", &[("Do-Connecting-Ip", "203.0.113.7")])
            .status,
        200
    );
    let requests = server.wait_for_events("request", 1);
    let event = requests.last().expect("a request event");
    assert_eq!(event["remote_addr"], "203.0.113.7", "{event}");
    // A different header is not consulted in its place.
    assert_eq!(
        server
            .request_with("GET", "/one", &[("X-Forwarded-For", "198.51.100.9")])
            .status,
        200
    );
    let requests = server.wait_for_events("request", 2);
    let event = requests.last().expect("a request event");
    assert!(
        event["remote_addr"]
            .as_str()
            .is_some_and(|addr| addr.starts_with("127.0.0.1:")),
        "{event}"
    );
}
