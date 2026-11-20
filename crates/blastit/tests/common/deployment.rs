//! A **deployment** boot of the real `blastit` image, for the §6 legs that only
//! exist under root.
//!
//! `blastit dev` is a single-uid simulation; the production path binds
//! per-view sockets, drops privileges and forks, so its promises (one OS user
//! identity per view, a credential that reaches no worker, a preset that is in
//! force) can only be observed by starting the binary the way an operator does:
//! as root, in a directory no view uid can write.
//!
//! Every leg built on this helper therefore skips **loudly** when the test
//! process is not root, exactly like the `smoke (root)` CI job's own legs. The
//! fixture root is picked by the same probe a deployment's provenance check
//! uses (`blastit::host::facts_of`), so a host with no protected directory makes
//! the leg print and pass instead of failing for the wrong reason.
#![allow(dead_code)]

use std::fs;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// How long a boot may take to announce `ready`.
const READY: Duration = Duration::from_secs(30);

/// How long a wait for one event, or for the exit, may take.
const PATIENCE: Duration = Duration::from_secs(30);

/// A counter for unique fixture directories.
static COUNTER: AtomicUsize = AtomicUsize::new(0);

/// Whether this process can bind and drop to another uid — the one precondition
/// a deployment boot has.
pub fn is_root() -> bool {
    // SAFETY: `getuid` takes no arguments and cannot fail.
    unsafe { libc::getuid() == 0 }
}

/// A `0755` fixture tree in a directory other uids cannot write.
struct Fixture {
    path: PathBuf,
}

impl Fixture {
    fn new(tag: &str) -> Option<Self> {
        let mut path = fixture_root()?;
        path.push(format!(
            "blastit-deploy-{}-{tag}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).ok()?;
        fs::set_permissions(&path, fs::Permissions::from_mode(0o755)).ok()?;
        Some(Self { path })
    }

    /// Writes a file every view uid may read (`0644`).
    fn write(&self, name: &str, contents: &str) -> PathBuf {
        let path = self.path.join(name);
        fs::write(&path, contents).expect("write a fixture file");
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644))
            .expect("a world-readable fixture");
        path
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.path);
    }
}

/// The directory a deployment fixture lives in: `/opt`, `/usr/local` or `/srv`
/// when the host's own provenance probe accepts it, `/tmp` otherwise (an
/// unprivileged run never reaches the deployment gate).
fn fixture_root() -> Option<PathBuf> {
    if is_root() {
        for parent in ["/opt", "/usr/local", "/srv"] {
            let parent = Path::new(parent);
            let protected = blastit::host::facts_of(parent).is_ok_and(|facts| {
                // SAFETY: `getuid` takes no arguments and cannot fail.
                blastit::host::protected(&facts, unsafe { libc::getuid() }).is_ok()
            });
            if parent.is_dir() && protected {
                return Some(parent.to_path_buf());
            }
        }
        println!("skipped: no directory only its owner can write, on this host");
        return None;
    }
    Some(std::env::temp_dir())
}

/// A running deployment boot, with both streams collected as they arrive.
pub struct Deployment {
    child: Child,
    port: u16,
    stdout: Arc<Mutex<Vec<String>>>,
    stderr: Arc<Mutex<Vec<String>>>,
    _fixture: Fixture,
}

impl Deployment {
    /// Starts the real image as root with `args`, waits for `ready`, and keeps
    /// the fixture alive for as long as the boot runs. `None` when this host
    /// cannot host a deployment leg (no root, no protected directory).
    pub fn start(
        tag: &str,
        uid_base: u32,
        manifest: &str,
        files: &[(&str, &str)],
        env: &[(&str, &str)],
        args: &[&str],
    ) -> Option<Self> {
        if !is_root() {
            println!("skipped: a deployment boot needs root (this process is not uid 0)");
            return None;
        }
        let fixture = Fixture::new(tag)?;
        let manifest_path = fixture.write("Blastfile", manifest);
        for (name, contents) in files {
            fixture.write(name, contents);
        }
        let port = free_port();
        let mut command = Command::new(env!("CARGO_BIN_EXE_blastit"));
        command
            .args(args)
            .env_clear()
            .env("PORT", port.to_string())
            .env("BLASTIT_APP", "app:application")
            .env("BLASTIT_MANIFEST", &manifest_path)
            .env("PYTHONPATH", fixture.path.to_string_lossy().into_owned())
            .env("BLASTIT_UID_BASE", uid_base.to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        for (name, value) in env {
            command.env(name, value);
        }
        if let Some(path) = library_path() {
            command.env("LD_LIBRARY_PATH", path);
        }
        let mut child = command.spawn().expect("the blastit binary starts");
        let stdout = Arc::new(Mutex::new(Vec::new()));
        let stderr = Arc::new(Mutex::new(Vec::new()));
        if let Some(pipe) = child.stdout.take() {
            spawn_reader(pipe, Arc::clone(&stdout));
        }
        if let Some(pipe) = child.stderr.take() {
            spawn_reader(pipe, Arc::clone(&stderr));
        }
        let mut deployment = Self {
            child,
            port,
            stdout,
            stderr,
            _fixture: fixture,
        };
        deployment.await_ready();
        Some(deployment)
    }

    /// The port the boot bound.
    pub fn port(&self) -> u16 {
        self.port
    }

    /// Waits for the `ready` event — the ordered signal, not a port probe. Both
    /// log formats are read, because a leg that is *about* the format asks for
    /// `pretty` and the two spellings share nothing.
    fn await_ready(&mut self) {
        let deadline = Instant::now() + READY;
        while Instant::now() < deadline {
            if self
                .stdout_lines()
                .iter()
                .any(|line| line.contains("\"event\":\"ready\"") || line.contains("INFO  ready"))
            {
                return;
            }
            if let Some(status) = self.child.try_wait().ok().flatten() {
                panic!(
                    "the boot exited with {status} before `ready`\nstdout:\n{}\nstderr:\n{}",
                    self.stdout_text(),
                    self.stderr_text()
                );
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        panic!(
            "the boot was not ready within {READY:?}\nstdout:\n{}\nstderr:\n{}",
            self.stdout_text(),
            self.stderr_text()
        );
    }

    /// Every complete line stdout has produced so far.
    pub fn stdout_lines(&self) -> Vec<String> {
        self.stdout.lock().expect("the stdout buffer").clone()
    }

    pub fn stdout_text(&self) -> String {
        self.stdout_lines().join("\n")
    }

    pub fn stderr_text(&self) -> String {
        self.stderr.lock().expect("the stderr buffer").join("\n")
    }

    /// Every event of the given name that stdout has produced, parsed.
    pub fn events(&self, name: &str) -> Vec<serde_json::Value> {
        self.stdout_lines()
            .iter()
            .filter_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
            .filter(|value| value["event"].as_str() == Some(name))
            .collect()
    }

    /// Waits until stdout holds a line containing `needle`.
    pub fn wait_for_event(&self, needle: &str) -> bool {
        let deadline = Instant::now() + PATIENCE;
        while Instant::now() < deadline {
            if self.stdout_lines().iter().any(|line| line.contains(needle)) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        false
    }

    /// `GET target`, answered in full.
    pub fn get(&self, target: &str) -> Response {
        self.request_with(target, &[])
    }

    /// The same, with headers of the caller's own: the trusted-client-header
    /// rules are about what a request *says*, so a leg has to be able to lie.
    pub fn request_with(&self, target: &str, extra: &[(&str, &str)]) -> Response {
        let mut stream = TcpStream::connect(("127.0.0.1", self.port)).unwrap_or_else(|err| {
            panic!(
                "connecting for {target}: {err}\nstdout:\n{}",
                self.stdout_text()
            )
        });
        stream
            .set_read_timeout(Some(Duration::from_secs(20)))
            .expect("a read timeout");
        let mut request =
            format!("GET {target} HTTP/1.1\r\nHost: blastit.test\r\nConnection: close\r\n");
        for (name, value) in extra {
            request.push_str(&format!("{name}: {value}\r\n"));
        }
        request.push_str("\r\n");
        stream
            .write_all(request.as_bytes())
            .expect("write the request");
        let mut raw = Vec::new();
        stream.read_to_end(&mut raw).expect("the answer");
        Response::parse(&raw)
    }

    /// The pids of the processes init forked, oldest first.
    pub fn children(&self) -> Vec<i32> {
        let path = format!(
            "/proc/{}/task/{}/children",
            self.child.id(),
            self.child.id()
        );
        let text = std::fs::read_to_string(&path).unwrap_or_default();
        let mut pids: Vec<i32> = text
            .split_whitespace()
            .filter_map(|pid| pid.parse().ok())
            .collect();
        pids.sort_unstable();
        pids
    }

    /// The pid of the component that runs under `uid`, if it is alive. Every
    /// process of a deployment has `gid == uid` and no groups, so the uid tells
    /// http, the scheduler, dbproxy and the views apart.
    pub fn component_uid(&self, uid: u32) -> Option<i32> {
        self.children()
            .into_iter()
            .find(|pid| process_uid(*pid) == Some(uid))
    }

    /// Sends `signal` to init.
    pub fn signal(&self, signal: i32) {
        // SAFETY: the pid came from `spawn`, so it is a live child of this
        // process, and `kill` takes two integers.
        let killed = unsafe { libc::kill(self.child.id() as i32, signal) };
        assert_eq!(killed, 0, "signalling pid {}", self.child.id());
    }

    /// Waits for init to exit, returning its status.
    pub fn wait_exit(&mut self) -> Option<ExitStatus> {
        let deadline = Instant::now() + PATIENCE;
        while Instant::now() < deadline {
            if let Some(status) = self.child.try_wait().expect("the boot's state") {
                return Some(status);
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        None
    }
}

impl Drop for Deployment {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// A parsed HTTP response.
pub struct Response {
    pub status: u16,
    pub body: String,
}

impl Response {
    fn parse(raw: &[u8]) -> Self {
        let text = String::from_utf8_lossy(raw);
        let (head, body) = match text.split_once("\r\n\r\n") {
            Some((head, body)) => (head, body.to_owned()),
            None => (text.as_ref(), String::new()),
        };
        let status = head
            .lines()
            .next()
            .and_then(|line| line.split(' ').nth(1))
            .and_then(|code| code.parse::<u16>().ok())
            .unwrap_or_else(|| panic!("no status line in {text:?}"));
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
                Ok(0) | Err(_) => return,
                Ok(_) => sink
                    .lock()
                    .expect("the line buffer")
                    .push(line.trim_end_matches('\n').to_owned()),
            }
        }
    });
}

/// A port nothing listens on, taken from the range the suites share.
fn free_port() -> u16 {
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("a port");
    let port = listener.local_addr().expect("the port").port();
    drop(listener);
    port
}

/// The real uid of `pid`, from `/proc/<pid>/status`; `None` when it is gone.
pub fn process_uid(pid: i32) -> Option<u32> {
    let status = fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
    status
        .lines()
        .find_map(|line| line.strip_prefix("Uid:"))
        .and_then(|line| line.split_whitespace().next())
        .and_then(|uid| uid.parse().ok())
}

/// The interpreter's library directory, for a deployment boot started under
/// `sudo` (which drops `LD_LIBRARY_PATH`), derived from `PYO3_PYTHON`.
fn library_path() -> Option<String> {
    let python = std::env::var_os("PYO3_PYTHON")?;
    let lib = PathBuf::from(python).parent()?.parent()?.join("lib");
    lib.is_dir().then(|| lib.to_string_lossy().into_owned())
}
